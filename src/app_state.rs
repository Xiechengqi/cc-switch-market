use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
    time::Instant,
};

use anyhow::Context;
use reqwest::Client;

use crate::{config::Config, db, object_store::ObjectStore};

#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub db: db::Db,
    pub object_store: ObjectStore,
    pub http: Client,
    pub metrics: AppMetrics,
    pub started_at: Instant,
}

#[derive(Clone, Default)]
pub struct AppMetrics {
    inner: Arc<Mutex<MetricsInner>>,
}

#[derive(Default)]
struct MetricsInner {
    total_requests: u64,
    error_responses: u64,
    by_endpoint: HashMap<String, EndpointMetrics>,
}

#[derive(Default, Clone)]
pub struct EndpointMetrics {
    pub requests: u64,
    pub errors: u64,
    pub latencies_ms: VecDeque<u64>,
}

#[derive(Clone, serde::Serialize)]
pub struct MetricsSnapshot {
    pub total_requests: u64,
    pub error_responses: u64,
    pub endpoints: Vec<EndpointSnapshot>,
}

#[derive(Clone, serde::Serialize)]
pub struct EndpointSnapshot {
    pub endpoint: String,
    pub requests: u64,
    pub errors: u64,
    pub p95_latency_ms: u64,
}

impl AppMetrics {
    pub fn record(&self, endpoint: &str, status: axum::http::StatusCode, latency_ms: u64) {
        let Ok(mut inner) = self.inner.lock() else {
            return;
        };
        inner.total_requests = inner.total_requests.saturating_add(1);
        if status.is_client_error() || status.is_server_error() {
            inner.error_responses = inner.error_responses.saturating_add(1);
        }
        let entry = inner.by_endpoint.entry(endpoint.to_string()).or_default();
        entry.requests = entry.requests.saturating_add(1);
        if status.is_client_error() || status.is_server_error() {
            entry.errors = entry.errors.saturating_add(1);
        }
        entry.latencies_ms.push_back(latency_ms);
        while entry.latencies_ms.len() > 512 {
            entry.latencies_ms.pop_front();
        }
    }

    pub fn snapshot(&self) -> MetricsSnapshot {
        let Ok(inner) = self.inner.lock() else {
            return MetricsSnapshot {
                total_requests: 0,
                error_responses: 0,
                endpoints: vec![],
            };
        };
        let mut endpoints = inner
            .by_endpoint
            .iter()
            .map(|(endpoint, metrics)| EndpointSnapshot {
                endpoint: endpoint.clone(),
                requests: metrics.requests,
                errors: metrics.errors,
                p95_latency_ms: p95(&metrics.latencies_ms),
            })
            .collect::<Vec<_>>();
        endpoints.sort_by(|a, b| a.endpoint.cmp(&b.endpoint));
        MetricsSnapshot {
            total_requests: inner.total_requests,
            error_responses: inner.error_responses,
            endpoints,
        }
    }
}

fn p95(values: &VecDeque<u64>) -> u64 {
    if values.is_empty() {
        return 0;
    }
    let mut values = values.iter().copied().collect::<Vec<_>>();
    values.sort_unstable();
    let index = ((values.len() as f64) * 0.95).ceil() as usize;
    values[index.saturating_sub(1).min(values.len() - 1)]
}

impl AppState {
    pub async fn new(config: Config) -> anyhow::Result<Self> {
        tracing::info!("connecting to configured libSQL database");
        let db = db::connect(&config)
            .await
            .context("database connection failed")?;
        db::migrate(&db)
            .await
            .context("database migration failed")?;
        tracing::info!(mode = %db.mode_name(), path = %db.path_for_log(), "database connection and migrations are ready");
        if config.object_store_backend != "local" {
            anyhow::bail!("only OBJECT_STORE_BACKEND=local is implemented in this build");
        }
        let object_store = ObjectStore::local(config.object_store_local_dir.clone())
            .context("object store initialization failed")?;
        tracing::info!(backend = %config.object_store_backend, path = %object_store.root_for_log(), "object store is ready");
        let state = Self {
            config,
            db,
            object_store,
            http: Client::new(),
            metrics: AppMetrics::default(),
            started_at: Instant::now(),
        };
        if state.config.gateio_auto_payout_enabled {
            match crate::gateio::self_check(&state).await {
                Ok(()) => tracing::info!("Gate.io automatic payout self-check passed"),
                Err(err) => {
                    tracing::warn!(error = %err, "Gate.io automatic payout self-check failed; payouts will enter review on execution failure")
                }
            }
        }
        Ok(state)
    }

    pub fn db(&self) -> &db::Db {
        &self.db
    }
}
