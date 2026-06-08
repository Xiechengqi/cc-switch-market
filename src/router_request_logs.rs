use serde::Serialize;
use serde_json::Value;
use tokio::time::{Duration, sleep};

use crate::{app_state::AppState, error::ApiError};

const SYNC_LIMIT: i64 = 200;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RouterRequestLogBatch<'a> {
    logs: &'a [RouterRequestLog],
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RouterRequestLog {
    request_id: String,
    user_email: Option<String>,
    api_key_prefix: Option<String>,
    router_id: Option<String>,
    share_id: Option<String>,
    share_subdomain: Option<String>,
    model: Option<String>,
    request_agent: String,
    requested_model: String,
    actual_model: String,
    actual_model_source: String,
    status: String,
    status_code: Option<u16>,
    error_message: Option<String>,
    latency_ms: Option<u64>,
    input_tokens: u32,
    output_tokens: u32,
    cache_read_tokens: u32,
    cache_creation_tokens: u32,
    usage_amount_usd: Option<String>,
    created_at: String,
    settled_at: Option<String>,
}

pub async fn sync_recent(state: &AppState) -> Result<usize, ApiError> {
    let rows = state
        .db()
        .query_all(
            r#"
            SELECT rc.request_id, u.email AS user_email, ak.prefix AS api_key_prefix,
                   rc.router_id, rc.share_id,
                   COALESCE(
                       json_extract(rs.raw_json, '$.subdomain'),
                       json_extract(rs.raw_json, '$.apiUrl'),
                       json_extract(rs.raw_json, '$.api_url')
                   ) AS share_subdomain,
                   rc.app_type, rc.model, rc.request_agent, rc.requested_model, rc.actual_model, rc.actual_model_source,
                   rc.pricing_model, rc.pricing_model_source,
                   rc.status, rc.usage_amount, rc.usage_json, rc.audit_flags,
                   rc.created_at, rc.settled_at,
                   ra.latency_ms
              FROM request_charges rc
              JOIN users u ON u.id = rc.user_id
              JOIN api_keys ak ON ak.id = rc.api_key_id
              LEFT JOIN router_shares rs ON rs.router_id = rc.router_id AND rs.share_id = rc.share_id
              LEFT JOIN (
                    SELECT request_id, MAX(latency_ms) AS latency_ms
                      FROM request_attempts
                     WHERE status = 'success'
                     GROUP BY request_id
              ) ra ON ra.request_id = rc.request_id
              LEFT JOIN router_request_log_sync_state sync ON sync.request_id = rc.request_id
             WHERE rc.status IN ('settled','failed_released','needs_review','streaming')
               AND (sync.last_synced_at IS NULL OR COALESCE(rc.settled_at, rc.created_at) > sync.last_synced_at)
             ORDER BY rc.created_at DESC
             LIMIT ?1
            "#,
            vec![crate::db::val(SYNC_LIMIT)],
        )
        .await?;
    if rows.is_empty() {
        return Ok(0);
    }

    let logs = rows
        .iter()
        .map(|row| {
            let usage = row
                .opt_string("usage_json")
                .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
                .unwrap_or(Value::Null);
            let audit_flags = row
                .opt_string("audit_flags")
                .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
                .unwrap_or(Value::Null);
            let error_message = failure_error_message(&audit_flags);
            RouterRequestLog {
                request_id: row.string("request_id"),
                user_email: row.opt_string("user_email"),
                api_key_prefix: row.opt_string("api_key_prefix"),
                router_id: row.opt_string("router_id"),
                share_id: row.opt_string("share_id"),
                share_subdomain: row.opt_string("share_subdomain"),
                model: row.opt_string("model"),
                request_agent: row.opt_string("request_agent").unwrap_or_else(|| {
                    request_agent_for_app_type(&row.string("app_type")).to_string()
                }),
                requested_model: row
                    .opt_string("requested_model")
                    .unwrap_or_else(|| row.string("model")),
                actual_model: row
                    .opt_string("actual_model")
                    .or_else(|| row.opt_string("pricing_model"))
                    .unwrap_or_else(|| row.string("model")),
                actual_model_source: row
                    .opt_string("actual_model_source")
                    .or_else(|| row.opt_string("pricing_model_source"))
                    .unwrap_or_else(|| "official".to_string()),
                status: row.string("status"),
                status_code: status_code_for_charge(
                    &row.string("status"),
                    error_message.as_deref(),
                ),
                error_message,
                latency_ms: row
                    .opt_string("latency_ms")
                    .and_then(|value| value.parse().ok()),
                input_tokens: usage_number(&usage, "input_tokens") as u32,
                output_tokens: usage_number(&usage, "output_tokens") as u32,
                cache_read_tokens: usage_number(&usage, "cache_read_tokens") as u32,
                cache_creation_tokens: usage_number(&usage, "cache_write_tokens") as u32,
                usage_amount_usd: row.opt_string("usage_amount"),
                created_at: row.string("created_at"),
                settled_at: row.opt_string("settled_at"),
            }
        })
        .collect::<Vec<_>>();

    let access_token = crate::router_account::access_token(&state.config)
        .await
        .map_err(|e| ApiError::service_unavailable(format!("router login required: {e}")))?;
    let url = format!(
        "{}/v1/market/request-logs/batch",
        state.config.router_api_base_url.trim_end_matches('/')
    );
    let response = state
        .http
        .post(url)
        .bearer_auth(access_token)
        .json(&RouterRequestLogBatch { logs: &logs })
        .send()
        .await
        .map_err(|e| {
            ApiError::service_unavailable(format!("router request log sync failed: {e}"))
        })?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        mark_sync_error(state, &logs, format!("router returned {status}: {body}")).await;
        return Err(ApiError::service_unavailable(format!(
            "router request log sync returned {status}"
        )));
    }
    mark_sync_success(state, &logs).await;
    Ok(logs.len())
}

