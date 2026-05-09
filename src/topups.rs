use axum::{
    Json,
    body::Bytes,
    extract::{Path, State},
    http::HeaderMap,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use chrono::{Duration, Utc};
use hmac::{Hmac, Mac};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use uuid::Uuid;

use crate::{
    app_state::AppState,
    auth::Principal,
    error::ApiError,
    ledger::{self, AccountRef},
    router_notifications,
};

const FIRST_TOPUP_MAX_USD: i64 = 10;
const FIRST_TOPUP_LIMIT_MESSAGE: &str = "首冲最多 10$，建议先小额体验后再充值更多";

#[derive(Deserialize)]
pub struct CheckoutRequest {
    pub amount_usd: Decimal,
}

#[derive(Serialize)]
pub struct TopupOrder {
    pub id: Uuid,
    pub gross_amount: Decimal,
    pub fee_amount: Decimal,
    pub net_amount: Decimal,
    pub status: String,
    pub checkout_url: Option<String>,
}

#[derive(Deserialize, Serialize)]
pub struct DodoWebhook {
    pub event_id: Option<String>,
    #[serde(alias = "type", alias = "event_type")]
    pub event_type: String,
    pub topup_id: Option<Uuid>,
    pub provider_payment_id: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub data: Option<serde_json::Value>,
    pub payment_id: Option<String>,
    pub session_id: Option<String>,
}

pub fn spawn_order_expiry(state: AppState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let interval = std::time::Duration::from_secs(300);
        loop {
            tokio::time::sleep(interval).await;
            match state
                .db()
                .execute(
                    "UPDATE topup_orders SET status='expired' WHERE status='pending' AND expires_at IS NOT NULL AND expires_at < ?1",
                    vec![crate::db::val(crate::db::now_string())],
                )
                .await
            {
                Ok(count) if count > 0 => tracing::info!(expired = count, "expired stale topup orders"),
                Ok(_) => {}
                Err(err) => tracing::warn!(error = %err, "topup expiry task failed"),
            }
        }
    })
}

pub async fn create_checkout(
    State(state): State<AppState>,
    principal: Principal,
    Json(input): Json<CheckoutRequest>,
) -> Result<Json<TopupOrder>, ApiError> {
    crate::rate_limit::check("topup_checkout", &principal.user_id.to_string(), 10)?;
    if input.amount_usd <= Decimal::ZERO {
        return Err(ApiError::bad_request(
            "invalid_amount",
            "amount must be positive",
        ));
    }
    let db = state.db();
    ledger::ensure_user_accounts(db, principal.user_id).await?;
    ledger::ensure_platform_accounts(db).await?;
    enforce_first_topup_limit(db, principal.user_id, input.amount_usd).await?;
    let (fee, fee_policy_snapshot) = fee_for_topup(db, input.amount_usd).await?;
    let net = input.amount_usd - fee;
    if net <= Decimal::ZERO {
        return Err(ApiError::bad_request(
            "invalid_topup_amount",
            "top-up amount must exceed fees",
        ));
    }
    let id = Uuid::new_v4();
    let now = crate::db::now_string();
    let expires_at = (Utc::now() + Duration::hours(24)).to_rfc3339();
    let (checkout_url, provider_payment_id) =
        create_dodo_checkout_url(&state, id, principal.user_id, input.amount_usd, net, fee).await?;
    db.execute(
        r#"
        INSERT INTO topup_orders (id, user_id, gross_amount, fee_amount, net_amount, checkout_url, provider_payment_id, metadata_json, created_at, expires_at)
        VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
        "#,
        vec![
            crate::db::uuid_val(id),
            crate::db::uuid_val(principal.user_id),
            crate::db::dec_val(input.amount_usd),
            crate::db::dec_val(fee),
            crate::db::dec_val(net),
            crate::db::val(&checkout_url),
            crate::db::opt_val(provider_payment_id),
            crate::db::json_val(serde_json::json!({"fee_policy_snapshot": fee_policy_snapshot})),
            crate::db::val(&now),
            crate::db::val(expires_at),
        ],
    )
    .await?;
    let row = db.query_one(
        "SELECT id, gross_amount, fee_amount, net_amount, status, checkout_url FROM topup_orders WHERE id = ?1",
        vec![crate::db::uuid_val(id)],
    ).await?;
    Ok(Json(row_to_topup(row)))
}

