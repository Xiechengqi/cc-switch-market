use axum::{
    Json,
    extract::{Path, State},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{app_state::AppState, auth::Principal, error::ApiError};

#[derive(Deserialize)]
pub struct CreateApiKeyRequest {
    pub name: Option<String>,
    pub scope_json: Option<serde_json::Value>,
    pub expires_at: Option<String>,
    pub monthly_spend_cap: Option<rust_decimal::Decimal>,
}

#[derive(Serialize)]
pub struct CreateApiKeyResponse {
    pub id: Uuid,
    pub key: String,
    pub prefix: String,
}

#[derive(Serialize)]
pub struct ApiKeyItem {
    pub id: Uuid,
    pub name: String,
    pub prefix: String,
    pub scope_json: Option<serde_json::Value>,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
    pub monthly_spend_cap: Option<rust_decimal::Decimal>,
    pub last_used_at: Option<chrono::DateTime<chrono::Utc>>,
    pub last_used_ip_country: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub revoked_at: Option<chrono::DateTime<chrono::Utc>>,
    pub paused_at: Option<chrono::DateTime<chrono::Utc>>,
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Deserialize)]
pub struct RenameApiKeyRequest {
    pub name: String,
}

#[derive(Deserialize)]
pub struct UpdateApiKeyLimitsRequest {
    pub scope_json: Option<serde_json::Value>,
    pub expires_at: Option<String>,
    pub monthly_spend_cap: Option<rust_decimal::Decimal>,
}

#[derive(Deserialize)]
pub struct ApiKeyStatusRequest {
    pub action: String,
}

#[derive(Deserialize)]
pub struct DeleteApiKeyRequest {
    pub confirm: bool,
}

#[derive(Serialize)]
pub struct ApiKeySecretItem {
    pub api_key_id: Uuid,
    pub prefix: String,
    pub key: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Serialize)]
pub struct ApiKeySecretListResponse {
    pub items: Vec<ApiKeySecretItem>,
}

#[derive(Serialize)]
pub struct ApiKeySecretCreateResponse {
    pub item: ApiKeySecretItem,
}

#[derive(Serialize)]
pub struct ApiKeyStatusResponse {
    pub item: ApiKeyItem,
}

#[derive(Serialize)]
pub struct ApiKeyDeleteResponse {
    pub deleted: bool,
    pub id: Uuid,
}

#[derive(Deserialize)]
pub struct ApiKeyShareAllowlistUpdateRequest {
    pub shares: Vec<ApiKeyShareRef>,
}

#[derive(Deserialize, Serialize, Clone, PartialEq, Eq, Hash)]
pub struct ApiKeyShareRef {
    pub router_id: String,
    pub share_id: String,
}

#[derive(Serialize)]
pub struct ApiKeyShareAllowlistResponse {
    pub shares: Vec<ApiKeyShareRef>,
}

#[derive(Serialize)]
pub struct AvailableShareItem {
    pub router_id: String,
    pub share_id: String,
    pub subdomain: Option<String>,
    pub app_type: String,
    pub capabilities: Vec<String>,
    pub online: bool,
    pub for_sale: String,
    pub share_status: String,
}

#[derive(Serialize)]
pub struct AvailableSharesResponse {
    pub items: Vec<AvailableShareItem>,
}

#[derive(Serialize, Deserialize, Clone)]
struct PersistedSecretRow {
    api_key_id: Uuid,
    prefix: String,
    key: String,
    created_at: chrono::DateTime<chrono::Utc>,
}

const API_KEY_SECRETS_DIR: &str = "api-key-secrets";

fn secrets_path_for_user(user_id: Uuid) -> std::path::PathBuf {
    crate::config::config_dir()
        .expect("config dir")
        .join(API_KEY_SECRETS_DIR)
        .join(format!("{user_id}.json"))
}

