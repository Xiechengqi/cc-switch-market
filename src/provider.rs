use axum::{
    Json,
    extract::{Path, Query, State},
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    app_state::AppState,
    auth::Principal,
    error::ApiError,
    ledger::{self, AccountRef},
    router_notifications,
};

#[derive(Serialize)]
pub struct ClaimSummary {
    pub owner_email: String,
    pub available_usd: Decimal,
    pub pending_usd: Decimal,
    pub paid_usd: Decimal,
    pub minimum_payout_usd: Decimal,
    pub can_payout: bool,
}

#[derive(Serialize)]
pub struct EarningItem {
    pub event_id: String,
    pub event_type: String,
    pub request_id: String,
    pub model: String,
    pub gross_amount: Decimal,
    pub fee_amount: Decimal,
    pub net_amount: Decimal,
    pub currency: String,
    pub usage_amount: Decimal,
    pub status: String,
    pub usage_json: Option<serde_json::Value>,
    pub price_snapshot: Option<serde_json::Value>,
    pub response_meta_object_key: Option<String>,
    pub response_meta_object_sha256: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Deserialize)]
pub struct GateioPayoutRequest {
    pub params: serde_json::Value,
    pub amount_usd: Decimal,
    #[serde(default, rename = "fee_usd")]
    pub _fee_usd: Decimal,
    #[serde(default, rename = "net_payout_usd")]
    pub _net_payout_usd: Decimal,
}

#[derive(Deserialize)]
pub struct ManualPayoutTicketRequest {
    pub amount_usd: Decimal,
    #[serde(default, rename = "fee_usd")]
    pub _fee_usd: Decimal,
    #[serde(default, rename = "net_payout_usd")]
    pub _net_payout_usd: Decimal,
    pub payout_details_text: String,
    pub attachment_ids: Option<Vec<Uuid>>,
}

#[derive(Deserialize)]
pub struct ConvertToBalanceRequest {
    pub amount_usd: Decimal,
}

#[derive(Deserialize)]
pub struct TransferProviderRequest {
    pub amount_usd: Decimal,
    pub target_owner_email: String,
}

#[derive(Deserialize)]
pub struct PayoutPreviewQuery {
    pub method: String,
    pub amount_usd: Decimal,
}