async fn enforce_first_topup_limit(
    db: &crate::db::Db,
    user_id: Uuid,
    amount: Decimal,
) -> Result<(), ApiError> {
    if amount <= Decimal::from(FIRST_TOPUP_MAX_USD) {
        return Ok(());
    }
    let paid_count = db
        .query_one(
            "SELECT COUNT(*) AS count FROM topup_orders WHERE user_id=?1 AND status='paid'",
            vec![crate::db::uuid_val(user_id)],
        )
        .await?
        .i64("count");
    if first_topup_exceeds_limit(amount, paid_count) {
        return Err(ApiError::bad_request(
            "first_topup_amount_exceeded",
            FIRST_TOPUP_LIMIT_MESSAGE,
        ));
    }
    Ok(())
}

fn first_topup_exceeds_limit(amount: Decimal, paid_topup_count: i64) -> bool {
    paid_topup_count == 0 && amount > Decimal::from(FIRST_TOPUP_MAX_USD)
}

async fn fee_for_topup(
    db: &crate::db::Db,
    amount: Decimal,
) -> Result<(Decimal, serde_json::Value), ApiError> {
    let row = db
        .query_optional(
            "SELECT id, fixed_usd, percent_bps, min_usd, max_usd, effective_from FROM fee_policies WHERE fee_type='topup' AND method='dodo' AND status='active' ORDER BY effective_from DESC LIMIT 1",
            vec![],
        )
        .await?;
    let Some(row) = row else {
        return Ok((Decimal::ZERO, serde_json::json!({"source": "none"})));
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
            "feeType": "topup",
            "method": "dodo",
            "fixedUsd": row.string("fixed_usd"),
            "percentBps": row.i64("percent_bps"),
            "minUsd": row.string("min_usd"),
            "maxUsd": row.opt_string("max_usd"),
            "effectiveFrom": row.string("effective_from"),
            "computedFeeUsd": fee.to_string(),
        }),
    ))
}

async fn create_dodo_checkout_url(
    state: &AppState,
    topup_id: Uuid,
    user_id: Uuid,
    gross: Decimal,
    net: Decimal,
    fee: Decimal,
) -> Result<(String, Option<String>), ApiError> {
    if state.config.dodo_api_key.trim().is_empty() || state.config.dodo_product_id.trim().is_empty()
    {
        if !state.config.dodo_mock_checkout_enabled {
            return Err(ApiError::service_unavailable(
                "Dodo checkout is not configured; set DODO_API_KEY/DODO_PRODUCT_ID or enable DODO_MOCK_CHECKOUT_ENABLED=true for local development",
            ));
        }
        return Ok((
            format!(
                "{}/dashboard?mock_topup=1&topup_id={topup_id}#wallet",
                state.config.market_public_base_url
            ),
            None,
        ));
    }
    let quantity = (gross * Decimal::from(100u64))
        .round()
        .to_string()
        .parse::<u64>()
        .unwrap_or(1)
        .max(1);
    let body = serde_json::json!({
        "product_cart": [{
            "product_id": state.config.dodo_product_id,
            "quantity": quantity
        }],
        "allowed_payment_method_types": state.config.dodo_allowed_payment_method_types,
        "return_url": format!("{}/dashboard?topup_id={topup_id}#wallet", state.config.market_public_base_url),
        "metadata": {
            "topup_id": topup_id.to_string(),
            "user_id": user_id.to_string(),
            "gross_amount": gross.to_string(),
            "fee_amount": fee.to_string(),
            "net_amount": net.to_string()
        }
    });
    let response = state
        .http
        .post(format!(
            "{}/checkouts",
            state.config.dodo_api_base.trim_end_matches('/')
        ))
        .bearer_auth(&state.config.dodo_api_key)
        .json(&body)
        .send()
        .await
        .map_err(|err| ApiError::service_unavailable(format!("Dodo checkout failed: {err}")))?;
    let status = response.status();
    let value = response
        .json::<serde_json::Value>()
        .await
        .unwrap_or_else(|_| serde_json::json!({}));
    if !status.is_success() {
        return Err(ApiError::service_unavailable(format!(
            "Dodo checkout returned {status}: {value}"
        )));
    }
    let checkout_url = value
        .get("checkout_url")
        .or_else(|| value.get("checkoutUrl"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            ApiError::service_unavailable("Dodo checkout response missing checkout_url")
        })?
        .to_string();
    let session_id = value
        .get("session_id")
        .or_else(|| value.get("sessionId"))
        .and_then(|v| v.as_str())
        .map(ToOwned::to_owned);
    Ok((checkout_url, session_id))
}

