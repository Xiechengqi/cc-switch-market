use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use tokio::time::{Duration, sleep};

use crate::{app_state::AppState, error::ApiError};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareSupport {
    #[serde(default)]
    pub claude: bool,
    #[serde(default)]
    pub codex: bool,
    #[serde(default)]
    pub gemini: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareUpstreamModel {
    pub slot: String,
    pub actual_model: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareUpstreamProvider {
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub app: String,
    #[serde(default)]
    pub for_sale_official_price_percent: Option<u16>,
    #[serde(default)]
    pub api_url: Option<String>,
    #[serde(default)]
    pub models: Vec<ShareUpstreamModel>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareAppRuntimes {
    #[serde(default)]
    pub claude: Option<ShareUpstreamProvider>,
    #[serde(default)]
    pub codex: Option<ShareUpstreamProvider>,
    #[serde(default)]
    pub gemini: Option<ShareUpstreamProvider>,
}

/// Router-computed scheduling signals. Mirrors the camelCase struct emitted by
/// the router (`crate::models::ShareSignals` in cc-switch-router). All values
/// are normalized so larger == preferred; `owner_penalty == 1.0` means no
/// penalty. Missing on responses from a pre-Sprint-1 router; defaults below
/// keep the share schedulable.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareSignals {
    #[serde(default = "default_quota_health")]
    pub quota_health: f64,
    #[serde(default = "default_stability")]
    pub stability: f64,
    #[serde(default = "default_headroom")]
    pub headroom: f64,
    #[serde(default)]
    pub samples_10m: u32,
    #[serde(default = "default_owner_penalty")]
    pub owner_penalty: f64,
}

impl Default for ShareSignals {
    fn default() -> Self {
        Self {
            quota_health: default_quota_health(),
            stability: default_stability(),
            headroom: default_headroom(),
            samples_10m: 0,
            owner_penalty: default_owner_penalty(),
        }
    }
}

fn default_quota_health() -> f64 {
    0.5
}
fn default_stability() -> f64 {
    1.0
}
fn default_headroom() -> f64 {
    1.0
}
fn default_owner_penalty() -> f64 {
    1.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouterShare {
    pub router_id: Option<String>,
    pub share_id: String,
    pub subdomain: Option<String>,
    #[serde(default, alias = "api_url")]
    pub api_url: Option<String>,
    pub installation_id: Option<String>,
    pub owner_email: Option<String>,
    pub installation_owner_email: Option<String>,
    pub app_type: Option<String>,
    pub for_sale: Option<String>,
    pub share_status: Option<String>,
    pub online: Option<bool>,
    pub active_requests: Option<i32>,
    pub parallel_limit: Option<i32>,
    pub online_rate_24h: Option<rust_decimal::Decimal>,
    #[serde(default)]
    pub disabled_by_market: bool,
    #[serde(default)]
    pub market_disabled_at: Option<String>,
    #[serde(default)]
    pub support: ShareSupport,
    #[serde(default)]
    pub app_runtimes: ShareAppRuntimes,
    #[serde(default)]
    pub signals: ShareSignals,
    #[serde(default)]
    pub share_created_at: Option<String>,
}

/// Report a 429/rate_limited event to the router so it scopes a per-owner
/// penalty in its in-memory `OverrideStore`. Fire-and-forget: any error is
/// logged and swallowed. The request hot-path must not block on this.
///
/// The router resolves `owner_email` from `share_id`; we don't need to know
/// the owner here.
pub async fn report_rate_limited(state: &AppState, router_id: &str, share_id: &str) {
    // We only call the router for the canonical "main" router_id today —
    // multi-router routing isn't wired yet, and the feedback endpoint speaks
    // to a single base URL anyway.
    let _ = router_id;
    let state = state.clone();
    let share_id = share_id.to_string();
    tokio::spawn(async move {
        if let Err(err) = report_rate_limited_inner(&state, &share_id).await {
            tracing::warn!(%share_id, error = %err, "router feedback POST failed");
        }
    });
}

async fn report_rate_limited_inner(state: &AppState, share_id: &str) -> Result<(), ApiError> {
    let access_token = crate::router_account::access_token(&state.config)
        .await
        .map_err(|e| ApiError::service_unavailable(format!("router login required: {e}")))?;
    let url = format!(
        "{}/v1/market/shares/feedback",
        state.config.router_api_base_url.trim_end_matches('/')
    );
    let body = serde_json::json!({
        "shareId": share_id,
        "kind": "rate_limited",
    });
    let response = state
        .http
        .post(url)
        .bearer_auth(access_token)
        .json(&body)
        .send()
        .await
        .map_err(|e| ApiError::service_unavailable(format!("router feedback failed: {e}")))?;
    if !response.status().is_success() {
        return Err(ApiError::service_unavailable(format!(
            "router feedback returned {}",
            response.status()
        )));
    }
    Ok(())
}

pub async fn sync_shares(state: &AppState) -> Result<usize, ApiError> {
    let access_token = crate::router_account::access_token(&state.config)
        .await
        .map_err(|e| ApiError::service_unavailable(format!("router login required: {e}")))?;
    let url = format!(
        "{}/v1/market/shares",
        state.config.router_api_base_url.trim_end_matches('/')
    );
    let response = state
        .http
        .get(url)
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|e| ApiError::service_unavailable(format!("router share sync failed: {e}")))?;
    if !response.status().is_success() {
        return Err(ApiError::service_unavailable(format!(
            "router share sync returned {}",
            response.status()
        )));
    }
    let shares: Vec<serde_json::Value> = response.json().await.unwrap_or_default();
    let db = state.db();
    let tx = db.begin_immediate().await?;
    let mut count = 0;
    let mut seen = HashSet::new();
    let mut had_invalid_share = false;
    for raw_share in shares {
        let share: RouterShare = match serde_json::from_value(raw_share.clone()) {
            Ok(share) => share,
            Err(err) => {
                tracing::warn!(error = %err, "skip invalid router share");
                had_invalid_share = true;
                continue;
            }
        };
        let owner = share
            .owner_email
            .clone()
            .or(share.installation_owner_email.clone());
        if owner.is_none() {
            continue;
        }
        let router_id = share
            .router_id
            .clone()
            .unwrap_or_else(|| "main".to_string());
        seen.insert((router_id.clone(), share.share_id.clone()));
        let app_type = share
            .app_type
            .clone()
            .unwrap_or_else(|| "openai".to_string());
        tx.execute(
            r#"
            INSERT INTO router_shares
              (router_id, share_id, subdomain, installation_id, owner_email, installation_owner_email, app_type, for_sale, share_status, online, active_requests, parallel_limit, online_rate_24h, enabled_claude, enabled_codex, enabled_gemini, disabled_by_market, market_disabled_at, raw_json, last_seen_at, last_success_at,
               quota_health, stability, headroom, samples_10m, owner_penalty, share_created_at)
            VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?20,
                    ?21,?22,?23,?24,?25,?26)
            ON CONFLICT (router_id, share_id) DO UPDATE SET
              subdomain = excluded.subdomain,
              installation_id = excluded.installation_id,
              owner_email = excluded.owner_email,
              installation_owner_email = excluded.installation_owner_email,
              app_type = excluded.app_type,
              for_sale = excluded.for_sale,
              share_status = excluded.share_status,
              online = excluded.online,
              active_requests = excluded.active_requests,
              parallel_limit = excluded.parallel_limit,
              online_rate_24h = excluded.online_rate_24h,
              enabled_claude = excluded.enabled_claude,
              enabled_codex = excluded.enabled_codex,
              enabled_gemini = excluded.enabled_gemini,
              disabled_by_market = excluded.disabled_by_market,
              market_disabled_at = excluded.market_disabled_at,
              raw_json = excluded.raw_json,
              last_seen_at = ?20,
              quota_health = excluded.quota_health,
              stability = excluded.stability,
              headroom = excluded.headroom,
              samples_10m = excluded.samples_10m,
              owner_penalty = excluded.owner_penalty,
              share_created_at = excluded.share_created_at
            "#,
            vec![
                crate::db::val(&router_id),
                crate::db::val(&share.share_id),
                crate::db::opt_val(share.subdomain.clone()),
                crate::db::opt_val(share.installation_id.clone()),
                crate::db::opt_val(owner.clone()),
                crate::db::opt_val(share.installation_owner_email.clone()),
                crate::db::val(app_type),
                crate::db::val(share.for_sale.clone().unwrap_or_else(|| "Yes".to_string())),
                crate::db::val(share.share_status.clone().unwrap_or_else(|| "active".to_string())),
                crate::db::val(share.online.unwrap_or(true)),
                crate::db::val(share.active_requests.unwrap_or(0) as i64),
                crate::db::val(share.parallel_limit.unwrap_or(3) as i64),
                crate::db::dec_val(share.online_rate_24h.unwrap_or(rust_decimal::Decimal::ONE)),
                crate::db::val(share.support.claude),
                crate::db::val(share.support.codex),
                crate::db::val(share.support.gemini),
                crate::db::val(share.disabled_by_market),
                crate::db::opt_val(share.market_disabled_at.clone()),
                crate::db::json_val(raw_share),
                crate::db::val(crate::db::now_string()),
                crate::db::val(share.signals.quota_health),
                crate::db::val(share.signals.stability),
                crate::db::val(share.signals.headroom),
                crate::db::val(share.signals.samples_10m as i64),
                crate::db::val(share.signals.owner_penalty),
                crate::db::opt_val(share.share_created_at.clone()),
            ],
        )
        .await?;
        sync_share_model_support(&tx, &router_id, &share.share_id, &share.app_runtimes).await?;
        count += 1;
    }
    if !had_invalid_share {
        prune_missing_router_shares(&tx, &seen).await?;
    }
    tx.commit().await?;
    Ok(count)
}

async fn prune_missing_router_shares(
    tx: &crate::db::DbTx,
    seen: &HashSet<(String, String)>,
) -> Result<(), ApiError> {
    let rows = tx
        .query_all("SELECT router_id, share_id FROM router_shares", vec![])
        .await?;
    for row in rows {
        let router_id = row.string("router_id");
        let share_id = row.string("share_id");
        if seen.contains(&(router_id.clone(), share_id.clone())) {
            continue;
        }
        tx.execute(
            "DELETE FROM router_share_model_support WHERE router_id=?1 AND share_id=?2",
            vec![crate::db::val(&router_id), crate::db::val(&share_id)],
        )
        .await?;
        tx.execute(
            "DELETE FROM router_shares WHERE router_id=?1 AND share_id=?2",
            vec![crate::db::val(&router_id), crate::db::val(&share_id)],
        )
        .await?;
        tracing::info!(%router_id, %share_id, "pruned router share missing from latest sync");
    }
    Ok(())
}

async fn sync_share_model_support(
    tx: &crate::db::DbTx,
    router_id: &str,
    share_id: &str,
    app_runtimes: &ShareAppRuntimes,
) -> Result<(), ApiError> {
    tx.execute(
        "DELETE FROM router_share_model_support WHERE router_id=?1 AND share_id=?2",
        vec![crate::db::val(router_id), crate::db::val(share_id)],
    )
    .await?;
    let now = crate::db::now_string();
    for (app, runtime) in [
        ("claude", app_runtimes.claude.as_ref()),
        ("codex", app_runtimes.codex.as_ref()),
        ("gemini", app_runtimes.gemini.as_ref()),
    ] {
        let Some(runtime) = runtime else {
            continue;
        };
        let official = runtime.kind == "official_oauth";
        if official {
            tx.execute(
                r#"
                INSERT OR REPLACE INTO router_share_model_support
                  (router_id, share_id, app, slot, actual_model, official, api_url, provider_kind, updated_at)
                VALUES (?1,?2,?3,'official',NULL,1,?4,?5,?6)
                "#,
                vec![
                    crate::db::val(router_id),
                    crate::db::val(share_id),
                    crate::db::val(app),
                    crate::db::opt_val(runtime.api_url.clone()),
                    crate::db::val(&runtime.kind),
                    crate::db::val(&now),
                ],
            )
            .await?;
            continue;
        }
        for model in &runtime.models {
            let slot = model.slot.trim();
            let actual_model = model.actual_model.trim();
            if slot.is_empty() || actual_model.is_empty() {
                continue;
            }
            tx.execute(
                r#"
                INSERT OR REPLACE INTO router_share_model_support
                  (router_id, share_id, app, slot, actual_model, official, api_url, provider_kind, updated_at)
                VALUES (?1,?2,?3,?4,?5,0,?6,?7,?8)
                "#,
                vec![
                    crate::db::val(router_id),
                    crate::db::val(share_id),
                    crate::db::val(app),
                    crate::db::val(slot),
                    crate::db::val(actual_model),
                    crate::db::opt_val(runtime.api_url.clone()),
                    crate::db::val(&runtime.kind),
                    crate::db::val(&now),
                ],
            )
            .await?;
        }
    }
    Ok(())
}

pub fn spawn_share_sync(state: AppState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            match sync_shares(&state).await {
                Ok(count) => tracing::debug!(synced = count, "router share sync completed"),
                Err(err) => tracing::warn!(error = %err, "router share sync failed"),
            }
            sleep(Duration::from_secs(30)).await;
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn router_share_deserializes_router_camel_case_contract() {
        let share: RouterShare = serde_json::from_value(serde_json::json!({
            "routerId": "main",
            "shareId": "share-1",
            "installationId": "inst-1",
            "ownerEmail": "owner@example.com",
            "installationOwnerEmail": null,
            "appType": "openai",
            "forSale": "Yes",
            "shareStatus": "active",
            "online": true,
            "activeRequests": 2,
            "parallelLimit": 3,
            "onlineRate24h": 0.5
        }))
        .expect("router share contract");

        assert_eq!(share.router_id.as_deref(), Some("main"));
        assert_eq!(share.share_id, "share-1");
        assert_eq!(share.for_sale.as_deref(), Some("Yes"));
        assert_eq!(share.online, Some(true));
        assert_eq!(share.active_requests, Some(2));
        assert_eq!(
            share.online_rate_24h,
            Some("0.5".parse::<rust_decimal::Decimal>().expect("decimal"))
        );
        // Missing signals → fall back to permissive defaults so the share is
        // still schedulable when talking to a pre-Sprint-1 router.
        assert!((share.signals.quota_health - 0.5).abs() < 1e-9);
        assert!((share.signals.stability - 1.0).abs() < 1e-9);
        assert!((share.signals.headroom - 1.0).abs() < 1e-9);
        assert_eq!(share.signals.samples_10m, 0);
        assert!((share.signals.owner_penalty - 1.0).abs() < 1e-9);
        assert!(share.share_created_at.is_none());
    }

    #[test]
    fn router_share_parses_signals_when_present() {
        let share: RouterShare = serde_json::from_value(serde_json::json!({
            "routerId": "main",
            "shareId": "share-2",
            "signals": {
                "quotaHealth": 0.72,
                "stability": 0.91,
                "headroom": 0.4,
                "samples10m": 7,
                "ownerPenalty": 0.5,
            },
            "shareCreatedAt": "2026-01-01T00:00:00Z",
        }))
        .expect("router share with signals");

        assert!((share.signals.quota_health - 0.72).abs() < 1e-9);
        assert!((share.signals.stability - 0.91).abs() < 1e-9);
        assert!((share.signals.headroom - 0.4).abs() < 1e-9);
        assert_eq!(share.signals.samples_10m, 7);
        assert!((share.signals.owner_penalty - 0.5).abs() < 1e-9);
        assert_eq!(share.share_created_at.as_deref(), Some("2026-01-01T00:00:00Z"));
    }
}
