use axum::{
    Json,
    extract::{Query, State},
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{app_state::AppState, auth::Principal, error::ApiError};

#[derive(Serialize)]
pub struct WalletSummary {
    pub user_cash_usd: Decimal,
    pub user_reserved_usd: Decimal,
}

#[derive(Deserialize)]
pub struct LedgerQuery {
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

pub async fn wallet_ledger(
    State(state): State<AppState>,
    principal: Principal,
    Query(query): Query<LedgerQuery>,
) -> Result<Json<crate::pagination::Page<serde_json::Value>>, ApiError> {
    let mut params = vec![crate::db::uuid_val(principal.user_id)];
    let cursor_clause = if let Some(cursor) = query.cursor.filter(|value| !value.trim().is_empty())
    {
        params.push(crate::db::val(cursor));
        format!(" AND le.created_at < ?{}", params.len())
    } else {
        String::new()
    };
    params.push(crate::db::val(crate::pagination::fetch_limit(query.limit)));
    let rows = state.db().query_all(
        &format!(
            r#"
        SELECT le.id, le.transaction_id, le.amount, le.currency, le.reference_type, le.reference_id, le.actor_type, le.created_at,
               from_ac.account_type AS from_account_type,
               to_ac.account_type AS to_account_type,
               to_ac.owner_email AS to_owner_email
          FROM ledger_entries le
          LEFT JOIN wallet_accounts from_ac ON from_ac.id = le.from_account_id
          LEFT JOIN wallet_accounts to_ac ON to_ac.id = le.to_account_id
         WHERE (from_ac.owner_user_id = ?1 OR to_ac.owner_user_id = ?1)
           {cursor_clause}
         ORDER BY le.created_at DESC
         LIMIT ?{}
        "#,
            params.len()
        ),
        params,
    )
    .await?;

    let items = rows.into_iter().map(ledger_event_json).collect::<Vec<_>>();
    Ok(Json(crate::pagination::Page::from_items(
        items,
        crate::pagination::query_limit(query.limit),
        |item| {
            item.get("created_at")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_string()
        },
    )))
}

pub async fn wallet_summary(
    State(state): State<AppState>,
    principal: Principal,
) -> Result<Json<WalletSummary>, ApiError> {
    ensure_user_accounts(state.db(), principal.user_id).await?;
    Ok(Json(WalletSummary {
        user_cash_usd: balance(
            state.db(),
            AccountRef::User {
                account_type: "user_cash",
                user_id: principal.user_id,
            },
        )
        .await?,
        user_reserved_usd: balance(
            state.db(),
            AccountRef::User {
                account_type: "user_reserved",
                user_id: principal.user_id,
            },
        )
        .await?,
    }))
}

pub async fn money_events(
    State(state): State<AppState>,
    principal: Principal,
    Query(query): Query<LedgerQuery>,
) -> Result<Json<crate::pagination::Page<serde_json::Value>>, ApiError> {
    let mut params = vec![
        crate::db::uuid_val(principal.user_id),
        crate::db::val(&principal.email),
    ];
    let cursor_clause = if let Some(cursor) = query.cursor.filter(|value| !value.trim().is_empty())
    {
        params.push(crate::db::val(cursor));
        format!(" AND le.created_at < ?{}", params.len())
    } else {
        String::new()
    };
    params.push(crate::db::val(crate::pagination::fetch_limit(query.limit)));
    let rows = state.db().query_all(
        &format!(
            r#"
            SELECT le.id, le.transaction_id, le.amount, le.currency, le.reference_type, le.reference_id,
                   le.actor_type, le.actor_id, le.created_at,
                   from_ac.account_type AS from_account_type,
                   from_ac.owner_user_id AS from_owner_user_id,
                   from_ac.owner_email AS from_owner_email,
                   to_ac.account_type AS to_account_type,
                   to_ac.owner_user_id AS to_owner_user_id,
                   to_ac.owner_email AS to_owner_email,
                   (
                     SELECT GROUP_CONCAT(object_key || '|' || content_sha256 || '|' || object_role, char(10))
                       FROM object_refs obj
                      WHERE obj.reference_type = le.reference_type
                        AND obj.reference_id = le.reference_id
                   ) AS object_refs
              FROM ledger_entries le
              LEFT JOIN wallet_accounts from_ac ON from_ac.id = le.from_account_id
              LEFT JOIN wallet_accounts to_ac ON to_ac.id = le.to_account_id
             WHERE (from_ac.owner_user_id = ?1
                OR to_ac.owner_user_id = ?1
                OR from_ac.owner_email = ?2
                OR to_ac.owner_email = ?2)
               {cursor_clause}
             ORDER BY le.created_at DESC
             LIMIT ?{}
            "#,
            params.len()
        ),
        params,
    )
        .await?;
    let items = rows
        .into_iter()
        .map(|row| {
            let object_refs = row
                .opt_string("object_refs")
                .unwrap_or_default()
                .lines()
                .filter_map(|line| {
                    let mut parts = line.split('|');
                    Some(serde_json::json!({
                        "object_key": parts.next()?,
                        "content_sha256": parts.next()?,
                        "object_role": parts.next().unwrap_or("object"),
                    }))
                })
                .collect::<Vec<_>>();
            serde_json::json!({
                "id": row.string("id"),
                "event_id": row.string("id"),
                "event_type": ledger_event_type(&row.opt_string("from_account_type"), &row.opt_string("to_account_type"), &row.opt_string("to_owner_email"), &row.string("reference_type")),
                "transaction_id": row.string("transaction_id"),
                "amount": row.string("amount"),
                "gross_amount": row.string("amount"),
                "fee_amount": "0",
                "net_amount": row.string("amount"),
                "currency": row.string("currency"),
                "status": "posted",
                "reference_type": row.string("reference_type"),
                "reference_id": row.string("reference_id"),
                "actor_type": row.string("actor_type"),
                "actor_id": row.opt_string("actor_id"),
                "from_account_type": row.opt_string("from_account_type"),
                "to_account_type": row.opt_string("to_account_type"),
                "object_refs": object_refs,
                "created_at": row.string("created_at"),
            })
        })
        .collect::<Vec<_>>();
    Ok(Json(crate::pagination::Page::from_items(
        items,
        crate::pagination::query_limit(query.limit),
        |item| {
            item.get("created_at")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_string()
        },
    )))
}

fn ledger_event_json(row: crate::db::DbRow) -> serde_json::Value {
    let amount = row.string("amount");
    let from_account_type = row.opt_string("from_account_type");
    let to_account_type = row.opt_string("to_account_type");
    let to_owner_email = row.opt_string("to_owner_email");
    let reference_type = row.string("reference_type");
    serde_json::json!({
        "id": row.string("id"),
        "event_id": row.string("id"),
        "event_type": ledger_event_type(&from_account_type, &to_account_type, &to_owner_email, &reference_type),
        "transaction_id": row.string("transaction_id"),
        "gross_amount": amount.clone(),
        "fee_amount": "0",
        "net_amount": amount.clone(),
        "amount": amount,
        "currency": row.string("currency"),
        "status": "posted",
        "reference_type": reference_type,
        "reference_id": row.string("reference_id"),
        "actor_type": row.string("actor_type"),
        "from_account_type": from_account_type,
        "to_account_type": to_account_type,
        "to_owner_email": to_owner_email,
        "created_at": row.string("created_at"),
    })
}

fn ledger_event_type(
    from_account_type: &Option<String>,
    to_account_type: &Option<String>,
    to_owner_email: &Option<String>,
    reference_type: &str,
) -> &'static str {
    if reference_type == "request_charge"
        && to_account_type.as_deref() == Some("client_payable")
        && to_owner_email
            .as_deref()
            .is_some_and(|email| email.starts_with("router@"))
    {
        return "router_commission";
    }
    match (
        reference_type,
        from_account_type.as_deref(),
        to_account_type.as_deref(),
    ) {
        ("topup", _, Some("user_cash")) => "topup",
        ("topup", _, Some("fee_revenue")) => "topup_fee",
        ("request_charge", Some("user_reserved"), Some("client_payable")) => "usage_charge",
        ("request_charge", Some("user_cash"), Some("client_payable")) => "usage_charge",
        ("request_charge", Some("risk_loss"), Some("client_payable")) => "usage_charge",
        ("request_charge", Some("user_reserved"), Some("fee_revenue")) => "platform_commission",
        ("request_charge", Some("user_cash"), Some("fee_revenue")) => "platform_commission",
        ("request_charge", Some("user_reserved"), Some("user_cash")) => "reservation_refund",
        ("provider_earning_to_balance", Some("client_payable"), Some("user_cash")) => {
            "provider_earning_to_balance"
        }
        ("provider_earning_transfer", Some("client_payable"), Some("client_payable")) => {
            "provider_earning_transfer"
        }
        ("payout_request", Some("client_payable"), Some("payout_reserved")) => "payout_reserved",
        ("payout_request", Some("payout_reserved"), Some("settlement_paid")) => "payout",
        ("payout_request", Some("payout_reserved"), Some("fee_revenue")) => "payout_fee",
        ("payout_request", Some("payout_reserved"), Some("client_payable")) => "payout_released",
        ("refund", _, _) => "refund",
        ("adjustment", _, _) => "manual_adjustment",
        _ => "ledger_entry",
    }
}

pub async fn ensure_user_accounts(db: &crate::db::Db, user_id: Uuid) -> Result<(), ApiError> {
    for account in ["user_cash", "user_reserved"] {
        db.execute(
            "INSERT OR IGNORE INTO wallet_accounts (id, account_type, owner_user_id, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?4)",
            vec![
                crate::db::uuid_val(Uuid::new_v4()),
                crate::db::val(account),
                crate::db::uuid_val(user_id),
                crate::db::val(crate::db::now_string()),
            ],
        )
        .await?;
    }
    Ok(())
}

pub async fn ensure_platform_accounts(db: &crate::db::Db) -> Result<(), ApiError> {
    for account in [
        "payment_clearing",
        "settlement_paid",
        "risk_loss",
        "fee_revenue",
    ] {
        db.execute(
            "INSERT OR IGNORE INTO wallet_accounts (id, account_type, created_at, updated_at) VALUES (?1, ?2, ?3, ?3)",
            vec![
                crate::db::uuid_val(Uuid::new_v4()),
                crate::db::val(account),
                crate::db::val(crate::db::now_string()),
            ],
        )
        .await?;
    }
    Ok(())
}

pub async fn ensure_provider_accounts(
    db: &crate::db::Db,
    owner_email: &str,
) -> Result<(), ApiError> {
    for account in ["client_payable", "payout_reserved"] {
        db.execute(
            "INSERT OR IGNORE INTO wallet_accounts (id, account_type, owner_email, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?4)",
            vec![
                crate::db::uuid_val(Uuid::new_v4()),
                crate::db::val(account),
                crate::db::val(owner_email),
                crate::db::val(crate::db::now_string()),
            ],
        )
        .await?;
    }
    Ok(())
}

pub async fn transfer(
    tx: &crate::db::DbTx,
    from: AccountRef<'_>,
    to: AccountRef<'_>,
    amount: Decimal,
    reference_type: &str,
    reference_id: Uuid,
    actor_type: &str,
    actor_id: Option<&str>,
) -> Result<(), ApiError> {
    if amount <= Decimal::ZERO {
        return Err(ApiError::bad_request(
            "invalid_amount",
            "ledger amount must be positive",
        ));
    }
    let from_id = account_id(tx, from).await?;
    let to_id = account_id(tx, to).await?;
    let transaction_id = Uuid::new_v4();

    let current_from_balance = balance_in_tx(tx, from_id).await?;
    let from_balance = current_from_balance - amount;
    if from_balance < Decimal::ZERO && !matches!(from, AccountRef::Platform { .. }) {
        return Err(ApiError::conflict(
            "insufficient_balance",
            "account balance is insufficient for this ledger transfer",
        ));
    }
    let to_balance = balance_in_tx(tx, to_id).await? + amount;
    tx.execute(
        "UPDATE wallet_accounts SET balance = ?1, updated_at = ?2 WHERE id = ?3",
        vec![
            crate::db::dec_val(from_balance),
            crate::db::val(crate::db::now_string()),
            crate::db::uuid_val(from_id),
        ],
    )
    .await?;
    tx.execute(
        "UPDATE wallet_accounts SET balance = ?1, updated_at = ?2 WHERE id = ?3",
        vec![
            crate::db::dec_val(to_balance),
            crate::db::val(crate::db::now_string()),
            crate::db::uuid_val(to_id),
        ],
    )
    .await?;
    tx.execute(
        r#"
        INSERT INTO ledger_entries
          (id, transaction_id, from_account_id, to_account_id, amount, reference_type, reference_id, actor_type, actor_id, metadata_json, created_at)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, '{}', ?10)
        "#,
        vec![
            crate::db::uuid_val(Uuid::new_v4()),
            crate::db::uuid_val(transaction_id),
            crate::db::uuid_val(from_id),
            crate::db::uuid_val(to_id),
            crate::db::dec_val(amount),
            crate::db::val(reference_type),
            crate::db::uuid_val(reference_id),
            crate::db::val(actor_type),
            crate::db::opt_val(actor_id),
            crate::db::val(crate::db::now_string()),
        ],
    )
    .await?;
    Ok(())
}

