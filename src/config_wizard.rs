use std::{collections::BTreeMap, fs, path::Path};

use anyhow::{Context, bail};
use dialoguer::{Confirm, Input, theme::ColorfulTheme};

#[derive(Clone, Copy)]
enum FieldKind {
    Text,
    Secret,
    Bool,
}

#[derive(Clone, Copy)]
struct Field {
    key: &'static str,
    prompt: &'static str,
    default: &'static str,
    kind: FieldKind,
    required: bool,
}

const FIELDS: &[Field] = &[
    Field {
        key: "MARKET_HTTP_ADDR",
        prompt: "HTTP listen address",
        default: "0.0.0.0:8080",
        kind: FieldKind::Text,
        required: true,
    },
    Field {
        key: "MARKET_TUNNEL_ENABLED",
        prompt: "Enable router market SSH tunnel",
        default: "true",
        kind: FieldKind::Bool,
        required: false,
    },
    Field {
        key: "RUST_LOG",
        prompt: "Rust log filter",
        default: "cc_switch_market=info,tower_http=info,axum=info",
        kind: FieldKind::Text,
        required: false,
    },
    Field {
        key: "MARKET_SESSION_COOKIE_NAME",
        prompt: "Market session cookie name",
        default: "cc_switch_market_session",
        kind: FieldKind::Text,
        required: true,
    },
    Field {
        key: "MARKET_SESSION_COOKIE_SECRET",
        prompt: "Market session cookie secret",
        default: "change-me-market-session-secret-32b",
        kind: FieldKind::Secret,
        required: true,
    },
    Field {
        key: "MARKET_SESSION_TTL_SECS",
        prompt: "Market session TTL seconds",
        default: "2592000",
        kind: FieldKind::Text,
        required: true,
    },
    Field {
        key: "MARKET_ADMIN_EMAILS",
        prompt: "Admin emails, comma-separated",
        default: "admin@example.com",
        kind: FieldKind::Text,
        required: false,
    },
    Field {
        key: "MARKET_MIN_REQUEST_BALANCE",
        prompt: "Minimum user balance required before proxying a request",
        default: "0.10",
        kind: FieldKind::Text,
        required: true,
    },
    Field {
        key: "MARKET_PLATFORM_COMMISSION_BPS",
        prompt: "Market commission basis points for token usage",
        default: "1000",
        kind: FieldKind::Text,
        required: true,
    },
    Field {
        key: "MARKET_ROUTER_COMMISSION_BPS",
        prompt: "Router commission basis points for token usage",
        default: "500",
        kind: FieldKind::Text,
        required: true,
    },
    Field {
        key: "MARKET_SQLITE_PATH",
        prompt: "Local SQLite path",
        default: "$HOME/.config/cc-switch-market/cc-switch-market.db",
        kind: FieldKind::Text,
        required: false,
    },
    Field {
        key: "TURSO_DATABASE_URL",
        prompt: "Turso database URL, empty uses local SQLite",
        default: "",
        kind: FieldKind::Text,
        required: false,
    },
    Field {
        key: "TURSO_AUTH_TOKEN",
        prompt: "Turso auth token",
        default: "",
        kind: FieldKind::Secret,
        required: false,
    },
    Field {
        key: "TURSO_REPLICA_PATH",
        prompt: "Turso embedded replica path",
        default: "$HOME/.config/cc-switch-market/turso-replica.db",
        kind: FieldKind::Text,
        required: false,
    },
    Field {
        key: "TURSO_SYNC_INTERVAL_SECS",
        prompt: "Turso sync interval seconds",
        default: "300",
        kind: FieldKind::Text,
        required: false,
    },
    Field {
        key: "TURSO_BACKUP_ENABLED",
        prompt: "Enable hourly Turso local replica backup",
        default: "true",
        kind: FieldKind::Bool,
        required: false,
    },
    Field {
        key: "TURSO_BACKUP_INTERVAL_SECS",
        prompt: "Turso backup interval seconds",
        default: "3600",
        kind: FieldKind::Text,
        required: false,
    },
    Field {
        key: "TURSO_BACKUP_RETENTION_DAYS",
        prompt: "Turso backup retention days",
        default: "7",
        kind: FieldKind::Text,
        required: false,
    },
    Field {
        key: "OBJECT_STORE_BACKEND",
        prompt: "Object store backend (local; r2 is reserved)",
        default: "local",
        kind: FieldKind::Text,
        required: true,
    },
    Field {
        key: "OBJECT_STORE_LOCAL_DIR",
        prompt: "Local object store dir",
        default: "$HOME/.config/cc-switch-market/objects",
        kind: FieldKind::Text,
        required: false,
    },
    Field {
        key: "R2_ACCOUNT_ID",
        prompt: "Cloudflare R2 account id, reserved",
        default: "",
        kind: FieldKind::Text,
        required: false,
    },
    Field {
        key: "R2_ACCESS_KEY_ID",
        prompt: "Cloudflare R2 access key id, reserved",
        default: "",
        kind: FieldKind::Secret,
        required: false,
    },
    Field {
        key: "R2_SECRET_ACCESS_KEY",
        prompt: "Cloudflare R2 secret access key, reserved",
        default: "",
        kind: FieldKind::Secret,
        required: false,
    },
    Field {
        key: "R2_BUCKET",
        prompt: "Cloudflare R2 bucket, reserved",
        default: "",
        kind: FieldKind::Text,
        required: false,
    },
    Field {
        key: "R2_PUBLIC_BASE_URL",
        prompt: "Cloudflare R2 public base URL, reserved",
        default: "",
        kind: FieldKind::Text,
        required: false,
    },
    Field {
        key: "ROUTER_BASE_DOMAIN",
        prompt: "Router base domain",
        default: "localhost:8081",
        kind: FieldKind::Text,
        required: true,
    },
    Field {
        key: "ROUTER_MARKET_SUBDOMAIN",
        prompt: "Router market subdomain",
        default: "market",
        kind: FieldKind::Text,
        required: true,
    },
    Field {
        key: "DODO_API_BASE",
        prompt: "Dodo API base",
        default: "https://test.dodopayments.com",
        kind: FieldKind::Text,
        required: false,
    },
    Field {
        key: "DODO_API_KEY",
        prompt: "Dodo API key, empty uses mock checkout",
        default: "",
        kind: FieldKind::Secret,
        required: false,
    },
    Field {
        key: "DODO_PRODUCT_ID",
        prompt: "Dodo top-up product id, empty uses mock checkout",
        default: "",
        kind: FieldKind::Text,
        required: false,
    },
    Field {
        key: "DODO_ALLOWED_PAYMENT_METHOD_TYPES",
        prompt: "Dodo allowed payment methods",
        default: "credit,debit,apple_pay,google_pay,we_chat_pay,crypto_currency",
        kind: FieldKind::Text,
        required: false,
    },
    Field {
        key: "DODO_WEBHOOK_SECRET",
        prompt: "Dodo webhook secret",
        default: "dev",
        kind: FieldKind::Secret,
        required: false,
    },
    Field {
        key: "DODO_MOCK_CHECKOUT_ENABLED",
        prompt: "Enable mock Dodo checkout when API key/product are empty",
        default: "true",
        kind: FieldKind::Bool,
        required: false,
    },
    Field {
        key: "GATEIO_AUTO_PAYOUT_ENABLED",
        prompt: "Enable Gate.io automatic payout",
        default: "false",
        kind: FieldKind::Bool,
        required: false,
    },
    Field {
        key: "GATEIO_PAYOUT_WORKER_INTERVAL_SECS",
        prompt: "Gate.io payout worker interval seconds",
        default: "60",
        kind: FieldKind::Text,
        required: false,
    },
    Field {
        key: "GATEIO_API_BASE",
        prompt: "Gate.io API base",
        default: "https://api.gateio.ws",
        kind: FieldKind::Text,
        required: false,
    },
    Field {
        key: "GATEIO_API_KEY",
        prompt: "Gate.io API key",
        default: "",
        kind: FieldKind::Secret,
        required: false,
    },
    Field {
        key: "GATEIO_API_SECRET",
        prompt: "Gate.io API secret",
        default: "",
        kind: FieldKind::Secret,
        required: false,
    },
    Field {
        key: "GATEIO_SETTLEMENT_CURRENCY",
        prompt: "Gate.io settlement currency",
        default: "USDT",
        kind: FieldKind::Text,
        required: false,
    },
    Field {
        key: "GATEIO_USD_USDT_RATE",
        prompt: "USD to USDT rate",
        default: "1.000000",
        kind: FieldKind::Text,
        required: false,
    },
];