fn read_persisted_secrets(user_id: Uuid) -> Vec<PersistedSecretRow> {
    let path = secrets_path_for_user(user_id);
    let Ok(raw) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

fn write_persisted_secrets(user_id: Uuid, items: &[PersistedSecretRow]) -> Result<(), ApiError> {
    let path = secrets_path_for_user(user_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(anyhow::Error::from)?;
    }
    let raw = serde_json::to_string_pretty(items).map_err(anyhow::Error::from)?;
    std::fs::write(path, raw).map_err(anyhow::Error::from)?;
    Ok(())
}

fn upsert_persisted_secret(user_id: Uuid, item: PersistedSecretRow) -> Result<(), ApiError> {
    let mut items = read_persisted_secrets(user_id);
    items.retain(|existing| existing.api_key_id != item.api_key_id);
    items.insert(0, item);
    write_persisted_secrets(user_id, &items)
}

fn remove_persisted_secret(user_id: Uuid, api_key_id: Uuid) -> Result<(), ApiError> {
    let mut items = read_persisted_secrets(user_id);
    items.retain(|existing| existing.api_key_id != api_key_id);
    write_persisted_secrets(user_id, &items)
}

fn to_secret_item(item: PersistedSecretRow) -> ApiKeySecretItem {
    ApiKeySecretItem {
        api_key_id: item.api_key_id,
        prefix: item.prefix,
        key: item.key,
        created_at: item.created_at,
    }
}

pub async fn create_api_key(
    State(state): State<AppState>,
    principal: Principal,
    Json(input): Json<CreateApiKeyRequest>,
) -> Result<Json<CreateApiKeyResponse>, ApiError> {
    crate::rate_limit::check("api_key_create", &principal.user_id.to_string(), 10)?;
    let expires_at = normalize_expires_at(input.expires_at)?;
    if input
        .monthly_spend_cap
        .is_some_and(|value| value < rust_decimal::Decimal::ZERO)
    {
        return Err(ApiError::bad_request(
            "invalid_monthly_spend_cap",
            "monthly spend cap must be zero or positive",
        ));
    }
    let key = generate_key();
    let prefix = key.chars().take(14).collect::<String>();
    let id = Uuid::new_v4();
    let now = crate::db::now_string();
    state
        .db()
        .execute(
            r#"
        INSERT INTO api_keys (id, user_id, name, key_hash, prefix, scope_json, expires_at, monthly_spend_cap, created_at, paused_at, deleted_at)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, NULL, NULL)
        "#,
            vec![
                crate::db::uuid_val(id),
                crate::db::uuid_val(principal.user_id),
                crate::db::val(input.name.unwrap_or_else(|| "Default key".to_string())),
                crate::db::val(hash_key(&key)),
                crate::db::val(&prefix),
                input
                    .scope_json
                    .map(crate::db::json_val)
                    .unwrap_or(libsql::Value::Null),
                crate::db::opt_val(expires_at),
                input
                    .monthly_spend_cap
                    .map(crate::db::dec_val)
                    .unwrap_or(libsql::Value::Null),
                crate::db::val(now.clone()),
            ],
        )
        .await?;

    let created_at = chrono::DateTime::parse_from_rfc3339(&now)
        .map(|value| value.with_timezone(&chrono::Utc))
        .unwrap_or_else(|_| chrono::Utc::now());
    upsert_persisted_secret(
        principal.user_id,
        PersistedSecretRow {
            api_key_id: id,
            prefix: prefix.clone(),
            key: key.clone(),
            created_at,
        },
    )?;

    Ok(Json(CreateApiKeyResponse { id, key, prefix }))
}

pub async fn create_api_key_secret_endpoint(
    State(state): State<AppState>,
    principal: Principal,
    Json(input): Json<CreateApiKeyRequest>,
) -> Result<Json<ApiKeySecretCreateResponse>, ApiError> {
    let Json(created) = create_api_key(State(state), principal.clone(), Json(input)).await?;
    let item = read_persisted_secrets(principal.user_id)
        .into_iter()
        .find(|row| row.api_key_id == created.id)
        .map(to_secret_item)
        .ok_or_else(|| anyhow::anyhow!("persisted api key secret missing"))?;
    Ok(Json(ApiKeySecretCreateResponse { item }))
}

pub async fn list_api_key_secrets_endpoint(
    principal: Principal,
) -> Result<Json<ApiKeySecretListResponse>, ApiError> {
    let items = read_persisted_secrets(principal.user_id)
        .into_iter()
        .map(to_secret_item)
        .collect();
    Ok(Json(ApiKeySecretListResponse { items }))
}

pub async fn list_api_keys(
    State(state): State<AppState>,
    principal: Principal,
) -> Result<Json<Vec<ApiKeyItem>>, ApiError> {
    let rows = state
        .db()
        .query_all(
            r#"
        SELECT id, name, prefix, scope_json, expires_at, monthly_spend_cap, last_used_at, last_used_ip_country, created_at, revoked_at, paused_at, deleted_at
          FROM api_keys
         WHERE user_id = ?1 AND deleted_at IS NULL
         ORDER BY created_at DESC
        "#,
            vec![crate::db::uuid_val(principal.user_id)],
        )
        .await?;

    Ok(Json(rows.into_iter().map(row_to_item).collect()))
}

