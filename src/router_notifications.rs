use std::time::Duration;

use anyhow::{Context, anyhow};
use serde::Serialize;
use serde_json::Value;

use crate::{config::Config, router_account};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MarketNotificationRequest<'a> {
    kind: &'a str,
    to: &'a str,
    locale: &'a str,
    data: Value,
}

pub async fn send_notification(
    config: &Config,
    kind: &str,
    to: &str,
    locale: &str,
    data: Value,
) -> anyhow::Result<()> {
    let session = router_account::refresh_session(config).await?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()?;
    let response = client
        .post(format!(
            "{}/v1/market/notifications/email",
            config.router_api_base_url.trim_end_matches('/')
        ))
        .bearer_auth(&session.access_token)
        .json(&MarketNotificationRequest {
            kind,
            to,
            locale,
            data,
        })
        .send()
        .await
        .with_context(|| format!("send router notification {kind} failed"))?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(anyhow!(
            "router notification {} returned {}: {}",
            kind,
            status,
            body
        ));
    }
    Ok(())
}

pub fn default_locale() -> &'static str {
    "zh-CN"
}