pub async fn get_topup(
    State(state): State<AppState>,
    principal: Principal,
    Path(id): Path<Uuid>,
) -> Result<Json<TopupOrder>, ApiError> {
    let row = state.db().query_one(
        "SELECT id, gross_amount, fee_amount, net_amount, status, checkout_url FROM topup_orders WHERE id = ?1 AND user_id = ?2",
        vec![crate::db::uuid_val(id), crate::db::uuid_val(principal.user_id)],
    )
    .await?;
    Ok(Json(row_to_topup(row)))
}

pub async fn dodo_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, ApiError> {
    let webhook_headers = validate_webhook_signature(&state, &headers, &body)?;
    let input: DodoWebhook = serde_json::from_slice(&body)
        .map_err(|_| ApiError::bad_request("invalid_webhook_body", "invalid Dodo webhook JSON"))?;
    let event_id = webhook_headers
        .webhook_id
        .or_else(|| input.event_id.clone())
        .ok_or_else(|| ApiError::bad_request("missing_webhook_id", "Dodo webhook id required"))?;
    let raw_object = state
        .object_store
        .put_bytes_once(format!("webhooks/dodo/{event_id}.json"), &body)
        .await?;
    let raw_sha256 = raw_object.content_sha256.clone();
    let reference_id_for_object = webhook_topup_id(&input).unwrap_or_else(Uuid::new_v4);
    crate::object_store::record_object_ref(
        &state,
        &raw_object,
        "dodo_webhook",
        reference_id_for_object,
        &input.event_type,
        Some("application/json"),
    )
    .await?;
    let db = state.db();
    let tx = db.begin_immediate().await?;
    let event_action = dodo_event_action(&input.event_type);
    let mut notification: Option<(String, String, serde_json::Value)> = None;
    let inserted = tx.query_optional(
        r#"
        INSERT INTO processed_webhooks (provider, event_id, event_type, status, raw_payload_object_key, raw_payload_sha256, processed_at, created_at)
        VALUES ('dodo', ?1, ?2, ?3, ?4, ?5, ?6, ?6)
        ON CONFLICT DO NOTHING
        RETURNING event_id
        "#,
        vec![
            crate::db::val(&event_id),
            crate::db::val(&input.event_type),
            crate::db::val(event_action.webhook_status),
            crate::db::val(raw_object.object_key),
            crate::db::val(&raw_sha256),
            crate::db::val(crate::db::now_string()),
        ],
    )
    .await?;
    if inserted.is_none() {
        tx.commit().await?;
        return Ok(Json(serde_json::json!({"ok": true, "duplicate": true})));
    }

    match event_action.kind {
        DodoEventKind::PaymentSucceeded => {
            let topup_id = webhook_topup_id(&input)
                .ok_or_else(|| ApiError::bad_request("missing_topup_id", "topup_id required"))?;
            let row = tx.query_optional(
            r#"
            UPDATE topup_orders
               SET status = 'paid', paid_at = ?3, provider_payment_id = COALESCE(?2, provider_payment_id), payment_method_type = COALESCE(?4, payment_method_type)
             WHERE id = ?1 AND status = 'pending'
             RETURNING user_id, net_amount, fee_amount
            "#,
            vec![
                crate::db::uuid_val(topup_id),
                crate::db::opt_val(webhook_provider_payment_id(&input)),
                crate::db::val(crate::db::now_string()),
                crate::db::opt_val(webhook_payment_method_type(&input)),
            ],
        )
        .await?;
            if let Some(row) = row {
                let user_id = row.uuid("user_id");
                let net = row.decimal("net_amount");
                let fee = row.decimal("fee_amount");
                let user_email = tx
                    .query_one(
                        "SELECT email FROM users WHERE id = ?1",
                        vec![crate::db::uuid_val(user_id)],
                    )
                    .await?
                    .string("email");
                let topup_meta = tx
                    .query_one(
                        "SELECT gross_amount, fee_amount, net_amount FROM topup_orders WHERE id = ?1",
                        vec![crate::db::uuid_val(topup_id)],
                    )
                    .await?;
                notification = Some((
                    "topup_paid".to_string(),
                    user_email,
                    serde_json::json!({
                        "topupId": topup_id.to_string(),
                        "grossAmountUsd": topup_meta.string("gross_amount"),
                        "feeAmountUsd": topup_meta.string("fee_amount"),
                        "netAmountUsd": topup_meta.string("net_amount"),
                        "dashboardUrl": format!("{}/dashboard#wallet", state.config.market_public_base_url),
                    }),
                ));
                ledger::transfer(
                    &tx,
                    AccountRef::Platform {
                        account_type: "payment_clearing",
                    },
                    AccountRef::User {
                        account_type: "user_cash",
                        user_id,
                    },
                    net,
                    "topup",
                    topup_id,
                    "webhook",
                    Some("dodo"),
                )
                .await?;
                if fee > Decimal::ZERO {
                    ledger::transfer(
                        &tx,
                        AccountRef::Platform {
                            account_type: "payment_clearing",
                        },
                        AccountRef::Platform {
                            account_type: "fee_revenue",
                        },
                        fee,
                        "topup",
                        topup_id,
                        "webhook",
                        Some("dodo"),
                    )
                    .await?;
                }
            }
        }
        DodoEventKind::RefundOrChargeback => {
            let topup_id = webhook_topup_id(&input)
                .ok_or_else(|| ApiError::bad_request("missing_topup_id", "topup_id required"))?;
            let row = tx
                .query_optional(
                    r#"
                    UPDATE topup_orders
                       SET status = 'refunded', refunded_at = ?2, provider_payment_id = COALESCE(?3, provider_payment_id), payment_method_type = COALESCE(?4, payment_method_type)
                     WHERE id = ?1 AND status = 'paid'
                     RETURNING user_id, net_amount
                    "#,
                    vec![
                        crate::db::uuid_val(topup_id),
                        crate::db::val(crate::db::now_string()),
                        crate::db::opt_val(webhook_provider_payment_id(&input)),
                        crate::db::opt_val(webhook_payment_method_type(&input)),
                    ],
                )
                .await?;
            if let Some(row) = row {
                let user_id = row.uuid("user_id");
                let net = row.decimal("net_amount");
                let user_email = tx
                    .query_one(
                        "SELECT email FROM users WHERE id = ?1",
                        vec![crate::db::uuid_val(user_id)],
                    )
                    .await?
                    .string("email");
                let topup_meta = tx
                    .query_one(
                        "SELECT gross_amount, fee_amount, net_amount FROM topup_orders WHERE id = ?1",
                        vec![crate::db::uuid_val(topup_id)],
                    )
                    .await?;
                let notification_kind =
                    if matches!(event_action.kind, DodoEventKind::RefundOrChargeback)
                        && matches!(
                            input.event_type.as_str(),
                            "chargeback.created" | "dispute.opened" | "dispute_opened"
                        )
                    {
                        "topup_chargeback"
                    } else {
                        "topup_refunded"
                    };
                notification = Some((
                    notification_kind.to_string(),
                    user_email,
                    serde_json::json!({
                        "topupId": topup_id.to_string(),
                        "grossAmountUsd": topup_meta.string("gross_amount"),
                        "feeAmountUsd": topup_meta.string("fee_amount"),
                        "netAmountUsd": topup_meta.string("net_amount"),
                        "dashboardUrl": format!("{}/dashboard#wallet", state.config.market_public_base_url),
                    }),
                ));
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
                        topup_id,
                        "webhook",
                        Some("dodo"),
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
                        topup_id,
                        "webhook",
                        Some("dodo"),
                    )
                    .await?;
                }
            }
        }
        DodoEventKind::PaymentFailed | DodoEventKind::Ignored => {
            if let Some(topup_id) = webhook_topup_id(&input) {
                if matches!(event_action.kind, DodoEventKind::PaymentFailed) {
                    tx.execute(
                        "UPDATE topup_orders SET status='failed', payment_method_type = COALESCE(?2, payment_method_type) WHERE id=?1 AND status='pending'",
                        vec![
                            crate::db::uuid_val(topup_id),
                            crate::db::opt_val(webhook_payment_method_type(&input)),
                        ],
                    )
                    .await?;
                }
            }
        }
    }
    tx.commit().await?;
    if let Some((kind, to, data)) = notification {
        if let Err(err) = router_notifications::send_notification(
            &state.config,
            &kind,
            &to,
            router_notifications::default_locale(),
            data,
        )
        .await
        {
            tracing::warn!(kind = %kind, to = %to, error = %err, "send topup notification failed");
        }
    }
    Ok(Json(serde_json::json!({"ok": true})))
}

