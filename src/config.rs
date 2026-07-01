use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, bail};

const DEFAULT_ENV: &str = include_str!("../.env.example");

#[derive(Clone, Debug)]
pub struct Config {
    pub market_http_addr: String,
    pub market_tunnel_enabled: bool,
    pub market_public_base_url: String,
    pub rust_log: String,
    pub market_session_cookie_name: String,
    pub market_session_cookie_secret: String,
    pub market_session_ttl_secs: i64,
    pub market_admin_emails: Vec<String>,
    pub market_min_request_balance: rust_decimal::Decimal,
    pub market_platform_commission_bps: i64,
    pub market_router_commission_bps: i64,
    pub market_share_sticky_enabled: bool,
    pub market_share_sticky_ttl_secs: i64,
    pub cloudflare_turnstile_enabled: bool,
    pub cloudflare_turnstile_site_key: String,
    pub cloudflare_turnstile_secret_key: String,
    pub market_sqlite_path: PathBuf,
    pub turso_database_url: Option<String>,
    pub turso_auth_token: Option<String>,
    pub turso_replica_path: PathBuf,
    pub turso_sync_interval_secs: u64,
    pub turso_backup_enabled: bool,
    pub turso_backup_interval_secs: u64,
    pub turso_backup_retention_days: i64,
    pub object_store_backend: String,
    pub object_store_local_dir: PathBuf,
    pub request_object_retention_days: i64,
    pub request_object_cleanup_batch_size: i64,
    pub r2_account_id: String,
    pub r2_access_key_id: String,
    pub r2_secret_access_key: String,
    pub r2_bucket: String,
    pub r2_public_base_url: String,
    pub router_base_domain: String,
    pub router_market_subdomain: String,
    pub router_api_base_url: String,
    pub dodo_api_base: String,
    pub dodo_api_key: String,
    pub dodo_product_id: String,
    pub dodo_webhook_secret: String,
    pub dodo_mock_checkout_enabled: bool,
    pub dodo_allowed_payment_method_types: Vec<String>,
    pub gateio_api_base: String,
    pub gateio_api_key: String,
    pub gateio_api_secret: String,
    pub gateio_settlement_currency: String,
    pub gateio_usd_usdt_rate: rust_decimal::Decimal,
    pub gateio_auto_payout_enabled: bool,
    pub gateio_payout_worker_interval_secs: u64,
}