pub async fn rename_api_key(
    State(state): State<AppState>,
    principal: Principal,
    Path(id): Path<Uuid>,
    Json(input): Json<RenameApiKeyRequest>,
) -> Result<Json<ApiKeyItem>, ApiError> {
    let row = state
        .db()
        .query_optional(
            r#"
        UPDATE api_keys
           SET name = ?3
         WHERE id = ?1 AND user_id = ?2 AND deleted_at IS NULL AND paused_at IS NULL
         RETURNING id, name, prefix, scope_json, expires_at, monthly_spend_cap, last_used_at, last_used_ip_country, created_at, revoked_at, paused_at, deleted_at
        "#,
            vec![
                crate::db::uuid_val(id),
                crate::db::uuid_val(principal.user_id),
                crate::db::val(input.name),
            ],
        )
        .await?
        .ok_or_else(|| ApiError::bad_request("api_key_not_found", "API key not found"))?;

    Ok(Json(row_to_item(row)))
}

pub async fn update_api_key_limits(
    State(state): State<AppState>,
    principal: Principal,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdateApiKeyLimitsRequest>,
) -> Result<Json<ApiKeyItem>, ApiError> {
    let expires_at = normalize_expires_at(input.expires_at)?;
    if input
        .monthly_spend_cap
        .is_some_and(|value| value < rust_decimal::Decimal::ZERO)
    {
        return Err(ApiError::bad_request(
            "invalid_monthly_spend_cap",
            "monthly spend cap must be zero or positive",
        ));
    }
    let row = state
        .db()
        .query_optional(
            r#"
        UPDATE api_keys
           SET scope_json = ?3,
               expires_at = ?4,
               monthly_spend_cap = ?5
         WHERE id = ?1 AND user_id = ?2 AND deleted_at IS NULL AND paused_at IS NULL
         RETURNING id, name, prefix, scope_json, expires_at, monthly_spend_cap, last_used_at, last_used_ip_country, created_at, revoked_at, paused_at, deleted_at
        "#,
            vec![
                crate::db::uuid_val(id),
                crate::db::uuid_val(principal.user_id),
                input
                    .scope_json
                    .map(crate::db::json_val)
                    .unwrap_or(libsql::Value::Null),
                crate::db::opt_val(expires_at),
                input
                    .monthly_spend_cap
                    .map(crate::db::dec_val)
                    .unwrap_or(libsql::Value::Null),
            ],
        )
        .await?
        .ok_or_else(|| ApiError::bad_request("api_key_not_found", "API key not found"))?;

    Ok(Json(row_to_item(row)))
}

pub async fn set_api_key_status_endpoint(
    State(state): State<AppState>,
    principal: Principal,
    Path(id): Path<Uuid>,
    Json(input): Json<ApiKeyStatusRequest>,
) -> Result<Json<ApiKeyStatusResponse>, ApiError> {
    let now = crate::db::now_string();
    let sql = match input.action.as_str() {
        "pause" => {
            r#"
            UPDATE api_keys
               SET paused_at = ?3
             WHERE id = ?1 AND user_id = ?2 AND deleted_at IS NULL AND paused_at IS NULL
             RETURNING id, name, prefix, scope_json, expires_at, monthly_spend_cap, last_used_at, last_used_ip_country, created_at, revoked_at, paused_at, deleted_at
            "#
        }
        "activate" => {
            r#"
            UPDATE api_keys
               SET paused_at = NULL, revoked_at = NULL
             WHERE id = ?1 AND user_id = ?2 AND deleted_at IS NULL AND paused_at IS NOT NULL
             RETURNING id, name, prefix, scope_json, expires_at, monthly_spend_cap, last_used_at, last_used_ip_country, created_at, revoked_at, paused_at, deleted_at
            "#
        }
        _ => {
            return Err(ApiError::bad_request(
                "invalid_action",
                "invalid api key action",
            ));
        }
    };

    let row = state
        .db()
        .query_optional(
            sql,
            vec![
                crate::db::uuid_val(id),
                crate::db::uuid_val(principal.user_id),
                crate::db::val(now),
            ],
        )
        .await?
        .ok_or_else(|| ApiError::bad_request("api_key_not_found", "API key not found"))?;

    Ok(Json(ApiKeyStatusResponse {
        item: row_to_item(row),
    }))
}