fn webhook_topup_id(input: &DodoWebhook) -> Option<Uuid> {
    input
        .topup_id
        .or_else(|| uuid_from_json_path(input.metadata.as_ref(), "topup_id"))
        .or_else(|| uuid_from_json_path(input.data.as_ref(), "topup_id"))
        .or_else(|| {
            input
                .data
                .as_ref()
                .and_then(|value| value.get("metadata"))
                .and_then(|value| uuid_from_json_path(Some(value), "topup_id"))
        })
}

fn webhook_provider_payment_id(input: &DodoWebhook) -> Option<String> {
    input
        .provider_payment_id
        .clone()
        .or_else(|| input.payment_id.clone())
        .or_else(|| input.session_id.clone())
        .or_else(|| string_from_json_path(input.data.as_ref(), "payment_id"))
        .or_else(|| string_from_json_path(input.data.as_ref(), "session_id"))
        .or_else(|| string_from_json_path(input.data.as_ref(), "id"))
}

fn webhook_payment_method_type(input: &DodoWebhook) -> Option<String> {
    for key in [
        "payment_method_type",
        "payment_method",
        "paymentMethod",
        "payment_method_name",
        "paymentMethodType",
    ] {
        if let Some(value) = string_from_json_path(input.data.as_ref(), key)
            .or_else(|| string_from_json_path(input.metadata.as_ref(), key))
        {
            return Some(value);
        }
    }
    input
        .data
        .as_ref()
        .and_then(|value| {
            value
                .get("payment_method")
                .or_else(|| value.get("paymentMethod"))
                .or_else(|| value.get("payment"))
        })
        .and_then(|value| {
            value
                .get("type")
                .or_else(|| value.get("method"))
                .or_else(|| value.get("name"))
                .and_then(|item| item.as_str())
        })
        .map(ToOwned::to_owned)
}

