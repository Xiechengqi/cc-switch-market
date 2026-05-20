use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    sync::{Mutex, OnceLock},
};

use anyhow::Context;
use axum::{
    Json,
    body::Body,
    extract::{FromRequestParts, State},
    http::{HeaderMap, HeaderValue, Method, Request, StatusCode, header, request::Parts},
    middleware::Next,
    response::IntoResponse,
};
use base64::{Engine, engine::general_purpose};
use chrono::{DateTime, Utc};
use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use rand::{RngCore, rngs::OsRng};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    app_state::AppState,
    config::{Config, config_dir},
    error::ApiError,
};

const LOGIN_PURPOSE: &str = "login";
static EMAIL_CODE_LIMITS: OnceLock<Mutex<HashMap<String, (i64, u32)>>> = OnceLock::new();

#[derive(Clone, Debug)]
pub struct Principal {
    pub user_id: Uuid,
    pub email: String,
    pub is_admin: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeResponse {
    id: Uuid,
    email: String,
    is_admin: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionStatus {
    authenticated: bool,
    user: Option<MeResponse>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestEmailCodeInput {
    email: String,
    turnstile_token: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestEmailCodeResponse {
    ok: bool,
    cooldown_secs: i64,
    masked_destination: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyEmailCodeInput {
    email: String,
    code: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyEmailCodeResponse {
    user: MeResponse,
    expires_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RouterUser {
    id: Option<String>,
    email: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RouterSessionResponse {
    user: RouterUser,
    expires_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RegisterInstallationRequest<'a> {
    public_key: &'a str,
    platform: &'a str,
    app_version: &'a str,
    instance_nonce: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RegisterInstallationResponse {
    installation_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredWebAuthIdentity {
    installation_id: String,
    private_key_base64: String,
    public_key_base64: String,
}

#[derive(Debug, Clone)]
struct WebAuthIdentity {
    installation_id: String,
    signing_key: SigningKey,
}

#[derive(Debug, Deserialize)]
struct RouterError {
    message: Option<String>,
    error: Option<RouterErrorBody>,
}

#[derive(Debug, Deserialize)]
struct RouterErrorBody {
    message: Option<String>,
}

pub async fn me(principal: Principal) -> Json<MeResponse> {
    Json(me_response(principal))
}

pub async fn session_status(principal: OptionPrincipal) -> impl IntoResponse {
    let user = principal.0.map(me_response);
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-store, no-cache, must-revalidate, private"),
    );
    (
        headers,
        Json(SessionStatus {
            authenticated: user.is_some(),
            user,
        }),
    )
}

pub async fn request_email_code(
    State(state): State<AppState>,
    Json(input): Json<RequestEmailCodeInput>,
) -> Result<Json<RequestEmailCodeResponse>, ApiError> {
    let email = normalize_email(&input.email)?;
    check_email_code_rate_limit(&email)?;
    verify_turnstile_if_enabled(&state, input.turnstile_token.as_deref()).await?;
    let mut identity = ensure_web_auth_identity(&state.config, &state.http).await?;
    let response = match request_router_email_code(&state, &identity, &email).await {
        Ok(response) => response,
        Err(err) if is_installation_not_found(&err) => {
            reset_web_auth_identity()?;
            identity = ensure_web_auth_identity(&state.config, &state.http).await?;
            request_router_email_code(&state, &identity, &email).await?
        }
        Err(err) => return Err(err),
    };
    Ok(Json(response))
}

async fn verify_turnstile_if_enabled(
    state: &AppState,
    token: Option<&str>,
) -> Result<(), ApiError> {
    if !state.config.cloudflare_turnstile_enabled() {
        return Ok(());
    }
    let token = token
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            ApiError::bad_request("turnstile_required", "Turnstile verification required")
        })?;
    let response = state
        .http
        .post("https://challenges.cloudflare.com/turnstile/v0/siteverify")
        .form(&[
            (
                "secret",
                state.config.cloudflare_turnstile_secret_key.as_str(),
            ),
            ("response", token),
        ])
        .send()
        .await
        .map_err(|err| {
            ApiError::service_unavailable(format!("Turnstile verification failed: {err}"))
        })?;
    let status = response.status();
    let body = response
        .json::<TurnstileVerifyResponse>()
        .await
        .unwrap_or_default();
    if !status.is_success() || !body.success {
        return Err(ApiError::bad_request(
            "turnstile_failed",
            "Turnstile verification failed",
        ));
    }
    Ok(())
}

#[derive(Debug, Default, Deserialize)]
struct TurnstileVerifyResponse {
    success: bool,
}

async fn request_router_email_code(
    state: &AppState,
    identity: &WebAuthIdentity,
    email: &str,
) -> Result<RequestEmailCodeResponse, ApiError> {
    let timestamp_ms = Utc::now().timestamp_millis();
    let nonce = Uuid::new_v4().to_string();
    let payload = serde_json::json!({ "email": email, "purpose": LOGIN_PURPOSE });
    let signature = sign_action_payload(
        &identity,
        "auth_request_code",
        &payload,
        timestamp_ms,
        &nonce,
    )?;
    let response = state
        .http
        .post(format!(
            "{}/v1/auth/email/request-code",
            state.config.router_api_base_url.trim_end_matches('/')
        ))
        .json(&serde_json::json!({
            "email": email,
            "installationId": identity.installation_id,
            "timestampMs": timestamp_ms,
            "nonce": nonce,
            "signature": signature,
        }))
        .send()
        .await
        .map_err(|err| {
            ApiError::service_unavailable(format!("router auth request failed: {err}"))
        })?;
    parse_router_response(response).await
}

fn check_email_code_rate_limit(email: &str) -> Result<(), ApiError> {
    let now_minute = Utc::now().timestamp() / 60;
    let limits = EMAIL_CODE_LIMITS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut limits = limits
        .lock()
        .map_err(|_| ApiError::service_unavailable("rate limit lock poisoned"))?;
    let entry = limits.entry(email.to_string()).or_insert((now_minute, 0));
    if entry.0 != now_minute {
        *entry = (now_minute, 0);
    }
    entry.1 += 1;
    if entry.1 > 5 {
        return Err(ApiError::bad_request(
            "rate_limited",
            "too many verification code requests; retry later",
        ));
    }
    Ok(())
}

pub async fn verify_email_code(
    State(state): State<AppState>,
    Json(input): Json<VerifyEmailCodeInput>,
) -> Result<impl IntoResponse, ApiError> {
    let email = normalize_email(&input.email)?;
    crate::rate_limit::check("email_code_verify", &email, 10)?;
    let code = input.code.trim();
    if code.len() != 6 || !code.chars().all(|ch| ch.is_ascii_digit()) {
        return Err(ApiError::unauthorized("invalid verification code"));
    }
    let identity = ensure_web_auth_identity(&state.config, &state.http).await?;
    let response = state
        .http
        .post(format!(
            "{}/v1/auth/email/verify-code",
            state.config.router_api_base_url.trim_end_matches('/')
        ))
        .json(&serde_json::json!({
            "email": email,
            "code": code,
            "installationId": identity.installation_id,
        }))
        .send()
        .await
        .map_err(|err| {
            ApiError::service_unavailable(format!("router auth verify failed: {err}"))
        })?;
    let router_session: RouterSessionResponse = parse_router_response(response).await?;
    let verified_email = normalize_email(&router_session.user.email)?;
    if verified_email != email {
        return Err(ApiError::unauthorized("verified email mismatch"));
    }

    let db = state.db();
    let user = upsert_user_by_email(db, &verified_email).await?;
    let expires_at = Utc::now() + chrono::Duration::seconds(state.config.market_session_ttl_secs);
    let token = generate_session_token();
    let token_hash = hash_session_token(&state.config, &token);
    db.execute(
        r#"
        INSERT INTO web_sessions
          (id, user_id, email, session_token_hash, router_user_id, router_access_expires_at, expires_at, last_seen_at, created_at)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)
        "#,
        vec![
            crate::db::uuid_val(Uuid::new_v4()),
            crate::db::uuid_val(user.user_id),
            crate::db::val(&user.email),
            crate::db::val(token_hash),
            crate::db::opt_val(router_session.user.id),
            crate::db::val(router_session.expires_at.to_rfc3339()),
            crate::db::val(expires_at.to_rfc3339()),
            crate::db::val(crate::db::now_string()),
        ],
    )
    .await?;

    let cookie = build_session_cookie(&state.config, &token, expires_at, false);
    let csrf_cookie = build_csrf_cookie(&generate_session_token(), expires_at, false);
    let mut headers = HeaderMap::new();
    headers.append(
        header::SET_COOKIE,
        HeaderValue::from_str(&cookie)
            .map_err(|_| ApiError::service_unavailable("invalid session cookie"))?,
    );
    headers.append(
        header::SET_COOKIE,
        HeaderValue::from_str(&csrf_cookie)
            .map_err(|_| ApiError::service_unavailable("invalid csrf cookie"))?,
    );
    Ok((
        headers,
        Json(VerifyEmailCodeResponse {
            user: me_response(user),
            expires_at,
        }),
    ))
}

pub async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    if let Some(token) = session_token_from_headers(&headers, &state.config) {
        let token_hash = hash_session_token(&state.config, &token);
        state
            .db()
            .execute(
                "UPDATE web_sessions SET revoked_at = ?2 WHERE session_token_hash = ?1",
                vec![
                    crate::db::val(token_hash),
                    crate::db::val(crate::db::now_string()),
                ],
            )
            .await?;
    }
    let mut response_headers = HeaderMap::new();
    response_headers.append(
        header::SET_COOKIE,
        HeaderValue::from_str(&build_session_cookie(&state.config, "", Utc::now(), true))
            .map_err(|_| ApiError::service_unavailable("invalid session cookie"))?,
    );
    response_headers.append(
        header::SET_COOKIE,
        HeaderValue::from_str(&build_csrf_cookie("", Utc::now(), true))
            .map_err(|_| ApiError::service_unavailable("invalid csrf cookie"))?,
    );
    Ok((response_headers, Json(serde_json::json!({ "ok": true }))))
}

pub async fn csrf_middleware(
    State(state): State<AppState>,
    request: Request<Body>,
    next: Next,
) -> Result<impl IntoResponse, ApiError> {
    if matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    ) || request.headers().get(header::AUTHORIZATION).is_some()
    {
        return Ok(next.run(request).await);
    }
    let path = request.uri().path();
    if matches!(
        path,
        "/v1/auth/email/request-code"
            | "/market-api/auth/email/request-code"
            | "/v1/auth/email/verify-code"
            | "/market-api/auth/email/verify-code"
            | "/v1/webhooks/dodo"
    ) || !path.starts_with("/v1/") && !path.starts_with("/market-api/")
    {
        return Ok(next.run(request).await);
    }
    if session_token_from_headers(request.headers(), &state.config).is_none() {
        return Ok(next.run(request).await);
    }
    if path.starts_with("/v1/admin/") || path.starts_with("/market-api/admin/") {
        let subject = session_token_from_headers(request.headers(), &state.config)
            .unwrap_or_else(|| "anonymous".to_string());
        crate::rate_limit::check("admin_write", &subject, 120)?;
    }
    let header_token = request
        .headers()
        .get("x-csrf-token")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let cookie_token = cookie_value(request.headers(), csrf_cookie_name())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    if header_token != cookie_token.as_deref() {
        return Err(ApiError::Http {
            status: StatusCode::FORBIDDEN,
            error_type: "permission_error",
            code: "csrf_failed",
            message: "invalid CSRF token".to_string(),
            param: None,
        });
    }
    Ok(next.run(request).await)
}

fn me_response(principal: Principal) -> MeResponse {
    MeResponse {
        id: principal.user_id,
        email: principal.email,
        is_admin: principal.is_admin,
    }
}

pub struct OptionPrincipal(pub Option<Principal>);

impl FromRequestParts<AppState> for OptionPrincipal {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        match principal_from_cookie(parts, state).await {
            Ok(p) => Ok(Self(Some(p))),
            Err(_) => Ok(Self(None)),
        }
    }
}

impl FromRequestParts<AppState> for Principal {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        principal_from_cookie(parts, state).await
    }
}

async fn principal_from_cookie(parts: &Parts, state: &AppState) -> Result<Principal, ApiError> {
    let token = session_token_from_headers(&parts.headers, &state.config)
        .ok_or_else(|| ApiError::unauthorized("missing market session"))?;
    let token_hash = hash_session_token(&state.config, &token);
    let row = state
        .db()
        .query_optional(
            r#"
        SELECT users.id, users.email
        FROM web_sessions
        JOIN users ON users.id = web_sessions.user_id
        WHERE web_sessions.session_token_hash = ?1
          AND web_sessions.revoked_at IS NULL
          AND web_sessions.expires_at > ?2
          AND users.status = 'active'
        "#,
            vec![
                crate::db::val(token_hash),
                crate::db::val(crate::db::now_string()),
            ],
        )
        .await?
        .ok_or_else(|| ApiError::unauthorized("invalid or expired market session"))?;
    let email = row.string("email");
    let is_admin = state
        .config
        .market_admin_emails
        .iter()
        .any(|admin| admin.eq_ignore_ascii_case(&email));
    Ok(Principal {
        user_id: row.uuid("id"),
        email,
        is_admin,
    })
}

pub struct AdminPrincipal(pub Principal);

impl FromRequestParts<AppState> for AdminPrincipal {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let principal = principal_from_cookie(parts, state).await?;
        if !principal.is_admin {
            return Err(ApiError::forbidden("admin role required"));
        }
        Ok(Self(principal))
    }
}

fn session_token_from_headers(headers: &HeaderMap, config: &Config) -> Option<String> {
    cookie_value(headers, &config.market_session_cookie_name)
}

fn cookie_value(headers: &HeaderMap, cookie_name: &str) -> Option<String> {
    let cookie = headers.get(header::COOKIE)?.to_str().ok()?;
    for part in cookie.split(';') {
        let (name, value) = part.trim().split_once('=')?;
        if name == cookie_name && !value.trim().is_empty() {
            return Some(value.trim().to_string());
        }
    }
    None
}

fn csrf_cookie_name() -> &'static str {
    "cc_switch_market_csrf"
}