pub async fn balance(db: &crate::db::Db, account: AccountRef<'_>) -> Result<Decimal, ApiError> {
    let row = match account {
        AccountRef::User { account_type, user_id } => {
            db.query_optional("SELECT balance FROM wallet_accounts WHERE account_type = ?1 AND owner_user_id = ?2", vec![crate::db::val(account_type), crate::db::uuid_val(user_id)]).await?
        }
        AccountRef::Provider { account_type, owner_email } => {
            db.query_optional("SELECT balance FROM wallet_accounts WHERE account_type = ?1 AND owner_email = ?2", vec![crate::db::val(account_type), crate::db::val(owner_email)]).await?
        }
        AccountRef::Platform { account_type } => {
            db.query_optional("SELECT balance FROM wallet_accounts WHERE account_type = ?1 AND owner_user_id IS NULL AND owner_email IS NULL", vec![crate::db::val(account_type)]).await?
        }
    };
    Ok(row.map(|r| r.decimal("balance")).unwrap_or(Decimal::ZERO))
}

pub async fn consistency_report(db: &crate::db::Db) -> Result<serde_json::Value, ApiError> {
    let rows = db
        .query_all(
            r#"
            SELECT wa.id, wa.account_type, wa.owner_user_id, wa.owner_email, wa.balance,
                   COALESCE((
                     SELECT SUM(CAST(amount AS REAL)) FROM ledger_entries WHERE to_account_id = wa.id
                   ), 0) -
                   COALESCE((
                     SELECT SUM(CAST(amount AS REAL)) FROM ledger_entries WHERE from_account_id = wa.id
                   ), 0) AS computed_balance
              FROM wallet_accounts wa
             ORDER BY wa.created_at
            "#,
            vec![],
        )
        .await?;
    let mut drift = Vec::new();
    for row in rows {
        let stored = row.decimal("balance");
        let computed = row.decimal("computed_balance");
        let delta = stored - computed;
        if delta.abs() > Decimal::new(1, 8) {
            drift.push(serde_json::json!({
                "account_id": row.string("id"),
                "account_type": row.string("account_type"),
                "owner_user_id": row.opt_string("owner_user_id"),
                "owner_email": row.opt_string("owner_email"),
                "stored_balance": stored.to_string(),
                "computed_balance": computed.to_string(),
                "delta": delta.to_string(),
            }));
        }
    }
    let reserved = reserved_consistency(db).await?;
    let payout_reserved = payout_reserved_consistency(db).await?;
    Ok(serde_json::json!({
        "ok": drift.is_empty() && reserved.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) && payout_reserved.get("ok").and_then(|v| v.as_bool()).unwrap_or(false),
        "balance_drift": drift,
        "user_reserved": reserved,
        "payout_reserved": payout_reserved,
    }))
}

