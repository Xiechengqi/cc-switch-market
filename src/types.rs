use axum::{Json, extract::State};
use serde::Serialize;

use crate::app_state::AppState;

#[derive(Serialize)]
pub struct Health {
    ok: bool,
    database: bool,
    database_writable: bool,
    database_mode: String,
    database_path: String,
    database_url: Option<String>,
    last_backup_at: Option<chrono::DateTime<chrono::Utc>>,
    object_store_backend: String,
    object_store_path: String,
    object_store_writable: bool,
    router_shares_cached: i64,
    router_last_seen_at: Option<String>,
    router_sync: serde_json::Value,
    ledger_consistent: bool,
    ledger_details: serde_json::Value,
    version: &'static str,
}

pub async fn healthz(State(state): State<AppState>) -> Json<Health> {
    let database = state.db().execute("SELECT 1", vec![]).await.is_ok();
    let database_writable = db_write_probe(state.db()).await;
    let ledger_details = crate::ledger::consistency_report(state.db())
        .await
        .unwrap_or_else(|err| serde_json::json!({"ok": false, "error": err.to_string()}));
    let ledger_consistent = ledger_details
        .get("ok")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let router_summary = router_sync_summary(state.db()).await;
    let object_store_writable = state.object_store.health_check().await;
    let router_sync = serde_json::json!({
        "ok": router_summary.0 > 0,
        "lastSuccessAt": router_summary.1,
        "lagSecs": router_summary.2,
    });
    Json(Health {
        ok: database && database_writable && object_store_writable && ledger_consistent,
        database,
        database_writable,
        database_mode: state.db().mode_name().to_string(),
        database_path: state.db().path_for_log(),
        database_url: state.db().database_url_for_log().map(ToOwned::to_owned),
        last_backup_at: state.db().last_backup_at(),
        object_store_backend: state.config.object_store_backend.clone(),
        object_store_path: state.object_store.root_for_log(),
        object_store_writable,
        router_shares_cached: router_summary.0,
        router_last_seen_at: router_summary.1.clone(),
        router_sync,
        ledger_consistent,
        ledger_details,
        version: env!("CARGO_PKG_VERSION"),
    })
}

#[derive(Serialize)]
pub struct Version {
    name: &'static str,
    version: &'static str,
}

pub async fn version() -> Json<Version> {
    Json(Version {
        name: env!("CARGO_PKG_NAME"),
        version: env!("CARGO_PKG_VERSION"),
    })
}

#[derive(Serialize)]
pub struct PublicInfo {
    market_display_name: String,
    market_public_base_url: String,
    auth_provider: &'static str,
    supports_claim: bool,
    supports_gateio: bool,
    supports_manual_payout_tickets: bool,
}

pub async fn public_info(State(state): State<AppState>) -> Json<PublicInfo> {
    Json(PublicInfo {
        market_display_name: state.config.market_display_name,
        market_public_base_url: state.config.market_public_base_url,
        auth_provider: "router_resend",
        supports_claim: true,
        supports_gateio: true,
        supports_manual_payout_tickets: true,
    })
}

#[derive(Serialize)]
pub struct PublicConfig {
    #[serde(rename = "platformCommissionBps")]
    platform_commission_bps: i64,
    #[serde(rename = "platformCommissionRate")]
    platform_commission_rate: String,
    #[serde(rename = "platformCommissionDecimal")]
    platform_commission_decimal: String,
    #[serde(rename = "marketCommissionBps")]
    market_commission_bps: i64,
    #[serde(rename = "marketCommissionRate")]
    market_commission_rate: String,
    #[serde(rename = "routerCommissionBps")]
    router_commission_bps: i64,
    #[serde(rename = "routerCommissionRate")]
    router_commission_rate: String,
    #[serde(rename = "totalCommissionBps")]
    total_commission_bps: i64,
    #[serde(rename = "totalCommissionRate")]
    total_commission_rate: String,
    #[serde(rename = "timeZoneOffsetMinutes")]
    time_zone_offset_minutes: i64,
    #[serde(rename = "adminTablePageSize")]
    admin_table_page_size: i64,
    #[serde(rename = "marketPublicBaseUrl")]
    market_public_base_url: String,
    #[serde(rename = "routerApiBaseUrl")]
    router_api_base_url: String,
    #[serde(rename = "cloudflareTurnstileSiteKey")]
    cloudflare_turnstile_site_key: String,
    #[serde(rename = "footerLinks")]
    footer_links: Vec<crate::admin::FooterLink>,
}