#[derive(Deserialize)]
pub struct ListQuery {
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

pub async fn claim_summary(
    State(state): State<AppState>,
    principal: Principal,
) -> Result<Json<ClaimSummary>, ApiError> {
    let db = state.db();
    ledger::ensure_provider_accounts(db, &principal.email).await?;
    let available = ledger::balance(
        db,
        AccountRef::Provider {
            account_type: "client_payable",
            owner_email: &principal.email,
        },
    )
    .await?;
    let pending = ledger::balance(
        db,
        AccountRef::Provider {
            account_type: "payout_reserved",
            owner_email: &principal.email,
        },
    )
    .await?;
    let paid = paid_total(db, &principal.email).await?;
    Ok(Json(ClaimSummary {
        owner_email: principal.email,
        available_usd: available,
        pending_usd: pending,
        paid_usd: paid,
        minimum_payout_usd: Decimal::ONE,
        can_payout: available >= Decimal::ONE,
    }))
}

pub async fn earnings(
    State(state): State<AppState>,
    principal: Principal,
    Query(query): Query<ListQuery>,
) -> Result<Json<crate::pagination::Page<EarningItem>>, ApiError> {
    let mut sql = r#"
        SELECT le.id AS event_id, le.reference_id, le.amount AS net_amount,
               to_ac.owner_email AS payee_owner_email,
               rc.request_id, rc.model, COALESCE(rc.usage_amount, le.amount) AS usage_amount,
               rc.status, rc.usage_json, rc.price_snapshot,
               rc.response_meta_object_key, rc.response_meta_object_sha256,
               COALESCE(rc.created_at, le.created_at) AS request_created_at,
               le.created_at AS ledger_created_at
          FROM ledger_entries le
          JOIN wallet_accounts to_ac ON to_ac.id = le.to_account_id
          LEFT JOIN request_charges rc ON rc.id = le.reference_id
         WHERE le.reference_type = 'request_charge'
           AND to_ac.account_type = 'client_payable'
           AND to_ac.owner_email = ?1
        "#
    .to_string();
    let mut params = vec![crate::db::val(&principal.email)];
    if let Some(cursor) = query.cursor.filter(|value| !value.trim().is_empty()) {
        sql.push_str(&format!(" AND le.created_at < ?{}", params.len() + 1));
        params.push(crate::db::val(cursor));
    }
    sql.push_str(&format!(
        " ORDER BY le.created_at DESC LIMIT ?{}",
        params.len() + 1
    ));
    params.push(crate::db::val(crate::pagination::fetch_limit(query.limit)));
    let rows = state.db().query_all(&sql, params).await?;
    let items = rows
        .into_iter()
        .map(|row| {
            let is_router_commission =
                row.string("payee_owner_email") == state.config.router_commission_owner_email();
            let usage_amount = if is_router_commission {
                row.decimal("net_amount")
            } else {
                row.decimal("usage_amount")
            };
            let net_amount = row.decimal("net_amount");
            let fee_amount = if is_router_commission || usage_amount <= net_amount {
                Decimal::ZERO
            } else {
                usage_amount - net_amount
            };
            EarningItem {
                event_id: row.string("event_id"),
                event_type: if is_router_commission {
                    "router_commission"
                } else {
                    "provider_income"
                }
                .to_string(),
                request_id: row
                    .opt_string("request_id")
                    .unwrap_or_else(|| row.string("reference_id")),
                model: row.string("model"),
                usage_amount,
                gross_amount: usage_amount,
                fee_amount,
                net_amount,
                currency: "USD".to_string(),
                status: row
                    .opt_string("status")
                    .unwrap_or_else(|| "settled".to_string()),
                usage_json: row
                    .opt_string("usage_json")
                    .and_then(|value| serde_json::from_str(&value).ok()),
                price_snapshot: row
                    .opt_string("price_snapshot")
                    .and_then(|value| serde_json::from_str(&value).ok()),
                response_meta_object_key: row.opt_string("response_meta_object_key"),
                response_meta_object_sha256: row.opt_string("response_meta_object_sha256"),
                created_at: row.datetime("ledger_created_at"),
            }
        })
        .collect::<Vec<_>>();
    Ok(Json(crate::pagination::Page::from_items(
        items,
        crate::pagination::query_limit(query.limit),
        |item| item.created_at.to_rfc3339(),
    )))
}

pub async fn payout_preview(
    State(state): State<AppState>,
    principal: Principal,
    Query(query): Query<PayoutPreviewQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    ledger::ensure_provider_accounts(state.db(), &principal.email).await?;
    let (fee, fee_policy_snapshot) =
        payout_fee(state.db(), &query.method, query.amount_usd).await?;
    let net = query.amount_usd - fee;
    let available = ledger::balance(
        state.db(),
        AccountRef::Provider {
            account_type: "client_payable",
            owner_email: &principal.email,
        },
    )
    .await?;
    Ok(Json(serde_json::json!({
        "method": query.method,
        "gross_amount_usd": query.amount_usd.to_string(),
        "payout_fee_usd": fee.to_string(),
        "net_payout_usd": net.to_string(),
        "available_usd": available.to_string(),
        "can_payout": query.amount_usd >= Decimal::ONE && net > Decimal::ZERO && available >= query.amount_usd,
        "fee_policy_snapshot": fee_policy_snapshot,
    })))
}

pub async fn create_gateio_payout(
    State(state): State<AppState>,
    principal: Principal,
    Json(input): Json<GateioPayoutRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    crate::rate_limit::check("provider_payout_create", &principal.email, 6)?;
    let has_email = input
        .params
        .get("email")
        .and_then(|value| value.as_str())
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false);
    let has_uid = input
        .params
        .get("uid")
        .and_then(|value| value.as_str())
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false);
    if !has_email && !has_uid {
        return Err(ApiError::bad_request(
            "missing_gateio_target",
            "Gate.io email or uid is required",
        ));
    }
    state
        .db()
        .execute(
            "INSERT INTO provider_claim_profiles (owner_email, method, params_json, updated_at) VALUES (?1,'gateio',?2,?3) ON CONFLICT(owner_email) DO UPDATE SET method='gateio', params_json=excluded.params_json, updated_at=excluded.updated_at",
            vec![
                crate::db::val(&principal.email),
                crate::db::json_val(input.params.clone()),
                crate::db::val(crate::db::now_string()),
            ],
        )
        .await?;
    create_payout(
        &state,
        &principal.email,
        "gateio",
        input.params,
        input.amount_usd,
        input._fee_usd,
        input._net_payout_usd,
        None,
    )
    .await
}