impl Config {
    pub fn ensure_default_env_file() -> anyhow::Result<PathBuf> {
        let path = default_env_path()?;
        if !path.exists() {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("create config dir {}", parent.display()))?;
            }
            fs::write(&path, DEFAULT_ENV)
                .with_context(|| format!("write default env file {}", path.display()))?;
        } else {
            append_missing_default_env_keys(&path)?;
        }
        Ok(path)
    }

    pub fn load_default_env_file() -> anyhow::Result<PathBuf> {
        let path = Self::ensure_default_env_file()?;
        dotenvy::from_path(&path).ok();
        Ok(path)
    }

    pub fn from_env() -> Self {
        let router_base_domain = std::env::var("ROUTER_BASE_DOMAIN")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(|value| normalize_domain(&value))
            .unwrap_or_else(|| "localhost:8081".to_string());
        let router_market_subdomain = env("ROUTER_MARKET_SUBDOMAIN", "market");
        let scheme = default_scheme_for_domain(&router_base_domain);
        let router_api_base_url = format!("{scheme}://{router_base_domain}");
        let derived_market_public_base_url =
            format!("{scheme}://{router_market_subdomain}.{router_base_domain}");
        Self {
            market_http_addr: env("MARKET_HTTP_ADDR", "0.0.0.0:8080"),
            market_tunnel_enabled: env_bool("MARKET_TUNNEL_ENABLED", true),
            market_public_base_url: derived_market_public_base_url,
            rust_log: env(
                "RUST_LOG",
                "cc_switch_market=info,tower_http=info,axum=info",
            ),
            market_session_cookie_name: env(
                "MARKET_SESSION_COOKIE_NAME",
                "cc_switch_market_session",
            ),
            market_session_cookie_secret: env(
                "MARKET_SESSION_COOKIE_SECRET",
                "change-me-market-session-secret",
            ),
            market_session_ttl_secs: env("MARKET_SESSION_TTL_SECS", "2592000")
                .parse()
                .unwrap_or(2_592_000),
            market_admin_emails: parse_email_list(&env("MARKET_ADMIN_EMAILS", "admin@example.com")),
            market_min_request_balance: env("MARKET_MIN_REQUEST_BALANCE", "0.10")
                .parse()
                .unwrap_or_else(|_| rust_decimal::Decimal::new(10, 2)),
            market_platform_commission_bps: env("MARKET_PLATFORM_COMMISSION_BPS", "1000")
                .parse()
                .unwrap_or(1000),
            market_router_commission_bps: env("MARKET_ROUTER_COMMISSION_BPS", "500")
                .parse()
                .unwrap_or(500),
            market_share_sticky_enabled: env_bool("MARKET_SHARE_STICKY_ENABLED", true),
            market_share_sticky_ttl_secs: env("MARKET_SHARE_STICKY_TTL_SECONDS", "1800")
                .parse()
                .unwrap_or(1800),
            cloudflare_turnstile_enabled: env_bool("CLOUDFLARE_TURNSTILE_ENABLED", false),
            cloudflare_turnstile_site_key: env("CLOUDFLARE_TURNSTILE_SITE_KEY", ""),
            cloudflare_turnstile_secret_key: env("CLOUDFLARE_TURNSTILE_SECRET_KEY", ""),
            market_sqlite_path: env_path("MARKET_SQLITE_PATH", "cc-switch-market.db"),
            turso_database_url: std::env::var("TURSO_DATABASE_URL")
                .ok()
                .filter(|s| !s.trim().is_empty()),
            turso_auth_token: std::env::var("TURSO_AUTH_TOKEN")
                .ok()
                .filter(|s| !s.trim().is_empty()),
            turso_replica_path: env_path("TURSO_REPLICA_PATH", "turso-replica.db"),
            turso_sync_interval_secs: env("TURSO_SYNC_INTERVAL_SECS", "300")
                .parse()
                .unwrap_or(300),
            turso_backup_enabled: env_bool("TURSO_BACKUP_ENABLED", true),
            turso_backup_interval_secs: env("TURSO_BACKUP_INTERVAL_SECS", "3600")
                .parse()
                .unwrap_or(3600),
            turso_backup_retention_days: env("TURSO_BACKUP_RETENTION_DAYS", "7")
                .parse()
                .unwrap_or(7),
            object_store_backend: env("OBJECT_STORE_BACKEND", "local"),
            object_store_local_dir: env_path("OBJECT_STORE_LOCAL_DIR", "objects"),
            request_object_retention_days: env("REQUEST_OBJECT_RETENTION_DAYS", "7")
                .parse()
                .unwrap_or(7),
            request_object_cleanup_batch_size: env("REQUEST_OBJECT_CLEANUP_BATCH_SIZE", "1000")
                .parse()
                .unwrap_or(1000),
            r2_account_id: env("R2_ACCOUNT_ID", ""),
            r2_access_key_id: env("R2_ACCESS_KEY_ID", ""),
            r2_secret_access_key: env("R2_SECRET_ACCESS_KEY", ""),
            r2_bucket: env("R2_BUCKET", ""),
            r2_public_base_url: env("R2_PUBLIC_BASE_URL", ""),
            router_base_domain,
            router_market_subdomain,
            router_api_base_url,
            dodo_api_base: env("DODO_API_BASE", "https://test.dodopayments.com"),
            dodo_api_key: env("DODO_API_KEY", ""),
            dodo_product_id: env("DODO_PRODUCT_ID", ""),
            dodo_webhook_secret: env("DODO_WEBHOOK_SECRET", "dev"),
            dodo_mock_checkout_enabled: env_bool("DODO_MOCK_CHECKOUT_ENABLED", false),
            dodo_allowed_payment_method_types: parse_csv_list(&env(
                "DODO_ALLOWED_PAYMENT_METHOD_TYPES",
                "credit,debit,apple_pay,google_pay,we_chat_pay,crypto_currency",
            )),
            gateio_api_base: env("GATEIO_API_BASE", "https://api.gateio.ws"),
            gateio_api_key: env("GATEIO_API_KEY", ""),
            gateio_api_secret: env("GATEIO_API_SECRET", ""),
            gateio_settlement_currency: env("GATEIO_SETTLEMENT_CURRENCY", "USDT"),
            gateio_usd_usdt_rate: env("GATEIO_USD_USDT_RATE", "1.0")
                .parse()
                .unwrap_or(rust_decimal::Decimal::ONE),
            gateio_auto_payout_enabled: env("GATEIO_AUTO_PAYOUT_ENABLED", "false") == "true",
            gateio_payout_worker_interval_secs: env("GATEIO_PAYOUT_WORKER_INTERVAL_SECS", "60")
                .parse()
                .unwrap_or(60),
        }
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        self.market_http_addr
            .parse::<std::net::SocketAddr>()
            .with_context(|| format!("invalid MARKET_HTTP_ADDR: {}", self.market_http_addr))?;
        if self.market_session_cookie_name.trim().is_empty()
            || !self
                .market_session_cookie_name
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
        {
            bail!(
                "MARKET_SESSION_COOKIE_NAME must contain only ASCII letters, digits, hyphen, or underscore"
            );
        }
        if self.market_session_cookie_secret.len() < 24 {
            bail!("MARKET_SESSION_COOKIE_SECRET must be at least 24 characters");
        }
        if self.market_session_ttl_secs < 300 {
            bail!("MARKET_SESSION_TTL_SECS must be at least 300");
        }
        if self.market_min_request_balance < rust_decimal::Decimal::ZERO {
            bail!("MARKET_MIN_REQUEST_BALANCE must be zero or positive");
        }
        if !(0..=10_000).contains(&self.market_platform_commission_bps) {
            bail!("MARKET_PLATFORM_COMMISSION_BPS must be between 0 and 10000");
        }
        if !(0..=10_000).contains(&self.market_router_commission_bps) {
            bail!("MARKET_ROUTER_COMMISSION_BPS must be between 0 and 10000");
        }
        if self.market_platform_commission_bps + self.market_router_commission_bps > 10_000 {
            bail!(
                "MARKET_PLATFORM_COMMISSION_BPS + MARKET_ROUTER_COMMISSION_BPS must be at most 10000"
            );
        }
        if self.market_share_sticky_ttl_secs < 0 {
            bail!("MARKET_SHARE_STICKY_TTL_SECONDS must be zero or positive");
        }
        if self.cloudflare_turnstile_enabled()
            && (self.cloudflare_turnstile_site_key.trim().is_empty()
                || self.cloudflare_turnstile_secret_key.trim().is_empty())
        {
            bail!(
                "CLOUDFLARE_TURNSTILE_SITE_KEY and CLOUDFLARE_TURNSTILE_SECRET_KEY are required when CLOUDFLARE_TURNSTILE_ENABLED=true"
            );
        }
        if self
            .market_admin_emails
            .iter()
            .any(|email| !looks_like_email(email))
        {
            bail!("MARKET_ADMIN_EMAILS contains an invalid email");
        }
        if self.router_base_domain.trim().is_empty()
            || self.router_base_domain.contains('/')
            || self.router_base_domain.starts_with("http://")
            || self.router_base_domain.starts_with("https://")
        {
            bail!("ROUTER_BASE_DOMAIN must be a domain only, without scheme or path");
        }
        if self.router_market_subdomain.trim().is_empty()
            || !self
                .router_market_subdomain
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
        {
            bail!("ROUTER_MARKET_SUBDOMAIN may only contain ASCII letters, digits, and hyphen");
        }
        if let Some(url) = self.turso_database_url.as_ref() {
            if !url.starts_with("libsql://") {
                bail!("TURSO_DATABASE_URL must start with libsql://");
            }
            if self
                .turso_auth_token
                .as_ref()
                .map(|token| token.trim().is_empty())
                .unwrap_or(true)
            {
                bail!("TURSO_AUTH_TOKEN is required when TURSO_DATABASE_URL is configured");
            }
        }
        if !matches!(self.object_store_backend.as_str(), "local" | "r2") {
            bail!("OBJECT_STORE_BACKEND must be local; r2 is reserved but not implemented");
        }
        if self.object_store_backend == "r2" {
            bail!("OBJECT_STORE_BACKEND=r2 is reserved; current binary supports local only");
        }
        if self.request_object_retention_days < 1 {
            bail!("REQUEST_OBJECT_RETENTION_DAYS must be at least 1");
        }
        if self.request_object_cleanup_batch_size < 1 {
            bail!("REQUEST_OBJECT_CLEANUP_BATCH_SIZE must be at least 1");
        }
        if !self.dodo_api_key.trim().is_empty() && self.dodo_product_id.trim().is_empty() {
            bail!("DODO_PRODUCT_ID is required when DODO_API_KEY is configured");
        }
        if self.dodo_api_key.trim().is_empty() && !self.dodo_product_id.trim().is_empty() {
            bail!("DODO_API_KEY is required when DODO_PRODUCT_ID is configured");
        }
        validate_dodo_payment_methods(&self.dodo_allowed_payment_method_types)?;
        if self.gateio_usd_usdt_rate <= rust_decimal::Decimal::ZERO {
            bail!("GATEIO_USD_USDT_RATE must be positive");
        }
        if self.gateio_auto_payout_enabled
            && (self.gateio_api_key.trim().is_empty() || self.gateio_api_secret.trim().is_empty())
        {
            bail!(
                "GATEIO_API_KEY and GATEIO_API_SECRET are required when Gate.io auto payout is enabled"
            );
        }
        Ok(())
    }

    pub fn env_report(&self, env_file: &Path) -> Vec<(String, String)> {
        vec![
            ("ENV_FILE".into(), env_file.display().to_string()),
            ("MARKET_HTTP_ADDR".into(), self.market_http_addr.clone()),
            (
                "MARKET_TUNNEL_ENABLED".into(),
                self.market_tunnel_enabled.to_string(),
            ),
            (
                "MARKET_PUBLIC_BASE_URL".into(),
                self.market_public_base_url.clone(),
            ),
            ("RUST_LOG".into(), self.rust_log.clone()),
            (
                "MARKET_SESSION_COOKIE_NAME".into(),
                self.market_session_cookie_name.clone(),
            ),
            (
                "MARKET_SESSION_COOKIE_SECRET".into(),
                self.market_session_cookie_secret.clone(),
            ),
            (
                "MARKET_SESSION_TTL_SECS".into(),
                self.market_session_ttl_secs.to_string(),
            ),
            (
                "MARKET_ADMIN_EMAILS".into(),
                self.market_admin_emails.join(","),
            ),
            (
                "MARKET_MIN_REQUEST_BALANCE".into(),
                self.market_min_request_balance.to_string(),
            ),
            (
                "MARKET_PLATFORM_COMMISSION_BPS".into(),
                self.market_platform_commission_bps.to_string(),
            ),
            (
                "MARKET_ROUTER_COMMISSION_BPS".into(),
                self.market_router_commission_bps.to_string(),
            ),
            (
                "MARKET_SHARE_STICKY_ENABLED".into(),
                self.market_share_sticky_enabled.to_string(),
            ),
            (
                "MARKET_SHARE_STICKY_TTL_SECONDS".into(),
                self.market_share_sticky_ttl_secs.to_string(),
            ),
            (
                "CLOUDFLARE_TURNSTILE_ENABLED".into(),
                self.cloudflare_turnstile_enabled.to_string(),
            ),
            (
                "CLOUDFLARE_TURNSTILE_SITE_KEY".into(),
                self.cloudflare_turnstile_site_key.clone(),
            ),
            (
                "CLOUDFLARE_TURNSTILE_SECRET_KEY".into(),
                self.cloudflare_turnstile_secret_key.clone(),
            ),
            (
                "MARKET_SQLITE_PATH".into(),
                self.market_sqlite_path.display().to_string(),
            ),
            (
                "TURSO_DATABASE_URL".into(),
                self.turso_database_url.clone().unwrap_or_default(),
            ),
            (
                "TURSO_AUTH_TOKEN".into(),
                self.turso_auth_token.clone().unwrap_or_default(),
            ),
            (
                "TURSO_REPLICA_PATH".into(),
                self.turso_replica_path.display().to_string(),
            ),
            (
                "TURSO_SYNC_INTERVAL_SECS".into(),
                self.turso_sync_interval_secs.to_string(),
            ),
            (
                "TURSO_BACKUP_ENABLED".into(),
                self.turso_backup_enabled.to_string(),
            ),
            (
                "TURSO_BACKUP_INTERVAL_SECS".into(),
                self.turso_backup_interval_secs.to_string(),
            ),
            (
                "TURSO_BACKUP_RETENTION_DAYS".into(),
                self.turso_backup_retention_days.to_string(),
            ),
            (
                "OBJECT_STORE_BACKEND".into(),
                self.object_store_backend.clone(),
            ),
            (
                "OBJECT_STORE_LOCAL_DIR".into(),
                self.object_store_local_dir.display().to_string(),
            ),
            (
                "REQUEST_OBJECT_RETENTION_DAYS".into(),
                self.request_object_retention_days.to_string(),
            ),
            (
                "REQUEST_OBJECT_CLEANUP_BATCH_SIZE".into(),
                self.request_object_cleanup_batch_size.to_string(),
            ),
            ("R2_ACCOUNT_ID".into(), self.r2_account_id.clone()),
            ("R2_ACCESS_KEY_ID".into(), self.r2_access_key_id.clone()),
            (
                "R2_SECRET_ACCESS_KEY".into(),
                self.r2_secret_access_key.clone(),
            ),
            ("R2_BUCKET".into(), self.r2_bucket.clone()),
            ("R2_PUBLIC_BASE_URL".into(), self.r2_public_base_url.clone()),
            ("ROUTER_BASE_DOMAIN".into(), self.router_base_domain.clone()),
            (
                "ROUTER_MARKET_SUBDOMAIN".into(),
                self.router_market_subdomain.clone(),
            ),
            (
                "DODO_WEBHOOK_SECRET".into(),
                self.dodo_webhook_secret.clone(),
            ),
            (
                "DODO_MOCK_CHECKOUT_ENABLED".into(),
                self.dodo_mock_checkout_enabled.to_string(),
            ),
            ("DODO_API_BASE".into(), self.dodo_api_base.clone()),
            ("DODO_API_KEY".into(), self.dodo_api_key.clone()),
            ("DODO_PRODUCT_ID".into(), self.dodo_product_id.clone()),
            (
                "DODO_ALLOWED_PAYMENT_METHOD_TYPES".into(),
                self.dodo_allowed_payment_method_types.join(","),
            ),
            ("GATEIO_API_BASE".into(), self.gateio_api_base.clone()),
            ("GATEIO_API_KEY".into(), self.gateio_api_key.clone()),
            ("GATEIO_API_SECRET".into(), self.gateio_api_secret.clone()),
            (
                "GATEIO_SETTLEMENT_CURRENCY".into(),
                self.gateio_settlement_currency.clone(),
            ),
            (
                "GATEIO_USD_USDT_RATE".into(),
                self.gateio_usd_usdt_rate.to_string(),
            ),
            (
                "GATEIO_AUTO_PAYOUT_ENABLED".into(),
                self.gateio_auto_payout_enabled.to_string(),
            ),
            (
                "GATEIO_PAYOUT_WORKER_INTERVAL_SECS".into(),
                self.gateio_payout_worker_interval_secs.to_string(),
            ),
        ]
    }

    pub fn router_commission_owner_email(&self) -> String {
        let host = url::Url::parse(&self.router_api_base_url)
            .ok()
            .and_then(|url| url.host_str().map(ToOwned::to_owned))
            .unwrap_or_else(|| {
                self.router_base_domain
                    .split('/')
                    .next()
                    .unwrap_or(self.router_base_domain.as_str())
                    .split(':')
                    .next()
                    .unwrap_or(self.router_base_domain.as_str())
                    .to_string()
            });
        format!("router@{}", host.trim().trim_matches('.'))
    }

    pub fn cloudflare_turnstile_enabled(&self) -> bool {
        self.cloudflare_turnstile_enabled
    }
}

