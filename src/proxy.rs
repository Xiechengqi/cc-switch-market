use axum::{
    Json,
    body::{Body, Bytes},
    extract::{FromRequestParts, Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header, request::Parts},
    response::{IntoResponse, Response},
};
use chrono::Datelike;
use futures_util::StreamExt;
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::{Map, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    app_state::AppState,
    auth::ApiKeyPrincipal,
    error::ApiError,
    failure::FailureKind,
    ledger::{self, AccountRef},
    pricing,
    usage::{SseUsageParser, UsageProtocol},
};

pub use crate::usage::UsageTokens;

enum UpstreamNonStreamResponse {
    Json(serde_json::Value),
    SseText(String),
}

#[derive(Clone, Debug, Default)]
struct UpstreamModelRoute {
    requested_model: Option<String>,
    actual_model: Option<String>,
    actual_model_source: Option<String>,
}

impl UpstreamModelRoute {
    fn is_empty(&self) -> bool {
        self.requested_model.is_none()
            && self.actual_model.is_none()
            && self.actual_model_source.is_none()
    }
}

struct NonStreamSseFallback {
    usage: Option<UsageTokens>,
    response_json: serde_json::Value,
    meta: serde_json::Value,
    audit_flags: serde_json::Value,
}

#[derive(Deserialize)]
pub struct UsageQuery {
    pub limit: Option<i64>,
    pub cursor: Option<String>,
    pub time_from: Option<String>,
    pub time_to: Option<String>,
    pub app_type: Option<String>,
    pub status: Option<String>,
}

#[derive(Deserialize)]
pub struct ShareSessionLoadsQuery {
    pub app: Option<String>,
    pub app_type: Option<String>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareSessionLoad {
    pub router_id: String,
    pub share_id: String,
    pub session_load: i64,
}

#[derive(Clone)]
struct StickyRouteScope {
    sticky_key: String,
    fallback_sticky_key: Option<String>,
    app_type: String,
}

impl StickyRouteScope {
    fn new(
        user_id: Uuid,
        api_key_id: Uuid,
        app_type: &str,
        model_id: Uuid,
        protocol_family: &str,
        session_key: RequestSessionKey,
    ) -> Self {
        let sticky_key = sticky_route_key(
            user_id,
            api_key_id,
            app_type,
            model_id,
            protocol_family,
            &session_key.primary_key,
        );
        let fallback_sticky_key = session_key.fallback_key.as_deref().map(|fallback_key| {
            sticky_route_key(
                user_id,
                api_key_id,
                app_type,
                model_id,
                protocol_family,
                fallback_key,
            )
        });
        Self {
            sticky_key,
            fallback_sticky_key,
            app_type: app_type.to_string(),
        }
    }