pub fn run(env_file: &Path) -> anyhow::Result<()> {
    let theme = ColorfulTheme::default();
    let mut values = read_env_file(env_file)?;

    println!("Configuring {}", env_file.display());
    println!("Press Enter to keep the shown default. Existing values are displayed.");

    for field in FIELDS {
        if gateio_field_is_skippable(field.key, &values) {
            continue;
        }
        let current = values
            .get(field.key)
            .cloned()
            .unwrap_or_else(|| field.default.to_string());
        let value = prompt_field(&theme, field, &current)?;
        values.insert(field.key.to_string(), value);
    }

    validate(&values)?;
    write_env_file(env_file, &values)?;
    println!("Saved {}", env_file.display());
    Ok(())
}

fn prompt_field(theme: &ColorfulTheme, field: &Field, current: &str) -> anyhow::Result<String> {
    match field.kind {
        FieldKind::Text => {
            let value: String = Input::with_theme(theme)
                .with_prompt(field.prompt)
                .default(current.to_string())
                .allow_empty(!field.required)
                .interact_text()?;
            Ok(value)
        }
        FieldKind::Secret => prompt_secret(theme, field, current),
        FieldKind::Bool => {
            let default = matches!(current, "true" | "1" | "yes" | "Yes" | "YES");
            let enabled = Confirm::with_theme(theme)
                .with_prompt(field.prompt)
                .default(default)
                .interact()?;
            Ok(enabled.to_string())
        }
    }
}