pub async fn public_config(State(state): State<AppState>) -> Json<PublicConfig> {
    let market_bps = state.config.market_platform_commission_bps;
    let router_bps = state.config.market_router_commission_bps;
    let total_bps = market_bps + router_bps;
    let time_zone_offset_minutes = crate::admin::read_time_zone_offset_minutes(&state)
        .await
        .unwrap_or(480);
    let admin_table_page_size = crate::admin::read_admin_table_page_size(&state)
        .await
        .unwrap_or(20);
    let footer_links = crate::admin::read_footer_links(&state)
        .await
        .unwrap_or_else(|_| crate::admin::default_footer_links());
    Json(PublicConfig {
        platform_commission_bps: total_bps,
        platform_commission_rate: format_bps_percent(total_bps),
        platform_commission_decimal: format!("{:.4}", total_bps as f64 / 10_000.0),
        market_commission_bps: market_bps,
        market_commission_rate: format_bps_percent(market_bps),
        router_commission_bps: router_bps,
        router_commission_rate: format_bps_percent(router_bps),
        total_commission_bps: total_bps,
        total_commission_rate: format_bps_percent(total_bps),
        time_zone_offset_minutes,
        admin_table_page_size,
        market_public_base_url: state.config.market_public_base_url.clone(),
        router_api_base_url: state.config.router_api_base_url.clone(),
        cloudflare_turnstile_site_key: if state.config.cloudflare_turnstile_enabled() {
            state.config.cloudflare_turnstile_site_key.clone()
        } else {
            String::new()
        },
        footer_links,
    })
}

fn format_bps_percent(bps: i64) -> String {
    let value = bps as f64 / 100.0;
    if bps % 100 == 0 {
        format!("{}%", value as i64)
    } else if bps % 10 == 0 {
        format!("{value:.1}%")
    } else {
        format!("{value:.2}%")
    }
}

#[derive(Serialize)]
pub struct Metrics {
    ok: bool,
    total_requests: i64,
    settled_requests: i64,
    failed_requests: i64,
    reserved_requests: i64,
    streaming_requests: i64,
    needs_review_requests: i64,
    pending_payouts: i64,
    open_tickets: i64,
    router_shares_cached: i64,
    routeable_shares: i64,
    share_success_events: i64,
    share_error_events: i64,
    webhooks_processed: i64,
    webhooks_rejected_or_ignored: i64,
    gateio_attempts_succeeded: i64,
    gateio_attempts_failed: i64,
    ledger_consistent: bool,
    object_store_writable: bool,
    http: crate::app_state::MetricsSnapshot,
}

pub async fn metrics(State(state): State<AppState>) -> Json<Metrics> {
    async fn count(db: &crate::db::Db, sql: &str) -> i64 {
        db.query_optional(sql, vec![])
            .await
            .ok()
            .flatten()
            .map(|row| row.i64("count"))
            .unwrap_or(0)
    }
    let total_requests = count(state.db(), "SELECT COUNT(*) AS count FROM request_charges").await;
    let settled_requests = count(
        state.db(),
        "SELECT COUNT(*) AS count FROM request_charges WHERE status='settled'",
    )
    .await;
    let failed_requests = count(
        state.db(),
        "SELECT COUNT(*) AS count FROM request_charges WHERE status LIKE 'failed%'",
    )
    .await;
    let reserved_requests = count(
        state.db(),
        "SELECT COUNT(*) AS count FROM request_charges WHERE status='reserved'",
    )
    .await;
    let streaming_requests = count(
        state.db(),
        "SELECT COUNT(*) AS count FROM request_charges WHERE status='streaming'",
    )
    .await;
    let needs_review_requests = count(
        state.db(),
        "SELECT COUNT(*) AS count FROM request_charges WHERE status='needs_review'",
    )
    .await;
    let pending_payouts = count(
        state.db(),
        "SELECT COUNT(*) AS count FROM payout_requests WHERE status IN ('pending','processing','needs_review')",
    )
    .await;
    let open_tickets = count(
        state.db(),
        "SELECT COUNT(*) AS count FROM tickets WHERE status NOT IN ('resolved','closed')",
    )
    .await;
    let ledger_consistent = crate::ledger::consistency_report(state.db())
        .await
        .map(|value| value.get("ok").and_then(|v| v.as_bool()).unwrap_or(false))
        .unwrap_or(false);
    let object_store_writable = state.object_store.health_check().await;
    let router_shares_cached =
        count(state.db(), "SELECT COUNT(*) AS count FROM router_shares").await;
    let routeable_shares = count(
        state.db(),
        "SELECT COUNT(*) AS count FROM router_shares WHERE online=1 AND share_status='active' AND for_sale='Yes'",
    )
    .await;
    let share_success_events = count(
        state.db(),
        "SELECT COUNT(*) AS count FROM share_health WHERE status='success'",
    )
    .await;
    let share_error_events = count(
        state.db(),
        "SELECT COUNT(*) AS count FROM share_health WHERE status='error'",
    )
    .await;
    let webhooks_processed = count(
        state.db(),
        "SELECT COUNT(*) AS count FROM processed_webhooks WHERE status='processed'",
    )
    .await;
    let webhooks_rejected_or_ignored = count(
        state.db(),
        "SELECT COUNT(*) AS count FROM processed_webhooks WHERE status <> 'processed'",
    )
    .await;
    let gateio_attempts_succeeded = count(
        state.db(),
        "SELECT COUNT(*) AS count FROM payout_attempts WHERE method='gateio' AND status='succeeded'",
    )
    .await;
    let gateio_attempts_failed = count(
        state.db(),
        "SELECT COUNT(*) AS count FROM payout_attempts WHERE method='gateio' AND status='failed'",
    )
    .await;
    Json(Metrics {
        ok: ledger_consistent && object_store_writable,
        total_requests,
        settled_requests,
        failed_requests,
        reserved_requests,
        streaming_requests,
        needs_review_requests,
        pending_payouts,
        open_tickets,
        router_shares_cached,
        routeable_shares,
        share_success_events,
        share_error_events,
        webhooks_processed,
        webhooks_rejected_or_ignored,
        gateio_attempts_succeeded,
        gateio_attempts_failed,
        ledger_consistent,
        object_store_writable,
        http: state.metrics.snapshot(),
    })
}