    fn sticky_keys(&self) -> Vec<&str> {
        let mut keys = vec![self.sticky_key.as_str()];
        if let Some(fallback) = self
            .fallback_sticky_key
            .as_deref()
            .filter(|fallback| *fallback != self.sticky_key.as_str())
        {
            keys.push(fallback);
        }
        keys
    }
}

struct RequestSessionKey {
    primary_key: String,
    fallback_key: Option<String>,
    previous_response_id: Option<String>,
}

pub async fn usage(
    State(state): State<AppState>,
    principal: crate::auth::Principal,
    Query(query): Query<UsageQuery>,
) -> Result<Json<crate::pagination::Page<serde_json::Value>>, ApiError> {
    let mut sql = r#"
        SELECT rc.id, rc.request_id, rc.app_type, rc.model,
               rc.request_agent, rc.requested_model, rc.actual_model, rc.actual_model_source,
               rc.pricing_model, rc.pricing_model_source, rc.status,
               rc.router_id, rc.share_id, rc.owner_email, rc.routing_rule_id,
               rs.raw_json AS share_raw_json,
               rc.reserved_amount, rc.usage_amount, rc.price_snapshot, rc.usage_json, rc.audit_flags,
               rc.request_object_key, rc.request_object_sha256,
               rc.response_meta_object_key, rc.response_meta_object_sha256,
               rc.created_at, rc.settled_at,
               ak.name AS api_key_name,
               ak.prefix AS api_key_prefix
          FROM request_charges rc
          LEFT JOIN router_shares rs ON rs.router_id = rc.router_id AND rs.share_id = rc.share_id
          LEFT JOIN api_keys ak ON ak.id = rc.api_key_id
         WHERE rc.user_id = ?1
        "#.to_string();
    let mut params = vec![crate::db::uuid_val(principal.user_id)];
    if let Some(cursor) = query.cursor.filter(|v| !v.trim().is_empty()) {
        sql.push_str(&format!(" AND rc.created_at < ?{}", params.len() + 1));
        params.push(crate::db::val(cursor));
    }
    if let Some(time_from) = query.time_from.filter(|v| !v.trim().is_empty()) {
        sql.push_str(&format!(" AND rc.created_at >= ?{}", params.len() + 1));
        params.push(crate::db::val(time_from));
    }
    if let Some(time_to) = query.time_to.filter(|v| !v.trim().is_empty()) {
        sql.push_str(&format!(" AND rc.created_at <= ?{}", params.len() + 1));
        params.push(crate::db::val(time_to));
    }
    if let Some(app_type) = query.app_type.filter(|v| !v.trim().is_empty()) {
        sql.push_str(&format!(" AND rc.app_type = ?{}", params.len() + 1));
        params.push(crate::db::val(app_type));
    }
    if let Some(status) = query.status.filter(|v| !v.trim().is_empty()) {
        sql.push_str(&format!(" AND rc.status = ?{}", params.len() + 1));
        params.push(crate::db::val(status));
    }
    sql.push_str(&format!(
        " ORDER BY rc.created_at DESC LIMIT ?{}",
        params.len() + 1
    ));
    params.push(crate::db::val(crate::pagination::fetch_limit(query.limit)));
    let rows = state.db().query_all(&sql, params).await?;
    let items = rows.into_iter().map(charge_json).collect::<Vec<_>>();
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

pub async fn share_session_loads(
    State(state): State<AppState>,
    Query(query): Query<ShareSessionLoadsQuery>,
) -> Result<Json<Vec<ShareSessionLoad>>, ApiError> {
    let app_type = query
        .app
        .or(query.app_type)
        .map(|value| share_capability(&value).to_string());
    let now = chrono::Utc::now().to_rfc3339();
    let mut sql = r#"
        SELECT router_id, share_id, COUNT(*) AS session_load
          FROM market_share_sticky_routes
         WHERE expires_at > ?1
        "#
    .to_string();
    let mut params = vec![crate::db::val(&now)];
    if let Some(app_type) = app_type {
        sql.push_str(" AND app_type = ?2");
        params.push(crate::db::val(app_type));
    }
    sql.push_str(
        " GROUP BY router_id, share_id ORDER BY session_load ASC, router_id ASC, share_id ASC",
    );
    let rows = state.db().query_all(&sql, params).await?;
    Ok(Json(
        rows.into_iter()
            .map(|row| ShareSessionLoad {
                router_id: row.string("router_id"),
                share_id: row.string("share_id"),
                session_load: row.i64("session_load"),
            })
            .collect(),
    ))
}

pub async fn chat_completions(
    State(state): State<AppState>,
    parts: RequestMeta,
    body: Bytes,
) -> Result<Response, ApiError> {
    handle_llm_request(state, parts, body, "openai", "/v1/chat/completions").await
}

pub async fn responses(
    State(state): State<AppState>,
    parts: RequestMeta,
    body: Bytes,
) -> Result<Response, ApiError> {
    handle_llm_request(state, parts, body, "openai", "/v1/responses").await
}

pub async fn messages(
    State(state): State<AppState>,
    parts: RequestMeta,
    body: Bytes,
) -> Result<Response, ApiError> {
    handle_llm_request(state, parts, body, "anthropic", "/v1/messages").await
}

pub async fn gemini_models_v1beta(
    State(state): State<AppState>,
    Path(path): Path<String>,
    parts: RequestMeta,
    body: Bytes,
) -> Result<Response, ApiError> {
    handle_gemini_models_request(state, parts, body, "v1beta", path).await
}

pub async fn gemini_models_v1(
    State(state): State<AppState>,
    Path(path): Path<String>,
    parts: RequestMeta,
    body: Bytes,
) -> Result<Response, ApiError> {
    handle_gemini_models_request(state, parts, body, "v1", path).await
}

async fn handle_gemini_models_request(
    state: AppState,
    parts: RequestMeta,
    body: Bytes,
    version: &str,
    path: String,
) -> Result<Response, ApiError> {
    let (model, action) = parse_gemini_model_action(&path)
        .ok_or_else(|| ApiError::bad_request("invalid_gemini_path", "missing Gemini action"))?;
    if !matches!(action, "generateContent" | "streamGenerateContent") {
        return Err(ApiError::bad_request(
            "unsupported_gemini_action",
            "only generateContent and streamGenerateContent are supported",
        ));
    }
    let upstream_path = format!("/{version}/models/{path}");
    handle_llm_request_with_model(
        state,
        parts,
        body,
        "gemini",
        &upstream_path,
        Some(model.to_string()),
    )
    .await
}

fn parse_gemini_model_action(path: &str) -> Option<(&str, &str)> {
    path.rsplit_once(':')
}

pub struct RequestMeta {
    pub headers: HeaderMap,
}

impl FromRequestParts<AppState> for RequestMeta {
    type Rejection = ApiError;
    async fn from_request_parts(
        parts: &mut Parts,
        _state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        Ok(Self {
            headers: parts.headers.clone(),
        })
    }
}

async fn handle_llm_request(
    state: AppState,
    meta: RequestMeta,
    body: Bytes,
    default_app_type: &str,
    upstream_path: &str,
) -> Result<Response, ApiError> {
    handle_llm_request_with_model(state, meta, body, default_app_type, upstream_path, None).await
}

async fn handle_llm_request_with_model(
    state: AppState,
    meta: RequestMeta,
    body: Bytes,
    default_app_type: &str,
    upstream_path: &str,
    model_override: Option<String>,
) -> Result<Response, ApiError> {
    let api = api_key_from_headers(&meta.headers, &state).await?;
    enforce_market_maintenance(&state, &api).await?;
    let db = state.db();
    ledger::ensure_user_accounts(db, api.user_id).await?;
    ledger::ensure_platform_accounts(db).await?;

    let body_json: serde_json::Value = serde_json::from_slice(&body).unwrap_or_else(|_| json!({}));
    let model = model_override.unwrap_or_else(|| {
        body_json
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string()
    });
    let is_stream = upstream_path.contains("streamGenerateContent")
        || body_json
            .get("stream")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
    let app_type = default_app_type;
    let input_tokens = ((body.len() as u64).saturating_add(1) / 2).max(1);
    let output_tokens = body_json
        .get("max_tokens")
        .or_else(|| body_json.get("max_completion_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(64)
        .min(4096);
    let downstream_path = upstream_path.to_string();
    let protocol_family = protocol_family(app_type, upstream_path);
    let initial_model_id = pricing::match_price(db, app_type, &model)
        .await
        .ok()
        .and_then(|price| price.model_id)
        .unwrap_or_else(Uuid::nil);
    let sticky_app_type = share_capability(app_type).to_string();
    let sticky_scope = resolve_sticky_route_scope(
        &state,
        &meta.headers,
        &body_json,
        &body,
        &api,
        &sticky_app_type,
        initial_model_id,
        protocol_family,
    )
    .await?;
    let sticky_keys = sticky_scope.sticky_keys();
    let market_email = state.market_runtime.read().await.owner_email.clone();
    let mut candidates = select_share_candidates_with_sync_retry(
        &state,
        &api,
        market_email.as_deref(),
        app_type,
        &model,
        100,
        &sticky_keys,
    )
    .await?;
    let use_responses_upstream = candidates
        .first()
        .is_some_and(|share| share_uses_responses_for_openai_chat(share, app_type, upstream_path));
    candidates.retain(|share| {
        share_uses_responses_for_openai_chat(share, app_type, upstream_path)
            == use_responses_upstream
    });
    let upstream_path = if use_responses_upstream {
        "/v1/responses"
    } else {
        upstream_path
    };
    candidates = order_share_candidates(&state, &sticky_scope, candidates).await?;
    candidates.truncate(3);
    let share = candidates.first().cloned().ok_or_else(|| {
        ApiError::service_unavailable(format!(
            "no available router share for app_type={app_type}, model={model}"
        ))
    })?;
    let price = share.price.clone();
    let model_id = price
        .model_id
        .ok_or_else(|| ApiError::bad_request("model_not_supported", "model is not supported"))?;
    let estimated_amount = pricing::cost(input_tokens, output_tokens, &price);
    let reserved_amount = estimated_amount.max(state.config.market_min_request_balance);
    let user_balance = ledger::balance(
        db,
        AccountRef::User {
            account_type: "user_cash",
            user_id: api.user_id,
        },
    )
    .await?;
    if user_balance < reserved_amount {
        return Err(ApiError::bad_request(
            "insufficient_balance",
            "user balance is insufficient for the estimated request cost",
        ));
    }
    enforce_monthly_spend_cap(db, &api, reserved_amount).await?;
    let body_json = if use_responses_upstream {
        chat_completions_body_to_responses(body_json)
    } else {
        body_json
    };
    let body = Bytes::from(
        serde_json::to_vec(&body_json)
            .map_err(|err| ApiError::bad_request("invalid_request_body", err.to_string()))?,
    );
    let usage_protocol = UsageProtocol::from_app_type(app_type, upstream_path);
    let request_id = format!("req_{}", Uuid::new_v4().simple());
    let charge_id = Uuid::new_v4();
    let request_hash = format!("sha256:{}", hex::encode(Sha256::digest(&body)));
    let idempotency_key = meta
        .headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let now = chrono::Utc::now();
    let request_object = state
        .object_store
        .put_bytes(
            format!(
                "requests/{}/{}/{}/request.json",
                now.year(),
                format!("{:02}", now.month()),
                request_id
            ),
            &body,
        )
        .await?;
    crate::object_store::record_object_ref(
        &state,
        &request_object,
        "request_charge",
        charge_id,
        "request_body",
        Some("application/json"),
    )
    .await?;
    if let Some(replay) = reserve_request(
        &state,
        &api,
        &share,
        &price,
        app_type,
        &model,
        &request_id,
        charge_id,
        &request_hash,
        &request_object.object_key,
        &request_object.content_sha256,
        idempotency_key.as_deref(),
        reserved_amount,
    )
    .await?
    {
        return Ok(Json(replay).into_response());
    }
    if is_stream {
        return handle_openai_stream(
            state,
            meta,
            body_json,
            &api,
            candidates,
            price,
            charge_id,
            request_id,
            idempotency_key,
            reserved_amount,
            upstream_path,
            now,
            usage_protocol,
            sticky_scope.sticky_key.clone(),
            sticky_scope.fallback_sticky_key.clone(),
            sticky_scope.app_type.clone(),
            protocol_family.to_string(),
            model_id,
        )
        .await;
    }
    let (upstream_response, upstream_share) = match forward_non_stream_with_retries(
        &state,
        &meta.headers,
        &body,
        &candidates,
        model_id,
        charge_id,
        &request_id,
        upstream_path,
        Some(&sticky_scope.sticky_key),
        sticky_scope.fallback_sticky_key.as_deref(),
    )
    .await
    {
        Ok(value) => value,
        Err(err) => {
            let error_message = err.to_string();
            let audit_flags = upstream_failure_audit_flags("upstream_failed", &error_message);
            if let Err(release_err) = release_reserved_request(
                &state,
                api.user_id,
                charge_id,
                idempotency_key.as_deref(),
                reserved_amount,
                audit_flags,
            )
            .await
            {
                tracing::warn!(
                    %charge_id,
                    error = %release_err,
                    "failed to release reserved charge after upstream failure"
                );
            }
            return Err(err);
        }
    };
    let (response_json, usage, audit_flags, response_meta_extra) = match upstream_response {
        UpstreamNonStreamResponse::Json(upstream_json) => {
            let Some(usage) = crate::usage::extract_response_usage(&upstream_json, usage_protocol)
            else {
                mark_stream_needs_review(
                    &state,
                    api.user_id,
                    charge_id,
                    idempotency_key.as_deref(),
                    "non_stream_usage_missing",
                    None,
                    serde_json::json!(["non_stream_usage_missing"]),
                )
                .await?;
                return Ok(Json(upstream_json).into_response());
            };
            (upstream_json, usage, serde_json::json!([]), None)
        }
        UpstreamNonStreamResponse::SseText(text) => {
            let fallback =
                parse_non_stream_sse_fallback(&text, usage_protocol, &model, &downstream_path);
            let Some(usage) = fallback.usage else {
                mark_stream_needs_review(
                    &state,
                    api.user_id,
                    charge_id,
                    idempotency_key.as_deref(),
                    "non_stream_usage_missing",
                    None,
                    fallback.audit_flags,
                )
                .await?;
                return Ok(Json(fallback.response_json).into_response());
            };
            (
                fallback.response_json,
                usage,
                fallback.audit_flags,
                Some(fallback.meta),
            )
        }
    };
    refresh_sticky_routes(
        &state,
        Some(&sticky_scope.sticky_key),
        sticky_scope.fallback_sticky_key.as_deref(),
        api.user_id,
        api.api_key_id,
        &sticky_scope.app_type,
        model_id,
        protocol_family,
        &upstream_share,
    )
    .await;
    if let Some(response_id) = response_id_from_json(&response_json) {
        record_response_sticky_route(
            &state,
            &response_id,
            &sticky_scope.sticky_key,
            api.user_id,
            api.api_key_id,
            &sticky_scope.app_type,
            model_id,
            protocol_family,
            &upstream_share,
        )
        .await;
    }
    settle_reserved_request(
        &state,
        api.user_id,
        &upstream_share.owner_email,
        charge_id,
        idempotency_key.as_deref(),
        reserved_amount,
        usage,
        &price,
        &request_id,
        now,
        audit_flags,
        response_meta_extra,
    )
    .await?;

    Ok(Json(response_json).into_response())
}

#[allow(clippy::too_many_arguments)]
async fn handle_openai_stream(
    state: AppState,
    meta: RequestMeta,
    mut body_json: serde_json::Value,
    api: &ApiKeyPrincipal,
    candidates: Vec<SelectedShare>,
    price: pricing::PriceItem,
    charge_id: Uuid,
    request_id: String,
    idempotency_key: Option<String>,
    reserved_amount: Decimal,
    upstream_path: &str,
    now: chrono::DateTime<chrono::Utc>,
    usage_protocol: UsageProtocol,
    sticky_key: String,
    fallback_sticky_key: Option<String>,
    app_type: String,
    protocol_family: String,
    model_id: Uuid,
) -> Result<Response, ApiError> {
    if usage_protocol == UsageProtocol::OpenAi {
        inject_openai_stream_usage(&mut body_json);
    }
    let stream_body = Bytes::from(
        serde_json::to_vec(&body_json)
            .map_err(|err| ApiError::bad_request("invalid_stream_body", err.to_string()))?,
    );
    let (upstream, share) = match forward_stream_with_retries(
        &state,
        &meta.headers,
        stream_body,
        &candidates,
        price.model_id,
        charge_id,
        &request_id,
        upstream_path,
        Some(&sticky_key),
        fallback_sticky_key.as_deref(),
        api.user_id,
        api.api_key_id,
        &app_type,
        model_id,
        &protocol_family,
    )
    .await
    {
        Ok(value) => value,
        Err(err) => {
            let error_message = err.to_string();
            let audit_flags =
                upstream_failure_audit_flags("upstream_failed_before_stream", &error_message);
            if let Err(release_err) = release_reserved_request(
                &state,
                api.user_id,
                charge_id,
                idempotency_key.as_deref(),
                reserved_amount,
                audit_flags,
            )
            .await
            {
                tracing::warn!(
                    %charge_id,
                    error = %release_err,
                    "failed to release reserved charge after stream upstream failure"
                );
            }
            return Err(err);
        }
    };
    state
        .db()
        .execute(
            "UPDATE request_charges SET status='streaming', audit_flags=?2 WHERE id=?1 AND status='reserved'",
            vec![
                crate::db::uuid_val(charge_id),
                crate::db::json_val(serde_json::json!(["stream_started"])),
            ],
        )
        .await?;

    let (tx, mut rx) = tokio::sync::mpsc::channel::<Result<Bytes, std::io::Error>>(16);
    let state_for_task = state.clone();
    let price_for_task = price.clone();
    let share_for_task = share.clone();
    let sticky_key_for_task = sticky_key.clone();
    let app_type_for_task = app_type.clone();
    let protocol_family_for_task = protocol_family.clone();
    let owner_email = share.owner_email.clone();
    let user_id = api.user_id;
    let api_key_id = api.api_key_id;
    let idempotency = idempotency_key.clone();
    tokio::spawn(async move {
        let mut parser = SseUsageParser::new(usage_protocol);
        let mut upstream_stream = upstream.bytes_stream();
        while let Some(next) = upstream_stream.next().await {
            match next {
                Ok(chunk) => {
                    parser.feed(&chunk);
                    if tx.send(Ok(chunk)).await.is_err() {
                        let _ = mark_stream_needs_review(
                            &state_for_task,
                            user_id,
                            charge_id,
                            idempotency.as_deref(),
                            "stream_client_disconnected",
                            parser.usage(),
                            parser.audit_flags(),
                        )
                        .await;
                        return;
                    }
                }
                Err(err) => {
                    let _ =
                        send_sse_error(&tx, "stream_upstream_interrupted", err.to_string()).await;
                    let _ = mark_stream_needs_review(
                        &state_for_task,
                        user_id,
                        charge_id,
                        idempotency.as_deref(),
                        "stream_upstream_interrupted",
                        parser.usage(),
                        parser.audit_flags(),
                    )
                    .await;
                    return;
                }
            }
        }
        parser.finish();
        if usage_protocol == UsageProtocol::OpenAi && !parser.saw_done() {
            let _ = tx.send(Ok(Bytes::from_static(b"data: [DONE]\n\n"))).await;
        }
        let response_id = parser.response_id().map(ToOwned::to_owned);
        if let Some(response_id) = response_id.as_deref() {
            record_response_sticky_route(
                &state_for_task,
                response_id,
                &sticky_key_for_task,
                user_id,
                api_key_id,
                &app_type_for_task,
                model_id,
                &protocol_family_for_task,
                &share_for_task,
            )
            .await;
        }
        if let Some(usage) = parser.usage() {
            if let Err(err) = settle_reserved_request(
                &state_for_task,
                user_id,
                &owner_email,
                charge_id,
                idempotency.as_deref(),
                reserved_amount,
                usage,
                &price_for_task,
                &request_id,
                now,
                parser.audit_flags(),
                response_id.map(|id| serde_json::json!({ "responseId": id })),
            )
            .await
            {
                tracing::warn!(%charge_id, error = %err, "stream settlement failed");
                let _ = mark_stream_needs_review(
                    &state_for_task,
                    user_id,
                    charge_id,
                    idempotency.as_deref(),
                    "stream_settlement_failed",
                    Some(usage),
                    serde_json::json!(["stream_settlement_failed"]),
                )
                .await;
            }
        } else {
            let _ = mark_stream_needs_review(
                &state_for_task,
                user_id,
                charge_id,
                idempotency.as_deref(),
                "stream_usage_missing",
                None,
                parser.audit_flags(),
            )
            .await;
        }
    });

    let body_stream = async_stream::stream! {
        while let Some(item) = rx.recv().await {
            yield item;
        }
    };
    let mut response = Response::new(Body::from_stream(body_stream));
    *response.status_mut() = StatusCode::OK;
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/event-stream"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    headers.insert(header::CONNECTION, HeaderValue::from_static("keep-alive"));
    headers.insert("x-accel-buffering", HeaderValue::from_static("no"));
    Ok(response)
}

fn inject_openai_stream_usage(value: &mut serde_json::Value) {
    if let Some(object) = value.as_object_mut() {
        object.insert("stream".to_string(), serde_json::Value::Bool(true));
        let options = object
            .entry("stream_options")
            .or_insert_with(|| serde_json::json!({}));
        if let Some(options) = options.as_object_mut() {
            options.insert("include_usage".to_string(), serde_json::Value::Bool(true));
        } else {
            *options = serde_json::json!({"include_usage": true});
        }
    }
}

async fn send_sse_error(
    tx: &tokio::sync::mpsc::Sender<Result<Bytes, std::io::Error>>,
    code: &str,
    message: String,
) -> Result<(), tokio::sync::mpsc::error::SendError<Result<Bytes, std::io::Error>>> {
    let payload = serde_json::json!({
        "error": {
            "type": "api_error",
            "message": message,
            "code": code
        }
    });
    tx.send(Ok(Bytes::from(format!("data: {payload}\n\n"))))
        .await?;
    tx.send(Ok(Bytes::from_static(b"data: [DONE]\n\n"))).await
}

#[allow(clippy::too_many_arguments)]
async fn reserve_request(
    state: &AppState,
    api: &ApiKeyPrincipal,
    share: &SelectedShare,
    price: &pricing::PriceItem,
    app_type: &str,
    model: &str,
    request_id: &str,
    charge_id: Uuid,
    request_hash: &str,
    request_object_key: &str,
    request_object_sha256: &str,
    idempotency_key: Option<&str>,
    reserved_amount: Decimal,
) -> Result<Option<serde_json::Value>, ApiError> {
    ledger::ensure_provider_accounts(state.db(), &share.owner_email).await?;
    let tx = state.db().begin_immediate().await?;
    if let Some(key) = idempotency_key {
        if let Some(existing) = tx
            .query_optional(
                "SELECT status, request_body_hash, charge_id FROM request_idempotency WHERE user_id=?1 AND idempotency_key=?2",
                vec![crate::db::uuid_val(api.user_id), crate::db::val(key)],
            )
            .await?
        {
            let existing_hash = existing.string("request_body_hash");
            if existing_hash != request_hash {
                return Err(ApiError::conflict(
                    "idempotency_key_conflict",
                    "idempotency key was already used with a different request body",
                ));
            }
            let status = existing.string("status");
            if status == "finalized" {
                let charge_id = existing.uuid("charge_id");
                tx.commit().await?;
                let row = state
                    .db()
                    .query_one(
                        "SELECT id, request_id, app_type, model, status, reserved_amount, usage_amount, price_snapshot, usage_json, audit_flags, request_object_key, request_object_sha256, response_meta_object_key, response_meta_object_sha256, created_at, settled_at FROM request_charges WHERE id=?1",
                        vec![crate::db::uuid_val(charge_id)],
                    )
                    .await?;
                let mut value = charge_json(row);
                value["idempotent_replay"] = serde_json::Value::Bool(true);
                return Ok(Some(value));
            }
            return Err(ApiError::conflict(
                "idempotency_key_in_progress",
                "idempotent request is still in progress or failed",
            ));
        }
        tx.execute(
            "INSERT INTO request_idempotency (user_id, idempotency_key, request_body_hash, status, created_at) VALUES (?1,?2,?3,'in_progress',?4)",
            vec![
                crate::db::uuid_val(api.user_id),
                crate::db::val(key),
                crate::db::val(request_hash),
                crate::db::val(crate::db::now_string()),
            ],
        )
        .await?;
    }
    tx.execute(
        r#"
        INSERT INTO request_charges
          (id, request_id, user_id, api_key_id, router_id, share_id, owner_email, model_id, routing_rule_id, app_type, model,
           request_agent, requested_model, actual_model, actual_model_source,
           pricing_model, pricing_slot, pricing_model_source, share_official, status,
           idempotency_key, request_body_hash, reserved_amount, price_snapshot, request_object_key, request_object_sha256, created_at)
        VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,'reserved',?20,?21,?22,?23,?24,?25,?26)
        "#,
        vec![
            crate::db::uuid_val(charge_id),
            crate::db::val(request_id),
            crate::db::uuid_val(api.user_id),
            crate::db::uuid_val(api.api_key_id),
            crate::db::val(&share.router_id),
            crate::db::val(&share.share_id),
            crate::db::val(&share.owner_email),
            crate::db::opt_uuid_val(price.model_id),
            crate::db::opt_uuid_val(share.routing_rule_id),
            crate::db::val(app_type),
            crate::db::val(model),
            crate::db::val(share_capability(app_type)),
            crate::db::val(model),
            crate::db::val(&share.pricing_model),
            crate::db::val(&share.pricing_model_source),
            crate::db::val(&share.pricing_model),
            crate::db::val(&share.pricing_slot),
            crate::db::val(&share.pricing_model_source),
            crate::db::val(share.share_official),
            crate::db::opt_val(idempotency_key),
            crate::db::val(request_hash),
            crate::db::dec_val(reserved_amount),
            crate::db::json_val(serde_json::to_value(price).unwrap_or_default()),
            crate::db::val(request_object_key),
            crate::db::val(request_object_sha256),
            crate::db::val(crate::db::now_string()),
        ],
    )
    .await?;
    ledger::transfer(
        &tx,
        AccountRef::User {
            account_type: "user_cash",
            user_id: api.user_id,
        },
        AccountRef::User {
            account_type: "user_reserved",
            user_id: api.user_id,
        },
        reserved_amount,
        "request_charge",
        charge_id,
        "system",
        Some("proxy"),
    )
    .await?;
    tx.commit().await?;
    Ok(None)
}

async fn release_reserved_request(
    state: &AppState,
    user_id: Uuid,
    charge_id: Uuid,
    idempotency_key: Option<&str>,
    reserved_amount: Decimal,
    audit_flags: serde_json::Value,
) -> Result<(), ApiError> {
    let tx = state.db().begin_immediate().await?;
    let changed = tx.execute(
        "UPDATE request_charges SET status='failed_released', audit_flags=?2, settled_at=?3 WHERE id=?1 AND status='reserved'",
        vec![
            crate::db::uuid_val(charge_id),
            crate::db::json_val(audit_flags),
            crate::db::val(crate::db::now_string()),
        ],
    )
    .await?;
    if changed > 0 {
        ledger::transfer(
            &tx,
            AccountRef::User {
                account_type: "user_reserved",
                user_id,
            },
            AccountRef::User {
                account_type: "user_cash",
                user_id,
            },
            reserved_amount,
            "request_charge",
            charge_id,
            "system",
            Some("proxy"),
        )
        .await?;
    }
    if changed > 0 {
        if let Some(key) = idempotency_key {
            tx.execute(
                "UPDATE request_idempotency SET status='failed_released', completed_at=?3 WHERE user_id=?1 AND idempotency_key=?2",
                vec![
                    crate::db::uuid_val(user_id),
                    crate::db::val(key),
                    crate::db::val(crate::db::now_string()),
                ],
            )
            .await?;
        }
    }
    tx.commit().await?;
    trigger_router_request_log_sync(state.clone());
    Ok(())
}

fn trigger_router_request_log_sync(state: AppState) {
    tokio::spawn(async move {
        if let Err(err) = crate::router_request_logs::sync_recent(&state).await {
            tracing::warn!(error = %err, "immediate router request log sync failed");
        }
    });
}

#[allow(clippy::too_many_arguments)]
async fn settle_reserved_request(
    state: &AppState,
    user_id: Uuid,
    owner_email: &str,
    charge_id: Uuid,
    idempotency_key: Option<&str>,
    reserved_amount: Decimal,
    usage: UsageTokens,
    price: &pricing::PriceItem,
    request_id: &str,
    now: chrono::DateTime<chrono::Utc>,
    mut audit_flags: serde_json::Value,
    response_meta_extra: Option<serde_json::Value>,
) -> Result<(), ApiError> {
    let amount = pricing::cost_with_cache(
        usage.input_tokens,
        usage.output_tokens,
        usage.cache_read_tokens,
        usage.cache_write_tokens,
        price,
    );
    if amount > reserved_amount {
        append_audit_flag(&mut audit_flags, "settlement_over_reserved");
    }
    let mut response_meta = serde_json::json!({
        "usage": usage,
        "amount": amount.to_string(),
        "usageSource": usage.source,
    });
    if let Some(extra) = response_meta_extra {
        response_meta["extra"] = extra;
    }
    let response_meta_object = state
        .object_store
        .put_json(
            format!(
                "requests/{}/{}/{}/response-meta.json",
                now.year(),
                format!("{:02}", now.month()),
                request_id
            ),
            &response_meta,
        )
        .await?;
    crate::object_store::record_object_ref(
        state,
        &response_meta_object,
        "request_charge",
        charge_id,
        "response_meta",
        Some("application/json"),
    )
    .await?;
    let router_commission_owner_email = state.config.router_commission_owner_email();
    if state.config.market_router_commission_bps > 0 {
        ledger::ensure_provider_accounts(state.db(), &router_commission_owner_email).await?;
    }
    let tx = state.db().begin_immediate().await?;
    tx.execute(
        r#"
        UPDATE request_charges
           SET status = 'settled',
               usage_amount = ?2,
               usage_json = ?3,
               audit_flags = ?4,
               response_meta_object_key = ?5,
               response_meta_object_sha256 = ?6,
               settled_at = ?7
         WHERE id = ?1 AND status IN ('reserved','streaming','needs_review')
        "#,
        vec![
            crate::db::uuid_val(charge_id),
            crate::db::dec_val(amount),
            crate::db::json_val(serde_json::to_value(usage).unwrap_or_default()),
            crate::db::json_val(audit_flags),
            crate::db::val(response_meta_object.object_key),
            crate::db::val(response_meta_object.content_sha256),
            crate::db::val(crate::db::now_string()),
        ],
    )
    .await?;
    if amount > Decimal::ZERO {
        let market_commission_bps = state.config.market_platform_commission_bps;
        let router_commission_bps = state.config.market_router_commission_bps;
        let from_reserved = reserved_amount.min(amount);
        transfer_collected_usage_amount(
            &tx,
            AccountRef::User {
                account_type: "user_reserved",
                user_id,
            },
            owner_email,
            &router_commission_owner_email,
            from_reserved,
            market_commission_bps,
            router_commission_bps,
            charge_id,
        )
        .await?;
        if reserved_amount > amount {
            ledger::transfer(
                &tx,
                AccountRef::User {
                    account_type: "user_reserved",
                    user_id,
                },
                AccountRef::User {
                    account_type: "user_cash",
                    user_id,
                },
                reserved_amount - amount,
                "request_charge",
                charge_id,
                "system",
                Some("proxy"),
            )
            .await?;
        }
        let overage = amount - from_reserved;
        if overage > Decimal::ZERO {
            let cash_balance = tx
                .query_one(
                    "SELECT balance FROM wallet_accounts WHERE account_type = 'user_cash' AND owner_user_id = ?1",
                    vec![crate::db::uuid_val(user_id)],
                )
                .await?
                .decimal("balance");
            let from_cash = cash_balance.min(overage);
            if from_cash > Decimal::ZERO {
                transfer_collected_usage_amount(
                    &tx,
                    AccountRef::User {
                        account_type: "user_cash",
                        user_id,
                    },
                    owner_email,
                    &router_commission_owner_email,
                    from_cash,
                    market_commission_bps,
                    router_commission_bps,
                    charge_id,
                )
                .await?;
            }
            let risk_loss = overage - from_cash;
            if risk_loss > Decimal::ZERO {
                ledger::transfer(
                    &tx,
                    AccountRef::Platform {
                        account_type: "risk_loss",
                    },
                    AccountRef::Provider {
                        account_type: "client_payable",
                        owner_email,
                    },
                    risk_loss,
                    "request_charge",
                    charge_id,
                    "system",
                    Some("proxy"),
                )
                .await?;
            }
        }
    } else {
        ledger::transfer(
            &tx,
            AccountRef::User {
                account_type: "user_reserved",
                user_id,
            },
            AccountRef::User {
                account_type: "user_cash",
                user_id,
            },
            reserved_amount,
            "request_charge",
            charge_id,
            "system",
            Some("proxy"),
        )
        .await?;
    }
    if let Some(key) = idempotency_key {
        tx.execute(
            "UPDATE request_idempotency SET status='finalized', charge_id=?3, completed_at=?4 WHERE user_id=?1 AND idempotency_key=?2",
            vec![
                crate::db::uuid_val(user_id),
                crate::db::val(key),
                crate::db::uuid_val(charge_id),
                crate::db::val(crate::db::now_string()),
            ],
        )
        .await?;
    }
    tx.commit().await?;
    trigger_router_request_log_sync(state.clone());
    Ok(())
}

pub async fn admin_settle_needs_review_charge(
    state: &AppState,
    admin_email: &str,
    charge_id: Uuid,
    usage: UsageTokens,
    reason: String,
) -> Result<(), ApiError> {
    let row = state
        .db()
        .query_one(
            "SELECT user_id, owner_email, reserved_amount, price_snapshot, request_id FROM request_charges WHERE id=?1 AND status='needs_review'",
            vec![crate::db::uuid_val(charge_id)],
        )
        .await?;
    let price = serde_json::from_str::<pricing::PriceItem>(&row.string("price_snapshot"))
        .map_err(|err| ApiError::bad_request("invalid_price_snapshot", err.to_string()))?;
    let mut audit_flags = serde_json::json!(["admin_settled_needs_review", reason]);
    append_audit_flag(&mut audit_flags, "manual_usage");
    settle_reserved_request(
        state,
        row.uuid("user_id"),
        &row.string("owner_email"),
        charge_id,
        None,
        row.decimal("reserved_amount"),
        usage,
        &price,
        &row.string("request_id"),
        chrono::Utc::now(),
        audit_flags,
        None,
    )
    .await?;
    let tx = state.db().begin_immediate().await?;
    tx.execute(
        "INSERT INTO admin_audit (id, admin_actor, action, reference_type, reference_id, metadata_json, created_at) VALUES (?1,?2,'charge.settle_manual','request_charge',?3,?4,?5)",
        vec![
            crate::db::uuid_val(Uuid::new_v4()),
            crate::db::val(admin_email),
            crate::db::uuid_val(charge_id),
            crate::db::json_val(serde_json::json!({"reason": reason})),
            crate::db::val(crate::db::now_string()),
        ],
    )
    .await?;
    tx.commit().await?;
    trigger_router_request_log_sync(state.clone());
    Ok(())
}

pub async fn admin_release_needs_review_charge(
    state: &AppState,
    admin_email: &str,
    charge_id: Uuid,
    reason: String,
) -> Result<(), ApiError> {
    let row = state
        .db()
        .query_one(
            "SELECT user_id, reserved_amount FROM request_charges WHERE id=?1 AND status='needs_review'",
            vec![crate::db::uuid_val(charge_id)],
        )
        .await?;
    let tx = state.db().begin_immediate().await?;
    tx.execute(
        "UPDATE request_charges SET status='failed_released', audit_flags=?2, settled_at=?3 WHERE id=?1 AND status='needs_review'",
        vec![
            crate::db::uuid_val(charge_id),
            crate::db::json_val(serde_json::json!(["admin_released_needs_review", reason])),
            crate::db::val(crate::db::now_string()),
        ],
    )
    .await?;
    ledger::transfer(
        &tx,
        AccountRef::User {
            account_type: "user_reserved",
            user_id: row.uuid("user_id"),
        },
        AccountRef::User {
            account_type: "user_cash",
            user_id: row.uuid("user_id"),
        },
        row.decimal("reserved_amount"),
        "request_charge",
        charge_id,
        "admin",
        Some(admin_email),
    )
    .await?;
    tx.execute(
        "INSERT INTO admin_audit (id, admin_actor, action, reference_type, reference_id, metadata_json, created_at) VALUES (?1,?2,'charge.release','request_charge',?3,?4,?5)",
        vec![
            crate::db::uuid_val(Uuid::new_v4()),
            crate::db::val(admin_email),
            crate::db::uuid_val(charge_id),
            crate::db::json_val(serde_json::json!({"reason": reason})),
            crate::db::val(crate::db::now_string()),
        ],
    )
    .await?;
    tx.commit().await?;
    trigger_router_request_log_sync(state.clone());
    Ok(())
}

async fn mark_stream_needs_review(
    state: &AppState,
    user_id: Uuid,
    charge_id: Uuid,
    idempotency_key: Option<&str>,
    reason: &str,
    usage: Option<UsageTokens>,
    mut audit_flags: serde_json::Value,
) -> Result<(), ApiError> {
    append_audit_flag(&mut audit_flags, reason);
    let failure_kind = match reason {
        "stream_client_disconnected" => FailureKind::ClientDisconnected,
        "stream_upstream_interrupted" => FailureKind::StreamInterrupted,
        "stream_usage_missing" | "non_stream_usage_missing" => FailureKind::StreamUsageMissing,
        "stream_settlement_failed" => FailureKind::SettlementFailed,
        _ => FailureKind::Unknown,
    };
    crate::failure::append_failure_audit_flags(
        &mut audit_flags,
        failure_kind,
        failure_kind.policy(),
    );
    let tx = state.db().begin_immediate().await?;
    tx.execute(
        r#"
        UPDATE request_charges
           SET status='needs_review',
               usage_json=?2,
               audit_flags=?3,
               settled_at=?4
         WHERE id=?1 AND status IN ('reserved','streaming')
        "#,
        vec![
            crate::db::uuid_val(charge_id),
            usage
                .map(|usage| crate::db::json_val(serde_json::to_value(usage).unwrap_or_default()))
                .unwrap_or(libsql::Value::Null),
            crate::db::json_val(audit_flags),
            crate::db::val(crate::db::now_string()),
        ],
    )
    .await?;
    if let Some(key) = idempotency_key {
        tx.execute(
            "UPDATE request_idempotency SET status='needs_review', completed_at=?4 WHERE user_id=?1 AND (charge_id=?2 OR idempotency_key=?3)",
            vec![
                crate::db::uuid_val(user_id),
                crate::db::uuid_val(charge_id),
                crate::db::val(key),
                crate::db::val(crate::db::now_string()),
            ],
        )
        .await?;
    }
    tx.commit().await?;
    trigger_router_request_log_sync(state.clone());
    Ok(())
}

fn append_audit_flag(flags: &mut serde_json::Value, flag: &str) {
    if let Some(array) = flags.as_array_mut() {
        if !array.iter().any(|value| value.as_str() == Some(flag)) {
            array.push(serde_json::Value::String(flag.to_string()));
        }
    } else {
        *flags = serde_json::json!([flag]);
    }
}

fn upstream_failure_audit_flags(code: &str, message: &str) -> serde_json::Value {
    let kind = classify_upstream_failure(message);
    let mut flags = serde_json::json!([{"code": code, "message": message}]);
    crate::failure::append_failure_audit_flags(&mut flags, kind, kind.policy());
    flags
}

async fn transfer_collected_usage_amount(
    tx: &crate::db::DbTx,
    from: AccountRef<'_>,
    owner_email: &str,
    router_owner_email: &str,
    amount: Decimal,
    market_commission_bps: i64,
    router_commission_bps: i64,
    charge_id: Uuid,
) -> Result<(), ApiError> {
    if amount <= Decimal::ZERO {
        return Ok(());
    }
    let (market_commission, router_commission) =
        commission_split(amount, market_commission_bps, router_commission_bps);
    let provider_amount = amount - market_commission - router_commission;
    if provider_amount > Decimal::ZERO {
        ledger::transfer(
            tx,
            from,
            AccountRef::Provider {
                account_type: "client_payable",
                owner_email,
            },
            provider_amount,
            "request_charge",
            charge_id,
            "system",
            Some("proxy"),
        )
        .await?;
    }
    if market_commission > Decimal::ZERO {
        ledger::transfer(
            tx,
            from,
            AccountRef::Platform {
                account_type: "fee_revenue",
            },
            market_commission,
            "request_charge",
            charge_id,
            "system",
            Some("proxy"),
        )
        .await?;
    }
    if router_commission > Decimal::ZERO {
        ledger::transfer(
            tx,
            from,
            AccountRef::Provider {
                account_type: "client_payable",
                owner_email: router_owner_email,
            },
            router_commission,
            "request_charge",
            charge_id,
            "system",
            Some("proxy"),
        )
        .await?;
    }
    Ok(())
}

fn commission_amount(amount: Decimal, commission_bps: i64) -> Decimal {
    if commission_bps <= 0 {
        return Decimal::ZERO;
    }
    if commission_bps >= 10_000 {
        return amount;
    }
    (amount * Decimal::from(commission_bps) / Decimal::from(10_000)).round_dp(8)
}

fn commission_split(amount: Decimal, market_bps: i64, router_bps: i64) -> (Decimal, Decimal) {
    let total_bps = (market_bps + router_bps).clamp(0, 10_000);
    let total = commission_amount(amount, total_bps);
    let market = commission_amount(amount, market_bps).min(total);
    (market, total - market)
}

fn apply_router_market_proxy_headers(
    mut request: reqwest::RequestBuilder,
    headers: &HeaderMap,
    default_accept: Option<HeaderValue>,
) -> reqwest::RequestBuilder {
    let mut has_accept = false;
    let mut has_content_type = false;
    for (name, value) in headers {
        if !is_allowed_router_market_proxy_header(name.as_str()) {
            continue;
        }
        if name == header::ACCEPT {
            has_accept = true;
        }
        if name == header::CONTENT_TYPE {
            has_content_type = true;
        }
        request = request.header(name, value);
    }
    if !has_accept {
        if let Some(value) = default_accept {
            request = request.header(header::ACCEPT, value);
        }
    }
    if !has_content_type {
        request = request.header(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
    }
    request
}

fn is_allowed_router_market_proxy_header(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    if is_blocked_router_market_proxy_header(&lower)
        || is_hop_by_hop_router_market_proxy_header(&lower)
        || lower.starts_with("proxy-")
    {
        return false;
    }
    matches!(
        lower.as_str(),
        "accept"
            | "content-type"
            | "cache-control"
            | "pragma"
            | "user-agent"
            | "x-request-id"
            | "anthropic-version"
            | "anthropic-beta"
            | "anthropic-dangerous-direct-browser-access"
    ) || lower.starts_with("x-stainless-")
        || lower.starts_with("anthropic-client-")
}

fn is_blocked_router_market_proxy_header(lower: &str) -> bool {
    matches!(
        lower,
        "authorization"
            | "x-api-key"
            | "api-key"
            | "x-share-token"
            | "cookie"
            | "set-cookie"
            | "host"
            | "x-cc-switch-market-request-id"
            | "x-cc-switch-request-id"
    )
}

fn is_hop_by_hop_router_market_proxy_header(lower: &str) -> bool {
    matches!(
        lower,
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
    )
}

fn model_route_from_headers(headers: &HeaderMap) -> UpstreamModelRoute {
    fn header_string(headers: &HeaderMap, name: &str) -> Option<String> {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
    }

    UpstreamModelRoute {
        requested_model: header_string(headers, "x-cc-switch-requested-model"),
        actual_model: header_string(headers, "x-cc-switch-actual-model"),
        actual_model_source: header_string(headers, "x-cc-switch-actual-model-source"),
    }
}

async fn forward_to_router_market_proxy(
    state: &AppState,
    headers: &HeaderMap,
    body: &Bytes,
    share: &SelectedShare,
    request_id: &str,
    upstream_path: &str,
) -> Result<(UpstreamNonStreamResponse, UpstreamModelRoute), ApiError> {
    let access_token = crate::router_account::access_token(&state.config)
        .await
        .map_err(|e| ApiError::service_unavailable(format!("router login required: {e}")))?;
    let url = format!(
        "{}/_market/proxy/{share_id}{upstream_path}",
        state.config.market_public_base_url.trim_end_matches('/'),
        share_id = share.share_id
    );
    let mut request = state
        .http
        .post(url)
        .bearer_auth(access_token)
        .header("X-CC-Switch-Market-Request-Id", request_id)
        .body(body.clone());
    request = apply_router_market_proxy_headers(request, headers, None);
    let response = request.send().await.map_err(|err| {
        ApiError::service_unavailable(format!("router market proxy failed: {err}"))
    })?;
    let status = response.status();
    let model_route = model_route_from_headers(response.headers());
    let is_event_stream = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.to_ascii_lowercase().contains("text/event-stream"));
    let text = response.text().await.map_err(|err| {
        ApiError::service_unavailable(format!("router market proxy read failed: {err}"))
    })?;
    if !status.is_success() {
        let value = serde_json::from_str::<serde_json::Value>(&text)
            .unwrap_or_else(|_| json!({ "raw": text }));
        let error_message = format!("router market proxy returned {status}: {value}");
        let kind = classify_upstream_failure(&error_message);
        if kind != FailureKind::BadRequest {
            record_share_failure(state, share, kind, Some(error_message.clone())).await;
        }
        return Err(router_market_proxy_error(status, error_message));
    }
    record_share_success(state, share).await;
    if is_event_stream || looks_like_sse(&text) {
        return Ok((UpstreamNonStreamResponse::SseText(text), model_route));
    }
    let value =
        serde_json::from_str::<serde_json::Value>(&text).unwrap_or_else(|_| json!({ "raw": text }));
    Ok((UpstreamNonStreamResponse::Json(value), model_route))
}

fn router_market_proxy_error(status: StatusCode, message: String) -> ApiError {
    match status {
        StatusCode::BAD_REQUEST => ApiError::bad_request("upstream_bad_request", message),
        StatusCode::UNAUTHORIZED => ApiError::unauthorized(message),
        StatusCode::FORBIDDEN => ApiError::forbidden(message),
        _ => ApiError::service_unavailable(message),
    }
}

async fn forward_to_router_market_proxy_stream(
    state: &AppState,
    headers: &HeaderMap,
    body: Bytes,
    share: &SelectedShare,
    request_id: &str,
    upstream_path: &str,
) -> Result<(reqwest::Response, UpstreamModelRoute), ApiError> {
    let access_token = crate::router_account::access_token(&state.config)
        .await
        .map_err(|e| ApiError::service_unavailable(format!("router login required: {e}")))?;
    let url = format!(
        "{}/_market/proxy/{share_id}{upstream_path}",
        state.config.market_public_base_url.trim_end_matches('/'),
        share_id = share.share_id
    );
    let mut request = state
        .http
        .post(url)
        .bearer_auth(access_token)
        .header("X-CC-Switch-Market-Request-Id", request_id)
        .body(body);
    request = apply_router_market_proxy_headers(
        request,
        headers,
        Some(HeaderValue::from_static("text/event-stream")),
    );
    let response = request.send().await.map_err(|err| {
        ApiError::service_unavailable(format!("router market proxy failed: {err}"))
    })?;
    let status = response.status();
    if !status.is_success() {
        let text = response.text().await.unwrap_or_default();
        let error_message = format!("router market proxy returned {status}: {text}");
        let kind = classify_upstream_failure(&error_message);
        if kind != FailureKind::BadRequest {
            record_share_failure(state, share, kind, Some(error_message.clone())).await;
        }
        return Err(router_market_proxy_error(status, error_message));
    }
    let model_route = model_route_from_headers(response.headers());
    record_share_success(state, share).await;
    Ok((response, model_route))
}

async fn forward_non_stream_with_retries(
    state: &AppState,
    headers: &HeaderMap,
    body: &Bytes,
    candidates: &[SelectedShare],
    model_id: Uuid,
    charge_id: Uuid,
    request_id: &str,
    upstream_path: &str,
    sticky_key: Option<&str>,
    fallback_sticky_key: Option<&str>,
) -> Result<(UpstreamNonStreamResponse, SelectedShare), ApiError> {
    let mut last_err = None;
    for (idx, share) in candidates.iter().enumerate() {
        update_charge_route(state, charge_id, share).await?;
        let started = chrono::Utc::now();
        match forward_to_router_market_proxy(state, headers, body, share, request_id, upstream_path)
            .await
        {
            Ok((value, model_route)) => {
                update_charge_model_route(state, charge_id, &model_route).await?;
                record_request_attempt(
                    state,
                    request_id,
                    charge_id,
                    model_id,
                    share,
                    idx + 1,
                    "success",
                    None,
                    None,
                    started,
                )
                .await;
                return Ok((value, share.clone()));
            }
            Err(err) => {
                let message = err.to_string();
                let kind = classify_upstream_failure(&message);
                record_request_attempt(
                    state,
                    request_id,
                    charge_id,
                    model_id,
                    share,
                    idx + 1,
                    "error",
                    Some(kind.code()),
                    Some(message.clone()),
                    started,
                )
                .await;
                clear_sticky_routes_for_share(state, sticky_key, fallback_sticky_key, share).await;
                maybe_block_model_share(state, model_id, share, kind, &message).await;
                maybe_report_router_feedback(state, share, kind, &message).await;
                let retryable = is_retryable_failure(kind);
                last_err = Some(err);
                if !retryable {
                    break;
                }
            }
        }
    }
    Err(last_err.unwrap_or_else(|| ApiError::service_unavailable("router market proxy failed")))
}

fn looks_like_sse(text: &str) -> bool {
    let trimmed = text.trim_start();
    trimmed.starts_with("event:") || trimmed.starts_with("data:")
}

fn parse_non_stream_sse_fallback(
    text: &str,
    usage_protocol: UsageProtocol,
    fallback_model: &str,
    downstream_path: &str,
) -> NonStreamSseFallback {
    let mut parser = SseUsageParser::new(usage_protocol);
    parser.feed(text.as_bytes());
    parser.finish();

    let events = parse_sse_json_events(text);
    let usage = parser.usage();
    let response_json = non_stream_sse_response_json(
        &events,
        usage,
        usage_protocol,
        fallback_model,
        downstream_path,
    );
    let event_types = events
        .iter()
        .filter_map(|value| value.get("type").and_then(|item| item.as_str()))
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    let mut audit_flags = serde_json::json!([
        format!("usage_protocol_{usage_protocol:?}").to_ascii_lowercase(),
        "non_stream_sse_fallback"
    ]);
    if parser.saw_done() {
        append_audit_flag(&mut audit_flags, "stream_done_seen");
    }
    if usage.is_none() {
        append_audit_flag(&mut audit_flags, "stream_usage_missing");
    }
    let meta = serde_json::json!({
        "source": "non_stream_sse_fallback",
        "usageProtocol": format!("{usage_protocol:?}").to_ascii_lowercase(),
        "eventTypes": event_types,
        "responseId": response_json.get("id").cloned().unwrap_or(serde_json::Value::Null),
        "model": response_json.get("model").cloned().unwrap_or(serde_json::Value::Null),
    });
    NonStreamSseFallback {
        usage,
        response_json,
        meta,
        audit_flags,
    }
}

fn parse_sse_json_events(text: &str) -> Vec<serde_json::Value> {
    let normalized = text.replace("\r\n", "\n");
    normalized
        .split("\n\n")
        .filter_map(|block| {
            let data = block
                .lines()
                .filter_map(|line| {
                    let line = line.trim_end_matches('\r');
                    line.strip_prefix("data: ")
                        .or_else(|| line.strip_prefix("data:"))
                })
                .collect::<Vec<_>>()
                .join("\n");
            let data = data.trim();
            if data.is_empty() || data == "[DONE]" {
                return None;
            }
            serde_json::from_str::<serde_json::Value>(data).ok()
        })
        .collect()
}

fn response_id_from_json(value: &serde_json::Value) -> Option<String> {
    value
        .get("id")
        .or_else(|| value.pointer("/response/id"))
        .and_then(|item| item.as_str())
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(ToOwned::to_owned)
}

fn non_stream_sse_response_json(
    events: &[serde_json::Value],
    usage: Option<UsageTokens>,
    usage_protocol: UsageProtocol,
    fallback_model: &str,
    downstream_path: &str,
) -> serde_json::Value {
    if downstream_path == "/v1/responses" {
        return responses_response_from_sse(events, usage, fallback_model);
    }
    if downstream_path == "/v1/messages" || usage_protocol == UsageProtocol::Anthropic {
        return anthropic_message_from_sse(events, usage, fallback_model);
    }
    if usage_protocol == UsageProtocol::Gemini || downstream_path.contains("generateContent") {
        return gemini_generate_content_from_sse(events, usage);
    }
    chat_completion_from_sse(events, usage, fallback_model)
}

fn responses_response_from_sse(
    events: &[serde_json::Value],
    usage: Option<UsageTokens>,
    fallback_model: &str,
) -> serde_json::Value {
    let response = events
        .iter()
        .rev()
        .find(|event| {
            event.get("type").and_then(|item| item.as_str()) == Some("response.completed")
        })
        .and_then(|event| event.get("response"))
        .or_else(|| events.iter().rev().find_map(|event| event.get("response")))
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let mut output = response
        .get("output")
        .and_then(|item| item.as_array())
        .filter(|items| !items.is_empty())
        .cloned()
        .unwrap_or_else(|| response_output_items_from_sse(events));
    if output.is_empty() {
        let content = response_output_text_from_sse(events);
        if !content.is_empty() {
            output.push(serde_json::json!({
                "id": "msg_non_stream_sse_fallback",
                "type": "message",
                "status": "completed",
                "role": "assistant",
                "content": [{
                    "type": "output_text",
                    "text": content,
                    "annotations": [],
                }],
            }));
        }
    }

    let id = response
        .get("id")
        .and_then(|item| item.as_str())
        .unwrap_or("resp_non_stream_sse_fallback");
    let model = response
        .get("model")
        .and_then(|item| item.as_str())
        .unwrap_or(fallback_model);
    let created_at = response
        .get("created_at")
        .and_then(|item| item.as_i64())
        .unwrap_or_else(|| chrono::Utc::now().timestamp());
    let status = response
        .get("status")
        .and_then(|item| item.as_str())
        .or_else(|| response_status_from_sse(events))
        .unwrap_or("completed");
    let mut out = response.as_object().cloned().unwrap_or_default();
    out.insert("id".to_string(), serde_json::json!(id));
    out.insert("object".to_string(), serde_json::json!("response"));
    out.insert("created_at".to_string(), serde_json::json!(created_at));
    out.insert("model".to_string(), serde_json::json!(model));
    out.insert("status".to_string(), serde_json::json!(status));
    out.insert("output".to_string(), serde_json::Value::Array(output));
    out.insert(
        "usage".to_string(),
        usage
            .map(responses_usage_json)
            .or_else(|| response.get("usage").cloned())
            .unwrap_or(serde_json::Value::Null),
    );
    serde_json::Value::Object(out)
}

fn response_output_items_from_sse(events: &[serde_json::Value]) -> Vec<serde_json::Value> {
    let mut seen = std::collections::HashSet::<String>::new();
    let mut items = Vec::new();
    for item in events
        .iter()
        .filter(|event| {
            event.get("type").and_then(|item| item.as_str()) == Some("response.output_item.done")
        })
        .filter_map(|event| event.get("item"))
    {
        let dedupe_key = item
            .get("id")
            .or_else(|| item.get("call_id"))
            .and_then(|value| value.as_str())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| format!("idx:{}", items.len()));
        if seen.insert(dedupe_key) {
            items.push(item.clone());
        }
    }
    items
}

fn response_output_text_from_sse(events: &[serde_json::Value]) -> String {
    events
        .iter()
        .rev()
        .find(|event| {
            event.get("type").and_then(|item| item.as_str()) == Some("response.output_text.done")
        })
        .and_then(|event| event.get("text").and_then(|item| item.as_str()))
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| {
            events
                .iter()
                .filter(|event| {
                    event.get("type").and_then(|item| item.as_str())
                        == Some("response.output_text.delta")
                })
                .filter_map(|event| event.get("delta").and_then(|item| item.as_str()))
                .collect::<String>()
        })
}

fn response_status_from_sse(events: &[serde_json::Value]) -> Option<&'static str> {
    if events
        .iter()
        .any(|event| event.get("type").and_then(|item| item.as_str()) == Some("response.failed"))
    {
        return Some("failed");
    }
    if events.iter().any(|event| {
        event.get("type").and_then(|item| item.as_str()) == Some("response.incomplete")
    }) {
        return Some("incomplete");
    }
    if events
        .iter()
        .any(|event| event.get("type").and_then(|item| item.as_str()) == Some("response.completed"))
    {
        return Some("completed");
    }
    None
}

