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
              (router_id, share_id, subdomain, installation_id, owner_email, installation_owner_email, app_type, for_sale, share_status, online, active_requests, parallel_limit, online_rate_24h, enabled_claude, enabled_codex, enabled_gemini, disabled_by_market, market_disabled_at, raw_json, last_seen_at, last_success_at)
            VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?20)
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
              last_seen_at = ?20
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
    }
}