async fn router_sync_summary(db: &crate::db::Db) -> (i64, Option<String>, Option<i64>) {
    db.query_optional(
        "SELECT COUNT(*) AS count, MAX(last_seen_at) AS last_seen_at FROM router_shares",
        vec![],
    )
    .await
    .ok()
    .flatten()
    .map(|row| {
        let last_seen = row.opt_string("last_seen_at");
        let lag = last_seen
            .as_deref()
            .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
            .map(|value| (chrono::Utc::now() - value.with_timezone(&chrono::Utc)).num_seconds());
        (row.i64("count"), last_seen, lag)
    })
    .unwrap_or((0, None, None))
}

async fn db_write_probe(db: &crate::db::Db) -> bool {
    let id = uuid::Uuid::new_v4();
    let Ok(tx) = db.begin_immediate().await else {
        return false;
    };
    if tx
        .execute(
            "INSERT INTO health_checks (id, created_at) VALUES (?1, ?2)",
            vec![
                crate::db::uuid_val(id),
                crate::db::val(crate::db::now_string()),
            ],
        )
        .await
        .is_err()
    {
        return false;
    }
    if tx
        .execute(
            "DELETE FROM health_checks WHERE id=?1",
            vec![crate::db::uuid_val(id)],
        )
        .await
        .is_err()
    {
        return false;
    }
    tx.commit().await.is_ok()
}

pub async fn docs() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "name": env!("CARGO_PKG_NAME"),
        "version": env!("CARGO_PKG_VERSION"),
        "auth": {
            "web": "router_resend_email_code",
            "api": "market_api_key"
        },
        "endpoints": {
            "public": [
                "GET /v1/healthz",
                "GET /v1/version",
                "GET /v1/public/info",
                "GET /v1/metrics"
            ],
            "auth": [
                "POST /v1/auth/email/request-code",
                "POST /v1/auth/email/verify-code",
                "POST /v1/auth/logout",
                "GET /v1/me"
            ],
            "user": [
                "GET/POST /v1/api-keys",
                "POST /v1/topups/checkout",
                "GET /v1/topups/{id}",
                "GET /v1/wallet/ledger",
                "GET /v1/usage"
            ],
            "proxy": [
                "POST /v1/chat/completions",
                "POST /v1/messages"
            ],
            "provider": [
                "GET /v1/provider/claim/summary",
                "GET /v1/provider/earnings",
                "POST /v1/provider/claim/payout",
                "POST /v1/provider/claim/payout-ticket",
                "GET /v1/provider/claim/payouts"
            ],
            "support": [
                "POST /v1/ticket-attachments/presign",
                "GET/POST /v1/tickets",
                "GET /v1/tickets/{id}",
                "POST /v1/tickets/{id}/messages",
                "GET /market-api/object-download/{object_key}"
            ],
            "admin": [
                "GET /v1/admin/users",
                "GET /v1/admin/topups",
                "GET /v1/admin/shares",
                "GET /v1/admin/charges",
                "GET /v1/admin/ledger/check",
                "GET /v1/admin/payout-requests",
                "GET /v1/admin/tickets"
            ]
        }
    }))
}