fn chat_completion_from_sse(
    events: &[serde_json::Value],
    usage: Option<UsageTokens>,
    fallback_model: &str,
) -> serde_json::Value {
    let response = events
        .iter()
        .rev()
        .find_map(|event| event.get("response"))
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let content = events
        .iter()
        .rev()
        .find(|event| {
            event.get("type").and_then(|item| item.as_str()) == Some("response.output_text.done")
        })
        .and_then(|event| event.get("text").and_then(|item| item.as_str()))
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| {
            events
                .iter()
                .filter(|event| {
                    event.get("type").and_then(|item| item.as_str())
                        == Some("response.output_text.delta")
                })
                .filter_map(|event| event.get("delta").and_then(|item| item.as_str()))
                .collect::<String>()
        });
    let id = response
        .get("id")
        .and_then(|item| item.as_str())
        .unwrap_or("chatcmpl_non_stream_sse_fallback");
    let model = response
        .get("model")
        .and_then(|item| item.as_str())
        .unwrap_or(fallback_model);
    let created = response
        .get("created_at")
        .and_then(|item| item.as_i64())
        .unwrap_or_else(|| chrono::Utc::now().timestamp());
    serde_json::json!({
        "id": id,
        "object": "chat.completion",
        "created": created,
        "model": model,
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": content,
            },
            "finish_reason": "stop",
        }],
        "usage": usage.map(openai_compatible_usage_json).unwrap_or(serde_json::Value::Null),
    })
}

