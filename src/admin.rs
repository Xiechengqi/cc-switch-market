use std::{
    collections::BTreeMap,
    fs,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

use anyhow::Context;
use axum::{
    Json,
    extract::{Path, Query, State},
};
use rust_decimal::Decimal;
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    app_state::AppState,
    auth::AdminPrincipal,
    config::Config,
    error::ApiError,
    gateio,
    ledger::{self, AccountRef},
    router_notifications,
};

#[derive(Deserialize)]
pub struct LimitQuery {
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

struct CachedJson {
    expires_at: Instant,
    value: serde_json::Value,
}

static LEDGER_REPORT_CACHE: OnceLock<Mutex<Option<CachedJson>>> = OnceLock::new();
static ADMIN_SUMMARY_CACHE: OnceLock<Mutex<Option<CachedJson>>> = OnceLock::new();

async fn cached_ledger_report(state: &AppState) -> Result<serde_json::Value, ApiError> {
    cached_json(&LEDGER_REPORT_CACHE, Duration::from_secs(30), || async {
        ledger::consistency_report(state.db()).await
    })
    .await
}

async fn cached_json<F, Fut>(
    cache: &OnceLock<Mutex<Option<CachedJson>>>,
    ttl: Duration,
    build: F,
) -> Result<serde_json::Value, ApiError>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<serde_json::Value, ApiError>>,
{
    let now = Instant::now();
    if let Ok(guard) = cache.get_or_init(|| Mutex::new(None)).lock() {
        if let Some(cached) = guard.as_ref().filter(|cached| cached.expires_at > now) {
            return Ok(cached.value.clone());
        }
    }
    let value = build().await?;
    if let Ok(mut guard) = cache.get_or_init(|| Mutex::new(None)).lock() {
        *guard = Some(CachedJson {
            expires_at: Instant::now() + ttl,
            value: value.clone(),
        });
    }
    Ok(value)
}

async fn json_page(
    state: &AppState,
    base_sql: &str,
    order_column: &str,
    mut params: Vec<libsql::Value>,
    query: LimitQuery,
) -> Result<Json<crate::pagination::Page<serde_json::Value>>, ApiError> {
    let mut sql = base_sql.to_string();
    if let Some(cursor) = query.cursor.filter(|value| !value.trim().is_empty()) {
        sql.push_str(&format!(" AND {order_column} < ?{}", params.len() + 1));
        params.push(crate::db::val(cursor));
    }
    sql.push_str(&format!(
        " ORDER BY {order_column} DESC LIMIT ?{}",
        params.len() + 1
    ));
    params.push(crate::db::val(crate::pagination::fetch_limit(query.limit)));
    let rows = state.db().query_all(&sql, params).await?;
    let items = rows
        .into_iter()
        .map(|row| row.to_json())
        .collect::<Vec<_>>();
    Ok(Json(crate::pagination::Page::from_items(
        items,
        crate::pagination::query_limit(query.limit),
        |item| {
            item.get(order_column)
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_string()
        },
    )))
}

pub async fn users(
    State(state): State<AppState>,
    _admin: AdminPrincipal,
    Query(query): Query<LimitQuery>,
) -> Result<Json<crate::pagination::Page<serde_json::Value>>, ApiError> {
    json_page(
        &state,
        r#"
        SELECT u.*,
               COALESCE((
                   SELECT wa.balance
                     FROM wallet_accounts wa
                    WHERE wa.owner_user_id = u.id
                      AND wa.account_type = 'user_cash'
                    LIMIT 1
               ), '0') AS user_cash_usd,
               COALESCE((
                   SELECT wa.balance
                     FROM wallet_accounts wa
                    WHERE wa.owner_user_id = u.id
                      AND wa.account_type = 'user_reserved'
                    LIMIT 1
               ), '0') AS user_reserved_usd
          FROM users u
         WHERE 1=1
        "#,
        "created_at",
        vec![],
        query,
    )
    .await
}

pub async fn user(
    State(state): State<AppState>,
    _admin: AdminPrincipal,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let user = state
        .db()
        .query_one(
            "SELECT * FROM users WHERE id = ?1",
            vec![crate::db::uuid_val(id)],
        )
        .await?;
    let api_keys = state
        .db()
        .query_all(
            "SELECT id, name, prefix, scope_json, expires_at, monthly_spend_cap, last_used_at, last_used_ip_country, created_at, revoked_at FROM api_keys WHERE user_id = ?1 ORDER BY created_at DESC",
            vec![crate::db::uuid_val(id)],
        )
        .await?;
    let balances = state
        .db()
        .query_all(
            "SELECT account_type, balance FROM wallet_accounts WHERE owner_user_id = ?1 ORDER BY account_type",
            vec![crate::db::uuid_val(id)],
        )
        .await?;
    Ok(Json(serde_json::json!({
        "user": user.to_json(),
        "api_keys": api_keys.into_iter().map(|r| r.to_json()).collect::<Vec<_>>(),
        "balances": balances.into_iter().map(|r| r.to_json()).collect::<Vec<_>>(),
    })))
}

pub async fn user_ledger(
    State(state): State<AppState>,
    _admin: AdminPrincipal,
    Path(id): Path<Uuid>,
    Query(query): Query<LimitQuery>,
) -> Result<Json<crate::pagination::Page<serde_json::Value>>, ApiError> {
    let mut sql = r#"SELECT le.* FROM ledger_entries le
           JOIN wallet_accounts wa ON wa.id = le.from_account_id OR wa.id = le.to_account_id
          WHERE wa.owner_user_id = ?1"#
        .to_string();
    let mut params = vec![crate::db::uuid_val(id)];
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
        .map(|row| row.to_json())
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

#[derive(Deserialize)]
pub struct UserAdjust {
    pub amount_usd: Decimal,
    pub direction: String,
    pub reason: String,
}

pub async fn adjust_user(
    State(state): State<AppState>,
    AdminPrincipal(admin): AdminPrincipal,
    Path(id): Path<Uuid>,
    Json(input): Json<UserAdjust>,
) -> Result<Json<serde_json::Value>, ApiError> {
    ledger::ensure_user_accounts(state.db(), id).await?;
    ledger::ensure_platform_accounts(state.db()).await?;
    let tx = state.db().begin_immediate().await?;
    let reference_id = Uuid::new_v4();
    let (from, to) = if input.direction == "credit" {
        (
            AccountRef::Platform {
                account_type: "risk_loss",
            },
            AccountRef::User {
                account_type: "user_cash",
                user_id: id,
            },
        )
    } else {
        (
            AccountRef::User {
                account_type: "user_cash",
                user_id: id,
            },
            AccountRef::Platform {
                account_type: "risk_loss",
            },
        )
    };
    ledger::transfer(
        &tx,
        from,
        to,
        input.amount_usd,
        "adjustment",
        reference_id,
        "admin",
        Some(&admin.email),
    )
    .await?;
    audit_tx(
        &tx,
        &admin.email,
        "user.adjust",
        "adjustment",
        reference_id,
        serde_json::json!({"reason": input.reason}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(
        serde_json::json!({"ok": true, "referenceId": reference_id}),
    ))
}

pub async fn topups(
    State(state): State<AppState>,
    _admin: AdminPrincipal,
    Query(query): Query<LimitQuery>,
) -> Result<Json<crate::pagination::Page<serde_json::Value>>, ApiError> {
    let mut params = vec![];
    let cursor_clause = if let Some(cursor) = query
        .cursor
        .as_ref()
        .filter(|value| !value.trim().is_empty())
    {
        params.push(crate::db::val(cursor));
        " WHERE t.created_at < ?1"
    } else {
        ""
    };
    params.push(crate::db::val(crate::pagination::fetch_limit(query.limit)));
    let rows = state
        .db()
        .query_all(
            &format!(
                r#"
                SELECT t.*, u.email AS user_email
                  FROM topup_orders t
                  JOIN users u ON u.id = t.user_id
                 {cursor_clause}
                 ORDER BY t.created_at DESC
                 LIMIT ?{}
                "#,
                params.len()
            ),
            params,
        )
        .await?;
    let items = rows
        .into_iter()
        .map(|row| row.to_json())
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

pub async fn topup(
    State(state): State<AppState>,
    _admin: AdminPrincipal,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let topup = state
        .db()
        .query_one(
            "SELECT t.*, u.email AS user_email FROM topup_orders t JOIN users u ON u.id = t.user_id WHERE t.id = ?1",
            vec![crate::db::uuid_val(id)],
        )
        .await?;
    let ledger = state
        .db()
        .query_all(
            "SELECT * FROM ledger_entries WHERE reference_type='topup' AND reference_id=?1 ORDER BY created_at",
            vec![crate::db::uuid_val(id)],
        )
        .await?;
    let webhooks = state
        .db()
        .query_all(
            "SELECT * FROM processed_webhooks WHERE provider='dodo' AND raw_payload_object_key IN (SELECT object_key FROM object_refs WHERE reference_type='dodo_webhook' AND reference_id=?1) ORDER BY created_at DESC",
            vec![crate::db::uuid_val(id)],
        )
        .await?;
    let objects = state
        .db()
        .query_all(
            "SELECT * FROM object_refs WHERE (reference_type='dodo_webhook' OR reference_type='topup') AND reference_id=?1 ORDER BY created_at DESC",
            vec![crate::db::uuid_val(id)],
        )
        .await?;
    let webhook_payloads = webhooks
        .iter()
        .map(|row| row.opt_string("raw_payload_object_key"))
        .collect::<Vec<_>>();
    let mut payloads = Vec::new();
    for object_key in webhook_payloads {
        if let Some(json) = read_json_object(&state, object_key.clone()).await {
            payloads.push(serde_json::json!({
                "objectKey": object_key,
                "json": json,
            }));
        }
    }
    Ok(Json(serde_json::json!({
        "topup": topup.to_json(),
        "ledger": ledger.into_iter().map(|r| r.to_json()).collect::<Vec<_>>(),
        "webhooks": webhooks.into_iter().map(|r| r.to_json()).collect::<Vec<_>>(),
        "objects": objects.into_iter().map(|r| r.to_json()).collect::<Vec<_>>(),
        "webhookPayloads": payloads,
    })))
}

#[derive(Deserialize)]
pub struct RefundTopup {
    pub reason: String,
    pub refund_fee: Option<bool>,
}

pub async fn refund_topup(
    State(state): State<AppState>,
    AdminPrincipal(admin): AdminPrincipal,
    Path(id): Path<Uuid>,
    Json(input): Json<RefundTopup>,
) -> Result<Json<serde_json::Value>, ApiError> {
    ledger::ensure_platform_accounts(state.db()).await?;
    let tx = state.db().begin_immediate().await?;
    let row = tx
        .query_optional(
            r#"
            UPDATE topup_orders
               SET status='refunded', refunded_at=?2
             WHERE id=?1 AND status='paid'
             RETURNING user_id, net_amount, fee_amount
            "#,
            vec![
                crate::db::uuid_val(id),
                crate::db::val(crate::db::now_string()),
            ],
        )
        .await?
        .ok_or_else(|| ApiError::conflict("invalid_topup_state", "topup is not paid"))?;
    let user_id = row.uuid("user_id");
    let net = row.decimal("net_amount");
    let fee = if input.refund_fee.unwrap_or(false) {
        row.decimal("fee_amount")
    } else {
        Decimal::ZERO
    };
    let cash = tx
        .query_one(
            "SELECT balance FROM wallet_accounts WHERE account_type='user_cash' AND owner_user_id=?1",
            vec![crate::db::uuid_val(user_id)],
        )
        .await?
        .decimal("balance");
    let from_user = cash.min(net);
    if from_user > Decimal::ZERO {
        ledger::transfer(
            &tx,
            AccountRef::User {
                account_type: "user_cash",
                user_id,
            },
            AccountRef::Platform {
                account_type: "payment_clearing",
            },
            from_user,
            "refund",
            id,
            "admin",
            Some(&admin.email),
        )
        .await?;
    }
    let shortfall = net - from_user;
    if shortfall > Decimal::ZERO {
        ledger::transfer(
            &tx,
            AccountRef::Platform {
                account_type: "risk_loss",
            },
            AccountRef::Platform {
                account_type: "payment_clearing",
            },
            shortfall,
            "refund",
            id,
            "admin",
            Some(&admin.email),
        )
        .await?;
    }
    if fee > Decimal::ZERO {
        ledger::transfer(
            &tx,
            AccountRef::Platform {
                account_type: "fee_revenue",
            },
            AccountRef::Platform {
                account_type: "payment_clearing",
            },
            fee,
            "refund",
            id,
            "admin",
            Some(&admin.email),
        )
        .await?;
    }
    audit_tx(
        &tx,
        &admin.email,
        "topup.refund",
        "topup",
        id,
        serde_json::json!({"reason": input.reason, "refund_fee": input.refund_fee.unwrap_or(false)}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn webhooks(
    State(state): State<AppState>,
    _admin: AdminPrincipal,
    Query(query): Query<LimitQuery>,
) -> Result<Json<crate::pagination::Page<serde_json::Value>>, ApiError> {
    json_page(
        &state,
        "SELECT * FROM processed_webhooks WHERE 1=1",
        "created_at",
        vec![],
        query,
    )
    .await
}

pub async fn shares(
    State(state): State<AppState>,
    _admin: AdminPrincipal,
    Query(query): Query<LimitQuery>,
) -> Result<Json<crate::pagination::Page<serde_json::Value>>, ApiError> {
    json_page(
        &state,
        r#"
        SELECT rs.*,
               EXISTS(SELECT 1 FROM market_share_capability_blocks b WHERE b.router_id=rs.router_id AND b.share_id=rs.share_id AND b.capability='claude') AS blocked_claude,
               EXISTS(SELECT 1 FROM market_share_capability_blocks b WHERE b.router_id=rs.router_id AND b.share_id=rs.share_id AND b.capability='codex') AS blocked_codex,
               EXISTS(SELECT 1 FROM market_share_capability_blocks b WHERE b.router_id=rs.router_id AND b.share_id=rs.share_id AND b.capability='gemini') AS blocked_gemini
          FROM router_shares rs
         WHERE 1=1
        "#,
        "last_seen_at",
        vec![],
        query,
    )
    .await
}

#[derive(Deserialize)]
pub struct ShareCapabilityBlockInput {
    pub router_id: String,
    pub share_id: String,
    pub capability: String,
    pub reason: Option<String>,
}

pub async fn share_capability_blocks(
    State(state): State<AppState>,
    _admin: AdminPrincipal,
    Query(query): Query<LimitQuery>,
) -> Result<Json<crate::pagination::Page<serde_json::Value>>, ApiError> {
    json_page(
        &state,
        "SELECT * FROM market_share_capability_blocks WHERE 1=1",
        "created_at",
        vec![],
        query,
    )
    .await
}

pub async fn create_share_capability_block(
    State(state): State<AppState>,
    AdminPrincipal(admin): AdminPrincipal,
    Json(input): Json<ShareCapabilityBlockInput>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let capability = normalize_share_capability(&input.capability)?;
    let router_id = input.router_id.trim();
    let share_id = input.share_id.trim();
    if router_id.is_empty() || share_id.is_empty() {
        return Err(ApiError::bad_request(
            "invalid_share",
            "router_id and share_id are required",
        ));
    }
    let now = crate::db::now_string();
    state
        .db()
        .execute(
            r#"
            INSERT INTO market_share_capability_blocks
              (router_id, share_id, capability, reason, created_by, created_at)
            VALUES (?1,?2,?3,?4,?5,?6)
            ON CONFLICT(router_id, share_id, capability) DO UPDATE SET
              reason=excluded.reason, created_by=excluded.created_by, created_at=excluded.created_at
            "#,
            vec![
                crate::db::val(router_id),
                crate::db::val(share_id),
                crate::db::val(capability),
                crate::db::opt_val(
                    input
                        .reason
                        .map(|value| value.trim().to_string())
                        .filter(|value| !value.is_empty()),
                ),
                crate::db::val(&admin.email),
                crate::db::val(now),
            ],
        )
        .await?;
    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn delete_share_capability_block(
    State(state): State<AppState>,
    _admin: AdminPrincipal,
    Path((router_id, share_id, capability)): Path<(String, String, String)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let capability = normalize_share_capability(&capability)?;
    state
        .db()
        .execute(
            "DELETE FROM market_share_capability_blocks WHERE router_id=?1 AND share_id=?2 AND capability=?3",
            vec![crate::db::val(router_id), crate::db::val(share_id), crate::db::val(capability)],
        )
        .await?;
    Ok(Json(serde_json::json!({"ok": true})))
}

fn normalize_share_capability(value: &str) -> Result<&'static str, ApiError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "claude" | "anthropic" => Ok("claude"),
        "codex" | "openai" => Ok("codex"),
        "gemini" => Ok("gemini"),
        _ => Err(ApiError::bad_request(
            "invalid_capability",
            "capability must be claude, codex, or gemini",
        )),
    }
}

pub async fn sync_shares(
    State(state): State<AppState>,
    AdminPrincipal(admin): AdminPrincipal,
) -> Result<Json<serde_json::Value>, ApiError> {
    let count = crate::router_client::sync_shares(&state).await?;
    state
        .db()
        .execute(
            "INSERT INTO admin_audit (id, admin_actor, action, reference_type, reference_id, metadata_json, created_at) VALUES (?1,?2,'shares.sync','router_shares',?3,?4,?5)",
            vec![
                crate::db::uuid_val(Uuid::new_v4()),
                crate::db::val(&admin.email),
                crate::db::uuid_val(Uuid::new_v4()),
                crate::db::json_val(serde_json::json!({"synced": count})),
                crate::db::val(crate::db::now_string()),
            ],
        )
        .await?;
    Ok(Json(serde_json::json!({"ok": true, "synced": count})))
}

pub async fn charges(
    State(state): State<AppState>,
    _admin: AdminPrincipal,
    Query(query): Query<LimitQuery>,
) -> Result<Json<crate::pagination::Page<serde_json::Value>>, ApiError> {
    let mut params = vec![];
    let cursor_clause = if let Some(cursor) = query
        .cursor
        .as_ref()
        .filter(|value| !value.trim().is_empty())
    {
        params.push(crate::db::val(cursor));
        " WHERE rc.created_at < ?1"
    } else {
        ""
    };
    params.push(crate::db::val(crate::pagination::fetch_limit(query.limit)));
    let rows = state
        .db()
        .query_all(
            &format!(
                r#"
                SELECT rc.*, u.email AS requester_email,
                       COALESCE(NULLIF(rs.subdomain, ''), json_extract(rs.raw_json, '$.subdomain')) AS share_subdomain
                  FROM request_charges rc
                  JOIN users u ON u.id = rc.user_id
                  LEFT JOIN router_shares rs ON rs.router_id = rc.router_id AND rs.share_id = rc.share_id
                 {cursor_clause}
                 ORDER BY rc.created_at DESC
                 LIMIT ?{}
                "#,
                params.len()
            ),
            params,
        )
        .await?;
    let items = rows
        .into_iter()
        .map(|row| row.to_json())
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

pub async fn charge_review_context(
    State(state): State<AppState>,
    _admin: AdminPrincipal,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let charge = state
        .db()
        .query_one(
            "SELECT rc.*, u.email AS requester_email, COALESCE(NULLIF(rs.subdomain, ''), json_extract(rs.raw_json, '$.subdomain')) AS share_subdomain FROM request_charges rc JOIN users u ON u.id = rc.user_id LEFT JOIN router_shares rs ON rs.router_id = rc.router_id AND rs.share_id = rc.share_id WHERE rc.id=?1",
            vec![crate::db::uuid_val(id)],
        )
        .await?;
    let attempts = state
        .db()
        .query_all(
            "SELECT * FROM request_attempts WHERE charge_id=?1 ORDER BY attempt_no ASC",
            vec![crate::db::uuid_val(id)],
        )
        .await?;
    let objects = state
        .db()
        .query_all(
            "SELECT * FROM object_refs WHERE reference_type='request_charge' AND reference_id=?1 ORDER BY created_at ASC",
            vec![crate::db::uuid_val(id)],
        )
        .await?;
    let router_share = state
        .db()
        .query_optional(
            "SELECT * FROM router_shares WHERE router_id=?1 AND share_id=?2 LIMIT 1",
            vec![
                crate::db::val(charge.string("router_id")),
                crate::db::val(charge.string("share_id")),
            ],
        )
        .await?;
    let request_object_key = charge.opt_string("request_object_key");
    let request_object_sha256 = charge.opt_string("request_object_sha256");
    let response_meta_object_key = charge.opt_string("response_meta_object_key");
    let response_meta_object_sha256 = charge.opt_string("response_meta_object_sha256");
    let request_json = read_json_object(&state, request_object_key.clone()).await;
    let response_meta_json = read_json_object(&state, response_meta_object_key.clone()).await;
    let request_object_expired = request_object_key.is_none() && request_object_sha256.is_some();
    let response_meta_object_expired =
        response_meta_object_key.is_none() && response_meta_object_sha256.is_some();
    let share_raw_json = router_share
        .as_ref()
        .and_then(|row| row.opt_string("raw_json"))
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok());
    let market_path = market_replay_path(&charge.string("app_type"), &charge.string("model"));
    let market_url = format!(
        "{}{}",
        state.config.market_public_base_url.trim_end_matches('/'),
        market_path
    );
    let share_url = share_raw_json.as_ref().and_then(extract_share_api_url);
    let sanitized_request_json = request_json.clone().map(sanitize_json);
    let market_curl = sanitized_request_json
        .as_ref()
        .map(|body| build_curl(&market_url, "Authorization: Bearer sk-cs-REPLACE_ME", body));
    let share_curl = match (share_url.as_ref(), sanitized_request_json.as_ref()) {
        (Some(url), Some(body)) => Some(build_curl(url, "X-Share-Token: REPLACE_ME", body)),
        _ => None,
    };

    Ok(Json(serde_json::json!({
        "charge": charge.to_json(),
        "attempts": attempts.into_iter().map(|row| row.to_json()).collect::<Vec<_>>(),
        "objects": objects.into_iter().map(|row| row.to_json()).collect::<Vec<_>>(),
        "routerShare": router_share.map(|row| row.to_json()),
        "requestObject": {
            "objectKey": request_object_key,
            "sha256": request_object_sha256,
            "expired": request_object_expired,
            "json": sanitized_request_json,
        },
        "responseMetaObject": {
            "objectKey": response_meta_object_key,
            "sha256": response_meta_object_sha256,
            "expired": response_meta_object_expired,
            "json": response_meta_json.map(sanitize_json),
        },
        "curl": {
            "marketReplay": market_curl,
            "shareReplay": share_curl,
        },
        "notes": {
            "rawResponseStored": false,
            "secretPlaceholders": true,
        }
    })))
}

async fn read_json_object(
    state: &AppState,
    object_key: Option<String>,
) -> Option<serde_json::Value> {
    let key = object_key?;
    let bytes = state.object_store.read_bytes(&key).await.ok()?;
    serde_json::from_slice::<serde_json::Value>(&bytes).ok()
}

fn market_replay_path(app_type: &str, model: &str) -> String {
    match app_type {
        "anthropic" | "claude" => "/v1/messages".to_string(),
        "gemini" => format!("/v1beta/models/{model}:generateContent"),
        _ => "/v1/chat/completions".to_string(),
    }
}

fn extract_share_api_url(raw: &serde_json::Value) -> Option<String> {
    raw.get("apiUrl")
        .or_else(|| raw.get("api_url"))
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn build_curl(url: &str, auth_header: &str, body: &serde_json::Value) -> String {
    let body_text = serde_json::to_string_pretty(body).unwrap_or_else(|_| "{}".to_string());
    format!(
        "curl '{}' \\\n  -H '{}' \\\n  -H 'content-type: application/json' \\\n  --data-binary '{}'",
        shell_single_quote(url),
        shell_single_quote(auth_header),
        shell_single_quote(&body_text)
    )
}

fn shell_single_quote(value: &str) -> String {
    value.replace('\'', r#"'\''"#)
}

fn sanitize_json(mut value: serde_json::Value) -> serde_json::Value {
    sanitize_json_in_place(&mut value);
    value
}

fn sanitize_json_in_place(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, nested) in map.iter_mut() {
                if is_sensitive_key(key) {
                    *nested = serde_json::Value::String("REDACTED".to_string());
                } else {
                    sanitize_json_in_place(nested);
                }
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                sanitize_json_in_place(item);
            }
        }
        _ => {}
    }
}

fn is_sensitive_key(key: &str) -> bool {
    matches!(
        key.to_ascii_lowercase().as_str(),
        "api_key" | "apikey" | "authorization" | "x-api-key" | "password" | "token" | "share_token"
    )
}

#[derive(Deserialize)]
pub struct SettleChargeManual {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
    pub reason: String,
}

pub async fn settle_charge_manual(
    State(state): State<AppState>,
    AdminPrincipal(admin): AdminPrincipal,
    Path(id): Path<Uuid>,
    Json(input): Json<SettleChargeManual>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if input.reason.trim().is_empty() {
        return Err(ApiError::bad_request(
            "missing_reason",
            "reason is required",
        ));
    }
    crate::proxy::admin_settle_needs_review_charge(
        &state,
        &admin.email,
        id,
        crate::proxy::UsageTokens {
            input_tokens: input.input_tokens,
            output_tokens: input.output_tokens,
            cache_read_tokens: input.cache_read_tokens.unwrap_or(0),
            cache_write_tokens: input.cache_write_tokens.unwrap_or(0),
            source: "admin_manual",
        },
        input.reason,
    )
    .await?;
    Ok(Json(serde_json::json!({"ok": true})))
}

#[derive(Deserialize)]
pub struct ReleaseCharge {
    pub reason: String,
}

pub async fn release_charge(
    State(state): State<AppState>,
    AdminPrincipal(admin): AdminPrincipal,
    Path(id): Path<Uuid>,
    Json(input): Json<ReleaseCharge>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if input.reason.trim().is_empty() {
        return Err(ApiError::bad_request(
            "missing_reason",
            "reason is required",
        ));
    }
    crate::proxy::admin_release_needs_review_charge(&state, &admin.email, id, input.reason).await?;
    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn earnings(
    State(state): State<AppState>,
    _admin: AdminPrincipal,
    Query(query): Query<LimitQuery>,
) -> Result<Json<crate::pagination::Page<serde_json::Value>>, ApiError> {
    let limit = crate::pagination::query_limit(query.limit);
    let fetch_limit = crate::pagination::fetch_limit(query.limit);
    let cursor = query.cursor.filter(|value| !value.trim().is_empty());
    let params = vec![crate::db::opt_val(cursor), crate::db::val(fetch_limit)];
    let limit_placeholder = 2;
    let rows = state
        .db()
        .query_all(
            &format!(
                r#"
                SELECT ta.owner_email, COALESCE(SUM(CAST(le.amount AS REAL)), 0) AS amount
                  FROM ledger_entries le
                  JOIN wallet_accounts ta ON ta.id = le.to_account_id
                 WHERE le.reference_type = 'request_charge'
                   AND ta.account_type = 'client_payable'
                   AND (?1 IS NULL OR ta.owner_email < ?1)
                 GROUP BY ta.owner_email
                 ORDER BY ta.owner_email DESC
                 LIMIT ?{limit_placeholder}
                "#
            ),
            params,
        )
        .await?;
    let items = rows
        .into_iter()
        .map(|row| {
            serde_json::json!({
                "owner_email": row.string("owner_email"),
                "amount": row.decimal("amount").to_string(),
            })
        })
        .collect::<Vec<_>>();
    Ok(Json(crate::pagination::Page::from_items(
        items,
        limit,
        |item| {
            item.get("owner_email")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string()
        },
    )))
}

pub async fn ledger(
    State(state): State<AppState>,
    _admin: AdminPrincipal,
    Query(q): Query<LimitQuery>,
) -> Result<Json<crate::pagination::Page<serde_json::Value>>, ApiError> {
    json_page(
        &state,
        "SELECT * FROM ledger_entries WHERE 1=1",
        "created_at",
        vec![],
        q,
    )
    .await
}

pub async fn money_events(
    State(state): State<AppState>,
    admin: AdminPrincipal,
    Query(query): Query<LimitQuery>,
) -> Result<Json<crate::pagination::Page<serde_json::Value>>, ApiError> {
    let _ = admin;
    let mut params = vec![];
    let cursor_clause = if let Some(cursor) = query
        .cursor
        .as_ref()
        .filter(|value| !value.trim().is_empty())
    {
        params.push(crate::db::val(cursor));
        " WHERE le.created_at < ?1"
    } else {
        ""
    };
    params.push(crate::db::val(crate::pagination::fetch_limit(query.limit)));
    let rows = state.db().query_all(
        &format!(r#"
        SELECT le.id, le.transaction_id, le.amount, le.currency, le.reference_type, le.reference_id,
               le.actor_type, le.actor_id, le.created_at,
               fa.account_type AS from_account_type, fa.owner_user_id AS from_user_id, fa.owner_email AS from_owner_email,
               ta.account_type AS to_account_type, ta.owner_user_id AS to_user_id, ta.owner_email AS to_owner_email,
               (
                 SELECT GROUP_CONCAT(object_key || '|' || content_sha256 || '|' || object_role, char(10))
                   FROM object_refs obj
                  WHERE obj.reference_type = le.reference_type
                    AND obj.reference_id = le.reference_id
               ) AS object_refs
          FROM ledger_entries le
          JOIN wallet_accounts fa ON fa.id = le.from_account_id
          JOIN wallet_accounts ta ON ta.id = le.to_account_id
         {cursor_clause}
         ORDER BY le.created_at DESC
         LIMIT ?{}
        "#, params.len()),
        params,
    ).await?;
    let items = rows.into_iter().map(money_event_json).collect::<Vec<_>>();
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

fn money_event_json(row: crate::db::DbRow) -> serde_json::Value {
    let from = row.string("from_account_type");
    let to = row.string("to_account_type");
    let to_owner_email = row.opt_string("to_owner_email");
    let reference_type = row.string("reference_type");
    let is_router_commission = reference_type == "request_charge"
        && to == "client_payable"
        && to_owner_email
            .as_deref()
            .is_some_and(|email| email.starts_with("router@"));
    let event_type = match (reference_type.as_str(), from.as_str(), to.as_str()) {
        _ if is_router_commission => "router_commission",
        ("topup", "payment_clearing", "user_cash") => "topup",
        ("topup", "payment_clearing", "fee_revenue") => "topup_fee",
        ("refund", "user_cash", "payment_clearing") => "refund",
        ("refund", "fee_revenue", "payment_clearing") => "refund_fee",
        ("request_charge", "user_cash", "user_reserved") => "usage_reserved",
        ("request_charge", "user_reserved", "client_payable") => "usage_charge",
        ("request_charge", "user_cash", "client_payable") => "usage_charge",
        ("request_charge", "risk_loss", "client_payable") => "usage_charge",
        ("request_charge", "user_reserved", "fee_revenue") => "platform_commission",
        ("request_charge", "user_cash", "fee_revenue") => "platform_commission",
        ("request_charge", "user_reserved", "user_cash") => "usage_release",
        ("payout_request", "client_payable", "payout_reserved") => "payout_reserved",
        ("payout_request", "payout_reserved", "settlement_paid") => "payout_paid",
        ("payout_request", "payout_reserved", "fee_revenue") => "payout_fee",
        ("payout_request", "payout_reserved", "client_payable") => "payout_released",
        ("adjustment", _, "client_payable") => "manual_adjustment_credit",
        ("adjustment", "client_payable", _) => "manual_adjustment_debit",
        _ => "ledger_transfer",
    };
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
    let mut value = row.to_json();
    if let Some(object) = value.as_object_mut() {
        object.insert(
            "event_type".to_string(),
            serde_json::Value::String(event_type.to_string()),
        );
        object.insert(
            "object_refs".to_string(),
            serde_json::Value::Array(object_refs),
        );
    }
    value
}

pub async fn money_overview(
    State(state): State<AppState>,
    _admin: AdminPrincipal,
) -> Result<Json<serde_json::Value>, ApiError> {
    let account_rows = state
        .db()
        .query_all(
            r#"
            SELECT account_type, COALESCE(SUM(CAST(balance AS REAL)), 0) AS balance
              FROM wallet_accounts
             GROUP BY account_type
            "#,
            vec![],
        )
        .await?;
    let mut balances = serde_json::Map::new();
    for row in account_rows {
        balances.insert(
            row.string("account_type"),
            serde_json::json!(row.string("balance")),
        );
    }
    let today = admin_today_start_utc(&state).await?;
    let router_commission_owner_email = state.config.router_commission_owner_email();
    let ledger_report = cached_ledger_report(&state).await?;
    Ok(Json(serde_json::json!({
        "ledgerOk": ledger_report.get("ok").and_then(|value| value.as_bool()).unwrap_or(false),
        "ledger": ledger_report,
        "balances": balances,
        "userCashUsd": balances.get("user_cash").cloned().unwrap_or_else(|| serde_json::json!("0")),
        "userReservedUsd": balances.get("user_reserved").cloned().unwrap_or_else(|| serde_json::json!("0")),
        "providerPayableUsd": balances.get("client_payable").cloned().unwrap_or_else(|| serde_json::json!("0")),
        "routerPayableUsd": admin_sum_with_param(&state, "SELECT COALESCE(SUM(CAST(balance AS REAL)), 0) AS amount FROM wallet_accounts WHERE account_type = 'client_payable' AND owner_email = ?1", &router_commission_owner_email).await?,
        "payoutReservedUsd": balances.get("payout_reserved").cloned().unwrap_or_else(|| serde_json::json!("0")),
        "feeRevenueUsd": balances.get("fee_revenue").cloned().unwrap_or_else(|| serde_json::json!("0")),
        "topupFeeRevenueUsd": admin_sum(&state, "SELECT COALESCE(SUM(CASE WHEN ta.account_type = 'fee_revenue' THEN CAST(le.amount AS REAL) WHEN fa.account_type = 'fee_revenue' THEN -CAST(le.amount AS REAL) ELSE 0 END), 0) AS amount FROM ledger_entries le LEFT JOIN wallet_accounts fa ON fa.id = le.from_account_id LEFT JOIN wallet_accounts ta ON ta.id = le.to_account_id WHERE le.reference_type IN ('topup','refund') AND (fa.account_type = 'fee_revenue' OR ta.account_type = 'fee_revenue')").await?,
        "platformCommissionRevenueUsd": admin_sum(&state, "SELECT COALESCE(SUM(CAST(le.amount AS REAL)), 0) AS amount FROM ledger_entries le JOIN wallet_accounts ta ON ta.id = le.to_account_id WHERE le.reference_type = 'request_charge' AND ta.account_type = 'fee_revenue'").await?,
        "payoutFeeRevenueUsd": admin_sum(&state, "SELECT COALESCE(SUM(CAST(le.amount AS REAL)), 0) AS amount FROM ledger_entries le JOIN wallet_accounts ta ON ta.id = le.to_account_id WHERE le.reference_type = 'payout_request' AND ta.account_type = 'fee_revenue'").await?,
        "riskLossUsd": balances.get("risk_loss").cloned().unwrap_or_else(|| serde_json::json!("0")),
        "platformCommissionBps": state.config.market_platform_commission_bps + state.config.market_router_commission_bps,
        "marketCommissionBps": state.config.market_platform_commission_bps,
        "routerCommissionBps": state.config.market_router_commission_bps,
        "routerCommissionOwnerEmail": router_commission_owner_email,
        "pendingTopups": admin_count(&state, "SELECT COUNT(*) AS count FROM topup_orders WHERE status='pending'").await?,
        "needsReviewCharges": admin_count(&state, "SELECT COUNT(*) AS count FROM request_charges WHERE status='needs_review'").await?,
        "pendingPayouts": admin_count(&state, "SELECT COUNT(*) AS count FROM payout_requests WHERE status IN ('pending','processing','needs_review')").await?,
        "todayUsageUsd": admin_sum_since(&state, "SELECT COALESCE(SUM(CAST(le.amount AS REAL)), 0) AS amount FROM ledger_entries le JOIN wallet_accounts fa ON fa.id = le.from_account_id JOIN wallet_accounts ta ON ta.id = le.to_account_id WHERE le.reference_type = 'request_charge' AND fa.account_type IN ('user_reserved','user_cash') AND ta.account_type IN ('client_payable','fee_revenue') AND le.created_at >= ?1", &today).await?,
        "todayTopupsUsd": admin_sum_since(&state, "SELECT COALESCE(SUM(CAST(gross_amount AS REAL)), 0) AS amount FROM topup_orders WHERE status = 'paid' AND paid_at >= ?1", &today).await?,
    })))
}

pub async fn summary(
    State(state): State<AppState>,
    _admin: AdminPrincipal,
) -> Result<Json<serde_json::Value>, ApiError> {
    let value = cached_json(&ADMIN_SUMMARY_CACHE, Duration::from_secs(10), || async {
        let today = admin_today_start_utc(&state).await?;
        let ledger_report = cached_ledger_report(&state).await?;
        Ok(serde_json::json!({
            "ledgerOk": ledger_report.get("ok").and_then(|value| value.as_bool()).unwrap_or(false),
            "openTickets": admin_count(&state, "SELECT COUNT(*) AS count FROM tickets WHERE status IN ('open','waiting_admin')").await?,
            "pendingTopups": admin_count(&state, "SELECT COUNT(*) AS count FROM topup_orders WHERE status='pending'").await?,
            "needsReviewCharges": admin_count(&state, "SELECT COUNT(*) AS count FROM request_charges WHERE status='needs_review'").await?,
            "pendingPayouts": admin_count(&state, "SELECT COUNT(*) AS count FROM payout_requests WHERE status IN ('pending','processing','needs_review')").await?,
            "todayUsageUsd": admin_sum_since(&state, "SELECT COALESCE(SUM(CAST(le.amount AS REAL)), 0) AS amount FROM ledger_entries le JOIN wallet_accounts fa ON fa.id = le.from_account_id JOIN wallet_accounts ta ON ta.id = le.to_account_id WHERE le.reference_type = 'request_charge' AND fa.account_type IN ('user_reserved','user_cash') AND ta.account_type IN ('client_payable','fee_revenue') AND le.created_at >= ?1", &today).await?,
            "todayTopupsUsd": admin_sum_since(&state, "SELECT COALESCE(SUM(CAST(gross_amount AS REAL)), 0) AS amount FROM topup_orders WHERE status = 'paid' AND paid_at >= ?1", &today).await?,
        }))
    })
    .await?;
    Ok(Json(value))
}

async fn admin_count(state: &AppState, sql: &str) -> Result<i64, ApiError> {
    Ok(state.db().query_one(sql, vec![]).await?.i64("count"))
}

async fn admin_sum(state: &AppState, sql: &str) -> Result<String, ApiError> {
    Ok(state.db().query_one(sql, vec![]).await?.string("amount"))
}

async fn admin_sum_with_param(
    state: &AppState,
    sql: &str,
    value: &str,
) -> Result<String, ApiError> {
    Ok(state
        .db()
        .query_one(sql, vec![crate::db::val(value)])
        .await?
        .string("amount"))
}

async fn admin_sum_since(state: &AppState, sql: &str, since: &str) -> Result<String, ApiError> {
    Ok(state
        .db()
        .query_one(sql, vec![crate::db::val(since)])
        .await?
        .string("amount"))
}

async fn admin_today_start_utc(state: &AppState) -> Result<String, ApiError> {
    let offset_minutes = state
        .db()
        .query_optional(
            "SELECT value FROM app_settings WHERE key='time_zone_offset_minutes'",
            vec![],
        )
        .await?
        .and_then(|row| row.string("value").parse::<i64>().ok())
        .unwrap_or(480)
        .clamp(-12 * 60, 14 * 60);
    let local_now = chrono::Utc::now() + chrono::Duration::minutes(offset_minutes);
    let local_midnight = local_now.date_naive().and_hms_opt(0, 0, 0).unwrap();
    Ok((local_midnight - chrono::Duration::minutes(offset_minutes))
        .and_utc()
        .to_rfc3339())
}

pub async fn ledger_check(
    State(state): State<AppState>,
    _admin: AdminPrincipal,
) -> Result<Json<serde_json::Value>, ApiError> {
    Ok(Json(cached_ledger_report(&state).await?))
}

pub async fn settlements(
    State(state): State<AppState>,
    _admin: AdminPrincipal,
    Query(query): Query<LimitQuery>,
) -> Result<Json<crate::pagination::Page<serde_json::Value>>, ApiError> {
    json_page(
        &state,
        "SELECT * FROM payout_requests WHERE status = 'paid'",
        "paid_at",
        vec![],
        query,
    )
    .await
}

pub async fn payout_requests(
    State(state): State<AppState>,
    _admin: AdminPrincipal,
    Query(query): Query<LimitQuery>,
) -> Result<Json<crate::pagination::Page<serde_json::Value>>, ApiError> {
    json_page(
        &state,
        "SELECT * FROM payout_requests WHERE 1=1",
        "created_at",
        vec![],
        query,
    )
    .await
}

pub async fn payout_request(
    State(state): State<AppState>,
    _admin: AdminPrincipal,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let payout = state
        .db()
        .query_one(
            "SELECT * FROM payout_requests WHERE id=?1",
            vec![crate::db::uuid_val(id)],
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
            "SELECT * FROM object_refs WHERE reference_type='payout_request' AND reference_id=?1 ORDER BY created_at",
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

pub async fn tickets(
    State(state): State<AppState>,
    _admin: AdminPrincipal,
    Query(query): Query<LimitQuery>,
) -> Result<Json<crate::pagination::Page<serde_json::Value>>, ApiError> {
    json_page(
        &state,
        r#"SELECT t.*,
            (SELECT MAX(created_at) FROM ticket_messages tm
              WHERE tm.ticket_id = t.id AND tm.sender_type = 'admin' AND tm.internal_note = 0) AS last_admin_at,
            (SELECT MAX(created_at) FROM ticket_messages tm
              WHERE tm.ticket_id = t.id AND tm.sender_type = 'user') AS last_user_at,
            (SELECT sender_type FROM ticket_messages tm
              WHERE tm.ticket_id = t.id AND tm.internal_note = 0
              ORDER BY created_at DESC LIMIT 1) AS last_external_sender,
            CASE
              WHEN t.status = 'waiting_user' THEN 'user'
              WHEN t.status = 'waiting_admin' THEN 'admin'
              WHEN t.status = 'open' THEN 'admin'
              ELSE NULL
            END AS waiting_for,
            CASE WHEN t.status = 'waiting_user' THEN datetime((SELECT MAX(created_at) FROM ticket_messages tm
              WHERE tm.ticket_id = t.id AND tm.sender_type = 'admin' AND tm.internal_note = 0), '+7 days') END AS auto_close_at
           FROM tickets t WHERE 1=1"#,
        "updated_at",
        vec![],
        query,
    )
    .await
}

pub async fn ticket(
    State(state): State<AppState>,
    _admin: AdminPrincipal,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let ticket = state
        .db()
        .query_one(
            "SELECT * FROM tickets WHERE id = ?1",
            vec![crate::db::uuid_val(id)],
        )
        .await?;
    let messages = state
        .db()
        .query_all(
            "SELECT * FROM ticket_messages WHERE ticket_id = ?1 ORDER BY created_at",
            vec![crate::db::uuid_val(id)],
        )
        .await?;
    let attachments = crate::support::ticket_attachments_json(&state, id).await?;
    let auto_close_after_secs: i64 = 7 * 24 * 60 * 60;
    let user_meta = if let Some(creator_user_id) = ticket.opt_uuid("creator_user_id") {
        let user = state
            .db()
            .query_optional(
                "SELECT id, email, status, last_login_at, created_at FROM users WHERE id = ?1",
                vec![crate::db::uuid_val(creator_user_id)],
            )
            .await?;
        let session = state
            .db()
            .query_optional(
                "SELECT last_seen_at, last_seen_ip, ip_country, user_agent FROM web_sessions WHERE user_id = ?1 ORDER BY last_seen_at DESC LIMIT 1",
                vec![crate::db::uuid_val(creator_user_id)],
            )
            .await?;
        let balances = state
            .db()
            .query_all(
                "SELECT account_type, balance FROM wallet_accounts WHERE owner_user_id = ?1 ORDER BY account_type",
                vec![crate::db::uuid_val(creator_user_id)],
            )
            .await?;
        let recent_topup = state
            .db()
            .query_optional(
                "SELECT id, status, net_amount, created_at, paid_at FROM topup_orders WHERE user_id = ?1 ORDER BY created_at DESC LIMIT 1",
                vec![crate::db::uuid_val(creator_user_id)],
            )
            .await?;
        Some(serde_json::json!({
            "user": user.map(|row| row.to_json()),
            "session": session.map(|row| row.to_json()),
            "balances": balances.into_iter().map(|row| row.to_json()).collect::<Vec<_>>(),
            "recent_topup": recent_topup.map(|row| row.to_json())
        }))
    } else {
        None
    };

    let last_admin_message = messages
        .iter()
        .filter(|row| row.string("sender_type") == "admin" && !row.bool("internal_note"))
        .last();
    let auto_close_at = if ticket.string("status") == "waiting_user" {
        last_admin_message.map(|row| {
            row.datetime("created_at") + chrono::Duration::seconds(auto_close_after_secs)
        })
    } else {
        None
    };

    Ok(Json(serde_json::json!({
        "ticket": ticket.to_json(),
        "messages": messages.into_iter().map(|r| r.to_json()).collect::<Vec<_>>(),
        "attachments": attachments,
        "user_meta": user_meta,
        "auto_close_at": auto_close_at,
        "auto_close_after_secs": auto_close_after_secs,
    })))
}

pub async fn audit(
    State(state): State<AppState>,
    _admin: AdminPrincipal,
    Query(query): Query<LimitQuery>,
) -> Result<Json<crate::pagination::Page<serde_json::Value>>, ApiError> {
    json_page(
        &state,
        "SELECT * FROM admin_audit WHERE 1=1",
        "created_at",
        vec![],
        query,
    )
    .await
}

pub async fn settings(
    State(state): State<AppState>,
    _admin: AdminPrincipal,
) -> Result<Json<serde_json::Value>, ApiError> {
    let time_zone_offset_minutes = read_time_zone_offset_minutes(&state).await?;
    let admin_table_page_size = read_admin_table_page_size(&state).await?;
    let footer_links = read_footer_links(&state).await?;
    Ok(Json(serde_json::json!({
        "timeZoneOffsetMinutes": time_zone_offset_minutes,
        "adminTablePageSize": admin_table_page_size,
        "env": env_settings_payload()?,
        "footerLinks": footer_links,
        "footerIcons": FOOTER_ICONS,
    })))
}

pub async fn read_time_zone_offset_minutes(state: &AppState) -> Result<i64, ApiError> {
    Ok(state
        .db()
        .query_optional(
            "SELECT value FROM app_settings WHERE key='time_zone_offset_minutes'",
            vec![],
        )
        .await?
        .and_then(|row| row.string("value").parse::<i64>().ok())
        .unwrap_or(480))
}

pub async fn read_admin_table_page_size(state: &AppState) -> Result<i64, ApiError> {
    Ok(state
        .db()
        .query_optional(
            "SELECT value FROM app_settings WHERE key='admin_table_page_size'",
            vec![],
        )
        .await?
        .and_then(|row| row.string("value").parse::<i64>().ok())
        .filter(|value| (1..=500).contains(value))
        .unwrap_or(20))
}

#[derive(Deserialize)]
pub struct UpdateSettings {
    #[serde(rename = "timeZoneOffsetMinutes")]
    pub time_zone_offset_minutes: Option<i64>,
    #[serde(rename = "adminTablePageSize")]
    pub admin_table_page_size: Option<i64>,
}

pub async fn update_settings(
    State(state): State<AppState>,
    AdminPrincipal(admin): AdminPrincipal,
    Json(input): Json<UpdateSettings>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if input.time_zone_offset_minutes.is_none() && input.admin_table_page_size.is_none() {
        return Err(ApiError::bad_request(
            "empty_settings_update",
            "at least one setting is required",
        ));
    }
    if let Some(offset) = input.time_zone_offset_minutes {
        if !(-12 * 60..=14 * 60).contains(&offset) {
            return Err(ApiError::bad_request(
                "invalid_time_zone_offset",
                "timeZoneOffsetMinutes must be between UTC-12 and UTC+14",
            ));
        }
    }
    if let Some(page_size) = input.admin_table_page_size {
        if !(1..=500).contains(&page_size) {
            return Err(ApiError::bad_request(
                "invalid_admin_table_page_size",
                "adminTablePageSize must be between 1 and 500",
            ));
        }
    }
    let tx = state.db().begin_immediate().await?;
    if let Some(offset) = input.time_zone_offset_minutes {
        tx.execute(
            r#"
            INSERT INTO app_settings (key, value, updated_at)
            VALUES ('time_zone_offset_minutes', ?1, ?2)
            ON CONFLICT(key) DO UPDATE SET value=excluded.value, updated_at=excluded.updated_at
            "#,
            vec![
                crate::db::val(offset.to_string()),
                crate::db::val(crate::db::now_string()),
            ],
        )
        .await?;
    }
    if let Some(page_size) = input.admin_table_page_size {
        tx.execute(
            r#"
            INSERT INTO app_settings (key, value, updated_at)
            VALUES ('admin_table_page_size', ?1, ?2)
            ON CONFLICT(key) DO UPDATE SET value=excluded.value, updated_at=excluded.updated_at
            "#,
            vec![
                crate::db::val(page_size.to_string()),
                crate::db::val(crate::db::now_string()),
            ],
        )
        .await?;
    }
    audit_tx(
        &tx,
        &admin.email,
        "settings.update",
        "app_settings",
        Uuid::nil(),
        serde_json::json!({
            "timeZoneOffsetMinutes": input.time_zone_offset_minutes,
            "adminTablePageSize": input.admin_table_page_size,
        }),
    )
    .await?;
    tx.commit().await?;
    let time_zone_offset_minutes = read_time_zone_offset_minutes(&state).await?;
    let admin_table_page_size = read_admin_table_page_size(&state).await?;
    Ok(Json(serde_json::json!({
        "ok": true,
        "timeZoneOffsetMinutes": time_zone_offset_minutes,
        "adminTablePageSize": admin_table_page_size,
    })))
}

#[derive(Deserialize)]
pub struct UpdateEnvSettings {
    pub values: BTreeMap<String, String>,
}

pub async fn update_env_settings(
    State(_state): State<AppState>,
    AdminPrincipal(admin): AdminPrincipal,
    Json(input): Json<UpdateEnvSettings>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let allowed = env_fields()
        .into_iter()
        .map(|field| field.key)
        .collect::<std::collections::HashSet<_>>();
    for key in input.values.keys() {
        if !allowed.contains(key.as_str()) {
            return Err(ApiError::bad_request(
                "invalid_env_key",
                format!("{key} is not configurable"),
            ));
        }
    }

    let env_file = Config::ensure_default_env_file()?;
    write_env_values(&env_file, &input.values)?;
    write_audit(
        &_state,
        &admin.email,
        "settings.env_update",
        "env",
        Uuid::nil(),
    )
    .await?;
    Ok(Json(serde_json::json!({
        "ok": true,
        "env": env_settings_payload()?,
    })))
}

pub const FOOTER_ICONS: &[&str] = &[
    "link", "twitter", "github", "globe", "book", "activity", "scroll",
];
pub const FOOTER_LINKS_LIMIT: usize = 24;

#[derive(serde::Serialize, Deserialize, Clone)]
pub struct FooterLink {
    #[serde(default, rename = "labelZh")]
    pub label_zh: String,
    #[serde(default, rename = "labelEn")]
    pub label_en: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub icon: String,
}

#[derive(Deserialize)]
pub struct UpdateFooterLinks {
    pub items: Vec<FooterLink>,
}

pub fn default_footer_links() -> Vec<FooterLink> {
    vec![
        FooterLink {
            label_zh: "X / Twitter".into(),
            label_en: "X / Twitter".into(),
            url: "#".into(),
            icon: "twitter".into(),
        },
        FooterLink {
            label_zh: "GitHub".into(),
            label_en: "GitHub".into(),
            url: "#".into(),
            icon: "github".into(),
        },
        FooterLink {
            label_zh: "官网".into(),
            label_en: "Website".into(),
            url: "#".into(),
            icon: "globe".into(),
        },
        FooterLink {
            label_zh: "文档".into(),
            label_en: "Docs".into(),
            url: "#".into(),
            icon: "book".into(),
        },
        FooterLink {
            label_zh: "状态页".into(),
            label_en: "Status".into(),
            url: "#".into(),
            icon: "activity".into(),
        },
        FooterLink {
            label_zh: "更新日志".into(),
            label_en: "Changelog".into(),
            url: "#".into(),
            icon: "scroll".into(),
        },
    ]
}

pub async fn read_footer_links(state: &AppState) -> Result<Vec<FooterLink>, ApiError> {
    let row = state
        .db()
        .query_optional(
            "SELECT value FROM app_settings WHERE key='footer_links'",
            vec![],
        )
        .await?;
    if let Some(row) = row {
        let raw = row.string("value");
        match serde_json::from_str::<Vec<FooterLink>>(&raw) {
            Ok(items) if !items.is_empty() => return Ok(items),
            Ok(_) => {}
            Err(_) => {
                tracing::warn!("footer_links app_settings value failed to parse, using defaults");
            }
        }
    }
    Ok(default_footer_links())
}

fn validate_footer_link(link: &FooterLink, idx: usize) -> Result<FooterLink, ApiError> {
    let label_zh = link.label_zh.trim().to_string();
    let label_en = link.label_en.trim().to_string();
    if label_zh.is_empty() && label_en.is_empty() {
        return Err(ApiError::bad_request(
            "invalid_footer_link",
            format!("item {idx}: at least one of labelZh / labelEn is required"),
        ));
    }
    let url = link.url.trim().to_string();
    if url.is_empty() {
        return Err(ApiError::bad_request(
            "invalid_footer_link",
            format!("item {idx}: url is required"),
        ));
    }
    let scheme_ok = url.starts_with("https://")
        || url.starts_with("http://")
        || url.starts_with("/")
        || url.starts_with("mailto:")
        || url == "#";
    if !scheme_ok {
        return Err(ApiError::bad_request(
            "invalid_footer_link",
            format!("item {idx}: url scheme not allowed"),
        ));
    }
    let icon = if link.icon.trim().is_empty() {
        "link".to_string()
    } else {
        link.icon.trim().to_string()
    };
    if !FOOTER_ICONS.contains(&icon.as_str()) {
        return Err(ApiError::bad_request(
            "invalid_footer_link",
            format!("item {idx}: icon '{icon}' is not a known icon"),
        ));
    }
    Ok(FooterLink {
        label_zh,
        label_en,
        url,
        icon,
    })
}

pub async fn update_footer_links(
    State(state): State<AppState>,
    AdminPrincipal(admin): AdminPrincipal,
    Json(input): Json<UpdateFooterLinks>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if input.items.len() > FOOTER_LINKS_LIMIT {
        return Err(ApiError::bad_request(
            "too_many_footer_links",
            format!("at most {FOOTER_LINKS_LIMIT} items are allowed"),
        ));
    }
    let cleaned = input
        .items
        .iter()
        .enumerate()
        .map(|(idx, link)| validate_footer_link(link, idx))
        .collect::<Result<Vec<_>, _>>()?;
    let value = serde_json::to_string(&cleaned).context("serialize footer_links")?;
    let tx = state.db().begin_immediate().await?;
    tx.execute(
        r#"
        INSERT INTO app_settings (key, value, updated_at)
        VALUES ('footer_links', ?1, ?2)
        ON CONFLICT(key) DO UPDATE SET value=excluded.value, updated_at=excluded.updated_at
        "#,
        vec![
            crate::db::val(value),
            crate::db::val(crate::db::now_string()),
        ],
    )
    .await?;
    audit_tx(
        &tx,
        &admin.email,
        "settings.footer_update",
        "app_settings",
        Uuid::nil(),
        serde_json::json!({"count": cleaned.len()}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(serde_json::json!({
        "ok": true,
        "footerLinks": cleaned,
    })))
}

#[derive(Clone, Copy)]
struct EnvField {
    key: &'static str,
    category: &'static str,
    label_zh: &'static str,
    label_en: &'static str,
    description_zh: &'static str,
    description_en: &'static str,
    kind: &'static str,
    secret: bool,
    required: bool,
    default_value: &'static str,
    placeholder: &'static str,
    unit: &'static str,
}

fn env_fields() -> Vec<EnvField> {
    vec![
        EnvField {
            key: "MARKET_HTTP_ADDR",
            category: "runtime",
            label_zh: "HTTP 监听地址",
            label_en: "HTTP listen address",
            description_zh: "market 进程监听的 host:port，必须是合法的 SocketAddr。",
            description_en: "host:port the market process listens on. Must be a valid SocketAddr.",
            kind: "text",
            secret: false,
            required: true,
            default_value: "0.0.0.0:8080",
            placeholder: "0.0.0.0:8080",
            unit: "",
        },
        EnvField {
            key: "MARKET_TUNNEL_ENABLED",
            category: "runtime",
            label_zh: "启用 Router SSH 隧道",
            label_en: "Enable router SSH tunnel",
            description_zh: "开启后 market 通过 cc-switch-router 的 market 子域反向暴露。",
            description_en: "When on, market is exposed via the cc-switch-router market subdomain.",
            kind: "bool",
            secret: false,
            required: true,
            default_value: "true",
            placeholder: "",
            unit: "",
        },
        EnvField {
            key: "RUST_LOG",
            category: "runtime",
            label_zh: "Rust 日志过滤器",
            label_en: "Rust log filter",
            description_zh: "标准 tracing/env_logger 过滤语法，控制各模块日志级别。",
            description_en: "Standard tracing/env_logger filter syntax controlling per-module log levels.",
            kind: "text",
            secret: false,
            required: true,
            default_value: "cc_switch_market=info,tower_http=info,axum=info",
            placeholder: "cc_switch_market=info,tower_http=info,axum=info",
            unit: "",
        },
        EnvField {
            key: "MARKET_SESSION_COOKIE_NAME",
            category: "auth",
            label_zh: "会话 Cookie 名称",
            label_en: "Session cookie name",
            description_zh: "登录态 Cookie 名，仅允许字母数字、下划线和连字符。",
            description_en: "Login session cookie name. Letters, digits, underscore and hyphen only.",
            kind: "text",
            secret: false,
            required: true,
            default_value: "cc_switch_market_session",
            placeholder: "cc_switch_market_session",
            unit: "",
        },
        EnvField {
            key: "MARKET_SESSION_COOKIE_SECRET",
            category: "auth",
            label_zh: "会话 Cookie 密钥",
            label_en: "Session cookie secret",
            description_zh: "用于签发/校验会话 Cookie 的密钥，至少 24 字符。生产环境必须替换默认值。",
            description_en: "Secret used to sign/verify session cookies. At least 24 chars; default is unsafe in prod.",
            kind: "password",
            secret: true,
            required: true,
            default_value: "change-me-market-session-secret",
            placeholder: "",
            unit: "",
        },
        EnvField {
            key: "MARKET_SESSION_TTL_SECS",
            category: "auth",
            label_zh: "会话有效期",
            label_en: "Session TTL",
            description_zh: "登录会话的存活秒数，最小 300 秒。默认 30 天。",
            description_en: "Login session lifetime in seconds. Minimum 300; default is 30 days.",
            kind: "number",
            secret: false,
            required: true,
            default_value: "2592000",
            placeholder: "2592000",
            unit: "secs",
        },
        EnvField {
            key: "MARKET_ADMIN_EMAILS",
            category: "auth",
            label_zh: "管理员邮箱列表",
            label_en: "Admin emails",
            description_zh: "可访问 /admin 的邮箱列表，逗号分隔。生产环境必须替换默认值。",
            description_en: "Comma-separated emails granted /admin access. Replace the default in production.",
            kind: "email_list",
            secret: false,
            required: true,
            default_value: "admin@example.com",
            placeholder: "you@example.com, ops@example.com",
            unit: "",
        },
        EnvField {
            key: "CLOUDFLARE_TURNSTILE_ENABLED",
            category: "cloudflare",
            label_zh: "启用 Turnstile",
            label_en: "Enable Turnstile",
            description_zh: "开启后，邮箱登录发送验证码前会要求 Cloudflare Turnstile 人机验证。默认关闭。",
            description_en: "When enabled, email login requires Cloudflare Turnstile before sending a verification code. Disabled by default.",
            kind: "bool",
            secret: false,
            required: true,
            default_value: "false",
            placeholder: "",
            unit: "",
        },
        EnvField {
            key: "CLOUDFLARE_TURNSTILE_SITE_KEY",
            category: "cloudflare",
            label_zh: "Turnstile Site Key",
            label_en: "Turnstile site key",
            description_zh: "Cloudflare Turnstile 站点密钥。启用 Turnstile 时必须配置。",
            description_en: "Cloudflare Turnstile site key. Required when Turnstile is enabled.",
            kind: "text",
            secret: false,
            required: false,
            default_value: "",
            placeholder: "0x4AAAA...",
            unit: "",
        },
        EnvField {
            key: "CLOUDFLARE_TURNSTILE_SECRET_KEY",
            category: "cloudflare",
            label_zh: "Turnstile Secret Key",
            label_en: "Turnstile secret key",
            description_zh: "Cloudflare Turnstile 服务端校验密钥。启用 Turnstile 时必须配置。",
            description_en: "Cloudflare Turnstile server-side verification key. Required when Turnstile is enabled.",
            kind: "password",
            secret: true,
            required: false,
            default_value: "",
            placeholder: "",
            unit: "",
        },
        EnvField {
            key: "MARKET_MIN_REQUEST_BALANCE",
            category: "billing",
            label_zh: "最低可发起请求余额",
            label_en: "Minimum request balance",
            description_zh: "用户余额低于此值时拒绝新请求，单位 USD，必须 ≥ 0。",
            description_en: "Reject new requests when user balance falls below this USD threshold. Must be ≥ 0.",
            kind: "number",
            secret: false,
            required: true,
            default_value: "0.10",
            placeholder: "0.10",
            unit: "USD",
        },
        EnvField {
            key: "MARKET_PLATFORM_COMMISSION_BPS",
            category: "billing",
            label_zh: "Market 抽成（基点）",
            label_en: "Market commission",
            description_zh: "Market 抽成，单位 bps（10000 = 100%）。允许范围 0–10000，默认 1000 = 10%。",
            description_en: "Market fee in basis points (10000 = 100%). Range 0–10000; default 1000 = 10%.",
            kind: "number",
            secret: false,
            required: true,
            default_value: "1000",
            placeholder: "1000",
            unit: "bps",
        },
        EnvField {
            key: "MARKET_ROUTER_COMMISSION_BPS",
            category: "billing",
            label_zh: "Router 抽成（基点）",
            label_en: "Router commission",
            description_zh: "Router 抽成，单位 bps（10000 = 100%）。默认 500 = 5%，会进入 router@router-host 的 Provider 余额。",
            description_en: "Router fee in basis points (10000 = 100%). Default 500 = 5%; posted to router@router-host provider balance.",
            kind: "number",
            secret: false,
            required: true,
            default_value: "500",
            placeholder: "500",
            unit: "bps",
        },
        EnvField {
            key: "MARKET_SHARE_STICKY_ENABLED",
            category: "routing",
            label_zh: "Share 粘性路由",
            label_en: "Share sticky routing",
            description_zh: "开启后同一用户、模型和协议族短时间内优先使用同一个 share，以提高缓存命中率。",
            description_en: "Prefer the same share for the same user, model, and protocol family to improve cache hit rate.",
            kind: "boolean",
            secret: false,
            required: true,
            default_value: "true",
            placeholder: "true",
            unit: "",
        },
        EnvField {
            key: "MARKET_SHARE_STICKY_TTL_SECONDS",
            category: "routing",
            label_zh: "Share 粘性时长",
            label_en: "Share sticky TTL",
            description_zh: "Share 粘性绑定有效秒数。默认 1800 秒；设为 0 等于不写入新粘性记录。",
            description_en: "Sticky route lifetime in seconds. Default 1800; set 0 to avoid writing new sticky routes.",
            kind: "number",
            secret: false,
            required: true,
            default_value: "1800",
            placeholder: "1800",
            unit: "s",
        },
        EnvField {
            key: "MARKET_SQLITE_PATH",
            category: "database",
            label_zh: "本地 SQLite 路径",
            label_en: "Local SQLite path",
            description_zh: "未配置 Turso 时使用的本地 SQLite 文件路径。",
            description_en: "Local SQLite file path when Turso is not configured.",
            kind: "path",
            secret: false,
            required: true,
            default_value: "$HOME/.config/cc-switch-market/cc-switch-market.db",
            placeholder: "$HOME/.config/cc-switch-market/cc-switch-market.db",
            unit: "",
        },
        EnvField {
            key: "TURSO_DATABASE_URL",
            category: "database",
            label_zh: "Turso 数据库 URL",
            label_en: "Turso database URL",
            description_zh: "可选。配置后启用 Turso 远程数据库 + 本地副本，必须以 libsql:// 开头。",
            description_en: "Optional. Enables Turso remote DB + local replica. Must start with libsql://.",
            kind: "url",
            secret: false,
            required: false,
            default_value: "",
            placeholder: "libsql://your-db.turso.io",
            unit: "",
        },
        EnvField {
            key: "TURSO_AUTH_TOKEN",
            category: "database",
            label_zh: "Turso 认证 Token",
            label_en: "Turso auth token",
            description_zh: "配置 TURSO_DATABASE_URL 时必填。",
            description_en: "Required when TURSO_DATABASE_URL is set.",
            kind: "password",
            secret: true,
            required: false,
            default_value: "",
            placeholder: "",
            unit: "",
        },
        EnvField {
            key: "TURSO_REPLICA_PATH",
            category: "database",
            label_zh: "Turso 副本路径",
            label_en: "Turso replica path",
            description_zh: "Turso 嵌入式副本的本地落盘路径。",
            description_en: "Local on-disk path for the Turso embedded replica.",
            kind: "path",
            secret: false,
            required: true,
            default_value: "$HOME/.config/cc-switch-market/turso-replica.db",
            placeholder: "$HOME/.config/cc-switch-market/turso-replica.db",
            unit: "",
        },
        EnvField {
            key: "TURSO_SYNC_INTERVAL_SECS",
            category: "database",
            label_zh: "Turso 同步间隔",
            label_en: "Turso sync interval",
            description_zh: "副本拉取远端变更的轮询间隔。",
            description_en: "How often the replica pulls remote changes.",
            kind: "number",
            secret: false,
            required: true,
            default_value: "300",
            placeholder: "300",
            unit: "secs",
        },
        EnvField {
            key: "TURSO_BACKUP_ENABLED",
            category: "database",
            label_zh: "启用 Turso 备份",
            label_en: "Enable Turso backup",
            description_zh: "开启后定期把 Turso 数据库快照写入对象存储。",
            description_en: "Periodically snapshot the Turso database to object storage.",
            kind: "bool",
            secret: false,
            required: true,
            default_value: "true",
            placeholder: "",
            unit: "",
        },
        EnvField {
            key: "TURSO_BACKUP_INTERVAL_SECS",
            category: "database",
            label_zh: "备份间隔",
            label_en: "Backup interval",
            description_zh: "Turso 备份的执行周期。",
            description_en: "How often the Turso backup runs.",
            kind: "number",
            secret: false,
            required: true,
            default_value: "3600",
            placeholder: "3600",
            unit: "secs",
        },
        EnvField {
            key: "TURSO_BACKUP_RETENTION_DAYS",
            category: "database",
            label_zh: "备份保留天数",
            label_en: "Backup retention",
            description_zh: "保留多少天的旧备份，超期自动清理。",
            description_en: "How many days of old backups to keep before cleanup.",
            kind: "number",
            secret: false,
            required: true,
            default_value: "7",
            placeholder: "7",
            unit: "days",
        },
        EnvField {
            key: "OBJECT_STORE_BACKEND",
            category: "storage",
            label_zh: "对象存储后端",
            label_en: "Object store backend",
            description_zh: "目前仅支持 local；r2 已预留但当前二进制未实现。",
            description_en: "Only local is supported; r2 is reserved but not implemented in this binary.",
            kind: "select",
            secret: false,
            required: true,
            default_value: "local",
            placeholder: "",
            unit: "",
        },
        EnvField {
            key: "OBJECT_STORE_LOCAL_DIR",
            category: "storage",
            label_zh: "本地对象存储目录",
            label_en: "Local object store dir",
            description_zh: "保存 webhook 原文、调试包、附件等文件的目录。",
            description_en: "Directory storing raw webhooks, debug bundles, attachments, etc.",
            kind: "path",
            secret: false,
            required: true,
            default_value: "$HOME/.config/cc-switch-market/objects",
            placeholder: "$HOME/.config/cc-switch-market/objects",
            unit: "",
        },
        EnvField {
            key: "REQUEST_OBJECT_RETENTION_DAYS",
            category: "storage",
            label_zh: "请求调试对象保留天数",
            label_en: "Request object retention",
            description_zh: "保留 API 请求调试对象的天数。超期后仅清理已终态且没有未关闭工单的 request body / response meta。",
            description_en: "How many days to keep API request debug objects. Expired cleanup only removes terminal charges without open tickets.",
            kind: "number",
            secret: false,
            required: true,
            default_value: "7",
            placeholder: "7",
            unit: "days",
        },
        EnvField {
            key: "REQUEST_OBJECT_CLEANUP_BATCH_SIZE",
            category: "storage",
            label_zh: "请求对象清理批量",
            label_en: "Request cleanup batch size",
            description_zh: "每轮维护任务最多清理的请求调试对象记录数，用于限制单次清理压力。",
            description_en: "Maximum request debug object records cleaned per maintenance run.",
            kind: "number",
            secret: false,
            required: true,
            default_value: "1000",
            placeholder: "1000",
            unit: "rows",
        },
        EnvField {
            key: "R2_ACCOUNT_ID",
            category: "storage",
            label_zh: "R2 账户 ID",
            label_en: "R2 account ID",
            description_zh: "可选，仅当 OBJECT_STORE_BACKEND=r2 时使用（暂未启用）。",
            description_en: "Optional. Only used when OBJECT_STORE_BACKEND=r2 (currently disabled).",
            kind: "text",
            secret: false,
            required: false,
            default_value: "",
            placeholder: "",
            unit: "",
        },
        EnvField {
            key: "R2_ACCESS_KEY_ID",
            category: "storage",
            label_zh: "R2 Access Key ID",
            label_en: "R2 access key ID",
            description_zh: "可选，仅当 OBJECT_STORE_BACKEND=r2 时使用（暂未启用）。",
            description_en: "Optional. Only used when OBJECT_STORE_BACKEND=r2 (currently disabled).",
            kind: "text",
            secret: false,
            required: false,
            default_value: "",
            placeholder: "",
            unit: "",
        },
        EnvField {
            key: "R2_SECRET_ACCESS_KEY",
            category: "storage",
            label_zh: "R2 Secret Access Key",
            label_en: "R2 secret access key",
            description_zh: "可选，仅当 OBJECT_STORE_BACKEND=r2 时使用（暂未启用）。",
            description_en: "Optional. Only used when OBJECT_STORE_BACKEND=r2 (currently disabled).",
            kind: "password",
            secret: true,
            required: false,
            default_value: "",
            placeholder: "",
            unit: "",
        },
        EnvField {
            key: "R2_BUCKET",
            category: "storage",
            label_zh: "R2 Bucket",
            label_en: "R2 bucket",
            description_zh: "可选，仅当 OBJECT_STORE_BACKEND=r2 时使用（暂未启用）。",
            description_en: "Optional. Only used when OBJECT_STORE_BACKEND=r2 (currently disabled).",
            kind: "text",
            secret: false,
            required: false,
            default_value: "",
            placeholder: "",
            unit: "",
        },
        EnvField {
            key: "R2_PUBLIC_BASE_URL",
            category: "storage",
            label_zh: "R2 公网访问基址",
            label_en: "R2 public base URL",
            description_zh: "可选。R2 静态资源公开访问域名。",
            description_en: "Optional. Public domain serving R2 static assets.",
            kind: "url",
            secret: false,
            required: false,
            default_value: "",
            placeholder: "https://cdn.example.com",
            unit: "",
        },
        EnvField {
            key: "ROUTER_BASE_DOMAIN",
            category: "runtime",
            label_zh: "Router 基础域名",
            label_en: "Router base domain",
            description_zh: "cc-switch-router 的域名，仅域名不含协议或路径。生产环境务必替换 localhost。",
            description_en: "cc-switch-router domain only, no scheme or path. Replace localhost for production.",
            kind: "text",
            secret: false,
            required: true,
            default_value: "localhost:8081",
            placeholder: "router.example.com",
            unit: "",
        },
        EnvField {
            key: "ROUTER_MARKET_SUBDOMAIN",
            category: "runtime",
            label_zh: "Market 子域",
            label_en: "Market subdomain",
            description_zh: "在 ROUTER_BASE_DOMAIN 上为本 market 暴露的子域名。",
            description_en: "Subdomain on ROUTER_BASE_DOMAIN that exposes this market.",
            kind: "text",
            secret: false,
            required: true,
            default_value: "market",
            placeholder: "market",
            unit: "",
        },
        EnvField {
            key: "DODO_API_BASE",
            category: "payments",
            label_zh: "Dodo API 基地址",
            label_en: "Dodo API base",
            description_zh: "Dodo Payments 的 API 入口，沙箱默认 test.dodopayments.com。",
            description_en: "Dodo Payments API base. Sandbox defaults to test.dodopayments.com.",
            kind: "url",
            secret: false,
            required: true,
            default_value: "https://test.dodopayments.com",
            placeholder: "https://test.dodopayments.com",
            unit: "",
        },
        EnvField {
            key: "DODO_API_KEY",
            category: "payments",
            label_zh: "Dodo API Key",
            label_en: "Dodo API key",
            description_zh: "可选。配合 DODO_PRODUCT_ID 一起填，二者必须同时存在。",
            description_en: "Optional. Must be provided together with DODO_PRODUCT_ID.",
            kind: "password",
            secret: true,
            required: false,
            default_value: "",
            placeholder: "",
            unit: "",
        },
        EnvField {
            key: "DODO_PRODUCT_ID",
            category: "payments",
            label_zh: "Dodo Product ID",
            label_en: "Dodo product ID",
            description_zh: "可选。配合 DODO_API_KEY 一起填，二者必须同时存在。",
            description_en: "Optional. Must be provided together with DODO_API_KEY.",
            kind: "text",
            secret: false,
            required: false,
            default_value: "",
            placeholder: "",
            unit: "",
        },
        EnvField {
            key: "DODO_ALLOWED_PAYMENT_METHOD_TYPES",
            category: "payments",
            label_zh: "Dodo 允许的支付方式",
            label_en: "Dodo allowed payment methods",
            description_zh: "逗号分隔，例如 credit,debit,apple_pay,google_pay,we_chat_pay,crypto_currency。",
            description_en: "Comma-separated, e.g. credit,debit,apple_pay,google_pay,we_chat_pay,crypto_currency.",
            kind: "csv",
            secret: false,
            required: true,
            default_value: "credit,debit,apple_pay,google_pay,we_chat_pay,crypto_currency",
            placeholder: "credit,debit,apple_pay,google_pay,we_chat_pay,crypto_currency",
            unit: "",
        },
        EnvField {
            key: "DODO_WEBHOOK_SECRET",
            category: "payments",
            label_zh: "Dodo Webhook 密钥",
            label_en: "Dodo webhook secret",
            description_zh: "校验 Dodo webhook 签名。生产环境必须替换 dev 默认值。",
            description_en: "Used to verify Dodo webhook signatures. Replace the dev default in production.",
            kind: "password",
            secret: true,
            required: true,
            default_value: "dev",
            placeholder: "",
            unit: "",
        },
        EnvField {
            key: "DODO_MOCK_CHECKOUT_ENABLED",
            category: "payments",
            label_zh: "启用 Dodo 模拟收银台",
            label_en: "Enable Dodo mock checkout",
            description_zh: "本地开发时跳过真实支付，仅生成模拟单据。",
            description_en: "Skip real payments and emit mock receipts during local development.",
            kind: "bool",
            secret: false,
            required: true,
            default_value: "false",
            placeholder: "",
            unit: "",
        },
        EnvField {
            key: "GATEIO_API_BASE",
            category: "payments",
            label_zh: "Gate.io API 基地址",
            label_en: "Gate.io API base",
            description_zh: "Gate.io API 入口，默认 api.gateio.ws。",
            description_en: "Gate.io API base. Defaults to api.gateio.ws.",
            kind: "url",
            secret: false,
            required: true,
            default_value: "https://api.gateio.ws",
            placeholder: "https://api.gateio.ws",
            unit: "",
        },
        EnvField {
            key: "GATEIO_API_KEY",
            category: "payments",
            label_zh: "Gate.io API Key",
            label_en: "Gate.io API key",
            description_zh: "可选。开启 GATEIO_AUTO_PAYOUT_ENABLED 时必填。",
            description_en: "Optional. Required when GATEIO_AUTO_PAYOUT_ENABLED is on.",
            kind: "password",
            secret: true,
            required: false,
            default_value: "",
            placeholder: "",
            unit: "",
        },
        EnvField {
            key: "GATEIO_API_SECRET",
            category: "payments",
            label_zh: "Gate.io API Secret",
            label_en: "Gate.io API secret",
            description_zh: "可选。开启 GATEIO_AUTO_PAYOUT_ENABLED 时必填。",
            description_en: "Optional. Required when GATEIO_AUTO_PAYOUT_ENABLED is on.",
            kind: "password",
            secret: true,
            required: false,
            default_value: "",
            placeholder: "",
            unit: "",
        },
        EnvField {
            key: "GATEIO_SETTLEMENT_CURRENCY",
            category: "payments",
            label_zh: "Gate.io 结算币种",
            label_en: "Gate.io settlement currency",
            description_zh: "提现到 Gate.io 时使用的币种，默认 USDT。",
            description_en: "Currency used for Gate.io payouts. Defaults to USDT.",
            kind: "text",
            secret: false,
            required: true,
            default_value: "USDT",
            placeholder: "USDT",
            unit: "",
        },
        EnvField {
            key: "GATEIO_USD_USDT_RATE",
            category: "payments",
            label_zh: "USD → USDT 汇率",
            label_en: "USD to USDT rate",
            description_zh: "市场内部 USD 余额折算为 USDT 的汇率，必须 > 0。",
            description_en: "Conversion rate from internal USD balance to USDT. Must be > 0.",
            kind: "number",
            secret: false,
            required: true,
            default_value: "1.0",
            placeholder: "1.0",
            unit: "",
        },
        EnvField {
            key: "GATEIO_AUTO_PAYOUT_ENABLED",
            category: "payments",
            label_zh: "启用 Gate.io 自动打款",
            label_en: "Enable Gate.io auto payout",
            description_zh: "开启后由后台 worker 自动执行 Gate.io 打款，需要同时配置 API Key/Secret。",
            description_en: "Background worker auto-executes Gate.io payouts. Requires API key & secret.",
            kind: "bool",
            secret: false,
            required: true,
            default_value: "false",
            placeholder: "",
            unit: "",
        },
        EnvField {
            key: "GATEIO_PAYOUT_WORKER_INTERVAL_SECS",
            category: "payments",
            label_zh: "自动打款轮询间隔",
            label_en: "Auto payout interval",
            description_zh: "Gate.io 自动打款 worker 的轮询周期。",
            description_en: "Polling interval for the Gate.io auto-payout worker.",
            kind: "number",
            secret: false,
            required: true,
            default_value: "60",
            placeholder: "60",
            unit: "secs",
        },
    ]
}

fn env_settings_payload() -> Result<serde_json::Value, ApiError> {
    let env_file = Config::ensure_default_env_file()?;
    let values = read_env_values(&env_file)?;
    let fields = env_fields()
        .into_iter()
        .map(|field| {
            serde_json::json!({
                "key": field.key,
                "category": field.category,
                "labelZh": field.label_zh,
                "labelEn": field.label_en,
                "descriptionZh": field.description_zh,
                "descriptionEn": field.description_en,
                "kind": field.kind,
                "secret": field.secret,
                "required": field.required,
                "defaultValue": field.default_value,
                "placeholder": field.placeholder,
                "unit": field.unit,
                "value": values.get(field.key).cloned().unwrap_or_default(),
            })
        })
        .collect::<Vec<_>>();
    Ok(serde_json::json!({
        "envFile": env_file.display().to_string(),
        "fields": fields,
        "categories": [
            {"key": "runtime"},
            {"key": "auth"},
            {"key": "cloudflare"},
            {"key": "billing"},
            {"key": "routing"},
            {"key": "database"},
            {"key": "storage"},
            {"key": "payments"}
        ],
    }))
}

fn read_env_values(path: &std::path::Path) -> Result<BTreeMap<String, String>, ApiError> {
    let content = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let mut values = BTreeMap::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = trimmed.split_once('=') {
            values.insert(key.trim().to_string(), unquote_env_value(value.trim()));
        }
    }
    Ok(values)
}

fn write_env_values(
    path: &std::path::Path,
    updates: &BTreeMap<String, String>,
) -> Result<(), ApiError> {
    let content = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let mut seen = std::collections::HashSet::new();
    let mut lines = Vec::new();
    for line in content.lines() {
        if let Some((key, _)) = line.trim().split_once('=') {
            let key = key.trim();
            if let Some(value) = updates.get(key) {
                lines.push(format!("{key}={}", quote_env_value(value)));
                seen.insert(key.to_string());
                continue;
            }
        }
        lines.push(line.to_string());
    }
    for (key, value) in updates {
        if !seen.contains(key) {
            lines.push(format!("{key}={}", quote_env_value(value)));
        }
    }
    let mut output = lines.join("\n");
    output.push('\n');
    fs::write(path, output).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

fn unquote_env_value(value: &str) -> String {
    let quoted = (value.starts_with('"') && value.ends_with('"'))
        || (value.starts_with('\'') && value.ends_with('\''));
    if quoted && value.len() >= 2 {
        value[1..value.len() - 1]
            .replace("\\n", "\n")
            .replace("\\\"", "\"")
            .replace("\\\\", "\\")
    } else {
        value.to_string()
    }
}

fn quote_env_value(value: &str) -> String {
    if value.is_empty()
        || value.chars().any(|ch| ch.is_whitespace())
        || value.contains('#')
        || value.contains('"')
        || value.contains('\'')
        || value.contains('\\')
    {
        format!(
            "\"{}\"",
            value
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('\n', "\\n")
        )
    } else {
        value.to_string()
    }
}

pub async fn execute_gateio(
    State(state): State<AppState>,
    admin: AdminPrincipal,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    execute_gateio_by_id(&state, &admin.0.email, id).await
}

pub async fn execute_gateio_by_id(
    state: &AppState,
    admin_email: &str,
    id: Uuid,
) -> Result<Json<serde_json::Value>, ApiError> {
    let tx = state.db().begin_immediate().await?;
    let row = tx.query_optional(
        "UPDATE payout_requests SET status = 'processing', processing_at = ?2 WHERE id = ?1 AND status = 'pending' RETURNING owner_email, net_payout_usd, payout_fee_usd, amount_usd, params_json",
        vec![crate::db::uuid_val(id), crate::db::val(crate::db::now_string())],
    ).await?
    .ok_or_else(|| ApiError::conflict("invalid_payout_state", "payout is not pending"))?;
    tx.commit().await?;
    let transfer_amount = row.decimal("net_payout_usd") / state.config.gateio_usd_usdt_rate;
    let proof = match gateio::execute_transfer(state, id, transfer_amount, row.json("params_json"))
        .await
    {
        Ok(proof) => proof,
        Err(err) => {
            state
                    .db()
                    .execute(
                        "UPDATE payout_requests SET status='needs_review', failure_reason=?2 WHERE id=?1 AND status='processing'",
                        vec![crate::db::uuid_val(id), crate::db::val(err.to_string())],
                    )
                    .await?;
            state
                .db()
                .execute(
                    "INSERT INTO payout_attempts (id, payout_request_id, method, status, error_message, created_at, completed_at) VALUES (?1,?2,'gateio','failed',?3,?4,?4)",
                    vec![
                        crate::db::uuid_val(Uuid::new_v4()),
                        crate::db::uuid_val(id),
                        crate::db::val(err.to_string()),
                        crate::db::val(crate::db::now_string()),
                    ],
                )
                .await?;
            if let Err(notify_err) = router_notifications::send_notification(
                &state.config,
                "payout_review",
                &row.string("owner_email"),
                router_notifications::default_locale(),
                serde_json::json!({
                    "payoutId": id.to_string(),
                    "amountUsd": row.string("amount_usd"),
                    "feeUsd": row.string("payout_fee_usd"),
                    "netPayoutUsd": row.string("net_payout_usd"),
                    "claimUrl": format!("{}/claim", state.config.market_public_base_url),
                }),
            )
            .await
            {
                tracing::warn!(payout_id = %id, error = %notify_err, "send payout review notification failed");
            }
            return Err(err);
        }
    };
    let attempt_row_id = Uuid::new_v4();
    state
        .db()
        .execute(
            r#"
            INSERT INTO payout_attempts
              (id, payout_request_id, method, status, request_object_key, request_object_sha256, response_object_key, response_object_sha256, external_tx_id, created_at, completed_at)
            VALUES (?1,?2,'gateio','succeeded',?3,?4,?5,?6,?7,?8,?8)
            "#,
            vec![
                crate::db::uuid_val(attempt_row_id),
                crate::db::uuid_val(id),
                crate::db::opt_val(proof.request_object_key.clone()),
                crate::db::opt_val(proof.request_object_sha256.clone()),
                crate::db::opt_val(proof.response_object_key.clone()),
                crate::db::opt_val(proof.response_object_sha256.clone()),
                crate::db::val(&proof.external_tx_id),
                crate::db::val(crate::db::now_string()),
            ],
        )
        .await?;
    state
        .db()
        .execute(
            "UPDATE payout_requests SET gateio_request_object_key = COALESCE(?2, gateio_request_object_key), gateio_response_object_key = COALESCE(?3, gateio_response_object_key), gateio_batch_id = COALESCE(?4, gateio_batch_id), gateio_request_object_sha256 = COALESCE(?5, gateio_request_object_sha256), gateio_response_object_sha256 = COALESCE(?6, gateio_response_object_sha256) WHERE id = ?1",
            vec![
                crate::db::uuid_val(id),
                crate::db::opt_val(proof.request_object_key.clone()),
                crate::db::opt_val(proof.response_object_key.clone()),
                crate::db::opt_val(proof.gateio_batch_id.clone()),
                crate::db::opt_val(proof.request_object_sha256.clone()),
                crate::db::opt_val(proof.response_object_sha256.clone()),
            ],
        )
        .await?;
    mark_paid_internal(
        state,
        admin_email,
        id,
        Some(proof.external_tx_id.clone()),
        Some(serde_json::to_value(proof).unwrap_or_default()),
        Some("gateio execution succeeded".to_string()),
    )
    .await
}

pub fn spawn_gateio_worker(state: AppState) -> Option<tokio::task::JoinHandle<()>> {
    if !state.config.gateio_auto_payout_enabled {
        return None;
    }
    Some(tokio::spawn(async move {
        let interval =
            std::time::Duration::from_secs(state.config.gateio_payout_worker_interval_secs.max(10));
        loop {
            tokio::time::sleep(interval).await;
            let rows = match state
                .db()
                .query_all(
                    "SELECT id FROM payout_requests WHERE method='gateio' AND status='pending' ORDER BY created_at LIMIT 10",
                    vec![],
                )
                .await
            {
                Ok(rows) => rows,
                Err(err) => {
                    tracing::warn!(error = %err, "gateio payout worker scan failed");
                    continue;
                }
            };
            for row in rows {
                let id = row.uuid("id");
                if let Err(err) = execute_gateio_by_id(&state, "system:gateio-worker", id).await {
                    tracing::warn!(%id, error = %err, "gateio payout worker execution failed");
                }
            }
        }
    }))
}

#[derive(Deserialize)]
pub struct MarkPaid {
    pub external_tx_id: Option<String>,
    pub proof: Option<serde_json::Value>,
    pub reason: Option<String>,
}

pub async fn mark_payout_paid(
    State(state): State<AppState>,
    AdminPrincipal(admin): AdminPrincipal,
    Path(id): Path<Uuid>,
    Json(input): Json<MarkPaid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let external_tx_id = input
        .external_tx_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let reason = input
        .reason
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if external_tx_id.is_none() || reason.is_none() || input.proof.is_none() {
        return Err(ApiError::bad_request(
            "missing_admin_confirmation",
            "external_tx_id, proof, and reason are required",
        ));
    }
    mark_paid_internal(
        &state,
        &admin.email,
        id,
        input.external_tx_id,
        input.proof,
        input.reason,
    )
    .await
}

async fn mark_paid_internal(
    state: &AppState,
    admin_email: &str,
    id: Uuid,
    external_tx_id: Option<String>,
    proof: Option<serde_json::Value>,
    reason: Option<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    ledger::ensure_platform_accounts(state.db()).await?;
    let proof_object_key = match proof {
        Some(proof) => {
            let proof_object_id = Uuid::new_v4();
            let stored = state
                .object_store
                .put_json_once(
                    format!("payouts/{id}/proofs/{proof_object_id}.json"),
                    &proof,
                )
                .await?;
            crate::object_store::record_object_ref(
                state,
                &stored,
                "payout_request",
                id,
                "payout_proof",
                Some("application/json"),
            )
            .await?;
            Some((stored.object_key, stored.content_sha256))
        }
        None => None,
    };
    let (proof_object_key, proof_object_sha256) = proof_object_key
        .map(|(key, sha)| (Some(key), Some(sha)))
        .unwrap_or((None, None));
    let tx = state.db().begin_immediate().await?;
    let row = tx.query_optional(
        "UPDATE payout_requests SET status='paid', paid_at=?5, external_tx_id=COALESCE(?2, external_tx_id), proof_object_key=COALESCE(?3, proof_object_key), proof_object_sha256=COALESCE(?4, proof_object_sha256) WHERE id=?1 AND status IN ('pending','processing','needs_review') RETURNING owner_email, amount_usd, net_payout_usd, payout_fee_usd",
        vec![
            crate::db::uuid_val(id),
            crate::db::opt_val(external_tx_id.clone()),
            crate::db::opt_val(proof_object_key),
            crate::db::opt_val(proof_object_sha256),
            crate::db::val(crate::db::now_string()),
        ],
    ).await?
    .ok_or_else(|| ApiError::conflict("invalid_payout_state", "payout cannot be marked paid"))?;
    let owner_email = row.string("owner_email");
    let gross = row.decimal("amount_usd");
    let net = row.decimal("net_payout_usd");
    let fee = row.decimal("payout_fee_usd");
    ledger::transfer(
        &tx,
        AccountRef::Provider {
            account_type: "payout_reserved",
            owner_email: &owner_email,
        },
        AccountRef::Platform {
            account_type: "settlement_paid",
        },
        net,
        "payout_request",
        id,
        "admin",
        Some(admin_email),
    )
    .await?;
    if fee > Decimal::ZERO {
        ledger::transfer(
            &tx,
            AccountRef::Provider {
                account_type: "payout_reserved",
                owner_email: &owner_email,
            },
            AccountRef::Platform {
                account_type: "fee_revenue",
            },
            fee,
            "payout_request",
            id,
            "admin",
            Some(admin_email),
        )
        .await?;
    }
    audit_tx(
        &tx,
        admin_email,
        "payout.mark_paid",
        "payout_request",
        id,
        serde_json::json!({"reason": reason}),
    )
    .await?;
    tx.execute(
        "INSERT INTO settlement_items (id, payout_request_id, owner_email, gross_amount_usd, fee_amount_usd, net_amount_usd, status, external_tx_id, created_at) VALUES (?1,?2,?3,?4,?5,?6,'paid',?7,?8)",
        vec![
            crate::db::uuid_val(Uuid::new_v4()),
            crate::db::uuid_val(id),
            crate::db::val(&owner_email),
            crate::db::dec_val(gross),
            crate::db::dec_val(fee),
            crate::db::dec_val(net),
            crate::db::opt_val(external_tx_id),
            crate::db::val(crate::db::now_string()),
        ],
    )
    .await?;
    tx.commit().await?;
    if let Err(err) = router_notifications::send_notification(
        &state.config,
        "payout_paid",
        &owner_email,
        router_notifications::default_locale(),
        serde_json::json!({
            "payoutId": id.to_string(),
            "amountUsd": gross.to_string(),
            "feeUsd": fee.to_string(),
            "netPayoutUsd": net.to_string(),
            "claimUrl": format!("{}/claim", state.config.market_public_base_url),
        }),
    )
    .await
    {
        tracing::warn!(owner_email = %owner_email, payout_id = %id, error = %err, "send payout paid notification failed");
    }
    Ok(Json(serde_json::json!({"ok": true})))
}

#[derive(Deserialize)]
pub struct FailureReason {
    pub reason: String,
}

pub async fn mark_payout_failed(
    State(state): State<AppState>,
    AdminPrincipal(admin): AdminPrincipal,
    Path(id): Path<Uuid>,
    Json(input): Json<FailureReason>,
) -> Result<Json<serde_json::Value>, ApiError> {
    release_payout(&state, &admin.email, id, "failed", Some(input.reason)).await
}

pub async fn cancel_payout(
    State(state): State<AppState>,
    AdminPrincipal(admin): AdminPrincipal,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    release_payout(&state, &admin.email, id, "cancelled", None).await
}

async fn release_payout(
    state: &AppState,
    admin_email: &str,
    id: Uuid,
    status: &str,
    reason: Option<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let tx = state.db().begin_immediate().await?;
    let now = crate::db::now_string();
    let row = tx.query_optional(
        "UPDATE payout_requests SET status=?2, failure_reason=COALESCE(?3, failure_reason), failed_at=CASE WHEN ?2='failed' THEN ?4 ELSE failed_at END, cancelled_at=CASE WHEN ?2='cancelled' THEN ?4 ELSE cancelled_at END WHERE id=?1 AND status IN ('pending','processing','needs_review') RETURNING owner_email, amount_usd",
        vec![crate::db::uuid_val(id), crate::db::val(status), crate::db::opt_val(reason), crate::db::val(now)],
    ).await?
    .ok_or_else(|| ApiError::conflict("invalid_payout_state", "payout cannot be released"))?;
    let owner_email = row.string("owner_email");
    let amount = row.decimal("amount_usd");
    ledger::transfer(
        &tx,
        AccountRef::Provider {
            account_type: "payout_reserved",
            owner_email: &owner_email,
        },
        AccountRef::Provider {
            account_type: "client_payable",
            owner_email: &owner_email,
        },
        amount,
        "payout_request",
        id,
        "admin",
        Some(admin_email),
    )
    .await?;
    audit_tx(
        &tx,
        admin_email,
        "payout.release",
        "payout_request",
        id,
        serde_json::json!({"status": status}),
    )
    .await?;
    tx.commit().await?;
    if status == "failed" || status == "cancelled" {
        let kind = if status == "failed" {
            "payout_failed"
        } else {
            "payout_cancelled"
        };
        if let Err(err) = router_notifications::send_notification(
            &state.config,
            kind,
            &owner_email,
            router_notifications::default_locale(),
            serde_json::json!({
                "payoutId": id.to_string(),
                "amountUsd": amount.to_string(),
                "feeUsd": "0",
                "netPayoutUsd": amount.to_string(),
                "claimUrl": format!("{}/claim", state.config.market_public_base_url),
            }),
        )
        .await
        {
            tracing::warn!(owner_email = %owner_email, payout_id = %id, kind = %kind, error = %err, "send payout release notification failed");
        }
    }
    Ok(Json(serde_json::json!({"ok": true})))
}

#[derive(Deserialize)]
pub struct TicketStatus {
    pub status: String,
}

#[derive(Deserialize)]
pub struct TicketAssign {
    pub admin_id: String,
}

#[derive(Deserialize)]
pub struct TicketMessage {
    pub body_text: String,
    pub internal_note: Option<bool>,
    pub attachment_ids: Option<Vec<Uuid>>,
}

pub async fn assign_ticket(
    State(state): State<AppState>,
    AdminPrincipal(admin): AdminPrincipal,
    Path(id): Path<Uuid>,
    Json(input): Json<TicketAssign>,
) -> Result<Json<serde_json::Value>, ApiError> {
    state
        .db()
        .execute(
            "UPDATE tickets SET assigned_admin_id=?2, updated_at=?3 WHERE id=?1",
            vec![
                crate::db::uuid_val(id),
                crate::db::val(input.admin_id),
                crate::db::val(crate::db::now_string()),
            ],
        )
        .await?;
    write_audit(&state, &admin.email, "ticket.assign", "ticket", id).await?;
    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn admin_ticket_message(
    State(state): State<AppState>,
    AdminPrincipal(admin): AdminPrincipal,
    Path(id): Path<Uuid>,
    Json(input): Json<TicketMessage>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let internal = input.internal_note.unwrap_or(false);
    let now = crate::db::now_string();
    let message_id = Uuid::new_v4();
    let admin_user = state
        .db()
        .query_optional(
            "SELECT user_id FROM web_sessions WHERE email = ?1 ORDER BY created_at DESC LIMIT 1",
            vec![crate::db::val(&admin.email)],
        )
        .await?
        .and_then(|row| row.opt_uuid("user_id"));
    let tx = state.db().begin_immediate().await?;
    tx.execute(
        "INSERT INTO ticket_messages (id, ticket_id, sender_type, sender_id, body_text, internal_note, created_at) VALUES (?1,?2,'admin',?3,?4,?5,?6)",
        vec![
            crate::db::uuid_val(message_id),
            crate::db::uuid_val(id),
            crate::db::val(&admin.email),
            crate::db::val(input.body_text),
            crate::db::val(internal),
            crate::db::val(&now),
        ],
    )
    .await?;
    if let Some(admin_user_id) = admin_user {
        crate::support::bind_attachments_locked(
            &tx,
            input.attachment_ids,
            admin_user_id,
            id,
            message_id,
        )
        .await?;
    } else if input
        .attachment_ids
        .as_ref()
        .is_some_and(|value| !value.is_empty())
    {
        return Err(ApiError::forbidden("admin attachment owner not found"));
    }
    if !internal {
        tx.execute(
            "UPDATE tickets SET status = CASE WHEN status IN ('closed','resolved') THEN status ELSE 'waiting_user' END, updated_at = ?2 WHERE id = ?1",
            vec![crate::db::uuid_val(id), crate::db::val(&now)],
        )
        .await?;
    } else {
        tx.execute(
            "UPDATE tickets SET updated_at = ?2 WHERE id = ?1",
            vec![crate::db::uuid_val(id), crate::db::val(&now)],
        )
        .await?;
    }
    tx.commit().await?;
    write_audit(&state, &admin.email, "ticket.message", "ticket", id).await?;
    Ok(Json(
        serde_json::json!({ "ok": true, "internal": internal }),
    ))
}

pub async fn ticket_status(
    State(state): State<AppState>,
    AdminPrincipal(admin): AdminPrincipal,
    Path(id): Path<Uuid>,
    Json(input): Json<TicketStatus>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let now = crate::db::now_string();
    state.db().execute("UPDATE tickets SET status=?2, updated_at=?3, closed_at=CASE WHEN ?2 IN ('resolved','closed') THEN ?3 ELSE closed_at END WHERE id=?1", vec![crate::db::uuid_val(id), crate::db::val(input.status), crate::db::val(now)]).await?;
    write_audit(&state, &admin.email, "ticket.status", "ticket", id).await?;
    Ok(Json(serde_json::json!({"ok": true})))
}

#[derive(Deserialize)]
pub struct LinkPayout {
    pub payout_request_id: Uuid,
}

pub async fn link_payout(
    State(state): State<AppState>,
    AdminPrincipal(admin): AdminPrincipal,
    Path(id): Path<Uuid>,
    Json(input): Json<LinkPayout>,
) -> Result<Json<serde_json::Value>, ApiError> {
    state
        .db()
        .execute(
            "UPDATE tickets SET related_payout_request_id=?2, updated_at=?3 WHERE id=?1",
            vec![
                crate::db::uuid_val(id),
                crate::db::uuid_val(input.payout_request_id),
                crate::db::val(crate::db::now_string()),
            ],
        )
        .await?;
    state
        .db()
        .execute(
            "UPDATE payout_requests SET ticket_id=?1 WHERE id=?2",
            vec![
                crate::db::uuid_val(id),
                crate::db::uuid_val(input.payout_request_id),
            ],
        )
        .await?;
    write_audit(&state, &admin.email, "ticket.link_payout", "ticket", id).await?;
    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn complete_manual_payout(
    State(state): State<AppState>,
    admin: AdminPrincipal,
    Path(id): Path<Uuid>,
    Json(input): Json<MarkPaid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let external_tx_id = input
        .external_tx_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let reason = input
        .reason
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if external_tx_id.is_none() || reason.is_none() || input.proof.is_none() {
        return Err(ApiError::bad_request(
            "missing_manual_payout_proof",
            "external_tx_id, proof, and reason are required",
        ));
    }
    let row = state
        .db()
        .query_one(
            "SELECT related_payout_request_id FROM tickets WHERE id=?1",
            vec![crate::db::uuid_val(id)],
        )
        .await?;
    let payout_id = row.uuid("related_payout_request_id");
    mark_paid_internal(
        &state,
        &admin.0.email,
        payout_id,
        input.external_tx_id,
        input.proof,
        input.reason,
    )
    .await
}

#[derive(Deserialize)]
pub struct ProviderAdjust {
    pub owner_email: String,
    pub amount_usd: Decimal,
    pub direction: String,
    pub reason: String,
}

pub async fn adjust_provider_payable(
    State(state): State<AppState>,
    AdminPrincipal(admin): AdminPrincipal,
    Path(ticket_id): Path<Uuid>,
    Json(input): Json<ProviderAdjust>,
) -> Result<Json<serde_json::Value>, ApiError> {
    ledger::ensure_provider_accounts(state.db(), &input.owner_email).await?;
    ledger::ensure_platform_accounts(state.db()).await?;
    let tx = state.db().begin_immediate().await?;
    let (from, to) = if input.direction == "credit" {
        (
            AccountRef::Platform {
                account_type: "risk_loss",
            },
            AccountRef::Provider {
                account_type: "client_payable",
                owner_email: &input.owner_email,
            },
        )
    } else {
        (
            AccountRef::Provider {
                account_type: "client_payable",
                owner_email: &input.owner_email,
            },
            AccountRef::Platform {
                account_type: "risk_loss",
            },
        )
    };
    ledger::transfer(
        &tx,
        from,
        to,
        input.amount_usd,
        "adjustment",
        ticket_id,
        "admin",
        Some(&admin.email),
    )
    .await?;
    audit_tx(
        &tx,
        &admin.email,
        "provider.adjust_payable",
        "ticket",
        ticket_id,
        serde_json::json!({"reason": input.reason}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(serde_json::json!({"ok": true})))
}

async fn write_audit(
    state: &AppState,
    admin: &str,
    action: &str,
    reference_type: &str,
    reference_id: Uuid,
) -> Result<(), ApiError> {
    state.db().execute(
        "INSERT INTO admin_audit (id, admin_actor, action, reference_type, reference_id, metadata_json, created_at) VALUES (?1,?2,?3,?4,?5,'{}',?6)",
        vec![crate::db::uuid_val(Uuid::new_v4()), crate::db::val(admin), crate::db::val(action), crate::db::val(reference_type), crate::db::uuid_val(reference_id), crate::db::val(crate::db::now_string())],
    ).await?;
    Ok(())
}

async fn audit_tx(
    tx: &crate::db::DbTx,
    admin: &str,
    action: &str,
    reference_type: &str,
    reference_id: Uuid,
    metadata: serde_json::Value,
) -> Result<(), ApiError> {
    tx.execute(
        "INSERT INTO admin_audit (id, admin_actor, action, reference_type, reference_id, metadata_json, created_at) VALUES (?1,?2,?3,?4,?5,?6,?7)",
        vec![crate::db::uuid_val(Uuid::new_v4()), crate::db::val(admin), crate::db::val(action), crate::db::val(reference_type), crate::db::uuid_val(reference_id), crate::db::json_val(metadata), crate::db::val(crate::db::now_string())],
    ).await?;
    Ok(())
}
