use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    str::FromStr,
    sync::{Arc, Mutex as StdMutex},
};

use anyhow::Context;
use chrono::{DateTime, Utc};
use libsql::{Builder, Connection, Database, Row, Transaction, TransactionBehavior, Value};
use rust_decimal::Decimal;
use serde_json::Value as JsonValue;
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};
use uuid::Uuid;

use crate::config::Config;

#[derive(Clone)]
pub struct Db {
    database: Arc<Database>,
    mode: DbMode,
    write_lock: Arc<AsyncMutex<()>>,
    last_backup_at: Arc<StdMutex<Option<DateTime<Utc>>>>,
}

#[derive(Clone)]
pub enum DbMode {
    Local { path: PathBuf },
    Turso { url: String, replica_path: PathBuf },
}

pub struct DbTx {
    tx: Transaction,
    _write_guard: OwnedMutexGuard<()>,
}

#[derive(Clone, Debug)]
pub struct DbRow {
    columns: HashMap<String, Value>,
}

pub async fn connect(config: &Config) -> anyhow::Result<Db> {
    if let Some(url) = config.turso_database_url.as_ref() {
        let token = config
            .turso_auth_token
            .as_ref()
            .filter(|value| !value.trim().is_empty())
            .context("TURSO_AUTH_TOKEN is required when TURSO_DATABASE_URL is configured")?;
        ensure_parent(&config.turso_replica_path)?;
        let database =
            Builder::new_remote_replica(&config.turso_replica_path, url.clone(), token.clone())
                .build()
                .await?;
        return Ok(Db {
            database: Arc::new(database),
            mode: DbMode::Turso {
                url: url.clone(),
                replica_path: config.turso_replica_path.clone(),
            },
            write_lock: Arc::new(AsyncMutex::new(())),
            last_backup_at: Arc::new(StdMutex::new(None)),
        });
    }

    ensure_parent(&config.market_sqlite_path)?;
    let database = Builder::new_local(&config.market_sqlite_path)
        .build()
        .await?;
    let db = Db {
        database: Arc::new(database),
        mode: DbMode::Local {
            path: config.market_sqlite_path.clone(),
        },
        write_lock: Arc::new(AsyncMutex::new(())),
        last_backup_at: Arc::new(StdMutex::new(None)),
    };
    db.configure_local_connection().await?;
    Ok(db)
}

pub async fn migrate(db: &Db) -> anyhow::Result<()> {
    db.execute_batch(SCHEMA).await?;
    additive_migrations(db).await?;
    migrate_model_prices_to_official(db).await?;
    seed_defaults(db).await?;
    backfill_model_price_models(db).await?;
    Ok(())
}