fn anthropic_message_from_sse(
    events: &[serde_json::Value],
    usage: Option<UsageTokens>,
    fallback_model: &str,
) -> serde_json::Value {
    let message = events
        .iter()
        .find_map(|event| event.get("message"))
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let content = anthropic_content_from_sse(events);
    let id = message
        .get("id")
        .and_then(|item| item.as_str())
        .unwrap_or("msg_non_stream_sse_fallback");
    let model = message
        .get("model")
        .and_then(|item| item.as_str())
        .unwrap_or(fallback_model);
    let stop_reason = events
        .iter()
        .rev()
        .find_map(|event| {
            event
                .pointer("/delta/stop_reason")
                .and_then(|item| item.as_str())
        })
        .or_else(|| message.get("stop_reason").and_then(|item| item.as_str()))
        .unwrap_or_else(|| {
            if content
                .iter()
                .any(|item| item.get("type").and_then(|value| value.as_str()) == Some("tool_use"))
            {
                "tool_use"
            } else {
                "end_turn"
            }
        });
    serde_json::json!({
        "id": id,
        "type": "message",
        "role": "assistant",
        "model": model,
        "content": content,
        "stop_reason": stop_reason,
        "stop_sequence": null,
        "usage": usage.map(anthropic_usage_json).unwrap_or(serde_json::Value::Null),
    })
}

fn anthropic_content_from_sse(events: &[serde_json::Value]) -> Vec<serde_json::Value> {
    let mut blocks = std::collections::BTreeMap::<i64, serde_json::Value>::new();
    let mut tool_input_json = std::collections::BTreeMap::<i64, String>::new();
    for event in events {
        let index = event
            .get("index")
            .and_then(|item| item.as_i64())
            .unwrap_or(0);
        match event.get("type").and_then(|item| item.as_str()) {
            Some("content_block_start") => {
                if let Some(block) = event.get("content_block") {
                    blocks.insert(index, block.clone());
                }
            }
            Some("content_block_delta") => {
                let Some(delta) = event.get("delta") else {
                    continue;
                };
                if let Some(text) = delta
                    .get("text")
                    .or_else(|| delta.get("thinking"))
                    .and_then(|item| item.as_str())
                {
                    let block = blocks.entry(index).or_insert_with(|| {
                        serde_json::json!({
                            "type": "text",
                            "text": "",
                        })
                    });
                    append_string_field(block, "text", text);
                } else if let Some(partial) =
                    delta.get("partial_json").and_then(|item| item.as_str())
                {
                    tool_input_json.entry(index).or_default().push_str(partial);
                }
            }
            _ => {}
        }
    }
    for (index, partial) in tool_input_json {
        let parsed = serde_json::from_str::<serde_json::Value>(&partial)
            .unwrap_or_else(|_| serde_json::json!({ "_partial_json": partial }));
        let block = blocks.entry(index).or_insert_with(|| {
            serde_json::json!({
                "type": "tool_use",
                "id": format!("toolu_non_stream_sse_fallback_{index}"),
                "name": "",
            })
        });
        if let Some(object) = block.as_object_mut() {
            object.insert("input".to_string(), parsed);
        }
    }
    let mut content = blocks.into_values().collect::<Vec<_>>();
    if content.is_empty() {
        let text = events
            .iter()
            .filter(|event| {
                event.get("type").and_then(|item| item.as_str()) == Some("content_block_delta")
            })
            .filter_map(|event| event.pointer("/delta/text").and_then(|item| item.as_str()))
            .collect::<String>();
        if !text.is_empty() {
            content.push(serde_json::json!({
                "type": "text",
                "text": text,
            }));
        }
    }
    content
}

fn gemini_generate_content_from_sse(
    events: &[serde_json::Value],
    usage: Option<UsageTokens>,
) -> serde_json::Value {
    let mut text = String::new();
    let mut parts = Vec::<serde_json::Value>::new();
    let mut finish_reason = None::<serde_json::Value>;
    let mut safety_ratings = None::<serde_json::Value>;
    let mut usage_metadata = None::<serde_json::Value>;
    for event in events {
        if let Some(usage) = event.get("usageMetadata") {
            usage_metadata = Some(usage.clone());
        }
        let Some(candidates) = event.get("candidates").and_then(|item| item.as_array()) else {
            continue;
        };
        for candidate in candidates {
            if let Some(value) = candidate.get("finishReason") {
                finish_reason = Some(value.clone());
            }
            if let Some(value) = candidate.get("safetyRatings") {
                safety_ratings = Some(value.clone());
            }
            for part in candidate
                .pointer("/content/parts")
                .and_then(|item| item.as_array())
                .into_iter()
                .flatten()
            {
                if let Some(value) = part.get("text").and_then(|item| item.as_str()) {
                    text.push_str(value);
                } else {
                    parts.push(part.clone());
                }
            }
        }
    }
    if !text.is_empty() {
        parts.insert(0, serde_json::json!({ "text": text }));
    }
    let mut candidate = Map::new();
    candidate.insert(
        "content".to_string(),
        serde_json::json!({
            "parts": parts,
            "role": "model",
        }),
    );
    if let Some(value) = finish_reason {
        candidate.insert("finishReason".to_string(), value);
    }
    if let Some(value) = safety_ratings {
        candidate.insert("safetyRatings".to_string(), value);
    }
    let mut out = Map::new();
    out.insert(
        "candidates".to_string(),
        serde_json::Value::Array(vec![serde_json::Value::Object(candidate)]),
    );
    out.insert(
        "usageMetadata".to_string(),
        usage_metadata
            .or_else(|| usage.map(gemini_usage_metadata_json))
            .unwrap_or(serde_json::Value::Null),
    );
    serde_json::Value::Object(out)
}

fn append_string_field(value: &mut serde_json::Value, key: &str, extra: &str) {
    let Some(object) = value.as_object_mut() else {
        return;
    };
    let current = object.get(key).and_then(|item| item.as_str()).unwrap_or("");
    object.insert(
        key.to_string(),
        serde_json::json!(format!("{current}{extra}")),
    );
}

fn openai_compatible_usage_json(usage: UsageTokens) -> serde_json::Value {
    serde_json::json!({
        "prompt_tokens": usage.input_tokens,
        "completion_tokens": usage.output_tokens,
        "total_tokens": usage.input_tokens + usage.output_tokens,
        "prompt_tokens_details": {
            "cached_tokens": usage.cache_read_tokens,
        },
        "cache_creation_input_tokens": usage.cache_write_tokens,
    })
}

fn responses_usage_json(usage: UsageTokens) -> serde_json::Value {
    serde_json::json!({
        "input_tokens": usage.input_tokens,
        "output_tokens": usage.output_tokens,
        "total_tokens": usage.input_tokens + usage.output_tokens,
        "input_tokens_details": {
            "cached_tokens": usage.cache_read_tokens,
        },
        "cache_creation_input_tokens": usage.cache_write_tokens,
    })
}

fn anthropic_usage_json(usage: UsageTokens) -> serde_json::Value {
    serde_json::json!({
        "input_tokens": usage.input_tokens,
        "output_tokens": usage.output_tokens,
        "cache_read_input_tokens": usage.cache_read_tokens,
        "cache_creation_input_tokens": usage.cache_write_tokens,
    })
}

fn gemini_usage_metadata_json(usage: UsageTokens) -> serde_json::Value {
    serde_json::json!({
        "promptTokenCount": usage.input_tokens,
        "candidatesTokenCount": usage.output_tokens,
        "totalTokenCount": usage.input_tokens + usage.output_tokens,
        "cachedContentTokenCount": usage.cache_read_tokens,
    })
}

async fn forward_stream_with_retries(
    state: &AppState,
    headers: &HeaderMap,
    body: Bytes,
    candidates: &[SelectedShare],
    model_id: Option<Uuid>,
    charge_id: Uuid,
    request_id: &str,
    upstream_path: &str,
    sticky_key: Option<&str>,
    fallback_sticky_key: Option<&str>,
    user_id: Uuid,
    api_key_id: Uuid,
    app_type: &str,
    sticky_model_id: Uuid,
    protocol_family: &str,
) -> Result<(reqwest::Response, SelectedShare), ApiError> {
    let mut last_err = None;
    let Some(model_id) = model_id else {
        return Err(ApiError::bad_request(
            "model_not_supported",
            "model is not supported",
        ));
    };
    for (idx, share) in candidates.iter().enumerate() {
        update_charge_route(state, charge_id, share).await?;
        let started = chrono::Utc::now();
        match forward_to_router_market_proxy_stream(
            state,
            headers,
            body.clone(),
            share,
            request_id,
            upstream_path,
        )
        .await
        {
            Ok((response, model_route)) => {
                update_charge_model_route(state, charge_id, &model_route).await?;
                record_request_attempt(
                    state,
                    request_id,
                    charge_id,
                    model_id,
                    share,
                    idx + 1,
                    "success",
                    None,
                    None,
                    started,
                )
                .await;
                refresh_sticky_routes(
                    state,
                    sticky_key,
                    fallback_sticky_key,
                    user_id,
                    api_key_id,
                    app_type,
                    sticky_model_id,
                    protocol_family,
                    share,
                )
                .await;
                return Ok((response, share.clone()));
            }
            Err(err) => {
                let message = err.to_string();
                let kind = classify_upstream_failure(&message);
                record_request_attempt(
                    state,
                    request_id,
                    charge_id,
                    model_id,
                    share,
                    idx + 1,
                    "error",
                    Some(kind.code()),
                    Some(message.clone()),
                    started,
                )
                .await;
                clear_sticky_routes_for_share(state, sticky_key, fallback_sticky_key, share).await;
                maybe_block_model_share(state, model_id, share, kind, &message).await;
                maybe_report_router_feedback(state, share, kind, &message).await;
                let retryable = is_retryable_failure(kind);
                last_err = Some(err);
                if !retryable {
                    break;
                }
            }
        }
    }
    Err(last_err.unwrap_or_else(|| ApiError::service_unavailable("router market proxy failed")))
}

async fn update_charge_route(
    state: &AppState,
    charge_id: Uuid,
    share: &SelectedShare,
) -> Result<(), ApiError> {
    ledger::ensure_provider_accounts(state.db(), &share.owner_email).await?;
    state.db().execute(
        "UPDATE request_charges SET router_id=?2, share_id=?3, owner_email=?4, routing_rule_id=?5 WHERE id=?1",
        vec![
            crate::db::uuid_val(charge_id),
            crate::db::val(&share.router_id),
            crate::db::val(&share.share_id),
            crate::db::val(&share.owner_email),
            crate::db::opt_uuid_val(share.routing_rule_id),
        ],
    ).await?;
    Ok(())
}

async fn update_charge_model_route(
    state: &AppState,
    charge_id: Uuid,
    route: &UpstreamModelRoute,
) -> Result<(), ApiError> {
    if route.is_empty() {
        return Ok(());
    }
    state
        .db()
        .execute(
            r#"
            UPDATE request_charges
               SET requested_model = COALESCE(?2, requested_model),
                   actual_model = COALESCE(?3, actual_model),
                   actual_model_source = COALESCE(?4, actual_model_source)
             WHERE id = ?1
            "#,
            vec![
                crate::db::uuid_val(charge_id),
                crate::db::opt_val(route.requested_model.as_deref()),
                crate::db::opt_val(route.actual_model.as_deref()),
                crate::db::opt_val(route.actual_model_source.as_deref()),
            ],
        )
        .await?;
    Ok(())
}

async fn record_share_health_sample(
    state: &AppState,
    share: &SelectedShare,
    status: &str,
    latency_ms: Option<i64>,
    error_message: Option<String>,
) {
    let now = crate::db::now_string();
    let _ = state
        .db()
        .execute(
            "INSERT INTO share_health (id, router_id, share_id, status, latency_ms, error_message, checked_at) VALUES (?1,?2,?3,?4,?5,?6,?7)",
            vec![
                crate::db::uuid_val(Uuid::new_v4()),
                crate::db::val(&share.router_id),
                crate::db::val(&share.share_id),
                crate::db::val(status),
                crate::db::opt_val(latency_ms),
                crate::db::opt_val(error_message.clone()),
                crate::db::val(&now),
            ],
        )
        .await;
}

async fn record_share_success(state: &AppState, share: &SelectedShare) {
    let now = crate::db::now_string();
    record_share_health_sample(state, share, "success", None, None).await;
    let _ = state
        .db()
        .execute(
            "UPDATE router_shares SET last_success_at=?3 WHERE router_id=?1 AND share_id=?2",
            vec![
                crate::db::val(&share.router_id),
                crate::db::val(&share.share_id),
                crate::db::val(&now),
            ],
        )
        .await;
    let changed = state
        .db()
        .execute(
            r#"
            UPDATE router_shares
               SET last_error_at=NULL,
                   last_error_message=NULL,
                   last_failure_kind=NULL,
                   last_failure_scope=NULL,
                   failure_count=0,
                   cooldown_until=NULL
             WHERE router_id=?1 AND share_id=?2
               AND (last_failure_scope IS NULL OR last_failure_scope IN ('share','market_path'))
               AND (
                   last_error_at IS NOT NULL
                   OR last_error_message IS NOT NULL
                   OR last_failure_kind IS NOT NULL
                   OR last_failure_scope IS NOT NULL
                   OR failure_count != 0
                   OR cooldown_until IS NOT NULL
               )
            "#,
            vec![
                crate::db::val(&share.router_id),
                crate::db::val(&share.share_id),
            ],
        )
        .await
        .unwrap_or(0);
    if changed > 0 {
        crate::router_client::report_share_runtime_state(
            state,
            crate::router_client::MarketShareRuntimeState {
                share_id: share.share_id.clone(),
                router_id: Some(share.router_id.clone()),
                scope: "share".to_string(),
                kind: "cooldown".to_string(),
                app_type: None,
                model_id: None,
                model_name: None,
                reason_kind: Some("share_cooldown_clear".to_string()),
                reason: None,
                failure_count: None,
                expires_at: Some(now),
            },
        );
    }
}

async fn record_share_failure(
    state: &AppState,
    share: &SelectedShare,
    kind: FailureKind,
    error_message: Option<String>,
) {
    let now = crate::db::now_string();
    record_share_health_sample(state, share, "error", None, error_message.clone()).await;
    let policy = kind.policy();
    let current_failures = state
        .db()
        .query_optional(
            "SELECT failure_count FROM router_shares WHERE router_id=?1 AND share_id=?2",
            vec![
                crate::db::val(&share.router_id),
                crate::db::val(&share.share_id),
            ],
        )
        .await
        .ok()
        .flatten()
        .map(|row| row.i64("failure_count"))
        .unwrap_or(0);
    let failure_count = if policy.counts_against_share {
        current_failures.saturating_add(1)
    } else {
        current_failures
    };
    let cooldown_secs = if policy.counts_against_share {
        crate::failure::cooldown_secs(kind, failure_count)
    } else {
        0
    };
    let cooldown_until = (cooldown_secs > 0)
        .then(|| (chrono::Utc::now() + chrono::Duration::seconds(cooldown_secs)).to_rfc3339());
    let reason = error_message.clone();
    let _ = state
        .db()
        .execute(
            r#"
            UPDATE router_shares
               SET last_error_at=?3,
                   last_error_message=?4,
                   last_failure_kind=?5,
                   last_failure_scope=?6,
                   failure_count=?7,
                   cooldown_until=?8
             WHERE router_id=?1 AND share_id=?2
            "#,
            vec![
                crate::db::val(&share.router_id),
                crate::db::val(&share.share_id),
                crate::db::val(now),
                crate::db::opt_val(error_message),
                crate::db::val(kind.code()),
                crate::db::val(policy.scope.code()),
                crate::db::val(failure_count),
                crate::db::opt_val(cooldown_until.clone()),
            ],
        )
        .await;
    crate::router_client::report_share_runtime_state(
        state,
        crate::router_client::MarketShareRuntimeState {
            share_id: share.share_id.clone(),
            router_id: Some(share.router_id.clone()),
            scope: policy.scope.code().to_string(),
            kind: if cooldown_until.is_some() {
                "cooldown".to_string()
            } else {
                "failure".to_string()
            },
            app_type: Some(share.app_type.clone()),
            model_id: share.price.model_id.map(|id| id.to_string()),
            model_name: Some(share.pricing_model.clone()),
            reason_kind: Some(kind.code().to_string()),
            reason,
            failure_count: Some(failure_count),
            expires_at: cooldown_until,
        },
    );
}

#[allow(clippy::too_many_arguments)]
async fn record_request_attempt(
    state: &AppState,
    request_id: &str,
    charge_id: Uuid,
    model_id: Uuid,
    share: &SelectedShare,
    attempt_no: usize,
    status: &str,
    failure_kind: Option<&str>,
    error_message: Option<String>,
    started_at: chrono::DateTime<chrono::Utc>,
) {
    let finished_at = chrono::Utc::now();
    let latency_ms = (finished_at - started_at).num_milliseconds().max(0);
    let _ = state.db().execute(
        r#"
        INSERT INTO request_attempts
          (id, request_id, charge_id, attempt_no, router_id, share_id, model_id, status, failure_kind, error_message, latency_ms, started_at, finished_at)
        VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)
        "#,
        vec![
            crate::db::uuid_val(Uuid::new_v4()),
            crate::db::val(request_id),
            crate::db::uuid_val(charge_id),
            crate::db::val(attempt_no as i64),
            crate::db::val(&share.router_id),
            crate::db::val(&share.share_id),
            crate::db::uuid_val(model_id),
            crate::db::val(status),
            crate::db::opt_val(failure_kind.map(ToOwned::to_owned)),
            crate::db::opt_val(error_message),
            crate::db::val(latency_ms),
            crate::db::val(started_at.to_rfc3339()),
            crate::db::val(finished_at.to_rfc3339()),
        ],
    ).await;
}

fn classify_upstream_failure(message: &str) -> FailureKind {
    let lower = message.to_ascii_lowercase();
    if lower.contains("timed out") || lower.contains("timeout") {
        FailureKind::ConnectTimeout
    } else if lower.contains("connection refused") || lower.contains("connect refused") {
        FailureKind::ConnectRefused
    } else if is_quota_exhausted_message(&lower) {
        FailureKind::QuotaExhausted
    } else if lower.contains("429") {
        FailureKind::Upstream429
    } else if lower.contains("502") || lower.contains("503") || lower.contains("504") {
        FailureKind::Upstream5xx
    } else if lower.contains("401") || lower.contains("403") {
        FailureKind::AuthInvalid
    } else if is_parameter_level_bad_request(&lower) {
        FailureKind::BadRequest
    } else if is_model_unsupported_message(&lower) {
        FailureKind::ModelUnsupported
    } else if lower.contains("400") || lower.contains("bad request") {
        FailureKind::BadRequest
    } else if lower.contains("router market proxy failed") {
        FailureKind::TunnelUnavailable
    } else {
        FailureKind::BadGatewayResponse
    }
}