pub async fn create_manual_payout_ticket(
    State(state): State<AppState>,
    principal: Principal,
    Json(input): Json<ManualPayoutTicketRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    crate::rate_limit::check("provider_manual_payout_create", &principal.email, 6)?;
    if input.payout_details_text.trim().is_empty()
        && input
            .attachment_ids
            .as_ref()
            .map(Vec::is_empty)
            .unwrap_or(true)
    {
        return Err(ApiError::bad_request(
            "missing_payout_details",
            "manual payout details required",
        ));
    }
    let ticket_no = format!(
        "CSM-{}",
        Uuid::new_v4().simple().to_string()[..8].to_uppercase()
    );
    let db = state.db();
    ledger::ensure_provider_accounts(db, &principal.email).await?;
    let (fee, fee_policy_snapshot) = payout_fee(db, "manual", input.amount_usd).await?;
    let net = input.amount_usd - fee;
    let tx = db.begin_immediate().await?;
    let ticket_id = Uuid::new_v4();
    tx.execute(
        r#"
        INSERT INTO tickets (id, ticket_no, ticket_type, status, priority, subject, creator_user_id, creator_owner_email, created_at, updated_at)
        VALUES (?1,?2,'payout_manual','open','normal','Manual payout request',?3,?4,?5,?5)
        "#,
        vec![
            crate::db::uuid_val(ticket_id),
            crate::db::val(ticket_no),
            crate::db::uuid_val(principal.user_id),
            crate::db::val(&principal.email),
            crate::db::val(crate::db::now_string()),
        ],
    )
    .await?;
    let message_id = Uuid::new_v4();
    tx.execute(
        "INSERT INTO ticket_messages (id, ticket_id, sender_type, sender_id, body_text, created_at) VALUES (?1,?2,'provider',?3,?4,?5)",
        vec![
            crate::db::uuid_val(message_id),
            crate::db::uuid_val(ticket_id),
            crate::db::val(&principal.email),
            crate::db::val(&input.payout_details_text),
            crate::db::val(crate::db::now_string()),
        ],
    ).await?;
    let payout = insert_payout_locked(
        &tx,
        &principal.email,
        "manual",
        serde_json::json!({"details": input.payout_details_text}),
        input.amount_usd,
        fee,
        net,
        fee_policy_snapshot,
        Some(ticket_id),
    )
    .await?;
    tx.execute(
        "UPDATE tickets SET related_payout_request_id = ?1 WHERE id = ?2",
        vec![crate::db::uuid_val(payout), crate::db::uuid_val(ticket_id)],
    )
    .await?;
    bind_attachments_locked(
        &tx,
        input.attachment_ids,
        principal.user_id,
        ticket_id,
        message_id,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(
        serde_json::json!({"payoutRequestId": payout, "ticketId": ticket_id}),
    ))
}