async fn additive_migrations(db: &Db) -> anyhow::Result<()> {
    for sql in [
        "ALTER TABLE ticket_attachments ADD COLUMN uploader_user_id TEXT",
        "ALTER TABLE ticket_attachments ADD COLUMN uploader_email TEXT",
        "ALTER TABLE ticket_attachments ADD COLUMN reference_type TEXT",
        "ALTER TABLE ticket_attachments ADD COLUMN reference_id TEXT",
        "ALTER TABLE request_charges ADD COLUMN request_object_sha256 TEXT",
        "ALTER TABLE request_charges ADD COLUMN response_meta_object_sha256 TEXT",
        "ALTER TABLE processed_webhooks ADD COLUMN raw_payload_sha256 TEXT",
        "ALTER TABLE payout_requests ADD COLUMN proof_object_sha256 TEXT",
        "ALTER TABLE payout_requests ADD COLUMN gateio_request_object_sha256 TEXT",
        "ALTER TABLE payout_requests ADD COLUMN gateio_response_object_sha256 TEXT",
        "ALTER TABLE router_shares ADD COLUMN last_success_at TEXT",
        "ALTER TABLE router_shares ADD COLUMN last_error_at TEXT",
        "ALTER TABLE router_shares ADD COLUMN last_error_message TEXT",
        "ALTER TABLE router_shares ADD COLUMN last_failure_kind TEXT",
        "ALTER TABLE router_shares ADD COLUMN last_failure_scope TEXT",
        "ALTER TABLE router_shares ADD COLUMN failure_count INTEGER DEFAULT 0",
        "ALTER TABLE router_shares ADD COLUMN cooldown_until TEXT",
        "ALTER TABLE router_shares ADD COLUMN subdomain TEXT",
        "ALTER TABLE router_shares ADD COLUMN enabled_claude INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE router_shares ADD COLUMN enabled_codex INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE router_shares ADD COLUMN enabled_gemini INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE router_shares ADD COLUMN disabled_by_market INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE router_shares ADD COLUMN market_disabled_at TEXT",
        // Router-computed scheduling signals; refreshed every share sync.
        // Defaults are intentionally generous (1.0 / 0.5) so a freshly-migrated
        // row doesn't get punished before the first sync overwrites them.
        "ALTER TABLE router_shares ADD COLUMN quota_health REAL NOT NULL DEFAULT 0.5",
        "ALTER TABLE router_shares ADD COLUMN stability REAL NOT NULL DEFAULT 1.0",
        "ALTER TABLE router_shares ADD COLUMN headroom REAL NOT NULL DEFAULT 1.0",
        "ALTER TABLE router_shares ADD COLUMN samples_10m INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE router_shares ADD COLUMN owner_penalty REAL NOT NULL DEFAULT 1.0",
        "ALTER TABLE router_shares ADD COLUMN share_created_at TEXT",
        "ALTER TABLE api_keys ADD COLUMN paused_at TEXT",
        "ALTER TABLE api_keys ADD COLUMN deleted_at TEXT",
        "ALTER TABLE market_share_sticky_routes ADD COLUMN api_key_id TEXT",
        "ALTER TABLE models ADD COLUMN model_pattern TEXT",
        "ALTER TABLE models ADD COLUMN display_name TEXT",
        "ALTER TABLE models ADD COLUMN status TEXT DEFAULT 'active'",
        "ALTER TABLE models ADD COLUMN is_public INTEGER DEFAULT 1",
        "ALTER TABLE models ADD COLUMN sort_order INTEGER DEFAULT 0",
        "ALTER TABLE model_prices ADD COLUMN model_id TEXT",
        "ALTER TABLE request_charges ADD COLUMN model_id TEXT",
        "ALTER TABLE request_charges ADD COLUMN routing_rule_id TEXT",
        "ALTER TABLE request_charges ADD COLUMN pricing_model TEXT",
        "ALTER TABLE request_charges ADD COLUMN pricing_slot TEXT",
        "ALTER TABLE request_charges ADD COLUMN pricing_model_source TEXT",
        "ALTER TABLE request_charges ADD COLUMN share_official INTEGER DEFAULT 0",
        "ALTER TABLE request_charges ADD COLUMN request_agent TEXT",
        "ALTER TABLE request_charges ADD COLUMN requested_model TEXT",
        "ALTER TABLE request_charges ADD COLUMN actual_model TEXT",
        "ALTER TABLE request_charges ADD COLUMN actual_model_source TEXT",
        "ALTER TABLE topup_orders ADD COLUMN payment_method_type TEXT",
    ] {
        if let Err(err) = db.execute(sql, vec![]).await {
            let message = err.to_string().to_ascii_lowercase();
            if !message.contains("duplicate column") {
                anyhow::bail!("run migration {sql}: {err}");
            }
        }
    }
    db.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS payout_attempts (
          id TEXT PRIMARY KEY,
          payout_request_id TEXT NOT NULL REFERENCES payout_requests(id),
          method TEXT NOT NULL,
          status TEXT NOT NULL,
          request_object_key TEXT,
          request_object_sha256 TEXT,
          response_object_key TEXT,
          response_object_sha256 TEXT,
          external_tx_id TEXT,
          error_message TEXT,
          created_at TEXT NOT NULL,
          completed_at TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_payout_attempts_payout ON payout_attempts(payout_request_id, created_at DESC);

        CREATE TABLE IF NOT EXISTS model_routing_rules (
          id TEXT PRIMARY KEY,
          model_id TEXT NOT NULL REFERENCES models(id),
          mode TEXT NOT NULL DEFAULT 'all',
          priority INTEGER NOT NULL DEFAULT 0,
          enabled INTEGER NOT NULL DEFAULT 1,
          notes TEXT,
          created_at TEXT NOT NULL,
          updated_at TEXT NOT NULL,
          UNIQUE(model_id)
        );
        CREATE TABLE IF NOT EXISTS model_routing_rule_shares (
          rule_id TEXT NOT NULL REFERENCES model_routing_rules(id),
          router_id TEXT NOT NULL,
          share_id TEXT NOT NULL,
          created_at TEXT NOT NULL,
          PRIMARY KEY(rule_id, router_id, share_id)
        );
        CREATE TABLE IF NOT EXISTS request_attempts (
          id TEXT PRIMARY KEY,
          request_id TEXT NOT NULL,
          charge_id TEXT,
          attempt_no INTEGER NOT NULL,
          router_id TEXT NOT NULL,
          share_id TEXT NOT NULL,
          model_id TEXT,
          status TEXT NOT NULL,
          failure_kind TEXT,
          error_message TEXT,
          latency_ms INTEGER,
          started_at TEXT NOT NULL,
          finished_at TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_request_attempts_request ON request_attempts(request_id, attempt_no);
        CREATE TABLE IF NOT EXISTS model_share_blocks (
          model_id TEXT NOT NULL REFERENCES models(id),
          router_id TEXT NOT NULL,
          share_id TEXT NOT NULL,
          reason TEXT NOT NULL,
          expires_at TEXT NOT NULL,
          created_at TEXT NOT NULL,
          PRIMARY KEY(model_id, router_id, share_id)
        );
        CREATE INDEX IF NOT EXISTS idx_model_share_blocks_expires ON model_share_blocks(expires_at);
        CREATE TABLE IF NOT EXISTS market_share_capability_blocks (
          router_id TEXT NOT NULL,
          share_id TEXT NOT NULL,
          capability TEXT NOT NULL CHECK(capability IN ('claude','codex','gemini')),
          reason TEXT,
          created_by TEXT,
          created_at TEXT NOT NULL,
          PRIMARY KEY(router_id, share_id, capability)
        );
        CREATE TABLE IF NOT EXISTS market_share_sticky_routes (
          sticky_key TEXT PRIMARY KEY,
          api_key_id TEXT,
          user_id TEXT NOT NULL,
          app_type TEXT NOT NULL,
          model_id TEXT NOT NULL,
          protocol_family TEXT NOT NULL,
          router_id TEXT NOT NULL,
          share_id TEXT NOT NULL,
          expires_at TEXT NOT NULL,
          last_success_at TEXT,
          created_at TEXT NOT NULL,
          updated_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_market_share_sticky_routes_expires ON market_share_sticky_routes(expires_at);
        CREATE INDEX IF NOT EXISTS idx_market_share_sticky_routes_api_key ON market_share_sticky_routes(api_key_id);
        CREATE TABLE IF NOT EXISTS market_response_sticky_routes (
          response_id TEXT PRIMARY KEY,
          sticky_key TEXT NOT NULL,
          api_key_id TEXT,
          user_id TEXT NOT NULL,
          app_type TEXT NOT NULL,
          model_id TEXT NOT NULL,
          protocol_family TEXT NOT NULL,
          router_id TEXT NOT NULL,
          share_id TEXT NOT NULL,
          expires_at TEXT NOT NULL,
          created_at TEXT NOT NULL,
          updated_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_market_response_sticky_routes_expires ON market_response_sticky_routes(expires_at);
        CREATE INDEX IF NOT EXISTS idx_market_response_sticky_routes_sticky ON market_response_sticky_routes(sticky_key);
        CREATE TABLE IF NOT EXISTS market_api_key_share_allowlist (
          api_key_id TEXT NOT NULL REFERENCES api_keys(id),
          router_id TEXT NOT NULL,
          share_id TEXT NOT NULL,
          created_at TEXT NOT NULL,
          PRIMARY KEY(api_key_id, router_id, share_id)
        );
        CREATE INDEX IF NOT EXISTS idx_market_api_key_share_allowlist_key ON market_api_key_share_allowlist(api_key_id);
        CREATE INDEX IF NOT EXISTS idx_users_created_at ON users(created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_wallet_accounts_type ON wallet_accounts(account_type);
        CREATE INDEX IF NOT EXISTS idx_ledger_from_account_created ON ledger_entries(from_account_id, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_ledger_to_account_created ON ledger_entries(to_account_id, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_topup_orders_created_at ON topup_orders(created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_topup_orders_status_created ON topup_orders(status, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_topup_orders_status_paid ON topup_orders(status, paid_at DESC);
        CREATE INDEX IF NOT EXISTS idx_processed_webhooks_created_at ON processed_webhooks(created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_router_shares_last_seen ON router_shares(last_seen_at DESC);
        CREATE INDEX IF NOT EXISTS idx_request_charges_created_at ON request_charges(created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_request_charges_status_created ON request_charges(status, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_request_charges_user_created ON request_charges(user_id, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_tickets_updated_at ON tickets(updated_at DESC);
        CREATE INDEX IF NOT EXISTS idx_tickets_status_updated ON tickets(status, updated_at DESC);
        CREATE INDEX IF NOT EXISTS idx_ticket_messages_ticket_created ON ticket_messages(ticket_id, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_admin_audit_created_at ON admin_audit(created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_payout_requests_status_created ON payout_requests(status, created_at DESC);
        CREATE TABLE IF NOT EXISTS app_settings (
          key TEXT PRIMARY KEY,
          value TEXT NOT NULL,
          updated_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS internal_migrations (
          name TEXT PRIMARY KEY,
          applied_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS model_vendor_discounts (
          app_type TEXT PRIMARY KEY,
          discount_percent TEXT NOT NULL DEFAULT '10',
          updated_at TEXT NOT NULL
        );
        "#
    ).await?;
    Ok(())
}

async fn migrate_model_prices_to_official(db: &Db) -> anyhow::Result<()> {
    let marker = "model_prices_store_official_prices_v1";
    let already_applied = db
        .query_optional(
            "SELECT name FROM internal_migrations WHERE name=?1 LIMIT 1",
            vec![val(marker)],
        )
        .await?
        .is_some();
    if !already_applied {
        let now = now_string();
        let rows = db
            .query_all(
                "SELECT id, input_per_million, output_per_million, cache_read_per_million, cache_write_per_million FROM model_prices",
                vec![],
            )
            .await?;
        for row in rows {
            db.execute(
                r#"
                UPDATE model_prices
                   SET input_per_million = ?2,
                       output_per_million = ?3,
                       cache_read_per_million = ?4,
                       cache_write_per_million = ?5,
                       updated_at = ?6
                 WHERE id = ?1
                "#,
                vec![
                    val(row.string("id")),
                    dec_val(row.decimal("input_per_million") * Decimal::TEN),
                    dec_val(row.decimal("output_per_million") * Decimal::TEN),
                    dec_val(row.decimal("cache_read_per_million") * Decimal::TEN),
                    dec_val(row.decimal("cache_write_per_million") * Decimal::TEN),
                    val(&now),
                ],
            )
            .await?;
        }
        db.execute(
            "INSERT INTO internal_migrations (name, applied_at) VALUES (?1, ?2)",
            vec![val(marker), val(&now)],
        )
        .await?;
    }
    ensure_model_vendor_discounts(db).await?;
    Ok(())
}

pub fn backup_dir() -> anyhow::Result<PathBuf> {
    Ok(crate::config::config_dir()?.join("turso-db-backup"))
}

pub fn spawn_turso_backup(config: Config, db: Db) -> Option<tokio::task::JoinHandle<()>> {
    if !matches!(db.mode, DbMode::Turso { .. }) || !config.turso_backup_enabled {
        return None;
    }
    Some(tokio::spawn(async move {
        let interval = std::time::Duration::from_secs(config.turso_backup_interval_secs.max(60));
        loop {
            if let Err(err) = backup_turso_once(&config, &db).await {
                tracing::warn!(error = %err, "turso backup failed");
            }
            tokio::time::sleep(interval).await;
        }
    }))
}

pub fn spawn_turso_sync(config: Config, db: Db) -> Option<tokio::task::JoinHandle<()>> {
    if !matches!(db.mode, DbMode::Turso { .. }) {
        return None;
    }
    Some(tokio::spawn(async move {
        let interval = std::time::Duration::from_secs(config.turso_sync_interval_secs.max(30));
        loop {
            tokio::time::sleep(interval).await;
            if let Err(err) = db.sync().await {
                tracing::warn!(error = %err, "turso replica sync failed");
            }
        }
    }))
}

async fn backup_turso_once(config: &Config, db: &Db) -> anyhow::Result<()> {
    db.sync().await?;
    let source = match &db.mode {
        DbMode::Turso { replica_path, .. } => replica_path,
        DbMode::Local { .. } => return Ok(()),
    };
    let dir = backup_dir()?;
    fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    let stamp = Utc::now().format("%Y%m%d-%H%M%S").to_string();
    let target = dir.join(format!("cc-switch-market-{stamp}.db"));
    fs::copy(source, &target)
        .with_context(|| format!("copy {} to {}", source.display(), target.display()))?;
    *db.last_backup_at.lock().expect("backup mutex") = Some(Utc::now());
    cleanup_old_backups(&dir, config.turso_backup_retention_days)?;
    Ok(())
}

fn cleanup_old_backups(dir: &Path, retention_days: i64) -> anyhow::Result<()> {
    let cutoff = std::time::SystemTime::now()
        - std::time::Duration::from_secs((retention_days.max(1) as u64) * 24 * 60 * 60);
    for entry in fs::read_dir(dir).with_context(|| format!("read {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("db") {
            continue;
        }
        if entry.metadata()?.modified()? < cutoff {
            let _ = fs::remove_file(path);
        }
    }
    Ok(())
}

impl Db {
    pub fn mode_name(&self) -> &'static str {
        match self.mode {
            DbMode::Local { .. } => "local_sqlite",
            DbMode::Turso { .. } => "turso",
        }
    }

    pub fn path_for_log(&self) -> String {
        match &self.mode {
            DbMode::Local { path } => path.display().to_string(),
            DbMode::Turso { replica_path, .. } => replica_path.display().to_string(),
        }
    }

    pub fn database_url_for_log(&self) -> Option<&str> {
        match &self.mode {
            DbMode::Local { .. } => None,
            DbMode::Turso { url, .. } => Some(url),
        }
    }

    pub fn last_backup_at(&self) -> Option<DateTime<Utc>> {
        *self.last_backup_at.lock().expect("backup mutex")
    }

    pub async fn sync(&self) -> anyhow::Result<()> {
        if matches!(self.mode, DbMode::Turso { .. }) {
            self.database.sync().await?;
        }
        Ok(())
    }

    pub fn conn(&self) -> Result<Connection, libsql::Error> {
        self.database.connect()
    }

    async fn configured_conn(&self) -> Result<Connection, libsql::Error> {
        let conn = self.conn()?;
        if matches!(self.mode, DbMode::Local { .. }) {
            conn.execute_batch("PRAGMA busy_timeout = 5000;").await?;
        }
        Ok(conn)
    }

    async fn configure_local_connection(&self) -> Result<(), libsql::Error> {
        if matches!(self.mode, DbMode::Local { .. }) {
            let conn = self.conn()?;
            conn.execute_batch(
                r#"
                PRAGMA journal_mode = WAL;
                PRAGMA busy_timeout = 5000;
                PRAGMA foreign_keys = ON;
                "#,
            )
            .await?;
        }
        Ok(())
    }

    pub async fn execute_batch(&self, sql: &str) -> Result<(), libsql::Error> {
        let _guard = self.write_lock.lock().await;
        self.configured_conn().await?.execute_batch(sql).await?;
        Ok(())
    }

    pub async fn execute(&self, sql: &str, params: Vec<Value>) -> Result<u64, libsql::Error> {
        let _guard = self.write_lock.lock().await;
        self.configured_conn().await?.execute(sql, params).await
    }

    pub async fn query_all(
        &self,
        sql: &str,
        params: Vec<Value>,
    ) -> Result<Vec<DbRow>, libsql::Error> {
        rows_to_vec(self.configured_conn().await?.query(sql, params).await?).await
    }

    pub async fn query_optional(
        &self,
        sql: &str,
        params: Vec<Value>,
    ) -> Result<Option<DbRow>, libsql::Error> {
        let mut rows = self.configured_conn().await?.query(sql, params).await?;
        rows.next().await?.map(row_to_map).transpose()
    }

    pub async fn query_one(&self, sql: &str, params: Vec<Value>) -> Result<DbRow, libsql::Error> {
        self.query_optional(sql, params)
            .await?
            .ok_or(libsql::Error::QueryReturnedNoRows)
    }

    pub async fn begin_immediate(&self) -> Result<DbTx, libsql::Error> {
        let write_guard = self.write_lock.clone().lock_owned().await;
        let conn = self.configured_conn().await?;
        Ok(DbTx {
            tx: conn
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .await?,
            _write_guard: write_guard,
        })
    }
}

impl DbTx {
    pub async fn execute(&self, sql: &str, params: Vec<Value>) -> Result<u64, libsql::Error> {
        self.tx.execute(sql, params).await
    }

    pub async fn query_all(
        &self,
        sql: &str,
        params: Vec<Value>,
    ) -> Result<Vec<DbRow>, libsql::Error> {
        rows_to_vec(self.tx.query(sql, params).await?).await
    }

    pub async fn query_optional(
        &self,
        sql: &str,
        params: Vec<Value>,
    ) -> Result<Option<DbRow>, libsql::Error> {
        let mut rows = self.tx.query(sql, params).await?;
        rows.next().await?.map(row_to_map).transpose()
    }

    pub async fn query_one(&self, sql: &str, params: Vec<Value>) -> Result<DbRow, libsql::Error> {
        self.query_optional(sql, params)
            .await?
            .ok_or(libsql::Error::QueryReturnedNoRows)
    }

    pub async fn commit(self) -> Result<(), libsql::Error> {
        self.tx.commit().await
    }
}

async fn rows_to_vec(mut rows: libsql::Rows) -> Result<Vec<DbRow>, libsql::Error> {
    let mut out = Vec::new();
    while let Some(row) = rows.next().await? {
        out.push(row_to_map(row)?);
    }
    Ok(out)
}

fn row_to_map(row: Row) -> Result<DbRow, libsql::Error> {
    let mut columns = HashMap::new();
    for idx in 0..row.column_count() {
        let name = row
            .column_name(idx)
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| idx.to_string());
        columns.insert(name, row.get_value(idx)?);
    }
    Ok(DbRow { columns })
}

impl DbRow {
    pub fn to_json(&self) -> JsonValue {
        let mut object = serde_json::Map::new();
        for (key, value) in &self.columns {
            object.insert(key.clone(), value_to_json(value));
        }
        JsonValue::Object(object)
    }

    pub fn string(&self, key: &str) -> String {
        match self.columns.get(key) {
            Some(Value::Text(value)) => value.clone(),
            Some(Value::Integer(value)) => value.to_string(),
            Some(Value::Real(value)) => value.to_string(),
            _ => String::new(),
        }
    }

    pub fn opt_string(&self, key: &str) -> Option<String> {
        match self.columns.get(key) {
            Some(Value::Text(value)) => Some(value.clone()),
            Some(Value::Integer(value)) => Some(value.to_string()),
            Some(Value::Real(value)) => Some(value.to_string()),
            _ => None,
        }
    }

    pub fn uuid(&self, key: &str) -> Uuid {
        Uuid::parse_str(&self.string(key)).expect("valid uuid in database")
    }

    #[allow(dead_code)]
    pub fn opt_uuid(&self, key: &str) -> Option<Uuid> {
        self.opt_string(key)
            .and_then(|value| Uuid::parse_str(&value).ok())
    }

    pub fn decimal(&self, key: &str) -> Decimal {
        Decimal::from_str(&self.string(key)).unwrap_or(Decimal::ZERO)
    }

    #[allow(dead_code)]
    pub fn opt_decimal(&self, key: &str) -> Option<Decimal> {
        self.opt_string(key)
            .and_then(|value| Decimal::from_str(&value).ok())
    }

    #[allow(dead_code)]
    pub fn i64(&self, key: &str) -> i64 {
        match self.columns.get(key) {
            Some(Value::Integer(value)) => *value,
            Some(Value::Text(value)) => value.parse().unwrap_or_default(),
            _ => 0,
        }
    }

    #[allow(dead_code)]
    pub fn bool(&self, key: &str) -> bool {
        self.i64(key) != 0
    }

    pub fn datetime(&self, key: &str) -> DateTime<Utc> {
        self.opt_datetime(key).unwrap_or_else(Utc::now)
    }

    pub fn opt_datetime(&self, key: &str) -> Option<DateTime<Utc>> {
        self.opt_string(key)
            .and_then(|value| DateTime::parse_from_rfc3339(&value).ok())
            .map(|value| value.with_timezone(&Utc))
    }

    pub fn json(&self, key: &str) -> JsonValue {
        self.opt_string(key)
            .and_then(|value| serde_json::from_str(&value).ok())
            .unwrap_or(JsonValue::Null)
    }
}

fn value_to_json(value: &Value) -> JsonValue {
    match value {
        Value::Null => JsonValue::Null,
        Value::Integer(value) => JsonValue::from(*value),
        Value::Real(value) => JsonValue::from(*value),
        Value::Text(value) => {
            serde_json::from_str(value).unwrap_or_else(|_| JsonValue::from(value.clone()))
        }
        Value::Blob(value) => JsonValue::from(format!("<{} bytes>", value.len())),
    }
}

pub trait IntoDbValue {
    fn into_db_value(self) -> Value;
}

impl IntoDbValue for String {
    fn into_db_value(self) -> Value {
        Value::Text(self)
    }
}

impl IntoDbValue for &String {
    fn into_db_value(self) -> Value {
        Value::Text(self.clone())
    }
}

impl IntoDbValue for &str {
    fn into_db_value(self) -> Value {
        Value::Text(self.to_string())
    }
}

impl IntoDbValue for i32 {
    fn into_db_value(self) -> Value {
        Value::Integer(self as i64)
    }
}

impl IntoDbValue for i64 {
    fn into_db_value(self) -> Value {
        Value::Integer(self)
    }
}

impl IntoDbValue for u64 {
    fn into_db_value(self) -> Value {
        Value::Integer(self as i64)
    }
}

impl IntoDbValue for bool {
    fn into_db_value(self) -> Value {
        Value::Integer(i64::from(self))
    }
}

impl IntoDbValue for f64 {
    fn into_db_value(self) -> Value {
        Value::Real(self)
    }
}

pub fn val<T: IntoDbValue>(value: T) -> Value {
    value.into_db_value()
}

pub fn opt_val<T: IntoDbValue>(value: Option<T>) -> Value {
    value.map(IntoDbValue::into_db_value).unwrap_or(Value::Null)
}

pub fn uuid_val(value: Uuid) -> Value {
    Value::Text(value.to_string())
}

pub fn opt_uuid_val(value: Option<Uuid>) -> Value {
    value
        .map(|value| Value::Text(value.to_string()))
        .unwrap_or(Value::Null)
}

pub fn dec_val(value: Decimal) -> Value {
    Value::Text(value.to_string())
}

pub fn json_val(value: JsonValue) -> Value {
    Value::Text(value.to_string())
}

pub fn now_string() -> String {
    Utc::now().to_rfc3339()
}

fn ensure_parent(path: &Path) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    Ok(())
}

async fn seed_defaults(db: &Db) -> anyhow::Result<()> {
    for price in default_model_prices() {
        let now = now_string();
        let model_id = ensure_model_row(db, price.app_type, price.model_pattern, None).await?;
        let input = default_official_price(price.input);
        let output = default_official_price(price.output);
        let cache_read = default_official_price(price.cache_read);
        let cache_write = default_official_price(price.cache_write);
        let existing = db
            .query_optional(
                "SELECT id FROM model_prices WHERE app_type = ?1 AND model_pattern = ?2 AND status = 'active' LIMIT 1",
                vec![val(price.app_type), val(price.model_pattern)],
            )
            .await?;
        if let Some(row) = existing {
            db.execute(
                r#"
                UPDATE model_prices
                   SET input_per_million = ?1,
                       output_per_million = ?2,
                       cache_read_per_million = ?3,
                       cache_write_per_million = ?4,
                       model_id = ?5,
                       updated_at = ?6
                 WHERE id = ?7
                "#,
                vec![
                    dec_val(input),
                    dec_val(output),
                    dec_val(cache_read),
                    dec_val(cache_write),
                    uuid_val(model_id),
                    val(&now),
                    val(row.string("id")),
                ],
            )
            .await?;
            continue;
        }
        db.execute(
            r#"
            INSERT INTO model_prices
              (id, model_id, app_type, model_pattern, input_per_million, output_per_million, cache_read_per_million, cache_write_per_million, status, effective_from, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'active', ?9, ?9, ?9)
            "#,
            vec![
                uuid_val(Uuid::new_v4()),
                uuid_val(model_id),
                val(price.app_type),
                val(price.model_pattern),
                dec_val(input),
                dec_val(output),
                dec_val(cache_read),
                dec_val(cache_write),
                val(&now),
            ],
        )
        .await?;
    }
    ensure_model_vendor_discounts(db).await?;
    let now = now_string();
    db.execute(
        r#"
        UPDATE model_prices
           SET status = 'inactive', updated_at = ?1
         WHERE app_type = 'deepseek'
           AND model_pattern NOT IN ('deepseek-v4-pro*', 'deepseek-v4-flash*')
           AND status = 'active'
        "#,
        vec![val(&now)],
    )
    .await?;
    db.execute(
        r#"
        UPDATE models
           SET status = 'inactive', updated_at = ?1
         WHERE app_type = 'deepseek'
           AND model_pattern NOT IN ('deepseek-v4-pro*', 'deepseek-v4-flash*')
           AND status = 'active'
        "#,
        vec![val(&now)],
    )
    .await?;
    for (fee_type, method, fixed, bps) in [
        ("topup", "dodo", "0.30000000", 290_i32),
        ("payout", "gateio", "0.00000000", 0_i32),
        ("payout", "manual", "0.00000000", 0_i32),
    ] {
        db.execute(
            r#"
            INSERT INTO fee_policies (id, fee_type, method, fixed_usd, percent_bps, min_usd, status, effective_from, created_at)
            SELECT ?1, ?2, ?3, ?4, ?5, '0', 'active', ?6, ?6
            WHERE NOT EXISTS (
              SELECT 1 FROM fee_policies WHERE fee_type = ?2 AND method = ?3 AND status = 'active'
            )
            "#,
            vec![
                uuid_val(Uuid::new_v4()),
                val(fee_type),
                val(method),
                val(fixed),
                val(bps),
                val(now_string()),
            ],
        )
        .await?;
    }
    Ok(())
}

async fn ensure_model_vendor_discounts(db: &Db) -> anyhow::Result<()> {
    let now = now_string();
    db.execute(
        r#"
        INSERT INTO model_vendor_discounts (app_type, discount_percent, updated_at)
        SELECT DISTINCT app_type, '10', ?1 FROM models WHERE app_type IS NOT NULL AND app_type <> ''
        ON CONFLICT(app_type) DO NOTHING
        "#,
        vec![val(&now)],
    )
    .await?;
    db.execute(
        r#"
        INSERT INTO model_vendor_discounts (app_type, discount_percent, updated_at)
        SELECT DISTINCT app_type, '10', ?1 FROM model_prices WHERE app_type IS NOT NULL AND app_type <> ''
        ON CONFLICT(app_type) DO NOTHING
        "#,
        vec![val(&now)],
    )
    .await?;
    Ok(())
}

fn default_official_price(value: &str) -> Decimal {
    Decimal::from_str(value).unwrap_or(Decimal::ZERO) * Decimal::TEN
}

async fn ensure_model_row(
    db: &Db,
    app_type: &str,
    model_pattern: &str,
    display_name: Option<&str>,
) -> anyhow::Result<Uuid> {
    let existing = db
        .query_optional(
            "SELECT id FROM models WHERE app_type = ?1 AND COALESCE(model_pattern, canonical_name) = ?2 LIMIT 1",
            vec![val(app_type), val(model_pattern)],
        )
        .await?;
    let now = now_string();
    let is_public = model_pattern != "*";
    if let Some(row) = existing {
        let id = row.uuid("id");
        db.execute(
            r#"
            UPDATE models
               SET model_pattern = COALESCE(model_pattern, ?2),
                   display_name = COALESCE(display_name, ?3),
                   status = COALESCE(status, 'active'),
                   is_public = COALESCE(is_public, ?4),
                   updated_at = ?5
             WHERE id = ?1
            "#,
            vec![
                uuid_val(id),
                val(model_pattern),
                opt_val(display_name.map(ToOwned::to_owned)),
                val(is_public),
                val(now),
            ],
        )
        .await?;
        return Ok(id);
    }

    let id = Uuid::new_v4();
    db.execute(
        r#"
        INSERT INTO models
          (id, app_type, canonical_name, model_pattern, display_name, status, is_public, sort_order, aliases_json, metadata_json, created_at, updated_at)
        VALUES (?1, ?2, ?3, ?3, ?4, 'active', ?5, 0, '[]', '{}', ?6, ?6)
        "#,
        vec![
            uuid_val(id),
            val(app_type),
            val(model_pattern),
            opt_val(display_name.map(ToOwned::to_owned)),
            val(is_public),
            val(now),
        ],
    )
    .await?;
    Ok(id)
}

async fn backfill_model_price_models(db: &Db) -> anyhow::Result<()> {
    let rows = db
        .query_all(
            "SELECT id, app_type, model_pattern FROM model_prices WHERE model_id IS NULL OR model_id = ''",
            vec![],
        )
        .await?;
    for row in rows {
        let model_id = ensure_model_row(
            db,
            &row.string("app_type"),
            &row.string("model_pattern"),
            None,
        )
        .await?;
        db.execute(
            "UPDATE model_prices SET model_id=?2 WHERE id=?1",
            vec![val(row.string("id")), uuid_val(model_id)],
        )
        .await?;
    }
    Ok(())
}

pub(crate) struct DefaultModelPrice {
    pub(crate) app_type: &'static str,
    pub(crate) model_pattern: &'static str,
    pub(crate) input: &'static str,
    pub(crate) output: &'static str,
    pub(crate) cache_read: &'static str,
    pub(crate) cache_write: &'static str,
}

pub(crate) fn default_model_prices() -> &'static [DefaultModelPrice] {
    // Default TokenMarket prices are public list prices divided by 10.
    // Reviewed against official OpenAI, Anthropic, Gemini, and DeepSeek pricing pages on 2026-05-10.
    &[
        DefaultModelPrice {
            app_type: "openai",
            model_pattern: "gpt-5.5*",
            input: "0.50000000",
            output: "3.00000000",
            cache_read: "0.05000000",
            cache_write: "0.00000000",
        },
        DefaultModelPrice {
            app_type: "openai",
            model_pattern: "gpt-5.4-mini*",
            input: "0.07500000",
            output: "0.45000000",
            cache_read: "0.00750000",
            cache_write: "0.00000000",
        },
        DefaultModelPrice {
            app_type: "openai",
            model_pattern: "gpt-5.4*",
            input: "0.25000000",
            output: "1.50000000",
            cache_read: "0.02500000",
            cache_write: "0.00000000",
        },
        DefaultModelPrice {
            app_type: "openai",
            model_pattern: "*",
            input: "0.25000000",
            output: "1.50000000",
            cache_read: "0.02500000",
            cache_write: "0.00000000",
        },
        DefaultModelPrice {
            app_type: "deepseek",
            model_pattern: "deepseek-v4-pro*",
            input: "0.04350000",
            output: "0.08700000",
            cache_read: "0.00000000",
            cache_write: "0.00000000",
        },
        DefaultModelPrice {
            app_type: "deepseek",
            model_pattern: "deepseek-v4-flash*",
            input: "0.01400000",
            output: "0.02800000",
            cache_read: "0.00000000",
            cache_write: "0.00000000",
        },
        DefaultModelPrice {
            app_type: "anthropic",
            model_pattern: "claude-opus-4-7*",
            input: "0.50000000",
            output: "2.50000000",
            cache_read: "0.05000000",
            cache_write: "0.62500000",
        },
        DefaultModelPrice {
            app_type: "anthropic",
            model_pattern: "claude-opus-4-6*",
            input: "0.50000000",
            output: "2.50000000",
            cache_read: "0.05000000",
            cache_write: "0.62500000",
        },
        DefaultModelPrice {
            app_type: "anthropic",
            model_pattern: "claude-opus-4-5*",
            input: "0.50000000",
            output: "2.50000000",
            cache_read: "0.05000000",
            cache_write: "0.62500000",
        },
        DefaultModelPrice {
            app_type: "anthropic",
            model_pattern: "claude-opus-4-1*",
            input: "1.50000000",
            output: "7.50000000",
            cache_read: "0.15000000",
            cache_write: "1.87500000",
        },
        DefaultModelPrice {
            app_type: "anthropic",
            model_pattern: "claude-opus-4*",
            input: "1.50000000",
            output: "7.50000000",
            cache_read: "0.15000000",
            cache_write: "1.87500000",
        },
        DefaultModelPrice {
            app_type: "anthropic",
            model_pattern: "claude-sonnet-4-6*",
            input: "0.30000000",
            output: "1.50000000",
            cache_read: "0.03000000",
            cache_write: "0.37500000",
        },
        DefaultModelPrice {
            app_type: "anthropic",
            model_pattern: "claude-sonnet-4-5*",
            input: "0.30000000",
            output: "1.50000000",
            cache_read: "0.03000000",
            cache_write: "0.37500000",
        },
        DefaultModelPrice {
            app_type: "anthropic",
            model_pattern: "claude-sonnet-4*",
            input: "0.30000000",
            output: "1.50000000",
            cache_read: "0.03000000",
            cache_write: "0.37500000",
        },
        DefaultModelPrice {
            app_type: "anthropic",
            model_pattern: "claude-3-7-sonnet*",
            input: "0.30000000",
            output: "1.50000000",
            cache_read: "0.03000000",
            cache_write: "0.37500000",
        },
        DefaultModelPrice {
            app_type: "anthropic",
            model_pattern: "claude-3-5-sonnet*",
            input: "0.30000000",
            output: "1.50000000",
            cache_read: "0.03000000",
            cache_write: "0.37500000",
        },
        DefaultModelPrice {
            app_type: "anthropic",
            model_pattern: "claude-haiku-4-5*",
            input: "0.10000000",
            output: "0.50000000",
            cache_read: "0.01000000",
            cache_write: "0.12500000",
        },
        DefaultModelPrice {
            app_type: "anthropic",
            model_pattern: "claude-3-5-haiku*",
            input: "0.08000000",
            output: "0.40000000",
            cache_read: "0.00800000",
            cache_write: "0.10000000",
        },
        DefaultModelPrice {
            app_type: "anthropic",
            model_pattern: "claude-3-haiku*",
            input: "0.02500000",
            output: "0.12500000",
            cache_read: "0.00300000",
            cache_write: "0.03000000",
        },
        DefaultModelPrice {
            app_type: "anthropic",
            model_pattern: "*",
            input: "0.30000000",
            output: "1.50000000",
            cache_read: "0.03000000",
            cache_write: "0.37500000",
        },
        DefaultModelPrice {
            app_type: "gemini",
            model_pattern: "gemini-3.1-pro*",
            input: "0.20000000",
            output: "1.20000000",
            cache_read: "0.02000000",
            cache_write: "0.20000000",
        },
        DefaultModelPrice {
            app_type: "gemini",
            model_pattern: "gemini-3-pro-image*",
            input: "0.20000000",
            output: "1.20000000",
            cache_read: "0.00000000",
            cache_write: "0.20000000",
        },
        DefaultModelPrice {
            app_type: "gemini",
            model_pattern: "gemini-3.1-flash-lite*",
            input: "0.02500000",
            output: "0.15000000",
            cache_read: "0.00250000",
            cache_write: "0.02500000",
        },
        DefaultModelPrice {
            app_type: "gemini",
            model_pattern: "gemini-3-flash*",
            input: "0.05000000",
            output: "0.30000000",
            cache_read: "0.00500000",
            cache_write: "0.05000000",
        },
        DefaultModelPrice {
            app_type: "gemini",
            model_pattern: "gemini-2.5-pro*",
            input: "0.12500000",
            output: "1.00000000",
            cache_read: "0.01250000",
            cache_write: "0.12500000",
        },
        DefaultModelPrice {
            app_type: "gemini",
            model_pattern: "gemini-2.5-flash-lite*",
            input: "0.01000000",
            output: "0.04000000",
            cache_read: "0.00100000",
            cache_write: "0.01000000",
        },
        DefaultModelPrice {
            app_type: "gemini",
            model_pattern: "gemini-2.5-flash*",
            input: "0.03000000",
            output: "0.25000000",
            cache_read: "0.00300000",
            cache_write: "0.03000000",
        },
        DefaultModelPrice {
            app_type: "gemini",
            model_pattern: "*",
            input: "0.03000000",
            output: "0.25000000",
            cache_read: "0.00300000",
            cache_write: "0.03000000",
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::default_model_prices;

    #[test]
    fn default_prices_include_deepseek_vendor_models() {
        let prices = default_model_prices();
        let deepseek_prices = prices
            .iter()
            .filter(|price| price.app_type == "deepseek")
            .collect::<Vec<_>>();
        let find = |pattern: &str| {
            deepseek_prices
                .iter()
                .copied()
                .find(|price| price.app_type == "deepseek" && price.model_pattern == pattern)
                .expect("deepseek price")
        };

        let flash = find("deepseek-v4-flash*");
        assert_eq!(flash.input, "0.01400000");
        assert_eq!(flash.output, "0.02800000");
        assert_eq!(flash.cache_read, "0.00000000");
        assert_eq!(flash.cache_write, "0.00000000");

        let pro = find("deepseek-v4-pro*");
        assert_eq!(pro.input, "0.04350000");
        assert_eq!(pro.output, "0.08700000");
        assert_eq!(pro.cache_read, "0.00000000");
        assert_eq!(pro.cache_write, "0.00000000");

        assert_eq!(
            deepseek_prices
                .iter()
                .map(|price| price.model_pattern)
                .collect::<Vec<_>>(),
            vec!["deepseek-v4-pro*", "deepseek-v4-flash*"]
        );
    }
}

const SCHEMA: &str = r#"
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS users (
  id TEXT PRIMARY KEY,
  email TEXT UNIQUE NOT NULL,
  email_verified_source TEXT NOT NULL DEFAULT 'router_resend',
  status TEXT NOT NULL DEFAULT 'active',
  locale TEXT,
  email_verified_at TEXT,
  last_login_at TEXT,
  metadata_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_users_created_at ON users(created_at DESC);

CREATE TABLE IF NOT EXISTS health_checks (
  id TEXT PRIMARY KEY,
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS web_sessions (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL REFERENCES users(id),
  email TEXT NOT NULL,
  session_token_hash TEXT NOT NULL UNIQUE,
  router_user_id TEXT,
  router_access_expires_at TEXT,
  expires_at TEXT NOT NULL,
  last_seen_at TEXT,
  last_seen_ip TEXT,
  ip_country TEXT,
  user_agent TEXT,
  created_at TEXT NOT NULL,
  revoked_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_web_sessions_user ON web_sessions(user_id);
CREATE INDEX IF NOT EXISTS idx_web_sessions_expires ON web_sessions(expires_at);

CREATE TABLE IF NOT EXISTS api_keys (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL REFERENCES users(id),
  name TEXT NOT NULL DEFAULT 'Default key',
  key_hash TEXT NOT NULL UNIQUE,
  prefix TEXT NOT NULL,
  scope_json TEXT,
  expires_at TEXT,
  monthly_spend_cap TEXT,
  last_used_at TEXT,
  last_used_ip_country TEXT,
  created_at TEXT NOT NULL,
  revoked_at TEXT,
  paused_at TEXT,
  deleted_at TEXT
);

CREATE TABLE IF NOT EXISTS wallet_accounts (
  id TEXT PRIMARY KEY,
  account_type TEXT NOT NULL,
  currency TEXT NOT NULL DEFAULT 'USD',
  owner_user_id TEXT,
  owner_email TEXT,
  balance TEXT NOT NULL DEFAULT '0',
  metadata_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS uq_wallet_user_account ON wallet_accounts(account_type, currency, owner_user_id) WHERE owner_user_id IS NOT NULL;
CREATE UNIQUE INDEX IF NOT EXISTS uq_wallet_email_account ON wallet_accounts(account_type, currency, owner_email) WHERE owner_email IS NOT NULL;
CREATE UNIQUE INDEX IF NOT EXISTS uq_wallet_platform_account ON wallet_accounts(account_type, currency) WHERE owner_user_id IS NULL AND owner_email IS NULL;
CREATE INDEX IF NOT EXISTS idx_wallet_accounts_type ON wallet_accounts(account_type);

CREATE TABLE IF NOT EXISTS ledger_entries (
  id TEXT PRIMARY KEY,
  transaction_id TEXT NOT NULL,
  from_account_id TEXT REFERENCES wallet_accounts(id),
  to_account_id TEXT REFERENCES wallet_accounts(id),
  amount TEXT NOT NULL,
  currency TEXT NOT NULL DEFAULT 'USD',
  reference_type TEXT NOT NULL,
  reference_id TEXT NOT NULL,
  actor_type TEXT NOT NULL,
  actor_id TEXT,
  client_ip TEXT,
  ip_country TEXT,
  metadata_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_ledger_reference ON ledger_entries(reference_type, reference_id);
CREATE INDEX IF NOT EXISTS idx_ledger_created_at ON ledger_entries(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_ledger_from_account_created ON ledger_entries(from_account_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_ledger_to_account_created ON ledger_entries(to_account_id, created_at DESC);

CREATE TABLE IF NOT EXISTS processed_webhooks (
  provider TEXT NOT NULL,
  event_id TEXT NOT NULL,
  event_type TEXT NOT NULL,
  status TEXT NOT NULL,
  raw_payload_object_key TEXT,
  raw_payload_sha256 TEXT,
  error_message TEXT,
  processed_at TEXT,
  created_at TEXT NOT NULL,
  PRIMARY KEY(provider, event_id)
);
CREATE INDEX IF NOT EXISTS idx_processed_webhooks_created_at ON processed_webhooks(created_at DESC);

CREATE TABLE IF NOT EXISTS topup_orders (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL REFERENCES users(id),
  payment_provider TEXT NOT NULL DEFAULT 'dodo',
  provider_payment_id TEXT,
  payment_method_type TEXT,
  gross_amount TEXT NOT NULL,
  fee_amount TEXT NOT NULL,
  net_amount TEXT NOT NULL,
  currency TEXT NOT NULL DEFAULT 'USD',
  status TEXT NOT NULL DEFAULT 'pending',
  checkout_url TEXT,
  metadata_json TEXT NOT NULL DEFAULT '{}',
  raw_payload_object_key TEXT,
  created_at TEXT NOT NULL,
  expires_at TEXT,
  paid_at TEXT,
  refunded_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_topup_orders_created_at ON topup_orders(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_topup_orders_status_created ON topup_orders(status, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_topup_orders_status_paid ON topup_orders(status, paid_at DESC);

CREATE TABLE IF NOT EXISTS model_prices (
  id TEXT PRIMARY KEY,
  model_id TEXT,
  app_type TEXT NOT NULL,
  model_pattern TEXT NOT NULL,
  input_per_million TEXT NOT NULL,
  output_per_million TEXT NOT NULL,
  cache_read_per_million TEXT NOT NULL DEFAULT '0',
  cache_write_per_million TEXT NOT NULL DEFAULT '0',
  currency TEXT NOT NULL DEFAULT 'USD',
  status TEXT NOT NULL,
  effective_from TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS internal_migrations (
  name TEXT PRIMARY KEY,
  applied_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS model_vendor_discounts (
  app_type TEXT PRIMARY KEY,
  discount_percent TEXT NOT NULL DEFAULT '10',
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS fee_policies (
  id TEXT PRIMARY KEY,
  fee_type TEXT NOT NULL,
  method TEXT NOT NULL,
  fixed_usd TEXT NOT NULL DEFAULT '0',
  percent_bps INTEGER NOT NULL DEFAULT 0,
  min_usd TEXT NOT NULL DEFAULT '0',
  max_usd TEXT,
  currency TEXT NOT NULL DEFAULT 'USD',
  status TEXT NOT NULL,
  effective_from TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS price_changes (
  id TEXT PRIMARY KEY,
  price_id TEXT,
  old_snapshot TEXT,
  new_snapshot TEXT NOT NULL,
  admin_actor TEXT,
  reason TEXT,
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS models (
  id TEXT PRIMARY KEY,
  app_type TEXT NOT NULL,
  canonical_name TEXT NOT NULL,
  model_pattern TEXT,
  display_name TEXT,
  status TEXT NOT NULL DEFAULT 'active',
  is_public INTEGER NOT NULL DEFAULT 1,
  sort_order INTEGER NOT NULL DEFAULT 0,
  aliases_json TEXT NOT NULL DEFAULT '[]',
  metadata_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE(app_type, canonical_name)
);

CREATE TABLE IF NOT EXISTS model_routing_rules (
  id TEXT PRIMARY KEY,
  model_id TEXT NOT NULL REFERENCES models(id),
  mode TEXT NOT NULL DEFAULT 'all',
  priority INTEGER NOT NULL DEFAULT 0,
  enabled INTEGER NOT NULL DEFAULT 1,
  notes TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE(model_id)
);

CREATE TABLE IF NOT EXISTS model_routing_rule_shares (
  rule_id TEXT NOT NULL REFERENCES model_routing_rules(id),
  router_id TEXT NOT NULL,
  share_id TEXT NOT NULL,
  created_at TEXT NOT NULL,
  PRIMARY KEY(rule_id, router_id, share_id)
);

CREATE TABLE IF NOT EXISTS router_shares (
  router_id TEXT NOT NULL,
  share_id TEXT NOT NULL,
  subdomain TEXT,
  installation_id TEXT,
  owner_email TEXT,
  installation_owner_email TEXT,
  app_type TEXT NOT NULL,
  for_sale TEXT NOT NULL DEFAULT 'Yes',
  share_status TEXT NOT NULL DEFAULT 'active',
  online INTEGER NOT NULL DEFAULT 1,
  active_requests INTEGER NOT NULL DEFAULT 0,
  parallel_limit INTEGER NOT NULL DEFAULT 3,
  online_rate_24h TEXT NOT NULL DEFAULT '1',
  priority INTEGER NOT NULL DEFAULT 0,
  enabled_claude INTEGER NOT NULL DEFAULT 0,
  enabled_codex INTEGER NOT NULL DEFAULT 0,
  enabled_gemini INTEGER NOT NULL DEFAULT 0,
  disabled_by_market INTEGER NOT NULL DEFAULT 0,
  market_disabled_at TEXT,
  raw_json TEXT NOT NULL DEFAULT '{}',
  last_success_at TEXT,
  last_error_at TEXT,
  last_error_message TEXT,
  last_failure_kind TEXT,
  last_failure_scope TEXT,
  failure_count INTEGER NOT NULL DEFAULT 0,
  cooldown_until TEXT,
  last_seen_at TEXT NOT NULL,
  PRIMARY KEY(router_id, share_id)
);
CREATE INDEX IF NOT EXISTS idx_router_shares_last_seen ON router_shares(last_seen_at DESC);

CREATE TABLE IF NOT EXISTS router_share_model_support (
  router_id TEXT NOT NULL,
  share_id TEXT NOT NULL,
  app TEXT NOT NULL,
  slot TEXT NOT NULL,
  actual_model TEXT,
  official INTEGER NOT NULL DEFAULT 0,
  api_url TEXT,
  provider_kind TEXT,
  updated_at TEXT NOT NULL,
  PRIMARY KEY(router_id, share_id, app, slot)
);
CREATE INDEX IF NOT EXISTS idx_router_share_model_support_lookup ON router_share_model_support(app, slot, actual_model);

CREATE TABLE IF NOT EXISTS share_health (
  id TEXT PRIMARY KEY,
  router_id TEXT NOT NULL,
  share_id TEXT NOT NULL,
  status TEXT NOT NULL,
  latency_ms INTEGER,
  error_message TEXT,
  checked_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_share_health_share ON share_health(router_id, share_id, checked_at DESC);

CREATE TABLE IF NOT EXISTS request_attempts (
  id TEXT PRIMARY KEY,
  request_id TEXT NOT NULL,
  charge_id TEXT,
  attempt_no INTEGER NOT NULL,
  router_id TEXT NOT NULL,
  share_id TEXT NOT NULL,
  model_id TEXT,
  status TEXT NOT NULL,
  failure_kind TEXT,
  error_message TEXT,
  latency_ms INTEGER,
  started_at TEXT NOT NULL,
  finished_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_request_attempts_request ON request_attempts(request_id, attempt_no);

CREATE TABLE IF NOT EXISTS router_request_log_sync_state (
  request_id TEXT PRIMARY KEY,
  last_synced_at TEXT,
  last_error TEXT,
  attempt_count INTEGER NOT NULL DEFAULT 0,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS model_share_blocks (
  model_id TEXT NOT NULL REFERENCES models(id),
  router_id TEXT NOT NULL,
  share_id TEXT NOT NULL,
  reason TEXT NOT NULL,
  expires_at TEXT NOT NULL,
  created_at TEXT NOT NULL,
  PRIMARY KEY(model_id, router_id, share_id)
);
CREATE INDEX IF NOT EXISTS idx_model_share_blocks_expires ON model_share_blocks(expires_at);

CREATE TABLE IF NOT EXISTS market_share_capability_blocks (
  router_id TEXT NOT NULL,
  share_id TEXT NOT NULL,
  capability TEXT NOT NULL CHECK(capability IN ('claude','codex','gemini')),
  reason TEXT,
  created_by TEXT,
  created_at TEXT NOT NULL,
  PRIMARY KEY(router_id, share_id, capability)
);

CREATE TABLE IF NOT EXISTS market_share_sticky_routes (
  sticky_key TEXT PRIMARY KEY,
  api_key_id TEXT,
  user_id TEXT NOT NULL,
  app_type TEXT NOT NULL,
  model_id TEXT NOT NULL,
  protocol_family TEXT NOT NULL,
  router_id TEXT NOT NULL,
  share_id TEXT NOT NULL,
  expires_at TEXT NOT NULL,
  last_success_at TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_market_share_sticky_routes_expires ON market_share_sticky_routes(expires_at);
CREATE INDEX IF NOT EXISTS idx_market_share_sticky_routes_api_key ON market_share_sticky_routes(api_key_id);

CREATE TABLE IF NOT EXISTS market_response_sticky_routes (
  response_id TEXT PRIMARY KEY,
  sticky_key TEXT NOT NULL,
  api_key_id TEXT,
  user_id TEXT NOT NULL,
  app_type TEXT NOT NULL,
  model_id TEXT NOT NULL,
  protocol_family TEXT NOT NULL,
  router_id TEXT NOT NULL,
  share_id TEXT NOT NULL,
  expires_at TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_market_response_sticky_routes_expires ON market_response_sticky_routes(expires_at);
CREATE INDEX IF NOT EXISTS idx_market_response_sticky_routes_sticky ON market_response_sticky_routes(sticky_key);

CREATE TABLE IF NOT EXISTS market_api_key_share_allowlist (
  api_key_id TEXT NOT NULL REFERENCES api_keys(id),
  router_id TEXT NOT NULL,
  share_id TEXT NOT NULL,
  created_at TEXT NOT NULL,
  PRIMARY KEY(api_key_id, router_id, share_id)
);
CREATE INDEX IF NOT EXISTS idx_market_api_key_share_allowlist_key ON market_api_key_share_allowlist(api_key_id);

CREATE TABLE IF NOT EXISTS request_charges (
  id TEXT PRIMARY KEY,
  request_id TEXT UNIQUE NOT NULL,
  user_id TEXT NOT NULL REFERENCES users(id),
  api_key_id TEXT NOT NULL REFERENCES api_keys(id),
  router_id TEXT NOT NULL,
  share_id TEXT NOT NULL,
  owner_email TEXT NOT NULL,
  model_id TEXT,
  routing_rule_id TEXT,
  app_type TEXT NOT NULL,
  model TEXT NOT NULL,
  request_agent TEXT,
  requested_model TEXT,
  actual_model TEXT,
  actual_model_source TEXT,
  pricing_model TEXT,
  pricing_slot TEXT,
  pricing_model_source TEXT,
  share_official INTEGER NOT NULL DEFAULT 0,
  status TEXT NOT NULL,
  idempotency_key TEXT,
  request_body_hash TEXT,
  reserved_amount TEXT NOT NULL,
  usage_amount TEXT,
  price_snapshot TEXT NOT NULL,
  usage_json TEXT,
  audit_flags TEXT NOT NULL DEFAULT '[]',
  request_object_key TEXT,
  request_object_sha256 TEXT,
  response_meta_object_key TEXT,
  response_meta_object_sha256 TEXT,
  created_at TEXT NOT NULL,
  settled_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_request_charges_created_at ON request_charges(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_request_charges_status_created ON request_charges(status, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_request_charges_user_created ON request_charges(user_id, created_at DESC);

CREATE TABLE IF NOT EXISTS request_idempotency (
  user_id TEXT NOT NULL REFERENCES users(id),
  idempotency_key TEXT NOT NULL,
  request_body_hash TEXT NOT NULL,
  charge_id TEXT,
  status TEXT NOT NULL,
  created_at TEXT NOT NULL,
  completed_at TEXT,
  PRIMARY KEY(user_id, idempotency_key)
);

CREATE TABLE IF NOT EXISTS provider_claim_profiles (
  owner_email TEXT PRIMARY KEY,
  method TEXT NOT NULL,
  params_json TEXT NOT NULL DEFAULT '{}',
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS payout_requests (
  id TEXT PRIMARY KEY,
  owner_email TEXT NOT NULL,
  amount_usd TEXT NOT NULL,
  payout_fee_usd TEXT NOT NULL,
  net_payout_usd TEXT NOT NULL,
  method TEXT NOT NULL,
  params_json TEXT NOT NULL DEFAULT '{}',
  fee_policy_snapshot TEXT,
  ticket_id TEXT,
  status TEXT NOT NULL,
  settlement_batch_id TEXT,
  settlement_item_id TEXT,
  external_tx_id TEXT,
  proof_object_key TEXT,
  proof_object_sha256 TEXT,
  gateio_batch_id TEXT,
  gateio_request_object_key TEXT,
  gateio_request_object_sha256 TEXT,
  gateio_response_object_key TEXT,
  gateio_response_object_sha256 TEXT,
  failure_reason TEXT,
  created_at TEXT NOT NULL,
  processing_at TEXT,
  paid_at TEXT,
  failed_at TEXT,
  cancelled_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_payout_owner_status ON payout_requests(owner_email, status);
CREATE INDEX IF NOT EXISTS idx_payout_requests_status_created ON payout_requests(status, created_at DESC);

CREATE TABLE IF NOT EXISTS payout_attempts (
  id TEXT PRIMARY KEY,
  payout_request_id TEXT NOT NULL REFERENCES payout_requests(id),
  method TEXT NOT NULL,
  status TEXT NOT NULL,
  request_object_key TEXT,
  request_object_sha256 TEXT,
  response_object_key TEXT,
  response_object_sha256 TEXT,
  external_tx_id TEXT,
  error_message TEXT,
  created_at TEXT NOT NULL,
  completed_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_payout_attempts_payout ON payout_attempts(payout_request_id, created_at DESC);

CREATE TABLE IF NOT EXISTS settlement_batches (
  id TEXT PRIMARY KEY,
  method TEXT NOT NULL,
  status TEXT NOT NULL,
  gross_amount_usd TEXT NOT NULL DEFAULT '0',
  fee_amount_usd TEXT NOT NULL DEFAULT '0',
  net_amount_usd TEXT NOT NULL DEFAULT '0',
  external_batch_id TEXT,
  proof_object_key TEXT,
  created_at TEXT NOT NULL,
  completed_at TEXT
);

CREATE TABLE IF NOT EXISTS settlement_items (
  id TEXT PRIMARY KEY,
  settlement_batch_id TEXT REFERENCES settlement_batches(id),
  payout_request_id TEXT REFERENCES payout_requests(id),
  owner_email TEXT NOT NULL,
  gross_amount_usd TEXT NOT NULL,
  fee_amount_usd TEXT NOT NULL,
  net_amount_usd TEXT NOT NULL,
  status TEXT NOT NULL,
  external_tx_id TEXT,
  created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_settlement_items_payout ON settlement_items(payout_request_id);

CREATE TABLE IF NOT EXISTS tickets (
  id TEXT PRIMARY KEY,
  ticket_no TEXT UNIQUE NOT NULL,
  ticket_type TEXT NOT NULL,
  status TEXT NOT NULL,
  priority TEXT NOT NULL,
  subject TEXT NOT NULL,
  creator_user_id TEXT,
  creator_owner_email TEXT,
  related_payout_request_id TEXT,
  related_reference_type TEXT,
  related_reference_id TEXT,
  assigned_admin_id TEXT,
  metadata_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  closed_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_tickets_updated_at ON tickets(updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_tickets_status_updated ON tickets(status, updated_at DESC);

CREATE TABLE IF NOT EXISTS ticket_messages (
  id TEXT PRIMARY KEY,
  ticket_id TEXT NOT NULL REFERENCES tickets(id),
  sender_type TEXT NOT NULL,
  sender_id TEXT,
  body_text TEXT NOT NULL,
  internal_note INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_ticket_messages_ticket_created ON ticket_messages(ticket_id, created_at DESC);

CREATE TABLE IF NOT EXISTS ticket_attachments (
  id TEXT PRIMARY KEY,
  ticket_id TEXT REFERENCES tickets(id),
  message_id TEXT REFERENCES ticket_messages(id),
  uploader_type TEXT NOT NULL,
  uploader_user_id TEXT,
  uploader_email TEXT,
  object_key TEXT NOT NULL,
  content_sha256 TEXT NOT NULL,
  content_type TEXT NOT NULL,
  byte_size INTEGER NOT NULL,
  original_filename TEXT,
  reference_type TEXT,
  reference_id TEXT,
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS object_refs (
  id TEXT PRIMARY KEY,
  object_key TEXT NOT NULL UNIQUE,
  content_sha256 TEXT NOT NULL,
  byte_size INTEGER NOT NULL DEFAULT 0,
  reference_type TEXT NOT NULL,
  reference_id TEXT NOT NULL,
  object_role TEXT NOT NULL,
  content_type TEXT,
  created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_object_refs_reference ON object_refs(reference_type, reference_id);

CREATE TABLE IF NOT EXISTS admin_audit (
  id TEXT PRIMARY KEY,
  admin_actor TEXT NOT NULL,
  action TEXT NOT NULL,
  reference_type TEXT,
  reference_id TEXT,
  metadata_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_admin_audit_created_at ON admin_audit(created_at DESC);

CREATE TABLE IF NOT EXISTS app_settings (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
"#;