fn prompt_secret(theme: &ColorfulTheme, field: &Field, current: &str) -> anyhow::Result<String> {
    let input = Input::<String>::with_theme(theme)
        .with_prompt(field.prompt)
        .default(current.to_string())
        .allow_empty(!field.required)
        .interact_text()?;
    if input.is_empty() {
        if field.required && current.is_empty() {
            bail!("{} is required", field.key);
        }
        Ok(current.to_string())
    } else {
        Ok(input)
    }
}

fn validate(values: &BTreeMap<String, String>) -> anyhow::Result<()> {
    if let Some(value) = values.get("MARKET_HTTP_ADDR") {
        value
            .parse::<std::net::SocketAddr>()
            .with_context(|| format!("invalid MARKET_HTTP_ADDR: {value}"))?;
    }
    validate_http_url(values, "GATEIO_API_BASE", false)?;
    validate_http_url(values, "DODO_API_BASE", false)?;
    validate_router_base_domain(values)?;
    validate_subdomain(values, "ROUTER_MARKET_SUBDOMAIN")?;
    validate_market_auth(values)?;
    validate_decimal(values, "MARKET_MIN_REQUEST_BALANCE", false)?;
    validate_platform_commission(values)?;

    validate_database_config(values)?;
    validate_object_store_config(values)?;
    validate_dodo_config(values)?;

    let rate = values
        .get("GATEIO_USD_USDT_RATE")
        .map(String::as_str)
        .unwrap_or("1.000000")
        .parse::<rust_decimal::Decimal>()
        .context("GATEIO_USD_USDT_RATE must be a decimal")?;
    if rate <= rust_decimal::Decimal::ZERO {
        bail!("GATEIO_USD_USDT_RATE must be positive");
    }

    if values
        .get("GATEIO_AUTO_PAYOUT_ENABLED")
        .map(|v| v == "true")
        .unwrap_or(false)
    {
        for key in ["GATEIO_API_KEY", "GATEIO_API_SECRET"] {
            if values.get(key).map(|v| v.trim().is_empty()).unwrap_or(true) {
                bail!("{key} is required when Gate.io automatic payout is enabled");
            }
        }
    }

    Ok(())
}