async fn reserved_consistency(db: &crate::db::Db) -> Result<serde_json::Value, ApiError> {
    let rows = db
        .query_all(
            r#"
            SELECT wa.owner_user_id, wa.balance,
                   COALESCE((
                     SELECT SUM(CAST(reserved_amount AS REAL))
                      FROM request_charges rc
                     WHERE rc.user_id = wa.owner_user_id AND rc.status IN ('reserved','streaming','needs_review')
                   ), 0) AS expected
              FROM wallet_accounts wa
             WHERE wa.account_type = 'user_reserved'
            "#,
            vec![],
        )
        .await?;
    let mut drift = Vec::new();
    for row in rows {
        let actual = row.decimal("balance");
        let expected = row.decimal("expected");
        let delta = actual - expected;
        if delta.abs() > Decimal::new(1, 8) {
            drift.push(serde_json::json!({
                "owner_user_id": row.opt_string("owner_user_id"),
                "actual": actual.to_string(),
                "expected": expected.to_string(),
                "delta": delta.to_string(),
            }));
        }
    }
    Ok(serde_json::json!({"ok": drift.is_empty(), "drift": drift}))
}

async fn payout_reserved_consistency(db: &crate::db::Db) -> Result<serde_json::Value, ApiError> {
    let rows = db
        .query_all(
            r#"
            SELECT wa.owner_email, wa.balance,
                   COALESCE((
                     SELECT SUM(CAST(amount_usd AS REAL))
                       FROM payout_requests pr
                      WHERE pr.owner_email = wa.owner_email
                        AND pr.status IN ('pending','processing','needs_review')
                   ), 0) AS expected
              FROM wallet_accounts wa
             WHERE wa.account_type = 'payout_reserved'
            "#,
            vec![],
        )
        .await?;
    let mut drift = Vec::new();
    for row in rows {
        let actual = row.decimal("balance");
        let expected = row.decimal("expected");
        let delta = actual - expected;
        if delta.abs() > Decimal::new(1, 8) {
            drift.push(serde_json::json!({
                "owner_email": row.opt_string("owner_email"),
                "actual": actual.to_string(),
                "expected": expected.to_string(),
                "delta": delta.to_string(),
            }));
        }
    }
    Ok(serde_json::json!({"ok": drift.is_empty(), "drift": drift}))
}