pub async fn convert_to_balance(
    State(state): State<AppState>,
    principal: Principal,
    Json(input): Json<ConvertToBalanceRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    crate::rate_limit::check("provider_internal_transfer", &principal.email, 12)?;
    validate_internal_transfer_amount(input.amount_usd)?;
    let db = state.db();
    ledger::ensure_provider_accounts(db, &principal.email).await?;
    ledger::ensure_user_accounts(db, principal.user_id).await?;
    let tx = db.begin_immediate().await?;
    let transfer_id = Uuid::new_v4();
    ledger::transfer(
        &tx,
        AccountRef::Provider {
            account_type: "client_payable",
            owner_email: &principal.email,
        },
        AccountRef::User {
            account_type: "user_cash",
            user_id: principal.user_id,
        },
        input.amount_usd,
        "provider_earning_to_balance",
        transfer_id,
        "provider",
        Some(&principal.email),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(serde_json::json!({
        "ok": true,
        "transferId": transfer_id,
        "amountUsd": input.amount_usd.to_string(),
    })))
}

pub async fn transfer_provider_earnings(
    State(state): State<AppState>,
    principal: Principal,
    Json(input): Json<TransferProviderRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    crate::rate_limit::check("provider_internal_transfer", &principal.email, 12)?;
    validate_internal_transfer_amount(input.amount_usd)?;
    let target_owner_email = input.target_owner_email.trim().to_ascii_lowercase();
    if !looks_like_email(&target_owner_email) {
        return Err(ApiError::bad_request(
            "invalid_target_owner_email",
            "target owner email is invalid",
        ));
    }
    if target_owner_email == principal.email.to_ascii_lowercase() {
        return Err(ApiError::bad_request(
            "same_target_owner_email",
            "target owner email must be different",
        ));
    }
    let db = state.db();
    ledger::ensure_provider_accounts(db, &principal.email).await?;
    ledger::ensure_provider_accounts(db, &target_owner_email).await?;
    let tx = db.begin_immediate().await?;
    let transfer_id = Uuid::new_v4();
    ledger::transfer(
        &tx,
        AccountRef::Provider {
            account_type: "client_payable",
            owner_email: &principal.email,
        },
        AccountRef::Provider {
            account_type: "client_payable",
            owner_email: &target_owner_email,
        },
        input.amount_usd,
        "provider_earning_transfer",
        transfer_id,
        "provider",
        Some(&principal.email),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(serde_json::json!({
        "ok": true,
        "transferId": transfer_id,
        "amountUsd": input.amount_usd.to_string(),
        "targetOwnerEmail": target_owner_email,
    })))
}

fn validate_internal_transfer_amount(amount: Decimal) -> Result<(), ApiError> {
    if amount <= Decimal::ZERO {
        return Err(ApiError::bad_request(
            "invalid_transfer_amount",
            "amount must be greater than zero",
        ));
    }
    Ok(())
}

fn looks_like_email(value: &str) -> bool {
    let Some((local, domain)) = value.split_once('@') else {
        return false;
    };
    !local.trim().is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
}

async fn bind_attachments_locked(
    tx: &crate::db::DbTx,
    attachment_ids: Option<Vec<Uuid>>,
    uploader_user_id: Uuid,
    ticket_id: Uuid,
    message_id: Uuid,
) -> Result<(), ApiError> {
    for attachment_id in attachment_ids.unwrap_or_default() {
        let updated = tx.execute(
            "UPDATE ticket_attachments SET ticket_id = ?2, message_id = ?3, reference_type='ticket', reference_id=?2 WHERE id = ?1 AND ticket_id IS NULL AND uploader_user_id = ?4",
            vec![
                crate::db::uuid_val(attachment_id),
                crate::db::uuid_val(ticket_id),
                crate::db::uuid_val(message_id),
                crate::db::uuid_val(uploader_user_id),
            ],
        )
        .await?;
        if updated == 0 {
            return Err(ApiError::forbidden(
                "attachment does not belong to this user",
            ));
        }
    }
    Ok(())
}