#[derive(Clone, Copy)]
enum DodoEventKind {
    PaymentSucceeded,
    PaymentFailed,
    RefundOrChargeback,
    Ignored,
}

struct DodoEventAction {
    kind: DodoEventKind,
    webhook_status: &'static str,
}

fn dodo_event_action(event_type: &str) -> DodoEventAction {
    match event_type {
        "payment.succeeded" | "payment_succeeded" | "checkout.session.completed" => {
            DodoEventAction {
                kind: DodoEventKind::PaymentSucceeded,
                webhook_status: "processed",
            }
        }
        "payment.failed" | "payment_failed" => DodoEventAction {
            kind: DodoEventKind::PaymentFailed,
            webhook_status: "processed",
        },
        "refund.succeeded" | "refund.created" | "payment.refunded" | "chargeback.created"
        | "dispute.opened" | "dispute_opened" => DodoEventAction {
            kind: DodoEventKind::RefundOrChargeback,
            webhook_status: "processed",
        },
        _ => DodoEventAction {
            kind: DodoEventKind::Ignored,
            webhook_status: "ignored",
        },
    }
}

struct DodoWebhookHeaders {
    webhook_id: Option<String>,
}

fn validate_webhook_signature(
    state: &AppState,
    headers: &HeaderMap,
    body: &[u8],
) -> Result<DodoWebhookHeaders, ApiError> {
    let secret = state.config.dodo_webhook_secret.trim();
    if secret.is_empty() || secret == "dev" {
        return Ok(DodoWebhookHeaders {
            webhook_id: header_string(headers, "webhook-id"),
        });
    }
    let webhook_id = header_string(headers, "webhook-id")
        .ok_or_else(|| ApiError::unauthorized("missing Dodo webhook-id"))?;
    let timestamp = header_string(headers, "webhook-timestamp")
        .ok_or_else(|| ApiError::unauthorized("missing Dodo webhook-timestamp"))?;
    let signature = header_string(headers, "webhook-signature")
        .ok_or_else(|| ApiError::unauthorized("missing Dodo webhook-signature"))?;
    let timestamp_secs = timestamp
        .parse::<i64>()
        .map_err(|_| ApiError::unauthorized("invalid Dodo webhook timestamp"))?;
    if (Utc::now().timestamp() - timestamp_secs).abs() > 300 {
        return Err(ApiError::unauthorized("stale Dodo webhook timestamp"));
    }
    let body = std::str::from_utf8(body)
        .map_err(|_| ApiError::bad_request("invalid_webhook_body", "webhook body must be utf-8"))?;
    let signed_payload = format!("{webhook_id}.{timestamp}.{body}");
    let expected = hmac_sha256(dodo_secret_bytes(secret), signed_payload.as_bytes())?;
    if !signature_matches(&signature, &expected) {
        return Err(ApiError::unauthorized("invalid Dodo webhook signature"));
    }
    Ok(DodoWebhookHeaders {
        webhook_id: Some(webhook_id),
    })
}