fn is_parameter_level_bad_request(lower: &str) -> bool {
    [
        "unsupported parameter",
        "unsupported param",
        "unknown parameter",
        "unknown param",
        "unsupported field",
        "unknown field",
        "unrecognized request argument",
        "unrecognized argument",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn is_model_unsupported_message(lower: &str) -> bool {
    [
        "unsupported model",
        "model unsupported",
        "model not supported",
        "model is not supported",
        "model isn't supported",
        "model not found",
        "model does not exist",
        "model doesn't exist",
        "invalid model",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn is_quota_exhausted_message(lower: &str) -> bool {
    [
        "quota exceeded",
        "quota_exceeded",
        "quota exhausted",
        "quota_exhausted",
        "usage limit",
        "usage_limit",
        "weekly limit",
        "monthly limit",
        "5h",
        "five hour",
        "five_hour",
        "usage credits are required",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn is_retryable_failure(kind: FailureKind) -> bool {
    kind.policy().retryable_hint
}

async fn maybe_report_router_feedback(
    state: &AppState,
    share: &SelectedShare,
    kind: FailureKind,
    message: &str,
) {
    if !kind.policy().report_to_router {
        return;
    }
    if !matches!(kind, FailureKind::Upstream429 | FailureKind::QuotaExhausted) {
        return;
    }
    let ttl_secs = (kind == FailureKind::QuotaExhausted).then(|| quota_exhausted_ttl_secs(message));
    crate::router_client::report_failure(state, &share.router_id, &share.share_id, kind, ttl_secs)
        .await;
}

fn quota_exhausted_ttl_secs(message: &str) -> u64 {
    let lower = message.to_ascii_lowercase();
    if lower.contains("5h") || lower.contains("five hour") || lower.contains("five_hour") {
        return 5 * 60 * 60;
    }
    if lower.contains("monthly") || lower.contains("month") {
        return 31 * 24 * 60 * 60;
    }
    7 * 24 * 60 * 60
}

async fn maybe_block_model_share(
    state: &AppState,
    model_id: Uuid,
    share: &SelectedShare,
    kind: FailureKind,
    message: &str,
) {
    if kind != FailureKind::ModelUnsupported {
        return;
    }
    let now = chrono::Utc::now();
    let expires_at = now + chrono::Duration::hours(24);
    let result = state.db().execute(
        r#"
        INSERT INTO model_share_blocks (model_id, router_id, share_id, reason, expires_at, created_at)
        VALUES (?1,?2,?3,?4,?5,?6)
        ON CONFLICT(model_id, router_id, share_id) DO UPDATE SET
          reason=excluded.reason, expires_at=excluded.expires_at, created_at=excluded.created_at
        "#,
        vec![
            crate::db::uuid_val(model_id),
            crate::db::val(&share.router_id),
            crate::db::val(&share.share_id),
            crate::db::val(message.chars().take(500).collect::<String>()),
            crate::db::val(expires_at.to_rfc3339()),
            crate::db::val(now.to_rfc3339()),
        ],
    ).await;
    if result.is_ok() {
        crate::router_client::report_share_runtime_state(
            state,
            crate::router_client::MarketShareRuntimeState {
                share_id: share.share_id.clone(),
                router_id: Some(share.router_id.clone()),
                scope: "model".to_string(),
                kind: "model_block".to_string(),
                app_type: Some(share.price.app_type.clone()),
                model_id: Some(model_id.to_string()),
                model_name: Some(share.pricing_model.clone()),
                reason_kind: Some(kind.code().to_string()),
                reason: Some(message.chars().take(500).collect::<String>()),
                failure_count: None,
                expires_at: Some(expires_at.to_rfc3339()),
            },
        );
    }
}

async fn api_key_from_headers(
    headers: &HeaderMap,
    state: &AppState,
) -> Result<ApiKeyPrincipal, ApiError> {
    let api_key = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .or_else(|| {
            headers
                .get("x-api-key")
                .and_then(|value| value.to_str().ok())
                .map(str::trim)
                .filter(|value| !value.is_empty())
        })
        .ok_or_else(|| ApiError::unauthorized("missing API key"))?;
    let limiter_subject = api_key.chars().take(18).collect::<String>();
    let hash = crate::api_keys::hash_key(api_key);
    let country = header_country(headers);
    let row = state
        .db()
        .query_optional(
            r#"
        UPDATE api_keys
           SET last_used_at = ?2,
               last_used_ip_country = COALESCE(?3, last_used_ip_country, 'unknown')
         WHERE key_hash = ?1
           AND revoked_at IS NULL
           AND paused_at IS NULL
           AND deleted_at IS NULL
           AND (expires_at IS NULL OR expires_at > ?2)
         RETURNING
            id,
            user_id,
            (SELECT email FROM users WHERE users.id = api_keys.user_id) AS user_email,
            monthly_spend_cap,
            scope_json
        "#,
            vec![
                crate::db::val(hash),
                crate::db::val(crate::db::now_string()),
                crate::db::opt_val(country),
            ],
        )
        .await?;
    let Some(row) = row else {
        crate::rate_limit::check("api_key_auth_failed", &limiter_subject, 30)?;
        return Err(ApiError::unauthorized("invalid API key"));
    };
    Ok(ApiKeyPrincipal {
        api_key_id: row.uuid("id"),
        user_id: row.uuid("user_id"),
        user_email: row.string("user_email"),
        is_admin: state
            .config
            .market_admin_emails
            .iter()
            .any(|email| email.eq_ignore_ascii_case(&row.string("user_email"))),
        monthly_spend_cap: row.opt_decimal("monthly_spend_cap"),
        scope_json: row
            .opt_string("scope_json")
            .and_then(|value| serde_json::from_str(&value).ok()),
    })
}

async fn enforce_market_maintenance(
    state: &AppState,
    api: &ApiKeyPrincipal,
) -> Result<(), ApiError> {
    let runtime = state.market_runtime.read().await.clone();
    if !runtime.maintenance_enabled {
        return Ok(());
    }
    let is_owner = runtime
        .owner_email
        .as_deref()
        .map(|owner| owner.eq_ignore_ascii_case(&api.user_email))
        .unwrap_or(false);
    if is_owner || api.is_admin {
        return Ok(());
    }
    let message = runtime
        .maintenance_message
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "Market 正在维护，预计稍后恢复。".to_string());
    Err(ApiError::Http {
        status: StatusCode::SERVICE_UNAVAILABLE,
        error_type: "api_error",
        code: "market_maintenance",
        message,
        param: None,
    })
}

async fn enforce_monthly_spend_cap(
    db: &crate::db::Db,
    api: &ApiKeyPrincipal,
    next_reserved_amount: Decimal,
) -> Result<(), ApiError> {
    let Some(cap) = api.monthly_spend_cap else {
        return Ok(());
    };
    let month_start = chrono::Utc::now()
        .format("%Y-%m-01T00:00:00+00:00")
        .to_string();
    let row = db
        .query_optional(
            r#"
            SELECT COALESCE(SUM(CAST(COALESCE(usage_amount, reserved_amount) AS REAL)), 0) AS spent
              FROM request_charges
             WHERE api_key_id = ?1
               AND created_at >= ?2
               AND status IN ('reserved','streaming','needs_review','settled')
            "#,
            vec![
                crate::db::uuid_val(api.api_key_id),
                crate::db::val(month_start),
            ],
        )
        .await?;
    let spent = row.map(|row| row.decimal("spent")).unwrap_or(Decimal::ZERO);
    if spent + next_reserved_amount > cap {
        return Err(ApiError::bad_request(
            "monthly_spend_cap_exceeded",
            "API key monthly spend cap would be exceeded",
        ));
    }
    Ok(())
}

fn header_country(headers: &HeaderMap) -> Option<String> {
    ["cf-ipcountry", "x-vercel-ip-country", "x-country-code"]
        .into_iter()
        .find_map(|key| {
            headers
                .get(key)
                .and_then(|value| value.to_str().ok())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|value| value.to_ascii_uppercase())
        })
        .or_else(|| Some("unknown".to_string()))
}

#[derive(Clone)]
struct SelectedShare {
    router_id: String,
    share_id: String,
    owner_email: String,
    app_type: String,
    enabled_codex: bool,
    active_requests: i64,
    parallel_limit: i64,
    priority: i64,
    online_rate_24h: f64,
    routing_rule_id: Option<Uuid>,
    pricing_model: String,
    pricing_slot: String,
    pricing_model_source: String,
    share_official: bool,
    price: pricing::PriceItem,
}

async fn select_share_candidates_with_sync_retry(
    state: &AppState,
    api: &ApiKeyPrincipal,
    market_email: Option<&str>,
    app_type: &str,
    model: &str,
    limit: i64,
    sticky_keys: &[&str],
) -> Result<Vec<SelectedShare>, ApiError> {
    match select_share_candidates(
        state.db(),
        api,
        market_email,
        app_type,
        model,
        limit,
        sticky_keys,
    )
    .await
    {
        Ok(candidates) => Ok(candidates),
        Err(err) if should_retry_after_share_sync(&err) => {
            tracing::info!(
                app_type,
                model,
                "no local router share candidate; syncing router shares before retry"
            );
            if let Err(sync_err) = crate::router_client::sync_shares(state).await {
                tracing::warn!(
                    error = %sync_err,
                    app_type,
                    model,
                    "router share sync retry failed"
                );
                return Err(err);
            }
            select_share_candidates(
                state.db(),
                api,
                market_email,
                app_type,
                model,
                limit,
                sticky_keys,
            )
            .await
        }
        Err(err) => Err(err),
    }
}

fn should_retry_after_share_sync(err: &ApiError) -> bool {
    matches!(
        err,
        ApiError::Http {
            status,
            code: "service_unavailable",
            message,
            ..
        } if *status == StatusCode::SERVICE_UNAVAILABLE
            && message.contains("no available router share")
    )
}

async fn select_share_candidates(
    db: &crate::db::Db,
    api: &ApiKeyPrincipal,
    market_email: Option<&str>,
    app_type: &str,
    model: &str,
    limit: i64,
    sticky_keys: &[&str],
) -> Result<Vec<SelectedShare>, ApiError> {
    let profile = crate::scheduling::resolve_profile(api.scope_json.as_ref());
    let weights = profile.weights();
    let app_type_alias = share_app_type_alias(app_type);
    let support = share_support_flags(app_type);
    let capability = share_capability(app_type);
    let requested_slot = requested_model_slot(capability, model);
    let request_price = pricing::match_price(db, app_type, model).await.ok();
    let model_id = request_price
        .as_ref()
        .and_then(|price| price.model_id)
        .unwrap_or_else(Uuid::nil);
    let rule = db
        .query_optional(
            "SELECT id, mode, enabled FROM model_routing_rules WHERE model_id=?1 LIMIT 1",
            vec![crate::db::uuid_val(model_id)],
        )
        .await?;
    let routing_rule_id = rule.as_ref().map(|row| row.uuid("id"));
    if let Some(rule_row) = &rule {
        if rule_row.bool("enabled") && rule_row.string("mode") == "include_only" {
            let bound = db
                .query_one(
                    "SELECT COUNT(*) AS count FROM model_routing_rule_shares WHERE rule_id=?1",
                    vec![crate::db::uuid_val(rule_row.uuid("id"))],
                )
                .await?
                .i64("count");
            if bound == 0 {
                return Err(ApiError::service_unavailable(format!(
                    "no available router share for app_type={app_type}, model={model}"
                )));
            }
        }
    }
    let allowlist_count = db
        .query_one(
            "SELECT COUNT(*) AS count FROM market_api_key_share_allowlist WHERE api_key_id=?1",
            vec![crate::db::uuid_val(api.api_key_id)],
        )
        .await?
        .i64("count");
    let now = chrono::Utc::now().to_rfc3339();
    let sticky_pair = lookup_sticky_route_pair_by_keys(db, sticky_keys, &now).await?;
    let sticky_router_id = sticky_pair
        .as_ref()
        .map(|(router_id, _)| router_id.as_str());
    let sticky_share_id = sticky_pair.as_ref().map(|(_, share_id)| share_id.as_str());
    let rows = db.query_all(
        r#"
        SELECT router_id, share_id, COALESCE(owner_email, installation_owner_email) AS owner_email,
               app_type, enabled_codex, active_requests, parallel_limit, priority, online_rate_24h, raw_json
          FROM router_shares
         WHERE (app_type IN (?1, ?8)
                OR (?9 = 1 AND enabled_codex = 1)
                OR (?10 = 1 AND enabled_claude = 1)
                OR (?11 = 1 AND enabled_gemini = 1))
           AND online = 1 AND share_status = 'active'
           AND COALESCE(disabled_by_market, 0) = 0
           AND (
             parallel_limit = -1
             OR active_requests < parallel_limit
             OR (?20 IS NOT NULL AND router_id = ?20 AND share_id = ?21)
           )
           AND COALESCE(owner_email, installation_owner_email) IS NOT NULL
           AND (cooldown_until IS NULL OR cooldown_until < ?2)
           AND (
             ?13 = 0
             OR EXISTS (
               SELECT 1 FROM market_api_key_share_allowlist aks
                WHERE aks.api_key_id = ?14
                  AND aks.router_id = router_shares.router_id
                  AND aks.share_id = router_shares.share_id
             )
           )
           AND NOT EXISTS (
             SELECT 1 FROM model_share_blocks msb
              WHERE msb.model_id = ?6 AND msb.router_id = router_shares.router_id
                AND msb.share_id = router_shares.share_id AND msb.expires_at > ?2
           )
           AND NOT EXISTS (
             SELECT 1 FROM market_share_capability_blocks mscb
              WHERE mscb.router_id = router_shares.router_id
                AND mscb.share_id = router_shares.share_id
                AND mscb.capability = ?12
           )
           AND COALESCE(json_extract(raw_json, '$.appAvailability.' || ?12 || '.status'), '') != 'unavailable'
           AND (
             ?3 IS NULL
             OR ?4 = 0
             OR ?5 = 'all'
             OR (?5 = 'include_only' AND EXISTS (
               SELECT 1 FROM model_routing_rule_shares mrs
                WHERE mrs.rule_id = ?3 AND mrs.router_id = router_shares.router_id AND mrs.share_id = router_shares.share_id
             ))
             OR (?5 = 'exclude' AND NOT EXISTS (
               SELECT 1 FROM model_routing_rule_shares mrs
                WHERE mrs.rule_id = ?3 AND mrs.router_id = router_shares.router_id AND mrs.share_id = router_shares.share_id
             ))
           )
         -- Base score: combines router-computed signals so a single ORDER BY
         -- replaces the older multi-key tiebreaker. Weights come from the
         -- caller's SchedulingProfile (?15-?18); ?19 (price_bias) shifts the
         -- score for cost-leaning profiles. owner_penalty here is the column
         -- on router_shares — a 429 from one share down-ranks every share of
         -- the same owner. failure_count subtracts a small linear penalty so
         -- a chronically-flaky share drops below healthy peers without being
         -- filtered out entirely.
         ORDER BY
           CASE WHEN ?20 IS NOT NULL AND router_id = ?20 AND share_id = ?21 THEN 0 ELSE 1 END ASC,
           (
             (?15 * stability
              + ?16 * quota_health
              + ?17 * CASE WHEN parallel_limit = -1 THEN 1.0
                           ELSE 1.0 - (CAST(active_requests AS REAL) / CAST(parallel_limit AS REAL))
                      END
              + ?18 * 1.0)
             * (1.0 + ?19 * 0.5)
             * owner_penalty
             - 0.05 * MIN(failure_count, 5)
           ) DESC,
           priority DESC,
           COALESCE(last_success_at, last_seen_at) DESC
         LIMIT ?7
        "#,
        vec![
            crate::db::val(app_type),
            crate::db::val(&now),
            crate::db::opt_uuid_val(routing_rule_id),
            crate::db::val(rule.as_ref().is_none_or(|row| row.bool("enabled"))),
            crate::db::val(rule.as_ref().map(|row| row.string("mode")).unwrap_or_else(|| "all".to_string())),
            crate::db::uuid_val(model_id),
            crate::db::val(limit.max(1)),
            crate::db::val(app_type_alias),
            crate::db::val(support.codex),
            crate::db::val(support.claude),
            crate::db::val(support.gemini),
            crate::db::val(capability),
            crate::db::val(allowlist_count),
            crate::db::uuid_val(api.api_key_id),
            crate::db::val(weights.stability),
            crate::db::val(weights.quota_health),
            crate::db::val(weights.headroom),
            crate::db::val(weights.freshness),
            crate::db::val(weights.price_bias),
            crate::db::opt_val(sticky_router_id),
            crate::db::opt_val(sticky_share_id),
        ],
    )
    .await?;
    let mut shares = Vec::new();
    for row in rows {
        let router_id = row.string("router_id");
        let share_id = row.string("share_id");
        let raw_json = row.opt_string("raw_json");
        if !raw_share_app_token_sale_visible(raw_json.as_deref(), capability, "Yes", market_email) {
            tracing::debug!(
                %router_id,
                %share_id,
                capability,
                "skip router share not listed for token market on requested app"
            );
            continue;
        }
        if raw_share_app_quota_blocked(raw_json.as_deref(), capability) {
            tracing::debug!(
                %router_id,
                %share_id,
                capability,
                "skip router share blocked by router app quota state"
            );
            continue;
        }
        let Some((pricing_model, pricing_slot, pricing_model_source, share_official, price)) =
            resolve_share_pricing(
                db,
                &router_id,
                &share_id,
                capability,
                requested_slot,
                model,
                request_price.as_ref(),
                raw_json.as_deref(),
            )
            .await?
        else {
            continue;
        };
        let share_sale_percent = share_sale_percent_from_raw(&raw_json, capability);
        if let Some(share_sale_percent) = share_sale_percent {
            if price.discount_percent < rust_decimal::Decimal::from(share_sale_percent) {
                continue;
            }
        }
        if !api_key_allows_model_access(
            api,
            capability,
            &pricing_slot,
            &pricing_model,
            &price.app_type,
            price.model_id,
        ) {
            continue;
        }
        shares.push(SelectedShare {
            router_id,
            share_id,
            owner_email: row.string("owner_email"),
            app_type: row.string("app_type"),
            enabled_codex: row.bool("enabled_codex"),
            active_requests: row.i64("active_requests"),
            parallel_limit: row.i64("parallel_limit"),
            priority: row.i64("priority"),
            online_rate_24h: row.string("online_rate_24h").parse().unwrap_or(1.0),
            routing_rule_id,
            pricing_model,
            pricing_slot,
            pricing_model_source,
            share_official,
            price,
        });
    }
    if shares.is_empty() {
        let disabled_count = db
            .query_one(
                r#"
                SELECT COUNT(*) AS count
                  FROM router_shares
                 WHERE (app_type IN (?1, ?2)
                        OR (?3 = 1 AND enabled_codex = 1)
                        OR (?4 = 1 AND enabled_claude = 1)
                        OR (?5 = 1 AND enabled_gemini = 1))
                   AND online = 1 AND share_status = 'active'
                   AND COALESCE(disabled_by_market, 0) = 1
                "#,
                vec![
                    crate::db::val(app_type),
                    crate::db::val(app_type_alias),
                    crate::db::val(support.codex),
                    crate::db::val(support.claude),
                    crate::db::val(support.gemini),
                ],
            )
            .await?
            .i64("count");
        if disabled_count > 0 {
            return Err(ApiError::service_unavailable(format!(
                "all linked router shares are disabled by this market, app_type={app_type}, model={model}"
            )));
        }
        if allowlist_count > 0 {
            Err(ApiError::service_unavailable(format!(
                "no available router share for this API key allowlist, app_type={app_type}, model={model}"
            )))
        } else {
            Err(ApiError::service_unavailable(format!(
                "no available router share for app_type={app_type}, model={model}"
            )))
        }
    } else {
        Ok(shares)
    }
}

fn share_sale_percent_from_raw(raw_json: &Option<String>, capability: &str) -> Option<u16> {
    let raw = raw_json.as_deref()?;
    let value = serde_json::from_str::<serde_json::Value>(raw).ok()?;
    let percent = value
        .get("appRuntimes")
        .and_then(|runtimes| runtimes.get(capability))
        .and_then(|runtime| runtime.get("forSaleOfficialPricePercent"))?;
    percent
        .as_u64()
        .and_then(|value| u16::try_from(value).ok())
        .filter(|value| (1..=100).contains(value))
}

pub(crate) fn raw_share_app_token_sale_visible(
    raw_json: Option<&str>,
    capability: &str,
    legacy_for_sale: &str,
    market_email: Option<&str>,
) -> bool {
    let Some(raw) = raw_json else {
        return legacy_for_sale == "Yes";
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
        return legacy_for_sale == "Yes";
    };
    if let Some(market_apps) = value.get("marketApps").and_then(|apps| apps.as_object()) {
        let Some(app) = market_apps.get(capability) else {
            return false;
        };
        return app
            .get("supported")
            .and_then(|value| value.as_bool())
            .unwrap_or(true)
            && app
                .get("visible")
                .and_then(|value| value.as_bool())
                .unwrap_or(false)
            && app
                .get("forSale")
                .and_then(|value| value.as_str())
                .is_some_and(|value| value == "Yes")
            && app
                .get("saleMarketKind")
                .and_then(|value| value.as_str())
                .is_some_and(|value| value == "token");
    }
    if let Some(app) = value
        .get("appSettings")
        .and_then(|settings| settings.get(capability))
    {
        let listed = app
            .get("forSale")
            .and_then(|value| value.as_str())
            .is_some_and(|value| value == "Yes")
            && app
                .get("saleMarketKind")
                .and_then(|value| value.as_str())
                .is_none_or(|value| value == "token");
        if !listed {
            return false;
        }
        let mode = app
            .get("marketAccessMode")
            .and_then(|value| value.as_str())
            .unwrap_or("selected");
        if mode == "all" {
            return true;
        }
        let Some(market_email) = market_email else {
            return false;
        };
        return app
            .get("sharedWithEmails")
            .and_then(|value| value.as_array())
            .is_some_and(|emails| {
                emails.iter().any(|email| {
                    email
                        .as_str()
                        .is_some_and(|email| email.eq_ignore_ascii_case(market_email))
                })
            });
    }
    legacy_for_sale == "Yes"
}

fn raw_share_app_quota_blocked(raw_json: Option<&str>, capability: &str) -> bool {
    let Some(raw) = raw_json else {
        return false;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
        return false;
    };
    if value
        .get("appAvailability")
        .and_then(|availability| availability.get(capability))
        .and_then(|entry| entry.get("status"))
        .and_then(|status| status.as_str())
        .is_some_and(|status| status.eq_ignore_ascii_case("unavailable"))
    {
        return true;
    }
    if value
        .get("appRuntimes")
        .and_then(|runtimes| runtimes.get(capability))
        .and_then(|runtime| runtime.get("quota"))
        .is_some_and(raw_quota_block_is_active)
    {
        return true;
    }
    value
        .get("modelHealth")
        .and_then(|health| health.get(capability))
        .and_then(|entries| entries.as_array())
        .is_some_and(|entries| {
            entries
                .iter()
                .any(|entry| raw_model_health_entry_is_quota_blocked(entry, capability))
        })
}

fn raw_quota_block_is_active(quota: &serde_json::Value) -> bool {
    let availability = quota
        .get("availability")
        .and_then(|value| value.as_str())
        .unwrap_or("available");
    if !matches!(
        availability,
        "short_window_exhausted" | "long_window_exhausted"
    ) {
        return false;
    }
    let Some(blocked_until) = quota.get("blockedUntil").and_then(|value| value.as_str()) else {
        return true;
    };
    chrono::DateTime::parse_from_rfc3339(blocked_until)
        .map(|dt| dt.with_timezone(&chrono::Utc) > chrono::Utc::now())
        .unwrap_or(true)
}

fn raw_model_health_entry_is_quota_blocked(entry: &serde_json::Value, capability: &str) -> bool {
    let status = entry
        .get("status")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    if status.eq_ignore_ascii_case("quota_blocked") {
        return true;
    }
    if !status.eq_ignore_ascii_case("failed") {
        return false;
    }
    let requested = entry
        .get("requestedModel")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    let actual = entry
        .get("actualModel")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    if requested != capability || actual != capability {
        return false;
    }
    let message = entry
        .get("errorMessage")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    message.contains("quota exhausted")
        || message.contains("quota_exhausted")
        || message.contains("usage limit")
        || message.contains("usage_limit")
        || message.contains("weekly limit")
        || message.contains("monthly limit")
        || message.contains("no credits")
}

#[allow(clippy::too_many_arguments)]
async fn resolve_share_pricing(
    db: &crate::db::Db,
    router_id: &str,
    share_id: &str,
    capability: &str,
    requested_slot: &str,
    request_model: &str,
    request_price: Option<&pricing::PriceItem>,
    raw_json: Option<&str>,
) -> Result<Option<(String, String, String, bool, pricing::PriceItem)>, ApiError> {
    let support = db
        .query_optional(
            r#"
            SELECT slot, actual_model, official
              FROM router_share_model_support
             WHERE router_id=?1 AND share_id=?2 AND app=?3 AND (slot=?4 OR slot='official')
             ORDER BY CASE WHEN slot=?4 THEN 0 ELSE 1 END
             LIMIT 1
            "#,
            vec![
                crate::db::val(router_id),
                crate::db::val(share_id),
                crate::db::val(capability),
                crate::db::val(requested_slot),
            ],
        )
        .await?;
    let Some(support) = support else {
        if raw_share_app_official_runtime(raw_json, capability) {
            let Some(price) = request_price.cloned() else {
                return Ok(None);
            };
            return Ok(Some((
                request_model.to_string(),
                requested_slot.to_string(),
                "official_runtime_raw".to_string(),
                true,
                price,
            )));
        }
        return Ok(None);
    };
    if support.bool("official") {
        let Some(price) = request_price.cloned() else {
            return Ok(None);
        };
        return Ok(Some((
            request_model.to_string(),
            requested_slot.to_string(),
            "official_request_model".to_string(),
            true,
            price,
        )));
    }
    let actual_model = support.opt_string("actual_model").unwrap_or_default();
    if actual_model.trim().is_empty() {
        return Ok(None);
    }
    let price = pricing::match_concrete_price_any_app(db, &actual_model)
        .await
        .ok();
    let Some(price) = price else {
        return Ok(None);
    };
    Ok(Some((
        actual_model,
        support.string("slot"),
        "share_runtime_mapping".to_string(),
        false,
        price,
    )))
}

fn raw_share_app_official_runtime(raw_json: Option<&str>, capability: &str) -> bool {
    let Some(raw) = raw_json else {
        return false;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
        return false;
    };
    value
        .get("appRuntimes")
        .and_then(|runtimes| runtimes.get(capability))
        .and_then(|runtime| runtime.get("kind"))
        .and_then(|kind| kind.as_str())
        .is_some_and(|kind| kind == "official_oauth")
}

fn requested_model_slot(capability: &str, model: &str) -> &'static str {
    if capability != "claude" {
        return "model";
    }
    let lower = model.to_ascii_lowercase();
    if lower.contains("haiku") {
        "haiku"
    } else if lower.contains("opus") {
        "opus"
    } else if lower.contains("sonnet") {
        "sonnet"
    } else {
        "default"
    }
}

fn api_key_allows_model_access(
    api: &ApiKeyPrincipal,
    capability: &str,
    slot: &str,
    pricing_model: &str,
    pricing_vendor: &str,
    model_id: Option<Uuid>,
) -> bool {
    let vendor = normalize_model_vendor(pricing_vendor);
    let Some(scope) = &api.scope_json else {
        return default_agent_model_vendors(capability).contains(&vendor);
    };
    if let Some(agent_model_vendors) = scope
        .get("agent_model_vendors")
        .or_else(|| scope.get("agentModelVendors"))
    {
        let Some(vendors) = agent_model_vendors
            .get(capability)
            .and_then(|value| value.as_array())
        else {
            return false;
        };
        return vendors
            .iter()
            .filter_map(|value| value.as_str())
            .map(normalize_model_vendor)
            .any(|allowed| allowed == "*" || allowed == vendor);
    }
    if let Some(model_access) = scope
        .get("model_access")
        .or_else(|| scope.get("modelAccess"))
    {
        let Some(app_scope) = model_access.get(capability) else {
            return false;
        };
        let Some(models) = app_scope.get(slot).and_then(|value| value.as_array()) else {
            return false;
        };
        return models
            .iter()
            .filter_map(|value| value.as_str())
            .any(|allowed| allowed == pricing_model);
    }
    if let Some(model_ids) = scope.get("model_ids").and_then(|value| value.as_array()) {
        let Some(model_id) = model_id else {
            return false;
        };
        let model_id = model_id.to_string();
        return model_ids
            .iter()
            .filter_map(|value| value.as_str())
            .any(|allowed| allowed == model_id);
    }
    default_agent_model_vendors(capability).contains(&vendor)
}

fn normalize_model_vendor(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "anthropic" | "claude" => "anthropic".to_string(),
        "openai" | "codex" => "openai".to_string(),
        "google" | "gemini" => "gemini".to_string(),
        "cursor" | "cursor_apikey" | "cursor-api-key" => "cursor".to_string(),
        "deepseek" => "deepseek".to_string(),
        other => other.to_string(),
    }
}

fn default_agent_model_vendors(capability: &str) -> Vec<String> {
    match capability {
        "claude" => vec!["*".to_string()],
        "codex" => vec!["*".to_string()],
        "gemini" => vec!["*".to_string()],
        _ => Vec::new(),
    }
}

async fn order_share_candidates(
    state: &AppState,
    sticky_scope: &StickyRouteScope,
    mut candidates: Vec<SelectedShare>,
) -> Result<Vec<SelectedShare>, ApiError> {
    if candidates.len() <= 1 || !state.config.market_share_sticky_enabled {
        return Ok(candidates);
    }
    let now = chrono::Utc::now().to_rfc3339();
    let _ = state
        .db()
        .execute(
            "DELETE FROM market_share_sticky_routes WHERE expires_at <= ?1",
            vec![crate::db::val(&now)],
        )
        .await;
    let sticky_pair = lookup_sticky_route_pair(state, sticky_scope, &now).await?;
    let session_loads = share_session_loads_for_app(state, &sticky_scope.app_type, &now).await?;
    candidates.sort_by(|a, b| {
        let a_sticky = sticky_pair
            .as_ref()
            .is_some_and(|(router, share)| router == &a.router_id && share == &a.share_id);
        let b_sticky = sticky_pair
            .as_ref()
            .is_some_and(|(router, share)| router == &b.router_id && share == &b.share_id);
        let a_session_load = session_loads
            .get(&(a.router_id.clone(), a.share_id.clone()))
            .copied()
            .unwrap_or(0);
        let b_session_load = session_loads
            .get(&(b.router_id.clone(), b.share_id.clone()))
            .copied()
            .unwrap_or(0);
        b_sticky
            .cmp(&a_sticky)
            .then_with(|| a_session_load.cmp(&b_session_load))
            .then_with(|| a.active_requests.cmp(&b.active_requests))
            .then_with(|| {
                rendezvous_share_score(&sticky_scope.sticky_key, b)
                    .total_cmp(&rendezvous_share_score(&sticky_scope.sticky_key, a))
            })
            .then_with(|| a.router_id.cmp(&b.router_id))
            .then_with(|| a.share_id.cmp(&b.share_id))
    });
    Ok(candidates)
}

async fn lookup_sticky_route_pair(
    state: &AppState,
    sticky_scope: &StickyRouteScope,
    now: &str,
) -> Result<Option<(String, String)>, ApiError> {
    let sticky_keys = sticky_scope.sticky_keys();
    lookup_sticky_route_pair_by_keys(state.db(), &sticky_keys, now).await
}

async fn lookup_sticky_route_pair_by_keys(
    db: &crate::db::Db,
    sticky_keys: &[&str],
    now: &str,
) -> Result<Option<(String, String)>, ApiError> {
    for sticky_key in sticky_keys {
        if let Some(row) = db
            .query_optional(
                r#"
                SELECT router_id, share_id
                  FROM market_share_sticky_routes
                 WHERE sticky_key=?1 AND expires_at>?2
                 LIMIT 1
                "#,
                vec![crate::db::val(*sticky_key), crate::db::val(now)],
            )
            .await?
        {
            return Ok(Some((row.string("router_id"), row.string("share_id"))));
        }
    }
    Ok(None)
}

async fn share_session_loads_for_app(
    state: &AppState,
    app_type: &str,
    now: &str,
) -> Result<std::collections::HashMap<(String, String), i64>, ApiError> {
    let rows = state
        .db()
        .query_all(
            r#"
            SELECT router_id, share_id, COUNT(*) AS session_load
              FROM market_share_sticky_routes
             WHERE app_type=?1 AND expires_at>?2
             GROUP BY router_id, share_id
            "#,
            vec![crate::db::val(app_type), crate::db::val(now)],
        )
        .await?;
    Ok(rows
        .into_iter()
        .map(|row| {
            (
                (row.string("router_id"), row.string("share_id")),
                row.i64("session_load"),
            )
        })
        .collect())
}

fn rendezvous_share_score(sticky_key: &str, share: &SelectedShare) -> f64 {
    let mut hasher = Sha256::new();
    hasher.update(sticky_key.as_bytes());
    hasher.update(b":");
    hasher.update(share.router_id.as_bytes());
    hasher.update(b":");
    hasher.update(share.share_id.as_bytes());
    let digest = hasher.finalize();
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&digest[..8]);
    let base = u64::from_be_bytes(bytes) as f64 / u64::MAX as f64;
    base * share_weight(share)
}

fn share_weight(share: &SelectedShare) -> f64 {
    let priority = (share.priority + 10).max(1) as f64;
    let online = share.online_rate_24h.clamp(0.1, 1.0);
    let capacity = if share.parallel_limit == -1 {
        1.0
    } else if share.parallel_limit <= 0 {
        0.1
    } else {
        (1.0 - (share.active_requests as f64 / share.parallel_limit as f64)).clamp(0.1, 1.0)
    };
    priority * online * capacity
}

#[allow(clippy::too_many_arguments)]
async fn refresh_sticky_routes(
    state: &AppState,
    sticky_key: Option<&str>,
    fallback_sticky_key: Option<&str>,
    user_id: Uuid,
    api_key_id: Uuid,
    app_type: &str,
    model_id: Uuid,
    protocol_family: &str,
    share: &SelectedShare,
) {
    refresh_sticky_route(
        state,
        sticky_key,
        user_id,
        api_key_id,
        app_type,
        model_id,
        protocol_family,
        share,
    )
    .await;
    if fallback_sticky_key.is_some() && fallback_sticky_key != sticky_key {
        refresh_sticky_route(
            state,
            fallback_sticky_key,
            user_id,
            api_key_id,
            app_type,
            model_id,
            protocol_family,
            share,
        )
        .await;
    }
}

#[allow(clippy::too_many_arguments)]
async fn refresh_sticky_route(
    state: &AppState,
    sticky_key: Option<&str>,
    user_id: Uuid,
    api_key_id: Uuid,
    app_type: &str,
    model_id: Uuid,
    protocol_family: &str,
    share: &SelectedShare,
) {
    let Some(sticky_key) = sticky_key else {
        return;
    };
    if !state.config.market_share_sticky_enabled || state.config.market_share_sticky_ttl_secs == 0 {
        return;
    }
    let now = chrono::Utc::now();
    let expires_at =
        (now + chrono::Duration::seconds(state.config.market_share_sticky_ttl_secs)).to_rfc3339();
    let _ = state
        .db()
        .execute(
            r#"
            INSERT INTO market_share_sticky_routes
              (sticky_key, api_key_id, user_id, app_type, model_id, protocol_family, router_id, share_id, expires_at, last_success_at, created_at, updated_at)
            VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?10,?10)
            ON CONFLICT(sticky_key) DO UPDATE SET
              api_key_id=excluded.api_key_id,
              router_id=excluded.router_id,
              share_id=excluded.share_id,
              expires_at=excluded.expires_at,
              last_success_at=excluded.last_success_at,
              updated_at=excluded.updated_at
            "#,
            vec![
                crate::db::val(sticky_key),
                crate::db::uuid_val(api_key_id),
                crate::db::uuid_val(user_id),
                crate::db::val(app_type),
                crate::db::uuid_val(model_id),
                crate::db::val(protocol_family),
                crate::db::val(&share.router_id),
                crate::db::val(&share.share_id),
                crate::db::val(expires_at),
                crate::db::val(now.to_rfc3339()),
            ],
        )
        .await;
}