fn env(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn env_bool(key: &str, default: bool) -> bool {
    std::env::var(key)
        .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
        .unwrap_or(default)
}

fn env_path(key: &str, default_file: &str) -> PathBuf {
    std::env::var_os(key)
        .map(|value| expand_home_path(&PathBuf::from(value)))
        .filter(|value| !value.as_os_str().is_empty())
        .unwrap_or_else(|| {
            config_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(default_file)
        })
}

fn expand_home_path(path: &Path) -> PathBuf {
    let Some(raw) = path.to_str() else {
        return path.to_path_buf();
    };
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return path.to_path_buf();
    };
    if raw == "$HOME" || raw == "~" {
        return home;
    }
    if let Some(rest) = raw.strip_prefix("$HOME/") {
        return home.join(rest);
    }
    if let Some(rest) = raw.strip_prefix("~/") {
        return home.join(rest);
    }
    path.to_path_buf()
}

fn parse_email_list(value: &str) -> Vec<String> {
    parse_csv_list(value)
        .into_iter()
        .map(|item| item.to_ascii_lowercase())
        .collect()
}

fn parse_csv_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect()
}

fn looks_like_email(value: &str) -> bool {
    let (local, domain) = match value.split_once('@') {
        Some(parts) => parts,
        None => return false,
    };
    !local.trim().is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
}