pub async fn payouts(
    State(state): State<AppState>,
    principal: Principal,
    Query(query): Query<ListQuery>,
) -> Result<Json<crate::pagination::Page<serde_json::Value>>, ApiError> {
    let fetch_limit = crate::pagination::fetch_limit(query.limit);
    let cursor = query
        .cursor
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string);
    let mut payout_sql = "SELECT id, owner_email, amount_usd, payout_fee_usd, net_payout_usd, method, params_json, status, ticket_id, external_tx_id, created_at, paid_at, failed_at, cancelled_at FROM payout_requests WHERE owner_email = ?1".to_string();
    let mut payout_params = vec![crate::db::val(&principal.email)];
    if let Some(cursor) = cursor.as_ref() {
        payout_sql.push_str(&format!(" AND created_at < ?{}", payout_params.len() + 1));
        payout_params.push(crate::db::val(cursor));
    }
    payout_sql.push_str(&format!(
        " ORDER BY created_at DESC LIMIT ?{}",
        payout_params.len() + 1
    ));
    payout_params.push(crate::db::val(fetch_limit));
    let payout_rows = state.db().query_all(&payout_sql, payout_params).await?;
    let mut items = payout_rows.into_iter().map(payout_json).collect::<Vec<_>>();

    let mut transfer_params = vec![crate::db::val(&principal.email)];
    let cursor_clause = if let Some(cursor) = cursor.as_ref() {
        transfer_params.push(crate::db::val(cursor));
        format!(" AND le.created_at < ?{}", transfer_params.len())
    } else {
        String::new()
    };
    transfer_params.push(crate::db::val(fetch_limit));
    let transfer_rows = state
        .db()
        .query_all(
            &format!(
                r#"
                SELECT le.id, le.amount, le.reference_type, le.reference_id, le.created_at,
                       CASE
                         WHEN le.reference_type = 'provider_earning_transfer'
                          AND to_ac.account_type = 'client_payable'
                          AND to_ac.owner_email = ?1
                         THEN 'incoming'
                         ELSE 'outgoing'
                       END AS transfer_direction,
                       from_ac.owner_email AS source_owner_email,
                       to_ac.owner_email AS target_owner_email,
                       to_ac.owner_user_id AS target_user_id
                  FROM ledger_entries le
                  JOIN wallet_accounts from_ac ON from_ac.id = le.from_account_id
                  JOIN wallet_accounts to_ac ON to_ac.id = le.to_account_id
                 WHERE (
                       (
                           le.reference_type IN ('provider_earning_to_balance','provider_earning_transfer')
                           AND from_ac.account_type = 'client_payable'
                           AND from_ac.owner_email = ?1
                       )
                        OR (
                           le.reference_type = 'provider_earning_transfer'
                           AND to_ac.account_type = 'client_payable'
                           AND to_ac.owner_email = ?1
                       )
                   )
                   {cursor_clause}
                 ORDER BY le.created_at DESC
                 LIMIT ?{}
                "#,
                transfer_params.len()
            ),
            transfer_params,
        )
        .await?;
    items.extend(transfer_rows.into_iter().map(internal_transfer_json));
    if principal.email == state.config.router_commission_owner_email() {
        let mut router_params = vec![crate::db::val(&principal.email)];
        let router_cursor_clause = if let Some(cursor) = cursor.as_ref() {
            router_params.push(crate::db::val(cursor));
            format!(" AND le.created_at < ?{}", router_params.len())
        } else {
            String::new()
        };
        router_params.push(crate::db::val(fetch_limit));
        let router_rows = state
            .db()
            .query_all(
                &format!(
                    r#"
                    SELECT le.id, le.amount, le.reference_id, le.created_at,
                           from_ac.owner_email AS source_owner_email,
                           rc.request_id, rc.model
                      FROM ledger_entries le
                      JOIN wallet_accounts to_ac ON to_ac.id = le.to_account_id
                      JOIN wallet_accounts from_ac ON from_ac.id = le.from_account_id
                      LEFT JOIN request_charges rc ON rc.id = le.reference_id
                     WHERE le.reference_type = 'request_charge'
                       AND to_ac.account_type = 'client_payable'
                       AND to_ac.owner_email = ?1
                       {router_cursor_clause}
                     ORDER BY le.created_at DESC
                     LIMIT ?{}
                    "#,
                    router_params.len()
                ),
                router_params,
            )
            .await?;
        items.extend(router_rows.into_iter().map(router_commission_json));
    }
    items.sort_by(|a, b| {
        let a_created = a
            .get("created_at")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let b_created = b
            .get("created_at")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        b_created.cmp(a_created)
    });
    Ok(Json(crate::pagination::Page::from_items(
        items,
        crate::pagination::query_limit(query.limit),
        |item| {
            item.get("created_at")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string()
        },
    )))
}

