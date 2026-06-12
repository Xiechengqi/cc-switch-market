use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, bail};
use base64::{Engine, engine::general_purpose};
use chrono::{DateTime, Utc};
use dialoguer::{Input, Password, theme::ColorfulTheme};
use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::config::{Config, config_dir};

const LOGIN_PURPOSE: &str = "login";

#[derive(Debug, Clone)]
struct RouterIdentity {
    installation_id: String,
    signing_key: SigningKey,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredIdentity {
    installation_id: String,
    private_key_base64: String,
    public_key_base64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouterSession {
    pub email: String,
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: DateTime<Utc>,
    pub refresh_expires_at: DateTime<Utc>,
    pub router_base_domain: String,
    pub installation_id: String,
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RequestCodeResponse {
    masked_destination: String,
    cooldown_secs: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AuthUser {
    email: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionResponse {
    user: AuthUser,
    access_token: String,
    refresh_token: String,
    expires_at: DateTime<Utc>,
    refresh_expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketRegistration {
    pub email: String,
    #[serde(default)]
    pub maintenance_enabled: bool,
    #[serde(default)]
    pub maintenance_message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ErrorResponse {
    message: String,
}

pub async fn login(config: &Config) -> anyhow::Result<()> {
    let theme = ColorfulTheme::default();
    let email: String = Input::with_theme(&theme)
        .with_prompt("Router login email")
        .interact_text()?;
    let email = normalize_email(&email)?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()?;
    let mut identity = ensure_identity(&client, config).await?;
    let sent = match request_code(&client, config, &identity, &email).await {
        Ok(sent) => sent,
        Err(err) if is_installation_not_found(&err) => {
            reset_identity()?;
            identity = ensure_identity(&client, config).await?;
            request_code(&client, config, &identity, &email).await?
        }
        Err(err) => return Err(err),
    };
    println!(
        "Verification code sent to {}. Cooldown: {}s",
        sent.masked_destination, sent.cooldown_secs
    );
    let code = Password::with_theme(&theme)
        .with_prompt("Verification code")
        .interact()?;
    let session = verify_code(&client, config, &identity, &email, code.trim()).await?;
    save_session(&session)?;
    println!("Logged in as {}", session.email);
    Ok(())
}

pub async fn account(config: &Config) -> anyhow::Result<()> {
    let Some(session) = load_session()? else {
        println!("router_session=missing");
        println!("hint=run cc-switch-market login");
        return Ok(());
    };
    println!("email={}", session.email);
    println!("router_base_domain={}", config.router_base_domain);
    println!("router_market_subdomain={}", config.router_market_subdomain);
    println!("market_public_base_url={}", config.market_public_base_url);
    println!("installation_id={}", session.installation_id);
    println!("access_expires_at={}", session.expires_at);
    println!("refresh_expires_at={}", session.refresh_expires_at);
    match refresh_session(config).await {
        Ok(refreshed) => {
            println!("session=valid");
            println!("refreshed_access_expires_at={}", refreshed.expires_at);
        }
        Err(err) => {
            println!("session=invalid");
            println!("error={err}");
            println!("hint=run cc-switch-market login");
        }
    }
    Ok(())
}

pub fn logout() -> anyhow::Result<()> {
    let path = session_path()?;
    if path.exists() {
        fs::remove_file(&path).with_context(|| format!("remove {}", path.display()))?;
    }
    println!("Logged out");
    Ok(())
}

pub async fn access_token(config: &Config) -> anyhow::Result<String> {
    let session = refresh_session(config).await?;
    Ok(session.access_token)
}

pub async fn refresh_session(config: &Config) -> anyhow::Result<RouterSession> {
    let session = load_session()?.ok_or_else(|| {
        anyhow::anyhow!("router account is not logged in. Run `cc-switch-market login` first")
    })?;
    if session.refresh_expires_at <= Utc::now() {
        bail!("router refresh session expired. Run `cc-switch-market login` first");
    }
    if session.expires_at > Utc::now() + chrono::Duration::seconds(60) {
        return Ok(session);
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()?;
    let response = client
        .post(format!(
            "{}/v1/auth/session/refresh",
            config.router_api_base_url.trim_end_matches('/')
        ))
        .json(&serde_json::json!({
            "refreshToken": session.refresh_token,
            "installationId": session.installation_id,
        }))
        .send()
        .await
        .context("refresh router session failed")?;
    let body: SessionResponse = parse_json_response(response).await?;
    let refreshed = RouterSession {
        email: body.user.email.trim().to_ascii_lowercase(),
        access_token: body.access_token,
        refresh_token: body.refresh_token,
        expires_at: body.expires_at,
        refresh_expires_at: body.refresh_expires_at,
        router_base_domain: config.router_base_domain.clone(),
        installation_id: session.installation_id,
    };
    save_session(&refreshed)?;
    Ok(refreshed)
}

pub async fn register_market(
    config: &Config,
    pricing_summary: Option<serde_json::Value>,
) -> anyhow::Result<(RouterSession, MarketRegistration)> {
    let session = refresh_session(config).await?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()?;
    let response = client
        .post(format!(
            "{}/v1/markets/register",
            config.router_api_base_url.trim_end_matches('/')
        ))
        .bearer_auth(&session.access_token)
        .json(&serde_json::json!({
            "subdomain": config.router_market_subdomain,
            "publicBaseUrl": config.market_public_base_url,
            "pricingSummary": pricing_summary,
        }))
        .send()
        .await
        .context("register market with router failed")?;
    let market: MarketRegistration = parse_json_response(response).await?;
    Ok((session, market))
}

fn normalize_email(value: &str) -> anyhow::Result<String> {
    let email = value.trim().to_ascii_lowercase();
    if !email.contains('@') || email.len() > 254 {
        bail!("invalid email");
    }
    Ok(email)
}

async fn request_code(
    client: &reqwest::Client,
    config: &Config,
    identity: &RouterIdentity,
    email: &str,
) -> anyhow::Result<RequestCodeResponse> {
    let timestamp_ms = Utc::now().timestamp_millis();
    let nonce = Uuid::new_v4().to_string();
    let payload = serde_json::json!({ "email": email, "purpose": LOGIN_PURPOSE });
    let signature = sign_action_payload(
        identity,
        "auth_request_code",
        &payload,
        timestamp_ms,
        &nonce,
    )?;
    let response = client
        .post(format!(
            "{}/v1/auth/email/request-code",
            config.router_api_base_url.trim_end_matches('/')
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
        .context("request router email code failed")?;
    parse_json_response(response).await
}

async fn verify_code(
    client: &reqwest::Client,
    config: &Config,
    identity: &RouterIdentity,
    email: &str,
    code: &str,
) -> anyhow::Result<RouterSession> {
    let response = client
        .post(format!(
            "{}/v1/auth/email/verify-code",
            config.router_api_base_url.trim_end_matches('/')
        ))
        .json(&serde_json::json!({
            "email": email,
            "code": code,
            "installationId": identity.installation_id,
        }))
        .send()
        .await
        .context("verify router email code failed")?;
    let body: SessionResponse = parse_json_response(response).await?;
    Ok(RouterSession {
        email: body.user.email.trim().to_ascii_lowercase(),
        access_token: body.access_token,
        refresh_token: body.refresh_token,
        expires_at: body.expires_at,
        refresh_expires_at: body.refresh_expires_at,
        router_base_domain: config.router_base_domain.clone(),
        installation_id: identity.installation_id.clone(),
    })
}

async fn ensure_identity(
    client: &reqwest::Client,
    config: &Config,
) -> anyhow::Result<RouterIdentity> {
    if let Some(identity) = load_identity()? {
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
            platform: std::env::consts::OS,
            app_version: env!("CARGO_PKG_VERSION"),
            instance_nonce: Uuid::new_v4().to_string(),
        })
        .send()
        .await
        .context("register router market identity failed")?;
    let body: RegisterInstallationResponse = parse_json_response(response).await?;
    let stored = StoredIdentity {
        installation_id: body.installation_id.clone(),
        private_key_base64: general_purpose::STANDARD.encode(signing_key.to_bytes()),
        public_key_base64,
    };
    save_identity(&stored)?;
    Ok(RouterIdentity {
        installation_id: body.installation_id,
        signing_key,
    })
}

fn load_identity() -> anyhow::Result<Option<RouterIdentity>> {
    let path = identity_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let stored: StoredIdentity =
        serde_json::from_str(&raw).with_context(|| format!("parse {}", path.display()))?;
    let private_bytes = general_purpose::STANDARD
        .decode(&stored.private_key_base64)
        .context("decode router identity private key")?;
    let private_array: [u8; 32] = private_bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("invalid router identity private key length"))?;
    let signing_key = SigningKey::from_bytes(&private_array);
    let public_bytes = general_purpose::STANDARD
        .decode(&stored.public_key_base64)
        .context("decode router identity public key")?;
    let public_array: [u8; 32] = public_bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("invalid router identity public key length"))?;
    VerifyingKey::from_bytes(&public_array).context("parse router identity public key")?;
    let derived_public = general_purpose::STANDARD.encode(signing_key.verifying_key().to_bytes());
    if derived_public != stored.public_key_base64 {
        bail!("router identity public key does not match private key");
    }
    Ok(Some(RouterIdentity {
        installation_id: stored.installation_id,
        signing_key,
    }))
}

fn sign_action_payload(
    identity: &RouterIdentity,
    action: &str,
    payload: &serde_json::Value,
    timestamp_ms: i64,
    nonce: &str,
) -> anyhow::Result<String> {
    let payload_json = serde_json::to_string(payload)?;
    let body = format!(
        "{}\n{}\n{}\n{}\n{}",
        identity.installation_id, action, payload_json, timestamp_ms, nonce
    );
    let signature = identity.signing_key.sign(body.as_bytes());
    Ok(general_purpose::STANDARD.encode(signature.to_bytes()))
}

fn load_session() -> anyhow::Result<Option<RouterSession>> {
    let path = session_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let session =
        serde_json::from_str(&raw).with_context(|| format!("parse {}", path.display()))?;
    Ok(Some(session))
}

fn save_session(session: &RouterSession) -> anyhow::Result<()> {
    write_private_json(&session_path()?, session)
}

fn save_identity(identity: &StoredIdentity) -> anyhow::Result<()> {
    write_private_json(&identity_path()?, identity)
}

fn reset_identity() -> anyhow::Result<()> {
    let identity_path = identity_path()?;
    if identity_path.exists() {
        fs::remove_file(&identity_path)
            .with_context(|| format!("remove {}", identity_path.display()))?;
    }
    let session_path = session_path()?;
    if session_path.exists() {
        fs::remove_file(&session_path)
            .with_context(|| format!("remove {}", session_path.display()))?;
    }
    Ok(())
}

fn is_installation_not_found(error: &anyhow::Error) -> bool {
    error
        .to_string()
        .to_ascii_lowercase()
        .contains("installation not found")
}

fn write_private_json<T: Serialize>(path: &Path, value: &T) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let tmp = path.with_extension("tmp");
    let mut file = create_private_file(&tmp)?;
    let raw = serde_json::to_vec_pretty(value)?;
    file.write_all(&raw)?;
    file.flush()?;
    fs::rename(&tmp, path).with_context(|| format!("replace {}", path.display()))?;
    Ok(())
}

fn create_private_file(path: &Path) -> anyhow::Result<fs::File> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        Ok(fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o600)
            .open(path)?)
    }
    #[cfg(not(unix))]
    {
        Ok(fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(path)?)
    }
}

async fn parse_json_response<T: for<'de> Deserialize<'de>>(
    response: reqwest::Response,
) -> anyhow::Result<T> {
    let status = response.status();
    if status.is_success() {
        return Ok(response.json().await?);
    }
    let text = response.text().await.unwrap_or_else(|_| status.to_string());
    if let Ok(error) = serde_json::from_str::<ErrorResponse>(&text) {
        bail!("{}", error.message);
    }
    bail!("HTTP {status}: {text}");
}

pub fn session_path() -> anyhow::Result<PathBuf> {
    Ok(config_dir()?.join("router-session.json"))
}

fn identity_path() -> anyhow::Result<PathBuf> {
    Ok(config_dir()?.join("router-identity.json"))
}