async fn upsert_user_by_email(db: &crate::db::Db, email: &str) -> Result<Principal, ApiError> {
    let id = Uuid::new_v4();
    let now = crate::db::now_string();
    let row = db.query_one(
        r#"
        INSERT INTO users (id, email, email_verified_source, email_verified_at, last_login_at, created_at, updated_at)
        VALUES (?1, ?2, 'router_resend', ?3, ?3, ?3, ?3)
        ON CONFLICT (email)
        DO UPDATE SET last_login_at = ?3, updated_at = ?3
        RETURNING id, email
        "#,
        vec![crate::db::uuid_val(id), crate::db::val(email), crate::db::val(now)],
    )
    .await?;
    Ok(Principal {
        user_id: row.uuid("id"),
        email: row.string("email"),
        is_admin: false,
    })
}

fn normalize_email(value: &str) -> Result<String, ApiError> {
    let email = value.trim().to_ascii_lowercase();
    if !email.contains('@') || email.len() > 254 {
        return Err(ApiError::bad_request("invalid_email", "invalid email"));
    }
    Ok(email)
}

fn generate_session_token() -> String {
    let mut bytes = [0_u8; 32];
    OsRng.fill_bytes(&mut bytes);
    general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn hash_session_token(config: &Config, token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(config.market_session_cookie_secret.as_bytes());
    hasher.update(b":");
    hasher.update(token.as_bytes());
    hex::encode(hasher.finalize())
}

fn build_session_cookie(
    config: &Config,
    token: &str,
    expires_at: DateTime<Utc>,
    clear: bool,
) -> String {
    let mut cookie = format!(
        "{}={}; Path=/; HttpOnly; SameSite=Lax; Expires={}",
        config.market_session_cookie_name,
        token,
        expires_at.format("%a, %d %b %Y %H:%M:%S GMT")
    );
    if clear {
        cookie.push_str("; Max-Age=0");
    } else {
        cookie.push_str(&format!(
            "; Max-Age={}",
            config.market_session_ttl_secs.max(0)
        ));
    }
    if config.market_public_base_url.starts_with("https://") {
        cookie.push_str("; Secure");
    }
    cookie
}

fn build_csrf_cookie(token: &str, expires_at: DateTime<Utc>, clear: bool) -> String {
    let mut cookie = format!(
        "{}={}; Path=/; SameSite=Lax; Expires={}",
        csrf_cookie_name(),
        token,
        expires_at.format("%a, %d %b %Y %H:%M:%S GMT")
    );
    if clear {
        cookie.push_str("; Max-Age=0");
    }
    cookie
}

async fn ensure_web_auth_identity(
    config: &Config,
    client: &reqwest::Client,
) -> Result<WebAuthIdentity, ApiError> {
    if let Some(identity) = load_web_auth_identity()? {
        return Ok(identity);
    }
    let signing_key = SigningKey::generate(&mut OsRng);
    let public_key_base64 =
        general_purpose::STANDARD.encode(signing_key.verifying_key().to_bytes());
    let response = client
        .post(format!(
            "{}/v1/installations/register",
            config.router_api_base_url.trim_end_matches('/')
        ))
        .json(&RegisterInstallationRequest {
            public_key: &public_key_base64,
            platform: "cc-switch-market-web",
            app_version: env!("CARGO_PKG_VERSION"),
            instance_nonce: Uuid::new_v4().to_string(),
        })
        .send()
        .await
        .map_err(|err| {
            ApiError::service_unavailable(format!("register web auth identity failed: {err}"))
        })?;
    let body: RegisterInstallationResponse = parse_router_response(response).await?;
    let stored = StoredWebAuthIdentity {
        installation_id: body.installation_id.clone(),
        private_key_base64: general_purpose::STANDARD.encode(signing_key.to_bytes()),
        public_key_base64,
    };
    save_web_auth_identity(&stored)?;
    Ok(WebAuthIdentity {
        installation_id: body.installation_id,
        signing_key,
    })
}

fn load_web_auth_identity() -> Result<Option<WebAuthIdentity>, ApiError> {
    let path = web_auth_identity_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let stored: StoredWebAuthIdentity =
        serde_json::from_str(&raw).with_context(|| format!("parse {}", path.display()))?;
    let private_bytes = general_purpose::STANDARD
        .decode(&stored.private_key_base64)
        .context("decode web auth private key")?;
    let private_array: [u8; 32] = private_bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("invalid web auth private key length"))?;
    let signing_key = SigningKey::from_bytes(&private_array);
    let public_bytes = general_purpose::STANDARD
        .decode(&stored.public_key_base64)
        .context("decode web auth public key")?;
    let public_array: [u8; 32] = public_bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("invalid web auth public key length"))?;
    VerifyingKey::from_bytes(&public_array).context("parse web auth public key")?;
    let derived_public = general_purpose::STANDARD.encode(signing_key.verifying_key().to_bytes());
    if derived_public != stored.public_key_base64 {
        return Err(anyhow::anyhow!("web auth public key does not match private key").into());
    }
    Ok(Some(WebAuthIdentity {
        installation_id: stored.installation_id,
        signing_key,
    }))
}