fn header_string(headers: &HeaderMap, key: &'static str) -> Option<String> {
    headers
        .get(key)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn dodo_secret_bytes(secret: &str) -> Vec<u8> {
    secret
        .strip_prefix("whsec_")
        .and_then(|value| STANDARD.decode(value).ok())
        .unwrap_or_else(|| secret.as_bytes().to_vec())
}

fn hmac_sha256(secret: Vec<u8>, payload: &[u8]) -> Result<Vec<u8>, ApiError> {
    let mut mac = Hmac::<Sha256>::new_from_slice(&secret)
        .map_err(|_| ApiError::bad_request("invalid_dodo_secret", "invalid Dodo webhook secret"))?;
    mac.update(payload);
    Ok(mac.finalize().into_bytes().to_vec())
}

#[cfg(test)]
mod tests {
    use super::{FIRST_TOPUP_LIMIT_MESSAGE, FIRST_TOPUP_MAX_USD, first_topup_exceeds_limit};
    use rust_decimal::Decimal;

    #[test]
    fn first_topup_limit_constants_match_policy() {
        assert_eq!(FIRST_TOPUP_MAX_USD, 10);
        assert_eq!(
            FIRST_TOPUP_LIMIT_MESSAGE,
            "首冲最多 10$，建议先小额体验后再充值更多"
        );
    }

    #[test]
    fn first_topup_limit_only_blocks_first_successful_topup_over_ten() {
        assert!(!first_topup_exceeds_limit(Decimal::new(999, 2), 0));
        assert!(!first_topup_exceeds_limit(Decimal::new(1000, 2), 0));
        assert!(first_topup_exceeds_limit(Decimal::new(1001, 2), 0));
        assert!(!first_topup_exceeds_limit(Decimal::new(1001, 2), 1));
    }
}

fn signature_matches(header: &str, expected: &[u8]) -> bool {
    let expected_hex = hex::encode(expected);
    let expected_b64 = STANDARD.encode(expected);
    header
        .split([',', ' '])
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != "v1")
        .any(|value| value == expected_hex || value == expected_b64)
        || header.contains(&expected_hex)
        || header.contains(&expected_b64)
}

fn uuid_from_json_path(value: Option<&serde_json::Value>, key: &str) -> Option<Uuid> {
    value
        .and_then(|value| value.get(key))
        .and_then(|value| value.as_str())
        .and_then(|value| Uuid::parse_str(value).ok())
}

fn string_from_json_path(value: Option<&serde_json::Value>, key: &str) -> Option<String> {
    value
        .and_then(|value| value.get(key))
        .and_then(|value| value.as_str())
        .map(ToOwned::to_owned)
}

fn row_to_topup(row: crate::db::DbRow) -> TopupOrder {
    TopupOrder {
        id: row.uuid("id"),
        gross_amount: row.decimal("gross_amount"),
        fee_amount: row.decimal("fee_amount"),
        net_amount: row.decimal("net_amount"),
        status: row.string("status"),
        checkout_url: row.opt_string("checkout_url"),
    }
}