fn internal_transfer_json(row: crate::db::DbRow) -> serde_json::Value {
    let reference_type = row.string("reference_type");
    let direction = row.string("transfer_direction");
    let method = if reference_type == "provider_earning_to_balance" {
        "balance"
    } else if direction == "incoming" {
        "provider_received"
    } else {
        "provider"
    };
    let owner_email = if direction == "incoming" {
        row.string("target_owner_email")
    } else {
        row.string("source_owner_email")
    };
    serde_json::json!({
        "id": row.string("id"),
        "owner_email": owner_email,
        "amount_usd": row.string("amount"),
        "payout_fee_usd": "0",
        "net_payout_usd": row.string("amount"),
        "method": method,
        "params_json": {
            "referenceType": reference_type,
            "direction": direction,
            "sourceOwnerEmail": row.opt_string("source_owner_email"),
            "targetOwnerEmail": row.opt_string("target_owner_email"),
            "targetUserId": row.opt_string("target_user_id"),
        },
        "status": "completed",
        "external_tx_id": "",
        "created_at": row.string("created_at"),
        "paid_at": row.string("created_at"),
        "failed_at": null,
        "cancelled_at": null,
    })
}

fn router_commission_json(row: crate::db::DbRow) -> serde_json::Value {
    serde_json::json!({
        "id": row.string("id"),
        "owner_email": row.opt_string("source_owner_email"),
        "amount_usd": row.string("amount"),
        "payout_fee_usd": "0",
        "net_payout_usd": row.string("amount"),
        "method": "router_commission",
        "params_json": {
            "referenceType": "request_charge",
            "requestId": row.opt_string("request_id").unwrap_or_else(|| row.string("reference_id")),
            "model": row.opt_string("model"),
            "sourceOwnerEmail": row.opt_string("source_owner_email"),
        },
        "status": "completed",
        "external_tx_id": row.opt_string("request_id").unwrap_or_else(|| row.string("reference_id")),
        "created_at": row.string("created_at"),
        "paid_at": row.string("created_at"),
        "failed_at": null,
        "cancelled_at": null,
    })
}

pub async fn payout_detail(
    State(state): State<AppState>,
    principal: Principal,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let payout = state
        .db()
        .query_one(
            "SELECT * FROM payout_requests WHERE id=?1 AND owner_email=?2",
            vec![crate::db::uuid_val(id), crate::db::val(&principal.email)],
        )
        .await?;
    let attempts = state
        .db()
        .query_all(
            "SELECT * FROM payout_attempts WHERE payout_request_id=?1 ORDER BY created_at DESC",
            vec![crate::db::uuid_val(id)],
        )
        .await?;
    let ledger = state
        .db()
        .query_all(
            "SELECT * FROM ledger_entries WHERE reference_type='payout_request' AND reference_id=?1 ORDER BY created_at",
            vec![crate::db::uuid_val(id)],
        )
        .await?;
    let objects = state
        .db()
        .query_all(
            "SELECT object_key, content_sha256, object_role, content_type, created_at FROM object_refs WHERE reference_type='payout_request' AND reference_id=?1 ORDER BY created_at",
            vec![crate::db::uuid_val(id)],
        )
        .await?;
    Ok(Json(serde_json::json!({
        "payout": payout.to_json(),
        "attempts": attempts.into_iter().map(|row| row.to_json()).collect::<Vec<_>>(),
        "ledger": ledger.into_iter().map(|row| row.to_json()).collect::<Vec<_>>(),
        "object_refs": objects.into_iter().map(|row| row.to_json()).collect::<Vec<_>>(),
    })))
}