pub fn spawn_sync(state: AppState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            if let Err(err) = sync_recent(&state).await {
                tracing::warn!(error = %err, "router request log sync failed");
            }
            sleep(Duration::from_secs(30)).await;
        }
    })
}

async fn mark_sync_success(state: &AppState, logs: &[RouterRequestLog]) {
    let now = crate::db::now_string();
    for log in logs {
        let _ = state
            .db()
            .execute(
                "INSERT INTO router_request_log_sync_state (request_id, last_synced_at, last_error, attempt_count, updated_at)
                 VALUES (?1,?2,NULL,0,?2)
                 ON CONFLICT(request_id) DO UPDATE SET last_synced_at=?2, last_error=NULL, attempt_count=0, updated_at=?2",
                vec![crate::db::val(&log.request_id), crate::db::val(&now)],
            )
            .await;
    }
}

async fn mark_sync_error(state: &AppState, logs: &[RouterRequestLog], message: String) {
    let now = crate::db::now_string();
    for log in logs {
        let _ = state
            .db()
            .execute(
                "INSERT INTO router_request_log_sync_state (request_id, last_synced_at, last_error, attempt_count, updated_at)
                 VALUES (?1,NULL,?2,1,?3)
                 ON CONFLICT(request_id) DO UPDATE SET last_error=?2, attempt_count=attempt_count+1, updated_at=?3",
                vec![
                    crate::db::val(&log.request_id),
                    crate::db::val(&message),
                    crate::db::val(&now),
                ],
            )
            .await;
    }
}

fn usage_number(value: &Value, key: &str) -> u64 {
    value.get(key).and_then(Value::as_u64).unwrap_or(0)
}

fn request_agent_for_app_type(app_type: &str) -> &'static str {
    match app_type {
        "anthropic" | "claude" => "claude",
        "gemini" => "gemini",
        _ => "codex",
    }
}

fn failure_error_message(audit_flags: &Value) -> Option<String> {
    let array = audit_flags.as_array()?;
    let mut messages = Vec::new();
    for value in array {
        if let Some(message) = value.get("message").and_then(Value::as_str) {
            if !message.trim().is_empty() {
                messages.push(message.trim().to_string());
            }
        } else if let Some(code) = value.get("code").and_then(Value::as_str) {
            if !code.trim().is_empty() {
                messages.push(code.trim().to_string());
            }
        }
    }
    (!messages.is_empty()).then(|| messages.join("; "))
}

fn status_code_for_charge(status: &str, error_message: Option<&str>) -> Option<u16> {
    match status {
        "settled" | "streaming" => Some(200),
        "failed_released" => Some(status_code_for_failure(error_message.unwrap_or_default())),
        _ => None,
    }
}

fn status_code_for_failure(message: &str) -> u16 {
    let lower = message.to_ascii_lowercase();
    if lower.contains("429")
        || lower.contains("rate limit")
        || lower.contains("rate_limit")
        || lower.contains("quota exceeded")
        || lower.contains("quota_exceeded")
        || lower.contains("quota exhausted")
        || lower.contains("quota_exhausted")
        || lower.contains("usage limit")
        || lower.contains("usage_limit")
        || lower.contains("usage credits are required")
    {
        429
    } else if lower.contains("400")
        || lower.contains("bad request")
        || lower.contains("model_max_prompt_tokens_exceeded")
        || lower.contains("prompt token count")
        || lower.contains("context length")
        || lower.contains("context_length_exceeded")
    {
        400
    } else if lower.contains("401") {
        401
    } else if lower.contains("403") {
        403
    } else if lower.contains("404") || lower.contains("not found") {
        404
    } else if lower.contains("422") {
        422
    } else {
        500
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failure_status_code_preserves_request_shape_and_rate_limit_errors() {
        assert_eq!(
            status_code_for_charge(
                "failed_released",
                Some("prompt token count of 128078 exceeds the limit of 128000"),
            ),
            Some(400)
        );
        assert_eq!(
            status_code_for_charge(
                "failed_released",
                Some("Usage credits are required for long context requests."),
            ),
            Some(429)
        );
        assert_eq!(status_code_for_charge("failed_released", None), Some(500));
    }

    #[test]
    fn failure_error_message_extracts_audit_object_messages() {
        let flags = serde_json::json!([
            {"code": "upstream_failed", "message": "router market proxy returned 400 Bad Request"},
            "manual_flag"
        ]);
        assert_eq!(
            failure_error_message(&flags).as_deref(),
            Some("router market proxy returned 400 Bad Request")
        );
    }
}