fn validate_dodo_payment_methods(values: &[String]) -> anyhow::Result<()> {
    if values.is_empty() {
        bail!("DODO_ALLOWED_PAYMENT_METHOD_TYPES must not be empty");
    }
    let allowed = [
        "ach",
        "affirm",
        "afterpay_clearpay",
        "alfamart",
        "ali_pay",
        "ali_pay_hk",
        "alma",
        "amazon_pay",
        "apple_pay",
        "atome",
        "bacs",
        "bancontact_card",
        "becs",
        "benefit",
        "bizum",
        "blik",
        "boleto",
        "bca_bank_transfer",
        "bni_va",
        "bri_va",
        "card_redirect",
        "cimb_va",
        "classic",
        "credit",
        "crypto_currency",
        "cashapp",
        "dana",
        "danamon_va",
        "debit",
        "duit_now",
        "efecty",
        "eft",
        "eps",
        "fps",
        "evoucher",
        "giropay",
        "givex",
        "google_pay",
        "go_pay",
        "gcash",
        "ideal",
        "interac",
        "indomaret",
        "klarna",
        "kakao_pay",
        "local_bank_redirect",
        "mandiri_va",
        "knet",
        "mb_way",
        "mobile_pay",
        "momo",
        "momo_atm",
        "multibanco",
        "online_banking_thailand",
        "online_banking_czech_republic",
        "online_banking_finland",
        "online_banking_fpx",
        "online_banking_poland",
        "online_banking_slovakia",
        "oxxo",
        "pago_efectivo",
        "permata_bank_transfer",
        "open_banking_uk",
        "pay_bright",
        "paypal",
        "paze",
        "pix",
        "pay_safe_card",
        "przelewy24",
        "prompt_pay",
        "pse",
        "red_compra",
        "red_pagos",
        "samsung_pay",
        "sepa",
        "sepa_bank_transfer",
        "sofort",
        "swish",
        "touch_n_go",
        "trustly",
        "twint",
        "upi_collect",
        "upi_intent",
        "vipps",
        "viet_qr",
        "venmo",
        "walley",
        "we_chat_pay",
        "seven_eleven",
        "lawson",
        "mini_stop",
        "family_mart",
        "seicomart",
        "pay_easy",
        "local_bank_transfer",
        "mifinity",
        "open_banking_pis",
        "direct_carrier_billing",
        "instant_bank_transfer",
        "billie",
        "zip",
        "revolut_pay",
        "naver_pay",
        "payco",
    ];
    for value in values {
        if !allowed.contains(&value.as_str()) {
            bail!("unsupported DODO_ALLOWED_PAYMENT_METHOD_TYPES value: {value}");
        }
    }
    Ok(())
}