fn save_web_auth_identity(stored: &StoredWebAuthIdentity) -> Result<(), ApiError> {
    let path = web_auth_identity_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let body = serde_json::to_vec_pretty(stored).context("serialize web auth identity")?;
    fs::write(&path, body).with_context(|| format!("write {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
            .with_context(|| format!("chmod 0600 {}", path.display()))?;
    }
    Ok(())
}

fn reset_web_auth_identity() -> Result<(), ApiError> {
    let path = web_auth_identity_path()?;
    if path.exists() {
        fs::remove_file(&path).with_context(|| format!("remove {}", path.display()))?;
    }
    Ok(())
}

fn web_auth_identity_path() -> Result<PathBuf, ApiError> {
    Ok(config_dir()?.join("web-auth-identity.json"))
}

fn is_installation_not_found(error: &ApiError) -> bool {
    error
        .to_string()
        .to_ascii_lowercase()
        .contains("installation not found")
}

fn sign_action_payload(
    identity: &WebAuthIdentity,
    action: &str,
    payload: &serde_json::Value,
    timestamp_ms: i64,
    nonce: &str,
) -> Result<String, ApiError> {
    let payload_json = serde_json::to_string(payload).context("serialize auth request payload")?;
    let body = format!(
        "{}\n{}\n{}\n{}\n{}",
        identity.installation_id, action, payload_json, timestamp_ms, nonce
    );
    let signature = identity.signing_key.sign(body.as_bytes());
    Ok(general_purpose::STANDARD.encode(signature.to_bytes()))
}

async fn parse_router_response<T: for<'de> Deserialize<'de>>(
    response: reqwest::Response,
) -> Result<T, ApiError> {
    let status = response.status();
    let text = response.text().await.map_err(|err| {
        ApiError::service_unavailable(format!("read router response failed: {err}"))
    })?;
    if !status.is_success() {
        let message = serde_json::from_str::<RouterError>(&text)
            .ok()
            .and_then(|body| body.message.or(body.error.and_then(|err| err.message)))
            .unwrap_or(text);
        return Err(ApiError::bad_request("router_auth_failed", message));
    }
    serde_json::from_str(&text).map_err(|err| {
        ApiError::service_unavailable(format!("parse router response failed: {err}; body={text}"))
    })
}

#[derive(Debug, Clone)]
pub struct ApiKeyPrincipal {
    pub user_id: Uuid,
    pub user_email: String,
    pub api_key_id: Uuid,
    pub is_admin: bool,
    pub monthly_spend_cap: Option<rust_decimal::Decimal>,
    pub scope_json: Option<serde_json::Value>,
}