pub async fn delete_api_key_endpoint(
    State(state): State<AppState>,
    principal: Principal,
    Path(id): Path<Uuid>,
    Json(input): Json<DeleteApiKeyRequest>,
) -> Result<Json<ApiKeyDeleteResponse>, ApiError> {
    if !input.confirm {
        return Err(ApiError::bad_request(
            "confirm_required",
            "delete confirmation required",
        ));
    }
    let deleted = state
        .db()
        .execute(
            r#"
            UPDATE api_keys
               SET deleted_at = ?3
             WHERE id = ?1 AND user_id = ?2 AND deleted_at IS NULL AND paused_at IS NOT NULL
            "#,
            vec![
                crate::db::uuid_val(id),
                crate::db::uuid_val(principal.user_id),
                crate::db::val(crate::db::now_string()),
            ],
        )
        .await?;
    if deleted == 0 {
        return Err(ApiError::bad_request(
            "api_key_not_found",
            "API key not found",
        ));
    }
    remove_persisted_secret(principal.user_id, id)?;
    state
        .db()
        .execute(
            "DELETE FROM market_api_key_share_allowlist WHERE api_key_id=?1",
            vec![crate::db::uuid_val(id)],
        )
        .await?;
    state
        .db()
        .execute(
            "DELETE FROM market_share_sticky_routes WHERE api_key_id=?1",
            vec![crate::db::uuid_val(id)],
        )
        .await?;
    Ok(Json(ApiKeyDeleteResponse { deleted: true, id }))
}

pub async fn available_shares_endpoint(
    State(state): State<AppState>,
    _principal: Principal,
) -> Result<Json<AvailableSharesResponse>, ApiError> {
    let rows = state
        .db()
        .query_all(
            r#"
            SELECT router_id, share_id,
                   COALESCE(NULLIF(subdomain, ''), json_extract(raw_json, '$.subdomain')) AS subdomain,
                   app_type, enabled_codex, enabled_claude, enabled_gemini, online, for_sale, share_status
              FROM router_shares
             WHERE for_sale = 'Yes' AND share_status = 'active'
             ORDER BY online DESC, COALESCE(NULLIF(subdomain, ''), json_extract(raw_json, '$.subdomain')) ASC, router_id ASC, share_id ASC
            "#,
            vec![],
        )
        .await?;
    let items = rows
        .into_iter()
        .map(|row| {
            let mut capabilities = vec![row.string("app_type")];
            if row.bool("enabled_codex") && !capabilities.iter().any(|value| value == "codex") {
                capabilities.push("codex".to_string());
            }
            if row.bool("enabled_claude") && !capabilities.iter().any(|value| value == "claude") {
                capabilities.push("claude".to_string());
            }
            if row.bool("enabled_gemini") && !capabilities.iter().any(|value| value == "gemini") {
                capabilities.push("gemini".to_string());
            }
            AvailableShareItem {
                router_id: row.string("router_id"),
                share_id: row.string("share_id"),
                subdomain: row.opt_string("subdomain"),
                app_type: row.string("app_type"),
                capabilities,
                online: row.bool("online"),
                for_sale: row.string("for_sale"),
                share_status: row.string("share_status"),
            }
        })
        .collect();
    Ok(Json(AvailableSharesResponse { items }))
}

pub async fn get_api_key_share_allowlist_endpoint(
    State(state): State<AppState>,
    principal: Principal,
    Path(id): Path<Uuid>,
) -> Result<Json<ApiKeyShareAllowlistResponse>, ApiError> {
    ensure_api_key_owner(&state, principal.user_id, id).await?;
    let rows = state
        .db()
        .query_all(
            r#"
            SELECT router_id, share_id
              FROM market_api_key_share_allowlist
             WHERE api_key_id = ?1
             ORDER BY router_id ASC, share_id ASC
            "#,
            vec![crate::db::uuid_val(id)],
        )
        .await?;
    Ok(Json(ApiKeyShareAllowlistResponse {
        shares: rows
            .into_iter()
            .map(|row| ApiKeyShareRef {
                router_id: row.string("router_id"),
                share_id: row.string("share_id"),
            })
            .collect(),
    }))
}