fn validate_platform_commission(values: &BTreeMap<String, String>) -> anyhow::Result<()> {
    let market_bps = values
        .get("MARKET_PLATFORM_COMMISSION_BPS")
        .map(String::as_str)
        .unwrap_or("1000")
        .parse::<i64>()
        .context("MARKET_PLATFORM_COMMISSION_BPS must be an integer")?;
    let router_bps = values
        .get("MARKET_ROUTER_COMMISSION_BPS")
        .map(String::as_str)
        .unwrap_or("500")
        .parse::<i64>()
        .context("MARKET_ROUTER_COMMISSION_BPS must be an integer")?;
    if !(0..=10_000).contains(&market_bps) {
        bail!("MARKET_PLATFORM_COMMISSION_BPS must be between 0 and 10000");
    }
    if !(0..=10_000).contains(&router_bps) {
        bail!("MARKET_ROUTER_COMMISSION_BPS must be between 0 and 10000");
    }
    if market_bps + router_bps > 10_000 {
        bail!(
            "MARKET_PLATFORM_COMMISSION_BPS + MARKET_ROUTER_COMMISSION_BPS must be at most 10000"
        );
    }
    Ok(())
}

fn validate_dodo_config(values: &BTreeMap<String, String>) -> anyhow::Result<()> {
    let api_key = values
        .get("DODO_API_KEY")
        .map(String::as_str)
        .unwrap_or_default()
        .trim();
    let product_id = values
        .get("DODO_PRODUCT_ID")
        .map(String::as_str)
        .unwrap_or_default()
        .trim();
    if !api_key.is_empty() && product_id.is_empty() {
        bail!("DODO_PRODUCT_ID is required when DODO_API_KEY is configured");
    }
    let mock_enabled = values
        .get("DODO_MOCK_CHECKOUT_ENABLED")
        .map(|value| matches!(value.as_str(), "true" | "1" | "yes" | "Yes" | "YES"))
        .unwrap_or(false);
    if !mock_enabled && (api_key.is_empty() || product_id.is_empty()) {
        bail!(
            "DODO_API_KEY and DODO_PRODUCT_ID are required when DODO_MOCK_CHECKOUT_ENABLED=false"
        );
    }
    Ok(())
}

fn validate_decimal(
    values: &BTreeMap<String, String>,
    key: &str,
    positive: bool,
) -> anyhow::Result<()> {
    let parsed = values
        .get(key)
        .map(String::as_str)
        .unwrap_or_default()
        .parse::<rust_decimal::Decimal>()
        .with_context(|| format!("{key} must be a decimal"))?;
    if positive && parsed <= rust_decimal::Decimal::ZERO {
        bail!("{key} must be positive");
    }
    if !positive && parsed < rust_decimal::Decimal::ZERO {
        bail!("{key} must be zero or positive");
    }
    Ok(())
}

fn validate_object_store_config(values: &BTreeMap<String, String>) -> anyhow::Result<()> {
    let backend = values
        .get("OBJECT_STORE_BACKEND")
        .map(String::as_str)
        .unwrap_or("local")
        .trim();
    match backend {
        "local" => Ok(()),
        "r2" => bail!("OBJECT_STORE_BACKEND=r2 is reserved; current binary supports local only"),
        _ => bail!("OBJECT_STORE_BACKEND must be local"),
    }
}

fn validate_market_auth(values: &BTreeMap<String, String>) -> anyhow::Result<()> {
    let cookie_name = values
        .get("MARKET_SESSION_COOKIE_NAME")
        .map(String::as_str)
        .unwrap_or_default();
    if cookie_name.trim().is_empty()
        || !cookie_name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    {
        bail!(
            "MARKET_SESSION_COOKIE_NAME must contain only ASCII letters, digits, hyphen, or underscore"
        );
    }
    let secret = values
        .get("MARKET_SESSION_COOKIE_SECRET")
        .map(String::as_str)
        .unwrap_or_default();
    if secret.len() < 24 {
        bail!("MARKET_SESSION_COOKIE_SECRET must be at least 24 characters");
    }
    let ttl = values
        .get("MARKET_SESSION_TTL_SECS")
        .map(String::as_str)
        .unwrap_or("2592000")
        .parse::<i64>()
        .context("MARKET_SESSION_TTL_SECS must be an integer")?;
    if ttl < 300 {
        bail!("MARKET_SESSION_TTL_SECS must be at least 300");
    }
    Ok(())
}