async fn create_payout(
    state: &AppState,
    owner_email: &str,
    method: &str,
    params: serde_json::Value,
    amount: Decimal,
    _fee: Decimal,
    _net: Decimal,
    ticket_id: Option<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let db = state.db();
    ledger::ensure_provider_accounts(db, owner_email).await?;
    let (fee, fee_policy_snapshot) = payout_fee(db, method, amount).await?;
    let net = amount - fee;
    let tx = db.begin_immediate().await?;
    let id = insert_payout_locked(
        &tx,
        owner_email,
        method,
        params,
        amount,
        fee,
        net,
        fee_policy_snapshot,
        ticket_id,
    )
    .await?;
    tx.commit().await?;
    if let Err(err) = router_notifications::send_notification(
        &state.config,
        "payout_submitted",
        owner_email,
        router_notifications::default_locale(),
        serde_json::json!({
            "payoutId": id.to_string(),
            "amountUsd": amount.to_string(),
            "feeUsd": fee.to_string(),
            "netPayoutUsd": net.to_string(),
            "claimUrl": format!("{}/claim", state.config.market_public_base_url),
        }),
    )
    .await
    {
        tracing::warn!(owner_email = %owner_email, payout_id = %id, error = %err, "send payout submitted notification failed");
    }
    Ok(Json(serde_json::json!({"id": id})))
}

pub async fn insert_payout_locked(
    tx: &crate::db::DbTx,
    owner_email: &str,
    method: &str,
    params: serde_json::Value,
    amount: Decimal,
    fee: Decimal,
    net: Decimal,
    fee_policy_snapshot: serde_json::Value,
    ticket_id: Option<Uuid>,
) -> Result<Uuid, ApiError> {
    if amount < Decimal::ONE || amount <= fee || net != amount - fee {
        return Err(ApiError::bad_request(
            "invalid_payout_amount",
            "invalid payout amount",
        ));
    }
    let existing = tx.query_optional(
        "SELECT id FROM payout_requests WHERE owner_email = ?1 AND status IN ('pending','processing','needs_review') LIMIT 1",
        vec![crate::db::val(owner_email)],
    )
    .await?;
    if existing.is_some() {
        return Err(ApiError::conflict(
            "payout_in_progress",
            "provider already has a payout in progress",
        ));
    }
    let available = tx
        .query_one(
            "SELECT balance FROM wallet_accounts WHERE account_type = 'client_payable' AND owner_email = ?1",
            vec![crate::db::val(owner_email)],
        )
        .await?
        .decimal("balance");
    if available < amount {
        return Err(ApiError::conflict(
            "insufficient_provider_balance",
            "provider available balance is insufficient for this payout",
        ));
    }
    let id = Uuid::new_v4();
    tx.execute(
        r#"
        INSERT INTO payout_requests
          (id, owner_email, amount_usd, payout_fee_usd, net_payout_usd, method, params_json, fee_policy_snapshot, status, ticket_id, created_at)
        VALUES (?1,?2,?3,?4,?5,?6,?7,?8,'pending',?9,?10)
        "#,
        vec![
            crate::db::uuid_val(id),
            crate::db::val(owner_email),
            crate::db::dec_val(amount),
            crate::db::dec_val(fee),
            crate::db::dec_val(net),
            crate::db::val(method),
            crate::db::json_val(params),
            crate::db::json_val(fee_policy_snapshot),
            crate::db::opt_uuid_val(ticket_id),
            crate::db::val(crate::db::now_string()),
        ],
    )
    .await?;
    ledger::transfer(
        tx,
        AccountRef::Provider {
            account_type: "client_payable",
            owner_email,
        },
        AccountRef::Provider {
            account_type: "payout_reserved",
            owner_email,
        },
        amount,
        "payout_request",
        id,
        "provider",
        Some(owner_email),
    )
    .await?;
    Ok(id)
}