async fn clear_sticky_routes_for_share(
    state: &AppState,
    sticky_key: Option<&str>,
    fallback_sticky_key: Option<&str>,
    share: &SelectedShare,
) {
    clear_sticky_route_for_share(state, sticky_key, share).await;
    if fallback_sticky_key.is_some() && fallback_sticky_key != sticky_key {
        clear_sticky_route_for_share(state, fallback_sticky_key, share).await;
    }
}

async fn clear_sticky_route_for_share(
    state: &AppState,
    sticky_key: Option<&str>,
    share: &SelectedShare,
) {
    let Some(sticky_key) = sticky_key else {
        return;
    };
    let _ = state
        .db()
        .execute(
            "DELETE FROM market_share_sticky_routes WHERE sticky_key=?1 AND router_id=?2 AND share_id=?3",
            vec![
                crate::db::val(sticky_key),
                crate::db::val(&share.router_id),
                crate::db::val(&share.share_id),
            ],
        )
        .await;
}

async fn resolve_sticky_route_scope(
    state: &AppState,
    headers: &HeaderMap,
    body_json: &serde_json::Value,
    body: &Bytes,
    api: &ApiKeyPrincipal,
    app_type: &str,
    model_id: Uuid,
    protocol_family: &str,
) -> Result<StickyRouteScope, ApiError> {
    let session = request_session_key_info(headers, body_json, body);
    let previous_response_id = session.previous_response_id.clone();
    let mut scope = StickyRouteScope::new(
        api.user_id,
        api.api_key_id,
        app_type,
        model_id,
        protocol_family,
        session,
    );
    if let Some(response_id) = previous_response_id.as_deref() {
        if let Some(sticky_key) = lookup_response_sticky_key(
            state,
            response_id,
            api.user_id,
            api.api_key_id,
            app_type,
            protocol_family,
        )
        .await?
        {
            if sticky_key != scope.sticky_key {
                scope.fallback_sticky_key = Some(sticky_key);
            }
        }
    }
    Ok(scope)
}

async fn lookup_response_sticky_key(
    state: &AppState,
    response_id: &str,
    user_id: Uuid,
    api_key_id: Uuid,
    app_type: &str,
    protocol_family: &str,
) -> Result<Option<String>, ApiError> {
    if !state.config.market_share_sticky_enabled {
        return Ok(None);
    }
    let response_id = response_id.trim();
    if response_id.is_empty() {
        return Ok(None);
    }
    let now = chrono::Utc::now().to_rfc3339();
    let _ = state
        .db()
        .execute(
            "DELETE FROM market_response_sticky_routes WHERE expires_at <= ?1",
            vec![crate::db::val(&now)],
        )
        .await;
    Ok(state
        .db()
        .query_optional(
            r#"
            SELECT sticky_key
              FROM market_response_sticky_routes
             WHERE response_id = ?1
               AND user_id = ?2
               AND api_key_id = ?3
               AND app_type = ?4
               AND protocol_family = ?5
               AND expires_at > ?6
             LIMIT 1
            "#,
            vec![
                crate::db::val(response_id),
                crate::db::uuid_val(user_id),
                crate::db::uuid_val(api_key_id),
                crate::db::val(app_type),
                crate::db::val(protocol_family),
                crate::db::val(now),
            ],
        )
        .await?
        .map(|row| row.string("sticky_key")))
}

#[allow(clippy::too_many_arguments)]
async fn record_response_sticky_route(
    state: &AppState,
    response_id: &str,
    sticky_key: &str,
    user_id: Uuid,
    api_key_id: Uuid,
    app_type: &str,
    model_id: Uuid,
    protocol_family: &str,
    share: &SelectedShare,
) {
    if !state.config.market_share_sticky_enabled || state.config.market_share_sticky_ttl_secs == 0 {
        return;
    }
    let response_id = response_id.trim();
    if response_id.is_empty() {
        return;
    }
    let now = chrono::Utc::now();
    let expires_at =
        (now + chrono::Duration::seconds(state.config.market_share_sticky_ttl_secs)).to_rfc3339();
    let _ = state
        .db()
        .execute(
            r#"
            INSERT INTO market_response_sticky_routes
              (response_id, sticky_key, api_key_id, user_id, app_type, model_id, protocol_family,
               router_id, share_id, expires_at, created_at, updated_at)
            VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?11)
            ON CONFLICT(response_id) DO UPDATE SET
              sticky_key=excluded.sticky_key,
              api_key_id=excluded.api_key_id,
              user_id=excluded.user_id,
              app_type=excluded.app_type,
              model_id=excluded.model_id,
              protocol_family=excluded.protocol_family,
              router_id=excluded.router_id,
              share_id=excluded.share_id,
              expires_at=excluded.expires_at,
              updated_at=excluded.updated_at
            "#,
            vec![
                crate::db::val(response_id),
                crate::db::val(sticky_key),
                crate::db::uuid_val(api_key_id),
                crate::db::uuid_val(user_id),
                crate::db::val(app_type),
                crate::db::uuid_val(model_id),
                crate::db::val(protocol_family),
                crate::db::val(&share.router_id),
                crate::db::val(&share.share_id),
                crate::db::val(expires_at),
                crate::db::val(now.to_rfc3339()),
            ],
        )
        .await;
}

#[cfg(test)]
fn request_session_key(headers: &HeaderMap, body_json: &serde_json::Value, body: &Bytes) -> String {
    request_session_key_info(headers, body_json, body).primary_key
}

fn request_session_key_info(
    headers: &HeaderMap,
    body_json: &serde_json::Value,
    body: &Bytes,
) -> RequestSessionKey {
    if let Some(value) = header_session_value(headers) {
        return RequestSessionKey {
            primary_key: hash_session_key("header", &value),
            fallback_key: None,
            previous_response_id: None,
        };
    }
    for path in [
        &["session_id"][..],
        &["sessionId"][..],
        &["conversation_id"][..],
        &["conversationId"][..],
        &["thread_id"][..],
        &["threadId"][..],
        &["prompt_cache_key"][..],
        &["promptCacheKey"][..],
        &["metadata", "session_id"][..],
        &["metadata", "sessionId"][..],
        &["metadata", "conversation_id"][..],
        &["metadata", "conversationId"][..],
        &["metadata", "user_id"][..],
        &["metadata", "userId"][..],
    ] {
        if let Some(value) = json_string_path(body_json, path) {
            return RequestSessionKey {
                primary_key: hash_session_key(&path.join("."), &value),
                fallback_key: None,
                previous_response_id: None,
            };
        }
    }
    let response_id = previous_response_id(body_json);
    if let Some(value) = json_string_path(body_json, &["user"]) {
        return RequestSessionKey {
            primary_key: hash_session_key("user", &value),
            fallback_key: None,
            previous_response_id: response_id,
        };
    }
    if let Some((primary, fallback)) = message_history_session_hint(body_json) {
        return RequestSessionKey {
            primary_key: hash_session_key("messages", &primary),
            fallback_key: fallback.map(|value| hash_session_key("messages", &value)),
            previous_response_id: response_id,
        };
    }
    if let Some((primary, fallback)) = responses_input_history_session_hint(body_json) {
        return RequestSessionKey {
            primary_key: hash_session_key("input_history", &primary),
            fallback_key: fallback.map(|value| hash_session_key("input_history", &value)),
            previous_response_id: response_id,
        };
    }
    if let Some(value) = response_id {
        return RequestSessionKey {
            primary_key: hash_session_key("previous_response_id", &value),
            fallback_key: None,
            previous_response_id: Some(value),
        };
    }
    RequestSessionKey {
        primary_key: format!("sha256:{}", hex::encode(Sha256::digest(body))),
        fallback_key: None,
        previous_response_id: None,
    }
}