fn validate_database_config(values: &BTreeMap<String, String>) -> anyhow::Result<()> {
    let turso_url = values
        .get("TURSO_DATABASE_URL")
        .map(String::as_str)
        .unwrap_or_default()
        .trim();
    let turso_token = values
        .get("TURSO_AUTH_TOKEN")
        .map(String::as_str)
        .unwrap_or_default()
        .trim();
    if !turso_url.is_empty() {
        if !turso_url.starts_with("libsql://") {
            bail!("TURSO_DATABASE_URL must start with libsql://");
        }
        if turso_token.is_empty() {
            bail!("TURSO_AUTH_TOKEN is required when TURSO_DATABASE_URL is configured");
        }
    }
    for key in [
        "TURSO_SYNC_INTERVAL_SECS",
        "TURSO_BACKUP_INTERVAL_SECS",
        "TURSO_BACKUP_RETENTION_DAYS",
    ] {
        let value = values
            .get(key)
            .map(String::as_str)
            .unwrap_or_default()
            .trim();
        if value.is_empty() {
            continue;
        }
        let parsed = value
            .parse::<u64>()
            .with_context(|| format!("{key} must be an integer"))?;
        if parsed == 0 {
            bail!("{key} must be positive");
        }
    }
    Ok(())
}

fn validate_http_url(
    values: &BTreeMap<String, String>,
    key: &str,
    required: bool,
) -> anyhow::Result<()> {
    let value = values.get(key).map(String::as_str).unwrap_or_default();
    if value.trim().is_empty() {
        if required {
            bail!("{key} is required");
        }
        return Ok(());
    }
    if !(value.starts_with("http://") || value.starts_with("https://")) {
        bail!("{key} must start with http:// or https://");
    }
    Ok(())
}

fn validate_router_base_domain(values: &BTreeMap<String, String>) -> anyhow::Result<()> {
    let value = values
        .get("ROUTER_BASE_DOMAIN")
        .map(String::as_str)
        .unwrap_or_default()
        .trim();
    if value.is_empty() {
        bail!("ROUTER_BASE_DOMAIN is required");
    }
    if value.starts_with("http://") || value.starts_with("https://") {
        bail!("ROUTER_BASE_DOMAIN must be a domain only, without http:// or https://");
    }
    if value.contains('/') {
        bail!("ROUTER_BASE_DOMAIN must not contain a path");
    }
    Ok(())
}

fn validate_subdomain(values: &BTreeMap<String, String>, key: &str) -> anyhow::Result<()> {
    let value = values.get(key).map(String::as_str).unwrap_or_default();
    if value.trim().is_empty() {
        bail!("{key} is required");
    }
    if !value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
    {
        bail!("{key} may only contain ASCII letters, digits, and hyphen");
    }
    Ok(())
}

fn gateio_field_is_skippable(key: &str, values: &BTreeMap<String, String>) -> bool {
    matches!(
        key,
        "GATEIO_API_BASE"
            | "GATEIO_API_KEY"
            | "GATEIO_API_SECRET"
            | "GATEIO_SETTLEMENT_CURRENCY"
            | "GATEIO_USD_USDT_RATE"
    ) && values
        .get("GATEIO_AUTO_PAYOUT_ENABLED")
        .map(|v| v != "true")
        .unwrap_or(true)
}

fn read_env_file(path: &Path) -> anyhow::Result<BTreeMap<String, String>> {
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let mut values = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            values.insert(key.trim().to_string(), value.trim().to_string());
        }
    }
    Ok(values)
}

fn write_env_file(path: &Path, values: &BTreeMap<String, String>) -> anyhow::Result<()> {
    let mut output = String::new();
    for field in FIELDS {
        let value = values
            .get(field.key)
            .map(String::as_str)
            .unwrap_or(field.default);
        output.push_str(field.key);
        output.push('=');
        output.push_str(value);
        output.push('\n');
    }
    fs::write(path, output).with_context(|| format!("write {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .with_context(|| format!("chmod 0600 {}", path.display()))?;
    }
    Ok(())
}