#[derive(Clone, Copy)]
pub enum AccountRef<'a> {
    User {
        account_type: &'a str,
        user_id: Uuid,
    },
    Provider {
        account_type: &'a str,
        owner_email: &'a str,
    },
    Platform {
        account_type: &'a str,
    },
}

async fn account_id(tx: &crate::db::DbTx, account: AccountRef<'_>) -> Result<Uuid, ApiError> {
    let row = match account {
        AccountRef::User { account_type, user_id } => {
            tx.query_one("SELECT id FROM wallet_accounts WHERE account_type = ?1 AND owner_user_id = ?2", vec![crate::db::val(account_type), crate::db::uuid_val(user_id)]).await?
        }
        AccountRef::Provider { account_type, owner_email } => {
            tx.query_one("SELECT id FROM wallet_accounts WHERE account_type = ?1 AND owner_email = ?2", vec![crate::db::val(account_type), crate::db::val(owner_email)]).await?
        }
        AccountRef::Platform { account_type } => {
            tx.query_one("SELECT id FROM wallet_accounts WHERE account_type = ?1 AND owner_user_id IS NULL AND owner_email IS NULL", vec![crate::db::val(account_type)]).await?
        }
    };
    Ok(row.uuid("id"))
}

async fn balance_in_tx(tx: &crate::db::DbTx, account_id: Uuid) -> Result<Decimal, ApiError> {
    let row = tx
        .query_one(
            "SELECT balance FROM wallet_accounts WHERE id = ?1",
            vec![crate::db::uuid_val(account_id)],
        )
        .await?;
    Ok(row.decimal("balance"))
}