fn header_session_value(headers: &HeaderMap) -> Option<String> {
    [
        "x-cc-session-id",
        "x-session-id",
        "session-id",
        "session_id",
        "x-conversation-id",
        "x-thread-id",
        "x-amp-thread-id",
        "x-client-request-id",
        "openai-conversation-id",
    ]
    .iter()
    .find_map(|name| {
        headers
            .get(*name)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
    })
}

fn previous_response_id(body_json: &serde_json::Value) -> Option<String> {
    json_string_path(body_json, &["previous_response_id"])
        .or_else(|| json_string_path(body_json, &["previousResponseId"]))
        .or_else(|| json_string_path(body_json, &["metadata", "previous_response_id"]))
        .or_else(|| json_string_path(body_json, &["metadata", "previousResponseId"]))
}

fn json_string_path(value: &serde_json::Value, path: &[&str]) -> Option<String> {
    let mut current = value;
    for segment in path {
        current = current.get(*segment)?;
    }
    current
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn message_history_session_hint(body_json: &serde_json::Value) -> Option<(String, Option<String>)> {
    let messages = body_json.get("messages")?.as_array()?;
    if messages.is_empty() {
        return None;
    }
    let mut system_prompt = String::new();
    let mut first_user = String::new();
    let mut first_assistant = String::new();

    for message in messages {
        let role = message
            .get("role")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .unwrap_or_default()
            .to_ascii_lowercase();
        let Some(content) = responses_input_content_text(
            message.get("content").unwrap_or(&serde_json::Value::Null),
        ) else {
            continue;
        };
        match role.as_str() {
            "developer" | "system" if system_prompt.is_empty() => system_prompt = content,
            "user" if first_user.is_empty() => first_user = content,
            "assistant" if first_assistant.is_empty() => first_assistant = content,
            _ => {}
        }
        if !system_prompt.is_empty() && !first_user.is_empty() && !first_assistant.is_empty() {
            break;
        }
    }
    session_hint_from_parts(&system_prompt, &first_user, &first_assistant)
}

fn responses_input_history_session_hint(
    body_json: &serde_json::Value,
) -> Option<(String, Option<String>)> {
    let mut system_prompt = body_json
        .get("instructions")
        .and_then(|value| value.as_str())
        .map(trim_session_text)
        .filter(|value| !value.is_empty())
        .unwrap_or_default();
    let input_value = body_json.get("input")?;
    if let Some(input) = input_value.as_str() {
        return session_hint_from_parts(&system_prompt, input, "");
    }
    let input = input_value.as_array()?;
    let mut first_user = String::new();
    let mut first_assistant = String::new();

    for item in input {
        let Some((role, content)) = responses_input_message_hint(item) else {
            continue;
        };
        match role.as_str() {
            "developer" | "system" if system_prompt.is_empty() => system_prompt = content,
            "user" if first_user.is_empty() => first_user = content,
            "assistant" if first_assistant.is_empty() => first_assistant = content,
            _ => {}
        }
        if !system_prompt.is_empty() && !first_user.is_empty() && !first_assistant.is_empty() {
            break;
        }
    }

    session_hint_from_parts(&system_prompt, &first_user, &first_assistant)
}

fn session_hint_from_parts(
    system_prompt: &str,
    first_user: &str,
    first_assistant: &str,
) -> Option<(String, Option<String>)> {
    if system_prompt.trim().is_empty() && first_user.trim().is_empty() {
        return None;
    }
    let short = format!(
        "sys:{}\nusr:{}",
        trim_session_text(system_prompt),
        trim_session_text(first_user)
    );
    if first_assistant.trim().is_empty() {
        return Some((short, None));
    }
    let full = format!("{short}\nast:{}", trim_session_text(first_assistant));
    Some((full, Some(short)))
}

fn responses_input_message_hint(item: &serde_json::Value) -> Option<(String, String)> {
    let item_type = item
        .get("type")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .unwrap_or_default();
    if item_type == "reasoning" || (!item_type.is_empty() && item_type != "message") {
        return None;
    }
    let role = item
        .get("role")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("unknown")
        .to_ascii_lowercase();
    if role == "unknown" && item_type.is_empty() {
        return None;
    }
    let content = responses_input_content_text(item.get("content").unwrap_or(item))?;
    Some((role, content))
}

fn trim_session_text(value: &str) -> String {
    value.trim().chars().take(4096).collect()
}

fn responses_input_content_text(content: &serde_json::Value) -> Option<String> {
    match content {
        serde_json::Value::String(value) => {
            let value = value.trim();
            if value.is_empty() {
                None
            } else {
                Some(trim_session_text(value))
            }
        }
        serde_json::Value::Array(items) => {
            let text = items
                .iter()
                .filter_map(responses_input_content_text)
                .collect::<Vec<_>>()
                .join("\n");
            (!text.trim().is_empty()).then_some(trim_session_text(&text))
        }
        serde_json::Value::Object(object) => {
            for key in ["text", "input_text", "output_text"] {
                if let Some(value) = object.get(key).and_then(|value| value.as_str()) {
                    let value = value.trim();
                    if !value.is_empty() {
                        return Some(trim_session_text(value));
                    }
                }
            }
            object.get("content").and_then(responses_input_content_text)
        }
        _ => None,
    }
}

fn hash_session_key(source: &str, value: &str) -> String {
    let raw = format!("{source}:{}", value.trim());
    format!("sha256:{}", hex::encode(Sha256::digest(raw.as_bytes())))
}

fn sticky_route_key(
    user_id: Uuid,
    api_key_id: Uuid,
    app_type: &str,
    _model_id: Uuid,
    protocol_family: &str,
    session_key: &str,
) -> String {
    let raw = format!("{user_id}:{api_key_id}:{app_type}:{protocol_family}:{session_key}");
    format!("sha256:{}", hex::encode(Sha256::digest(raw.as_bytes())))
}

fn protocol_family(app_type: &str, upstream_path: &str) -> &'static str {
    if app_type == "gemini" {
        "gemini"
    } else if app_type == "anthropic" || app_type == "claude" {
        "anthropic"
    } else if upstream_path.contains("/responses") {
        "responses"
    } else {
        "openai-chat"
    }
}

fn share_app_type_alias(app_type: &str) -> &str {
    match app_type {
        "openai" => "codex",
        "anthropic" => "claude",
        other => other,
    }
}

struct ShareSupportFlags {
    claude: bool,
    codex: bool,
    gemini: bool,
}

fn share_support_flags(app_type: &str) -> ShareSupportFlags {
    ShareSupportFlags {
        claude: matches!(app_type, "anthropic" | "claude" | "cursor"),
        codex: matches!(app_type, "openai" | "codex" | "cursor"),
        gemini: app_type == "gemini",
    }
}

fn share_capability(app_type: &str) -> &'static str {
    match app_type {
        "anthropic" | "claude" => "claude",
        "gemini" => "gemini",
        _ => "codex",
    }
}

fn share_uses_responses_for_openai_chat(
    share: &SelectedShare,
    app_type: &str,
    upstream_path: &str,
) -> bool {
    app_type == "openai"
        && upstream_path == "/v1/chat/completions"
        && (share.app_type == "codex"
            || (share.app_type == "proxy" && share.enabled_codex)
            || (share.app_type != "openai" && share.enabled_codex))
}

fn chat_completions_body_to_responses(mut value: serde_json::Value) -> serde_json::Value {
    if value.get("input").is_none() {
        if let Some(messages) = value.get("messages").cloned() {
            if let Some(object) = value.as_object_mut() {
                object.insert("input".to_string(), messages);
            }
        }
    }
    if let Some(object) = value.as_object_mut() {
        object.remove("messages");
        if let Some(max_tokens) = object.remove("max_tokens") {
            object.entry("max_output_tokens").or_insert(max_tokens);
        }
        if let Some(max_completion_tokens) = object.remove("max_completion_tokens") {
            object
                .entry("max_output_tokens")
                .or_insert(max_completion_tokens);
        }
        for field in CHAT_COMPLETIONS_ONLY_FIELDS {
            object.remove(*field);
        }
    }
    value
}

const CHAT_COMPLETIONS_ONLY_FIELDS: &[&str] = &[
    "frequency_penalty",
    "presence_penalty",
    "logit_bias",
    "logprobs",
    "top_logprobs",
    "n",
    "stop",
    "response_format",
    "seed",
    "stream_options",
    "user",
];

fn charge_json(row: crate::db::DbRow) -> serde_json::Value {
    let usage_json = row
        .opt_string("usage_json")
        .and_then(|v| serde_json::from_str::<serde_json::Value>(&v).ok());
    let usage_amount = row.opt_string("usage_amount");
    let reserved_amount = row.opt_string("reserved_amount");
    let gross_amount = usage_amount
        .clone()
        .or_else(|| reserved_amount.clone())
        .unwrap_or_else(|| "0".to_string());
    serde_json::json!({
        "id": row.string("id"),
        "event_id": row.string("id"),
        "event_type": "usage_charge",
        "request_id": row.string("request_id"),
        "api_key_name": row.opt_string("api_key_name"),
        "api_key_prefix": row.opt_string("api_key_prefix"),
        "app_type": row.string("app_type"),
        "model": row.string("model"),
        "request_agent": row.opt_string("request_agent").unwrap_or_else(|| share_capability(&row.string("app_type")).to_string()),
        "requested_model": row.opt_string("requested_model").unwrap_or_else(|| row.string("model")),
        "actual_model": row.opt_string("actual_model").or_else(|| row.opt_string("pricing_model")).unwrap_or_else(|| row.string("model")),
        "actual_model_source": row.opt_string("actual_model_source").or_else(|| row.opt_string("pricing_model_source")).unwrap_or_else(|| "official".to_string()),
        "router_id": row.string("router_id"),
        "share_id": row.string("share_id"),
        "share_subdomain": share_subdomain(row.opt_string("share_raw_json").as_deref()),
        "owner_email": row.string("owner_email"),
        "routing_rule_id": row.opt_string("routing_rule_id"),
        "status": row.string("status"),
        "reserved_amount": reserved_amount,
        "usage_amount": usage_amount,
        "gross_amount": gross_amount,
        "fee_amount": "0",
        "net_amount": gross_amount,
        "currency": "USD",
        "input_tokens": usage_number(&usage_json, "input_tokens"),
        "output_tokens": usage_number(&usage_json, "output_tokens"),
        "cache_read_tokens": usage_number(&usage_json, "cache_read_tokens"),
        "cache_write_tokens": usage_number(&usage_json, "cache_write_tokens"),
        "price_snapshot": row.opt_string("price_snapshot").and_then(|v| serde_json::from_str::<serde_json::Value>(&v).ok()),
        "usage_json": usage_json,
        "audit_flags": row.opt_string("audit_flags").and_then(|v| serde_json::from_str::<serde_json::Value>(&v).ok()),
        "request_object_key": row.opt_string("request_object_key"),
        "request_object_sha256": row.opt_string("request_object_sha256"),
        "response_meta_object_key": row.opt_string("response_meta_object_key"),
        "response_meta_object_sha256": row.opt_string("response_meta_object_sha256"),
        "created_at": row.opt_string("created_at"),
        "settled_at": row.opt_string("settled_at"),
    })
}

fn usage_number(usage: &Option<serde_json::Value>, key: &str) -> u64 {
    usage
        .as_ref()
        .and_then(|value| value.get(key))
        .and_then(|value| value.as_u64())
        .unwrap_or(0)
}

pub(crate) fn share_subdomain(raw_json: Option<&str>) -> Option<String> {
    let value = raw_json.and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())?;
    if let Some(subdomain) = value
        .get("subdomain")
        .and_then(|item| item.as_str())
        .map(str::trim)
        .filter(|item| !item.is_empty())
    {
        return Some(subdomain.to_string());
    }
    share_url_from_json(&value).and_then(|url| subdomain_from_url(url))
}

fn share_url_from_json(value: &serde_json::Value) -> Option<&str> {
    for key in [
        "apiUrl",
        "api_url",
        "apiURL",
        "apiBaseUrl",
        "api_base_url",
        "baseUrl",
        "base_url",
        "url",
    ] {
        if let Some(url) = value.get(key).and_then(|item| item.as_str()) {
            return Some(url);
        }
    }
    None
}

fn subdomain_from_url(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    let with_scheme = if trimmed.contains("://") {
        trimmed.to_string()
    } else {
        format!("https://{trimmed}")
    };
    let parsed = url::Url::parse(&with_scheme).ok()?;
    let host = parsed.host_str()?.trim();
    host.split('.')
        .next()
        .filter(|part| !part.is_empty())
        .map(ToOwned::to_owned)
}

#[cfg(test)]
mod tests {
    use super::{
        SelectedShare, api_key_allows_model_access, chat_completions_body_to_responses,
        classify_upstream_failure, commission_amount, commission_split, inject_openai_stream_usage,
        is_allowed_router_market_proxy_header, is_retryable_failure, looks_like_sse,
        parse_gemini_model_action, parse_non_stream_sse_fallback, quota_exhausted_ttl_secs,
        raw_share_app_official_runtime, raw_share_app_quota_blocked,
        raw_share_app_token_sale_visible, rendezvous_share_score, request_session_key,
        share_subdomain, share_weight, sticky_route_key,
    };
    use crate::{auth::ApiKeyPrincipal, failure::FailureKind, pricing, usage::UsageProtocol};
    use axum::{body::Bytes, http::HeaderMap};
    use rust_decimal::Decimal;
    use serde_json::json;
    use uuid::Uuid;

    #[test]
    fn inject_openai_stream_usage_sets_include_usage() {
        let mut body = json!({"model":"gpt-4o","stream":false});
        inject_openai_stream_usage(&mut body);
        assert_eq!(body["stream"], true);
        assert_eq!(body["stream_options"]["include_usage"], true);
    }

    #[test]
    fn router_market_proxy_header_allowlist_keeps_protocol_headers() {
        for name in [
            "anthropic-version",
            "anthropic-beta",
            "anthropic-dangerous-direct-browser-access",
            "user-agent",
            "accept",
            "content-type",
            "cache-control",
            "pragma",
            "x-request-id",
            "x-stainless-lang",
            "anthropic-client-sha",
        ] {
            assert!(is_allowed_router_market_proxy_header(name), "{name}");
        }
    }

    #[test]
    fn router_market_proxy_header_allowlist_blocks_credentials_and_hop_headers() {
        for name in [
            "authorization",
            "x-api-key",
            "api-key",
            "x-share-token",
            "cookie",
            "set-cookie",
            "host",
            "connection",
            "transfer-encoding",
            "upgrade",
            "proxy-authorization",
            "proxy-anything",
            "x-cc-switch-market-request-id",
            "x-cc-switch-request-id",
        ] {
            assert!(!is_allowed_router_market_proxy_header(name), "{name}");
        }
    }

    #[test]
    fn upstream_unsupported_parameter_is_bad_request_not_model_unsupported() {
        let message = r#"router market proxy returned 400 Bad Request: {"error":{"code":"cc_switch_upstream_error","message":"OpenAI upstream returned 400 for {\"model\":\"gpt-5.5\"}: Unsupported parameter: frequency_penalty","upstream_status":400}}"#;
        assert_eq!(classify_upstream_failure(message), FailureKind::BadRequest);
    }

    #[test]
    fn upstream_model_unsupported_uses_phrase_match() {
        assert_eq!(
            classify_upstream_failure("OpenAI upstream returned 400: model is not supported"),
            FailureKind::ModelUnsupported
        );
        assert_eq!(
            classify_upstream_failure("OpenAI upstream returned 404: unsupported model gpt-old"),
            FailureKind::ModelUnsupported
        );
    }

    #[test]
    fn upstream_plain_400_is_bad_request() {
        assert_eq!(
            classify_upstream_failure(
                "router market proxy returned 400 Bad Request: invalid request body"
            ),
            FailureKind::BadRequest
        );
    }

    #[test]
    fn upstream_quota_exhausted_is_retryable_long_feedback() {
        assert_eq!(
            classify_upstream_failure("OpenAI upstream returned 429: weekly limit exceeded"),
            FailureKind::QuotaExhausted
        );
        assert!(is_retryable_failure(FailureKind::QuotaExhausted));
        assert_eq!(
            quota_exhausted_ttl_secs("monthly quota exhausted"),
            31 * 24 * 60 * 60
        );
        assert_eq!(quota_exhausted_ttl_secs("5h quota exceeded"), 5 * 60 * 60);
    }

    #[test]
    fn chat_completions_to_responses_strips_chat_only_fields() {
        let input = json!({
            "model": "gpt-5.5",
            "messages": [{"role": "user", "content": "hello"}],
            "max_tokens": 100,
            "temperature": 1,
            "top_p": 1,
            "frequency_penalty": 0,
            "presence_penalty": 0,
            "logit_bias": {"42": -100},
            "n": 2,
            "stop": ["END"],
            "response_format": {"type": "json_object"},
            "seed": 123,
            "stream_options": {"include_usage": true},
            "user": "user-1"
        });

        let output = chat_completions_body_to_responses(input);

        assert_eq!(
            output["input"],
            json!([{"role": "user", "content": "hello"}])
        );
        assert_eq!(output["max_output_tokens"], json!(100));
        assert!(output.get("messages").is_none());
        assert!(output.get("max_tokens").is_none());
        for field in [
            "frequency_penalty",
            "presence_penalty",
            "logit_bias",
            "n",
            "stop",
            "response_format",
            "seed",
            "stream_options",
            "user",
        ] {
            assert!(output.get(field).is_none(), "{field}");
        }
    }

    #[test]
    fn parses_gemini_model_action_without_partial_route_capture() {
        assert_eq!(
            parse_gemini_model_action("gemini-2.5-flash:generateContent"),
            Some(("gemini-2.5-flash", "generateContent"))
        );
        assert_eq!(
            parse_gemini_model_action(
                "publishers/google/models/gemini-2.5-flash:streamGenerateContent"
            ),
            Some((
                "publishers/google/models/gemini-2.5-flash",
                "streamGenerateContent"
            ))
        );
    }

    #[test]
    fn commission_amount_uses_basis_points() {
        assert_eq!(
            commission_amount(Decimal::new(12345, 2), 1000),
            Decimal::new(12345, 3)
        );
        assert_eq!(commission_amount(Decimal::new(12345, 2), 0), Decimal::ZERO);
        assert_eq!(
            commission_amount(Decimal::new(12345, 2), 10_000),
            Decimal::new(12345, 2)
        );
    }

    #[test]
    fn commission_split_never_exceeds_total_commission() {
        assert_eq!(
            commission_split(Decimal::new(1, 0), 1000, 500),
            (Decimal::new(1, 1), Decimal::new(5, 2))
        );
        assert_eq!(
            commission_split(Decimal::new(1, 8), 5000, 5000),
            (Decimal::ZERO, Decimal::new(1, 8))
        );
    }