async fn payout_fee(
    db: &crate::db::Db,
    method: &str,
    amount: Decimal,
) -> Result<(Decimal, serde_json::Value), ApiError> {
    let row = db
        .query_optional(
            "SELECT id, fixed_usd, percent_bps, min_usd, max_usd, effective_from FROM fee_policies WHERE fee_type = 'payout' AND method = ?1 AND status = 'active' ORDER BY effective_from DESC LIMIT 1",
            vec![crate::db::val(method)],
        )
        .await?;
    let Some(row) = row else {
        return Ok((
            Decimal::ZERO,
            serde_json::json!({"method": method, "source": "none"}),
        ));
    };
    let mut fee = row.decimal("fixed_usd")
        + amount * Decimal::from(row.i64("percent_bps")) / Decimal::from(10_000u64);
    let min = row.decimal("min_usd");
    if fee < min {
        fee = min;
    }
    if let Some(max) = row.opt_decimal("max_usd") {
        if max > Decimal::ZERO && fee > max {
            fee = max;
        }
    }
    Ok((
        fee,
        serde_json::json!({
            "id": row.string("id"),
            "method": method,
            "fixedUsd": row.string("fixed_usd"),
            "percentBps": row.i64("percent_bps"),
            "minUsd": row.string("min_usd"),
            "maxUsd": row.opt_string("max_usd"),
            "effectiveFrom": row.string("effective_from"),
            "computedFeeUsd": fee.to_string(),
        }),
    ))
}

async fn paid_total(db: &crate::db::Db, owner_email: &str) -> Result<Decimal, ApiError> {
    let payout_rows = db
        .query_all(
            "SELECT net_payout_usd FROM payout_requests WHERE owner_email = ?1 AND status = 'paid'",
            vec![crate::db::val(owner_email)],
        )
        .await?;
    let transfer_rows = db
        .query_all(
            r#"
            SELECT le.amount
              FROM ledger_entries le
              JOIN wallet_accounts from_ac ON from_ac.id = le.from_account_id
             WHERE le.reference_type IN ('provider_earning_to_balance','provider_earning_transfer')
               AND from_ac.account_type = 'client_payable'
               AND from_ac.owner_email = ?1
            "#,
            vec![crate::db::val(owner_email)],
        )
        .await?;
    Ok(payout_rows
        .into_iter()
        .map(|row| row.decimal("net_payout_usd"))
        .chain(transfer_rows.into_iter().map(|row| row.decimal("amount")))
        .sum())
}

fn payout_json(row: crate::db::DbRow) -> serde_json::Value {
    serde_json::json!({
        "id": row.string("id"),
        "event_id": row.string("id"),
        "event_type": "payout_request",
        "owner_email": row.string("owner_email"),
        "amount_usd": row.string("amount_usd"),
        "payout_fee_usd": row.string("payout_fee_usd"),
        "net_payout_usd": row.string("net_payout_usd"),
        "gross_amount": row.string("amount_usd"),
        "fee_amount": row.string("payout_fee_usd"),
        "net_amount": row.string("net_payout_usd"),
        "currency": "USD",
        "method": row.string("method"),
        "params_json": row.opt_string("params_json").and_then(|v| serde_json::from_str::<serde_json::Value>(&v).ok()),
        "status": row.string("status"),
        "ticket_id": row.opt_string("ticket_id"),
        "external_tx_id": row.opt_string("external_tx_id"),
        "created_at": row.opt_string("created_at"),
        "paid_at": row.opt_string("paid_at"),
        "failed_at": row.opt_string("failed_at"),
        "cancelled_at": row.opt_string("cancelled_at"),
    })
}