pub async fn set_api_key_share_allowlist_endpoint(
    State(state): State<AppState>,
    principal: Principal,
    Path(id): Path<Uuid>,
    Json(input): Json<ApiKeyShareAllowlistUpdateRequest>,
) -> Result<Json<ApiKeyShareAllowlistResponse>, ApiError> {
    ensure_api_key_owner(&state, principal.user_id, id).await?;
    let mut shares = input
        .shares
        .into_iter()
        .map(|share| ApiKeyShareRef {
            router_id: share.router_id.trim().to_string(),
            share_id: share.share_id.trim().to_string(),
        })
        .filter(|share| !share.router_id.is_empty() && !share.share_id.is_empty())
        .collect::<Vec<_>>();
    shares.sort_by(|a, b| {
        a.router_id
            .cmp(&b.router_id)
            .then_with(|| a.share_id.cmp(&b.share_id))
    });
    shares.dedup();

    for share in &shares {
        let exists = state
            .db()
            .query_optional(
                "SELECT 1 AS found FROM router_shares WHERE router_id=?1 AND share_id=?2 AND for_sale='Yes' AND share_status='active' LIMIT 1",
                vec![crate::db::val(&share.router_id), crate::db::val(&share.share_id)],
            )
            .await?;
        if exists.is_none() {
            return Err(ApiError::bad_request(
                "share_not_available",
                format!(
                    "share {}:{} is not available",
                    share.router_id, share.share_id
                ),
            ));
        }
    }

    let tx = state.db().begin_immediate().await?;
    tx.execute(
        "DELETE FROM market_api_key_share_allowlist WHERE api_key_id=?1",
        vec![crate::db::uuid_val(id)],
    )
    .await?;
    let now = crate::db::now_string();
    for share in &shares {
        tx.execute(
            r#"
            INSERT INTO market_api_key_share_allowlist (api_key_id, router_id, share_id, created_at)
            VALUES (?1, ?2, ?3, ?4)
            "#,
            vec![
                crate::db::uuid_val(id),
                crate::db::val(&share.router_id),
                crate::db::val(&share.share_id),
                crate::db::val(&now),
            ],
        )
        .await?;
    }
    tx.execute(
        "DELETE FROM market_share_sticky_routes WHERE api_key_id=?1",
        vec![crate::db::uuid_val(id)],
    )
    .await?;
    tx.commit().await?;
    Ok(Json(ApiKeyShareAllowlistResponse { shares }))
}

pub fn hash_key(key: &str) -> String {
    let digest = Sha256::digest(key.as_bytes());
    format!("sha256:{}", hex::encode(digest))
}

fn generate_key() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    format!("sk-cs-{}", URL_SAFE_NO_PAD.encode(bytes))
}

async fn ensure_api_key_owner(
    state: &AppState,
    user_id: Uuid,
    api_key_id: Uuid,
) -> Result<(), ApiError> {
    let found = state
        .db()
        .query_optional(
            "SELECT 1 AS found FROM api_keys WHERE id=?1 AND user_id=?2 AND deleted_at IS NULL LIMIT 1",
            vec![crate::db::uuid_val(api_key_id), crate::db::uuid_val(user_id)],
        )
        .await?;
    found
        .map(|_| ())
        .ok_or_else(|| ApiError::bad_request("api_key_not_found", "API key not found"))
}

fn normalize_expires_at(value: Option<String>) -> Result<Option<String>, ApiError> {
    let Some(value) = value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };
    let parsed = chrono::DateTime::parse_from_rfc3339(&value)
        .map_err(|_| ApiError::bad_request("invalid_expires_at", "expires_at must be RFC3339"))?;
    let utc = parsed.with_timezone(&chrono::Utc);
    if utc <= chrono::Utc::now() {
        return Err(ApiError::bad_request(
            "invalid_expires_at",
            "expires_at must be in the future",
        ));
    }
    Ok(Some(utc.to_rfc3339()))
}

fn row_to_item(row: crate::db::DbRow) -> ApiKeyItem {
    ApiKeyItem {
        id: row.uuid("id"),
        name: row.string("name"),
        prefix: row.string("prefix"),
        scope_json: row
            .opt_string("scope_json")
            .and_then(|value| serde_json::from_str(&value).ok()),
        expires_at: row.opt_datetime("expires_at"),
        monthly_spend_cap: row.opt_decimal("monthly_spend_cap"),
        last_used_at: row.opt_datetime("last_used_at"),
        last_used_ip_country: row.opt_string("last_used_ip_country"),
        created_at: row.datetime("created_at"),
        revoked_at: row.opt_datetime("revoked_at"),
        paused_at: row.opt_datetime("paused_at"),
        deleted_at: row.opt_datetime("deleted_at"),
    }
}