fn normalize_domain(value: &str) -> String {
    value
        .trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/')
        .to_string()
}

fn default_scheme_for_domain(domain: &str) -> &'static str {
    let host = domain.split(':').next().unwrap_or(domain);
    if matches!(host, "localhost" | "127.0.0.1" | "0.0.0.0") {
        "http"
    } else {
        "https"
    }
}

fn default_env_path() -> anyhow::Result<PathBuf> {
    Ok(config_dir()?.join(".env"))
}

fn append_missing_default_env_keys(path: &Path) -> anyhow::Result<()> {
    let current = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let existing = current
        .lines()
        .filter_map(|line| line.split_once('='))
        .map(|(key, _)| key.trim())
        .collect::<std::collections::HashSet<_>>();
    let missing = DEFAULT_ENV
        .lines()
        .filter(|line| {
            line.split_once('=')
                .map(|(key, _)| !existing.contains(key.trim()))
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return Ok(());
    }
    let mut output = current;
    if !output.ends_with('\n') {
        output.push('\n');
    }
    output.push('\n');
    output.push_str("# Added by newer cc-switch-market defaults\n");
    for line in missing {
        output.push_str(line);
        output.push('\n');
    }
    fs::write(path, output).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

pub fn config_dir() -> anyhow::Result<PathBuf> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME environment variable is not set")?;
    Ok(home.join(".config").join("cc-switch-market"))
}