    #[test]
    fn share_subdomain_prefers_explicit_router_subdomain() {
        assert_eq!(
            share_subdomain(Some(
                r#"{"subdomain":"bbb","apiUrl":"https://other.example.com"}"#
            ))
            .as_deref(),
            Some("bbb")
        );
        assert_eq!(
            share_subdomain(Some(
                r#"{"apiUrl":"https://bbb.jptokenswitch.cc/v1/responses"}"#
            ))
            .as_deref(),
            Some("bbb")
        );
        assert_eq!(share_subdomain(Some(r#"{"shareId":"share-1"}"#)), None);
    }

    #[test]
    fn raw_share_app_quota_blocked_detects_router_unavailable_state() {
        let raw = json!({
            "appAvailability": {
                "codex": {
                    "status": "unavailable",
                    "reason": "weekly quota exhausted"
                }
            }
        })
        .to_string();

        assert!(raw_share_app_quota_blocked(Some(&raw), "codex"));
        assert!(!raw_share_app_quota_blocked(Some(&raw), "claude"));
    }

    #[test]
    fn raw_share_app_token_sale_visible_uses_market_apps() {
        let raw = json!({
            "marketApps": {
                "codex": {
                    "supported": true,
                    "visible": true,
                    "forSale": "Yes",
                    "saleMarketKind": "token",
                    "marketAccessMode": "all"
                },
                "claude": {
                    "supported": true,
                    "visible": true,
                    "forSale": "Yes",
                    "saleMarketKind": "share",
                    "marketAccessMode": "selected"
                }
            }
        })
        .to_string();

        assert!(raw_share_app_token_sale_visible(
            Some(&raw),
            "codex",
            "Yes",
            None
        ));
        assert!(!raw_share_app_token_sale_visible(
            Some(&raw),
            "claude",
            "Yes",
            None
        ));
        assert!(!raw_share_app_token_sale_visible(
            Some(&raw),
            "gemini",
            "Yes",
            None
        ));
    }

    #[test]
    fn raw_share_app_token_sale_visible_checks_app_settings_market_acl() {
        let raw = json!({
            "appSettings": {
                "codex": {
                    "forSale": "Yes",
                    "saleMarketKind": "token",
                    "marketAccessMode": "selected",
                    "sharedWithEmails": ["market@example.com"]
                },
                "claude": {
                    "forSale": "Yes",
                    "saleMarketKind": "token",
                    "marketAccessMode": "all"
                }
            }
        })
        .to_string();

        assert!(raw_share_app_token_sale_visible(
            Some(&raw),
            "codex",
            "No",
            Some("market@example.com")
        ));
        assert!(!raw_share_app_token_sale_visible(
            Some(&raw),
            "codex",
            "No",
            Some("other@example.com")
        ));
        assert!(raw_share_app_token_sale_visible(
            Some(&raw),
            "claude",
            "No",
            None
        ));
    }

    #[test]
    fn raw_share_app_official_runtime_detects_runtime_kind() {
        let raw = json!({
            "appRuntimes": {
                "codex": {
                    "kind": "official_oauth",
                    "app": "codex"
                },
                "claude": {
                    "kind": "proxy",
                    "app": "claude",
                    "models": [{"slot": "sonnet", "actualModel": "claude-sonnet-4"}]
                }
            }
        })
        .to_string();

        assert!(raw_share_app_official_runtime(Some(&raw), "codex"));
        assert!(!raw_share_app_official_runtime(Some(&raw), "claude"));
        assert!(!raw_share_app_official_runtime(Some(&raw), "gemini"));
    }

    #[test]
    fn request_session_key_prefers_explicit_header() {
        let mut headers = HeaderMap::new();
        headers.insert("x-session-id", "session-a".parse().unwrap());
        let body = Bytes::from_static(br#"{"metadata":{"session_id":"session-b"}}"#);
        let body_json = json!({"metadata": {"session_id": "session-b"}});

        assert_eq!(
            request_session_key(&headers, &body_json, &body),
            request_session_key(
                &headers,
                &json!({"user": "other"}),
                &Bytes::from_static(b"{}")
            )
        );
    }

    #[test]
    fn request_session_key_uses_metadata_and_request_fallback() {
        let headers = HeaderMap::new();
        let session_a = request_session_key(
            &headers,
            &json!({"metadata": {"session_id": "stable-session"}}),
            &Bytes::from_static(br#"{"metadata":{"session_id":"stable-session"},"n":1}"#),
        );
        let session_b = request_session_key(
            &headers,
            &json!({"metadata": {"session_id": "stable-session"}}),
            &Bytes::from_static(br#"{"metadata":{"session_id":"stable-session"},"n":2}"#),
        );
        let fallback_a = request_session_key(
            &headers,
            &json!({"input": "one"}),
            &Bytes::from_static(br#"{"input":"one"}"#),
        );
        let fallback_b = request_session_key(
            &headers,
            &json!({"input": "two"}),
            &Bytes::from_static(br#"{"input":"two"}"#),
        );

        assert_eq!(session_a, session_b);
        assert_ne!(fallback_a, fallback_b);
    }

    #[test]
    fn request_session_key_tracks_previous_response_id_without_overriding_content() {
        let headers = HeaderMap::new();
        let session_a = request_session_key_info(
            &headers,
            &json!({"previous_response_id": "resp_abc", "input": "one"}),
            &Bytes::from_static(br#"{"previous_response_id":"resp_abc","input":"one"}"#),
        );
        let session_b = request_session_key_info(
            &headers,
            &json!({"previous_response_id": "resp_abc", "input": "two"}),
            &Bytes::from_static(br#"{"previous_response_id":"resp_abc","input":"two"}"#),
        );
        let session_c = request_session_key_info(
            &headers,
            &json!({"previous_response_id": "resp_other", "input": "two"}),
            &Bytes::from_static(br#"{"previous_response_id":"resp_other","input":"two"}"#),
        );

        assert_eq!(session_a.previous_response_id.as_deref(), Some("resp_abc"));
        assert_eq!(session_b.previous_response_id.as_deref(), Some("resp_abc"));
        assert_eq!(
            session_c.previous_response_id.as_deref(),
            Some("resp_other")
        );
        assert_ne!(session_a.primary_key, session_b.primary_key);
        assert_eq!(session_b.primary_key, session_c.primary_key);
    }

    #[test]
    fn request_session_key_accepts_codex_session_headers() {
        let mut codex_headers = HeaderMap::new();
        codex_headers.insert("session_id", "codex-session".parse().unwrap());
        let mut generic_headers = HeaderMap::new();
        generic_headers.insert("x-session-id", "codex-session".parse().unwrap());
        let body_json = json!({"input": "ignored"});
        let body = Bytes::from_static(br#"{"input":"ignored"}"#);

        assert_eq!(
            request_session_key(&codex_headers, &body_json, &body),
            request_session_key(&generic_headers, &body_json, &body)
        );
    }

    #[test]
    fn request_session_key_uses_prompt_cache_key_before_content() {
        let headers = HeaderMap::new();
        let session_a = request_session_key(
            &headers,
            &json!({"prompt_cache_key": "stable-cache", "input": "first"}),
            &Bytes::from_static(br#"{"prompt_cache_key":"stable-cache","input":"first"}"#),
        );
        let session_b = request_session_key(
            &headers,
            &json!({"prompt_cache_key": "stable-cache", "input": "second"}),
            &Bytes::from_static(br#"{"prompt_cache_key":"stable-cache","input":"second"}"#),
        );

        assert_eq!(session_a, session_b);
    }

    #[test]
    fn request_session_key_uses_responses_input_history_hint() {
        let headers = HeaderMap::new();
        let session_a = request_session_key(
            &headers,
            &json!({
                "model": "gpt-5.5",
                "input": [
                    {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "first question"}]},
                    {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "first answer"}]},
                    {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "second question"}]},
                    {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "current turn A"}]}
                ]
            }),
            &Bytes::from_static(br#"{"input":"different-a"}"#),
        );
        let session_b = request_session_key(
            &headers,
            &json!({
                "model": "gpt-5.5",
                "input": [
                    {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "first question"}]},
                    {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "first answer"}]},
                    {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "second question"}]},
                    {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "current turn B"}]}
                ]
            }),
            &Bytes::from_static(br#"{"input":"different-b"}"#),
        );
        let session_c = request_session_key(
            &headers,
            &json!({
                "model": "gpt-5.5",
                "input": [
                    {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "another session"}]},
                    {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "second question"}]}
                ]
            }),
            &Bytes::from_static(br#"{"input":"different-c"}"#),
        );

        assert_eq!(session_a, session_b);
        assert_ne!(session_a, session_c);
    }

    #[test]
    fn request_session_key_uses_responses_fallback_to_inherit_first_turn() {
        let headers = HeaderMap::new();
        let first_turn = request_session_key_info(
            &headers,
            &json!({
                "instructions": "be concise",
                "input": [
                    {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "same opening"}]}
                ]
            }),
            &Bytes::from_static(br#"{"input":"first"}"#),
        );
        let followup_turn = request_session_key_info(
            &headers,
            &json!({
                "instructions": "be concise",
                "input": [
                    {"type": "reasoning", "summary": []},
                    {"type": "function_call", "name": "ignored"},
                    {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "same opening"}]},
                    {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "first assistant response"}]},
                    {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "next question"}]}
                ]
            }),
            &Bytes::from_static(br#"{"input":"followup"}"#),
        );

        assert_ne!(first_turn.primary_key, followup_turn.primary_key);
        assert_eq!(
            followup_turn.fallback_key.as_deref(),
            Some(first_turn.primary_key.as_str())
        );
    }

    #[test]
    fn sticky_route_key_includes_session_key() {
        let user_id = Uuid::nil();
        let api_key_id = Uuid::nil();
        let model_id = Uuid::nil();
        let key_a = sticky_route_key(
            user_id,
            api_key_id,
            "codex",
            model_id,
            "responses",
            "session-a",
        );
        let key_b = sticky_route_key(
            user_id,
            api_key_id,
            "codex",
            model_id,
            "responses",
            "session-b",
        );

        assert_ne!(key_a, key_b);
    }

    #[test]
    fn sticky_route_key_is_model_independent() {
        let user_id = Uuid::nil();
        let api_key_id = Uuid::nil();
        let key_a = sticky_route_key(
            user_id,
            api_key_id,
            "codex",
            Uuid::nil(),
            "responses",
            "session-a",
        );
        let key_b = sticky_route_key(
            user_id,
            api_key_id,
            "codex",
            Uuid::new_v4(),
            "responses",
            "session-a",
        );

        assert_eq!(key_a, key_b);
    }

    #[test]
    fn raw_share_app_quota_blocked_detects_model_health_quota_block() {
        let raw = json!({
            "modelHealth": {
                "codex": [{
                    "status": "failed",
                    "requestedModel": "codex",
                    "actualModel": "codex",
                    "errorMessage": "weekly quota exhausted until 2026-06-16T06:37:16+00:00"
                }]
            }
        })
        .to_string();

        assert!(raw_share_app_quota_blocked(Some(&raw), "codex"));
    }

    #[test]
    fn rendezvous_share_score_is_stable_and_weighted() {
        let share_a = SelectedShare {
            router_id: "router".to_string(),
            share_id: "a".to_string(),
            owner_email: "a@example.com".to_string(),
            app_type: "codex".to_string(),
            enabled_codex: true,
            active_requests: 0,
            parallel_limit: 3,
            priority: 0,
            online_rate_24h: 1.0,
            routing_rule_id: None,
            pricing_model: "gpt-5.4".to_string(),
            pricing_slot: "model".to_string(),
            pricing_model_source: "test".to_string(),
            share_official: false,
            price: pricing::PriceItem {
                id: Uuid::nil(),
                model_id: Some(Uuid::nil()),
                app_type: "openai".to_string(),
                model_pattern: "gpt-5.4".to_string(),
                display_name: None,
                is_public: Some(true),
                input_per_million: Decimal::ZERO,
                output_per_million: Decimal::ZERO,
                cache_read_per_million: Some(Decimal::ZERO),
                cache_write_per_million: Some(Decimal::ZERO),
                official_input_per_million: None,
                official_output_per_million: None,
                official_cache_read_per_million: None,
                official_cache_write_per_million: None,
                discount_percent: Decimal::TEN,
                currency: "USD".to_string(),
                status: "active".to_string(),
            },
        };
        let busy_share = SelectedShare {
            active_requests: 9,
            parallel_limit: 10,
            ..share_a.clone()
        };
        assert_eq!(
            rendezvous_share_score("user-1:model", &share_a),
            rendezvous_share_score("user-1:model", &share_a)
        );
        assert!(share_weight(&share_a) > share_weight(&busy_share));
    }

    #[test]
    fn api_key_vendor_scope_defaults_by_agent() {
        let api = ApiKeyPrincipal {
            user_id: Uuid::nil(),
            user_email: "user@example.com".into(),
            api_key_id: Uuid::nil(),
            is_admin: false,
            monthly_spend_cap: None,
            scope_json: None,
        };

        assert!(api_key_allows_model_access(
            &api,
            "claude",
            "sonnet",
            "claude-sonnet-4-6",
            "anthropic",
            Some(Uuid::nil()),
        ));
        assert!(api_key_allows_model_access(
            &api,
            "claude",
            "sonnet",
            "gpt-5.5",
            "openai",
            Some(Uuid::nil()),
        ));
        assert!(api_key_allows_model_access(
            &api,
            "codex",
            "model",
            "gpt-5.5",
            "openai",
            Some(Uuid::nil()),
        ));
    }

    #[test]
    fn api_key_vendor_scope_allows_agent_specific_vendor() {
        let api = ApiKeyPrincipal {
            user_id: Uuid::nil(),
            user_email: "user@example.com".into(),
            api_key_id: Uuid::nil(),
            is_admin: false,
            monthly_spend_cap: None,
            scope_json: Some(json!({
                "agent_model_vendors": {
                    "claude": ["anthropic", "openai"],
                    "codex": ["openai"],
                    "gemini": ["gemini"]
                }
            })),
        };

        assert!(api_key_allows_model_access(
            &api,
            "claude",
            "sonnet",
            "gpt-5.5",
            "openai",
            Some(Uuid::nil()),
        ));
        assert!(!api_key_allows_model_access(
            &api,
            "codex",
            "model",
            "claude-sonnet-4-6",
            "anthropic",
            Some(Uuid::nil()),
        ));
    }

    #[test]
    fn api_key_vendor_scope_wildcard_allows_all_vendors() {
        let api = ApiKeyPrincipal {
            user_id: Uuid::nil(),
            user_email: "user@example.com".into(),
            api_key_id: Uuid::nil(),
            is_admin: false,
            monthly_spend_cap: None,
            scope_json: Some(json!({
                "agent_model_vendors": {
                    "claude": ["*"],
                    "codex": ["*"],
                    "gemini": ["*"]
                }
            })),
        };

        // Wildcard allows standard vendors.
        assert!(api_key_allows_model_access(
            &api,
            "codex",
            "model",
            "gpt-5.5",
            "openai",
            Some(Uuid::nil()),
        ));
        // Wildcard allows non-standard vendors like glm.
        assert!(api_key_allows_model_access(
            &api,
            "codex",
            "model",
            "glm-5.2",
            "glm",
            Some(Uuid::nil()),
        ));
    }

    #[test]
    fn api_key_vendor_scope_defaults_allow_all_vendors() {
        let api = ApiKeyPrincipal {
            user_id: Uuid::nil(),
            user_email: "user@example.com".into(),
            api_key_id: Uuid::nil(),
            is_admin: false,
            monthly_spend_cap: None,
            scope_json: None,
        };

        // Default (no scope) now allows all vendors including non-standard ones.
        assert!(api_key_allows_model_access(
            &api,
            "codex",
            "model",
            "glm-5.2",
            "glm",
            Some(Uuid::nil()),
        ));
    }

    #[test]
    fn parses_non_stream_responses_sse_fallback_usage_and_chat_json() {
        let text = r#"event: response.created
data: {"type":"response.created","response":{"id":"resp_1","model":"gpt-5.5","created_at":1778144036,"usage":null}}

event: response.output_text.delta
data: {"type":"response.output_text.delta","delta":"Hi"}

event: response.output_text.delta
data: {"type":"response.output_text.delta","delta":"!"}

event: response.completed
data: {"type":"response.completed","response":{"id":"resp_1","model":"gpt-5.5","created_at":1778144036,"usage":{"input_tokens":7,"output_tokens":13,"total_tokens":20}}}

"#;
        assert!(looks_like_sse(text));
        let fallback = parse_non_stream_sse_fallback(
            text,
            UsageProtocol::Codex,
            "gpt-5.5",
            "/v1/chat/completions",
        );
        let usage = fallback.usage.expect("usage");
        assert_eq!(usage.input_tokens, 7);
        assert_eq!(usage.output_tokens, 13);
        assert_eq!(fallback.response_json["object"], "chat.completion");
        assert_eq!(fallback.response_json["id"], "resp_1");
        assert_eq!(
            fallback.response_json["choices"][0]["message"]["content"],
            "Hi!"
        );
        assert_eq!(fallback.response_json["usage"]["prompt_tokens"], 7);
        assert!(
            fallback
                .audit_flags
                .as_array()
                .unwrap()
                .iter()
                .any(|flag| flag.as_str() == Some("non_stream_sse_fallback"))
        );
    }

    #[test]
    fn parses_non_stream_responses_sse_fallback_as_responses_json_with_function_call() {
        let text = r#"event: response.created
data: {"type":"response.created","response":{"id":"resp_tool","model":"gpt-5.5","created_at":1778144036,"status":"in_progress","usage":null}}

event: response.output_item.done
data: {"type":"response.output_item.done","output_index":0,"item":{"id":"fc_1","type":"function_call","status":"completed","call_id":"call_1","name":"act","arguments":"{\"action\":\"click\",\"x\":1}"}}

event: response.completed
data: {"type":"response.completed","response":{"id":"resp_tool","model":"gpt-5.5","created_at":1778144036,"status":"completed","usage":{"input_tokens":5,"output_tokens":6,"total_tokens":11}}}

"#;
        let fallback =
            parse_non_stream_sse_fallback(text, UsageProtocol::Codex, "gpt-5.5", "/v1/responses");

        assert_eq!(fallback.response_json["object"], "response");
        assert_eq!(fallback.response_json["id"], "resp_tool");
        assert_eq!(fallback.response_json["status"], "completed");
        assert_eq!(fallback.response_json["output"][0]["type"], "function_call");
        assert_eq!(fallback.response_json["output"][0]["name"], "act");
        assert_eq!(fallback.response_json["output"][0]["call_id"], "call_1");
        assert_eq!(
            fallback.response_json["output"][0]["arguments"],
            "{\"action\":\"click\",\"x\":1}"
        );
        assert_eq!(fallback.response_json["usage"]["input_tokens"], 5);
    }

    #[test]
    fn parses_non_stream_responses_sse_fallback_as_responses_text_output() {
        let text = r#"event: response.created
data: {"type":"response.created","response":{"id":"resp_text","model":"gpt-5.5","created_at":1778144036,"usage":null}}

event: response.output_text.delta
data: {"type":"response.output_text.delta","delta":"Hi"}

event: response.output_text.delta
data: {"type":"response.output_text.delta","delta":"!"}

event: response.completed
data: {"type":"response.completed","response":{"id":"resp_text","model":"gpt-5.5","created_at":1778144036,"status":"completed","usage":{"input_tokens":7,"output_tokens":2,"total_tokens":9}}}

"#;
        let fallback =
            parse_non_stream_sse_fallback(text, UsageProtocol::Codex, "gpt-5.5", "/v1/responses");

        assert_eq!(fallback.response_json["object"], "response");
        assert_eq!(fallback.response_json["output"][0]["type"], "message");
        assert_eq!(
            fallback.response_json["output"][0]["content"][0]["text"],
            "Hi!"
        );
    }

    #[test]
    fn parses_non_stream_anthropic_sse_fallback_with_tool_use() {
        let text = r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_1","type":"message","role":"assistant","model":"claude-sonnet-4-6","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":1}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_1","name":"act","input":{}}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"action\":\"click\""}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":",\"x\":1}"}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":6}}

event: message_stop
data: {"type":"message_stop"}

"#;
        let fallback = parse_non_stream_sse_fallback(
            text,
            UsageProtocol::Anthropic,
            "claude-sonnet-4-6",
            "/v1/messages",
        );

        assert_eq!(fallback.response_json["type"], "message");
        assert_eq!(fallback.response_json["stop_reason"], "tool_use");
        assert_eq!(fallback.response_json["content"][0]["type"], "tool_use");
        assert_eq!(fallback.response_json["content"][0]["name"], "act");
        assert_eq!(
            fallback.response_json["content"][0]["input"],
            json!({"action":"click","x":1})
        );
    }

    #[test]
    fn parses_non_stream_gemini_sse_fallback_with_function_call() {
        let text = r#"data: {"candidates":[{"content":{"parts":[{"text":"Working "}],"role":"model"}}],"usageMetadata":{"promptTokenCount":4,"candidatesTokenCount":2,"totalTokenCount":6}}

data: {"candidates":[{"content":{"parts":[{"functionCall":{"name":"act","args":{"action":"click","x":1}}}],"role":"model"},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":4,"candidatesTokenCount":5,"totalTokenCount":9}}

"#;
        let fallback = parse_non_stream_sse_fallback(
            text,
            UsageProtocol::Gemini,
            "gemini-2.5-flash",
            "/v1beta/models/gemini-2.5-flash:generateContent",
        );

        assert!(fallback.response_json.get("choices").is_none());
        assert_eq!(
            fallback.response_json["candidates"][0]["content"]["parts"][0]["text"],
            "Working "
        );
        assert_eq!(
            fallback.response_json["candidates"][0]["content"]["parts"][1]["functionCall"]["name"],
            "act"
        );
        assert_eq!(
            fallback.response_json["candidates"][0]["finishReason"],
            "STOP"
        );
        assert_eq!(
            fallback.response_json["usageMetadata"]["promptTokenCount"],
            4
        );
    }
}
