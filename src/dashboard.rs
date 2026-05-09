use std::{
    collections::{BTreeMap, HashMap},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

use axum::{
    Json,
    extract::{Query, State},
};
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::{Value, json};
use std::str::FromStr;

use crate::{app_state::AppState, error::ApiError};

const KPI_CACHE_TTL: Duration = Duration::from_secs(60);
const TREND_CACHE_TTL: Duration = Duration::from_secs(300);
const BREAKDOWN_CACHE_TTL: Duration = Duration::from_secs(300);
const TOP_CACHE_TTL: Duration = Duration::from_secs(300);

static CACHE: OnceLock<Mutex<HashMap<String, (Instant, Value)>>> = OnceLock::new();

fn cache_get(key: &str) -> Option<Value> {
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = cache.lock().ok()?;
    if let Some((expires, value)) = guard.get(key) {
        if *expires > Instant::now() {
            return Some(value.clone());
        }
    }
    guard.remove(key);
    None
}

fn cache_set(key: String, value: Value, ttl: Duration) {
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(mut guard) = cache.lock() {
        if guard.len() > 256 {
            let now = Instant::now();
            guard.retain(|_, (exp, _)| *exp > now);
        }
        guard.insert(key, (Instant::now() + ttl, value));
    }
}

async fn timezone_offset_minutes(state: &AppState) -> i64 {
    match state
        .db()
        .query_optional(
            "SELECT value FROM app_settings WHERE key='time_zone_offset_minutes'",
            vec![],
        )
        .await
    {
        Ok(Some(row)) => row.string("value").parse::<i64>().unwrap_or(480),
        _ => 480,
    }
}

fn datetime_modifier(offset_minutes: i64) -> String {
    if offset_minutes >= 0 {
        format!("+{} minutes", offset_minutes)
    } else {
        format!("{} minutes", offset_minutes)
    }
}

fn cutoff_iso(days: i64) -> String {
    let now = chrono::Utc::now();
    let cutoff = now
        .checked_sub_signed(chrono::Duration::days(days))
        .unwrap_or(now);
    cutoff.to_rfc3339()
}

fn money_string(raw: String) -> String {
    Decimal::from_str(&raw)
        .map(|value| value.round_dp(6).normalize().to_string())
        .unwrap_or(raw)
}

fn clamp_days(days: Option<i64>) -> i64 {
    days.unwrap_or(30).clamp(1, 365)
}

fn clamp_limit(limit: Option<i64>) -> i64 {
    limit.unwrap_or(10).clamp(1, 50)
}

fn window_days(window: &str) -> i64 {
    match window {
        "24h" => 1,
        "7d" => 7,
        "30d" => 30,
        _ => 7,
    }
}

#[derive(Deserialize)]
pub struct WindowQuery {
    pub window: Option<String>,
}

#[derive(Deserialize)]
pub struct DaysQuery {
    pub days: Option<i64>,
}

#[derive(Deserialize)]
pub struct DimDaysQuery {
    pub dim: Option<String>,
    pub days: Option<i64>,
}

#[derive(Deserialize)]
pub struct LimitDaysQuery {
    pub days: Option<i64>,
    pub limit: Option<i64>,
}

pub async fn kpis(
    State(state): State<AppState>,
    Query(query): Query<WindowQuery>,
) -> Result<Json<Value>, ApiError> {
    crate::rate_limit::check("public_dashboard", "kpis", 120)?;
    let window = query.window.as_deref().unwrap_or("7d");
    let days = window_days(window);
    let cache_key = format!("kpis:{}", days);
    if let Some(value) = cache_get(&cache_key) {
        return Ok(Json(value));
    }
    let cutoff = cutoff_iso(days);
    let spend_row = state
        .db()
        .query_one(
            "SELECT COALESCE(SUM(CAST(usage_amount AS REAL)), 0) AS total, COUNT(*) AS calls FROM request_charges WHERE status='settled' AND created_at >= ?1",
            vec![crate::db::val(cutoff.clone())],
        )
        .await?;
    let topup_row = state
        .db()
        .query_one(
            "SELECT COALESCE(SUM(CAST(net_amount AS REAL)), 0) AS total, COUNT(*) AS orders FROM topup_orders WHERE status='paid' AND paid_at >= ?1",
            vec![crate::db::val(cutoff.clone())],
        )
        .await?;
    let active_users_row = state
        .db()
        .query_one(
            "SELECT COUNT(DISTINCT user_id) AS count FROM request_charges WHERE created_at >= ?1",
            vec![crate::db::val(cutoff.clone())],
        )
        .await?;
    let active_providers_row = state
        .db()
        .query_one(
            "SELECT COUNT(DISTINCT owner_email) AS count FROM request_charges WHERE created_at >= ?1",
            vec![crate::db::val(cutoff.clone())],
        )
        .await?;
    let registered_users_row = state
        .db()
        .query_one(
            "SELECT COUNT(*) AS count FROM users WHERE status='active'",
            vec![],
        )
        .await?;
    let online_shares_row = state
        .db()
        .query_one(
            "SELECT COUNT(*) AS count FROM router_shares WHERE online = 1 AND share_status='active' AND for_sale='Yes'",
            vec![],
        )
        .await?;
    let value = json!({
        "windowKey": window,
        "windowDays": days,
        "totalSpendUsd": money_string(spend_row.string("total")),
        "totalRequests": spend_row.i64("calls"),
        "totalTopupUsd": money_string(topup_row.string("total")),
        "totalTopupOrders": topup_row.i64("orders"),
        "activeApiUsers": active_users_row.i64("count"),
        "activeProviders": active_providers_row.i64("count"),
        "registeredUsers": registered_users_row.i64("count"),
        "onlineShares": online_shares_row.i64("count"),
    });
    cache_set(cache_key, value.clone(), KPI_CACHE_TTL);
    Ok(Json(value))
}

pub async fn trend(
    State(state): State<AppState>,
    Query(query): Query<DaysQuery>,
) -> Result<Json<Value>, ApiError> {
    crate::rate_limit::check("public_dashboard", "trend", 120)?;
    let days = clamp_days(query.days);
    let cache_key = format!("trend:{}", days);
    if let Some(value) = cache_get(&cache_key) {
        return Ok(Json(value));
    }
    let offset = timezone_offset_minutes(&state).await;
    let modifier = datetime_modifier(offset);
    let cutoff = cutoff_iso(days);
    let spend_rows = state
        .db()
        .query_all(
            "SELECT substr(datetime(created_at, ?1), 1, 10) AS day, COALESCE(SUM(CAST(usage_amount AS REAL)), 0) AS spend, COUNT(*) AS calls FROM request_charges WHERE status='settled' AND created_at >= ?2 GROUP BY day ORDER BY day",
            vec![crate::db::val(modifier.clone()), crate::db::val(cutoff.clone())],
        )
        .await?;
    let topup_rows = state
        .db()
        .query_all(
            "SELECT substr(datetime(paid_at, ?1), 1, 10) AS day, COALESCE(SUM(CAST(net_amount AS REAL)), 0) AS topup FROM topup_orders WHERE status='paid' AND paid_at IS NOT NULL AND paid_at >= ?2 GROUP BY day ORDER BY day",
            vec![crate::db::val(modifier), crate::db::val(cutoff)],
        )
        .await?;
    let mut days_map: BTreeMap<String, (String, String, i64)> = BTreeMap::new();
    for row in spend_rows {
        let day = row.string("day");
        let spend = money_string(row.string("spend"));
        let calls = row.i64("calls");
        days_map.insert(day, (spend, "0".into(), calls));
    }
    for row in topup_rows {
        let day = row.string("day");
        let topup = money_string(row.string("topup"));
        days_map
            .entry(day)
            .and_modify(|entry| entry.1 = topup.clone())
            .or_insert(("0".into(), topup, 0));
    }
    let series: Vec<Value> = days_map
        .into_iter()
        .map(|(day, (spend, topup, calls))| {
            json!({
                "date": day,
                "spendUsd": spend,
                "topupUsd": topup,
                "requests": calls,
            })
        })
        .collect();
    let value = json!({
        "days": days,
        "series": series,
    });
    cache_set(cache_key, value.clone(), TREND_CACHE_TTL);
    Ok(Json(value))
}

pub async fn breakdown(
    State(state): State<AppState>,
    Query(query): Query<DimDaysQuery>,
) -> Result<Json<Value>, ApiError> {
    crate::rate_limit::check("public_dashboard", "breakdown", 120)?;
    let dim = query.dim.as_deref().unwrap_or("app_type");
    let days = clamp_days(query.days);
    let cache_key = format!("breakdown:{}:{}", dim, days);
    if let Some(value) = cache_get(&cache_key) {
        return Ok(Json(value));
    }
    let group_col = match dim {
        "app_type" => "app_type",
        "model" => "model",
        "provider" => "owner_email",
        _ => {
            return Err(ApiError::bad_request(
                "invalid_dim",
                "dim must be one of app_type | model | provider",
            ));
        }
    };
    let cutoff = cutoff_iso(days);
    let sql = format!(
        "SELECT {col} AS bucket, COALESCE(SUM(CAST(usage_amount AS REAL)), 0) AS total, COUNT(*) AS calls FROM request_charges WHERE status='settled' AND created_at >= ?1 GROUP BY {col} ORDER BY total DESC LIMIT 16",
        col = group_col
    );
    let rows = state
        .db()
        .query_all(&sql, vec![crate::db::val(cutoff)])
        .await?;
    let buckets: Vec<Value> = rows
        .into_iter()
        .map(|row| {
            json!({
                "name": row.string("bucket"),
                "spendUsd": money_string(row.string("total")),
                "requests": row.i64("calls"),
            })
        })
        .collect();
    let value = json!({
        "dim": dim,
        "days": days,
        "buckets": buckets,
    });
    cache_set(cache_key, value.clone(), BREAKDOWN_CACHE_TTL);
    Ok(Json(value))
}

pub async fn top_models(
    State(state): State<AppState>,
    Query(query): Query<LimitDaysQuery>,
) -> Result<Json<Value>, ApiError> {
    crate::rate_limit::check("public_dashboard", "top-models", 120)?;
    let days = clamp_days(query.days);
    let limit = clamp_limit(query.limit);
    let cache_key = format!("top-models:{}:{}", days, limit);
    if let Some(value) = cache_get(&cache_key) {
        return Ok(Json(value));
    }
    let cutoff = cutoff_iso(days);
    let rows = state
        .db()
        .query_all(
            "SELECT app_type, model, COALESCE(SUM(CAST(usage_amount AS REAL)), 0) AS total, COUNT(*) AS calls, COUNT(DISTINCT user_id) AS users FROM request_charges WHERE status='settled' AND created_at >= ?1 GROUP BY app_type, model ORDER BY total DESC LIMIT ?2",
            vec![crate::db::val(cutoff), crate::db::val(limit.to_string())],
        )
        .await?;
    let items: Vec<Value> = rows
        .into_iter()
        .map(|row| {
            json!({
                "appType": row.string("app_type"),
                "model": row.string("model"),
                "spendUsd": money_string(row.string("total")),
                "requests": row.i64("calls"),
                "uniqueUsers": row.i64("users"),
            })
        })
        .collect();
    let value = json!({
        "days": days,
        "limit": limit,
        "items": items,
    });
    cache_set(cache_key, value.clone(), TOP_CACHE_TTL);
    Ok(Json(value))
}

pub async fn top_providers(
    State(state): State<AppState>,
    Query(query): Query<LimitDaysQuery>,
) -> Result<Json<Value>, ApiError> {
    crate::rate_limit::check("public_dashboard", "top-providers", 120)?;
    let days = clamp_days(query.days);
    let limit = clamp_limit(query.limit);
    let cache_key = format!("top-providers:{}:{}", days, limit);
    if let Some(value) = cache_get(&cache_key) {
        return Ok(Json(value));
    }
    let cutoff = cutoff_iso(days);
    let rows = state
        .db()
        .query_all(
            "SELECT owner_email, COALESCE(SUM(CAST(usage_amount AS REAL)), 0) AS total, COUNT(*) AS calls, COUNT(DISTINCT share_id) AS shares FROM request_charges WHERE status='settled' AND created_at >= ?1 GROUP BY owner_email ORDER BY total DESC LIMIT ?2",
            vec![crate::db::val(cutoff), crate::db::val(limit.to_string())],
        )
        .await?;
    let items: Vec<Value> = rows
        .into_iter()
        .map(|row| {
            json!({
                "ownerEmail": row.string("owner_email"),
                "grossSpendUsd": money_string(row.string("total")),
                "requests": row.i64("calls"),
                "uniqueShares": row.i64("shares"),
            })
        })
        .collect();
    let value = json!({
        "days": days,
        "limit": limit,
        "items": items,
    });
    cache_set(cache_key, value.clone(), TOP_CACHE_TTL);
    Ok(Json(value))
}

pub async fn top_users(
    State(state): State<AppState>,
    Query(query): Query<LimitDaysQuery>,
) -> Result<Json<Value>, ApiError> {
    crate::rate_limit::check("public_dashboard", "top-users", 120)?;
    let days = clamp_days(query.days);
    let limit = clamp_limit(query.limit);
    let cache_key = format!("top-users:{}:{}", days, limit);
    if let Some(value) = cache_get(&cache_key) {
        return Ok(Json(value));
    }
    let cutoff = cutoff_iso(days);
    let rows = state
        .db()
        .query_all(
            "SELECT u.email AS email, COALESCE(SUM(CAST(rc.usage_amount AS REAL)), 0) AS total, COUNT(*) AS calls FROM request_charges rc JOIN users u ON u.id = rc.user_id WHERE rc.status='settled' AND rc.created_at >= ?1 GROUP BY u.email ORDER BY total DESC LIMIT ?2",
            vec![crate::db::val(cutoff), crate::db::val(limit.to_string())],
        )
        .await?;
    let items: Vec<Value> = rows
        .into_iter()
        .map(|row| {
            json!({
                "email": row.string("email"),
                "spendUsd": money_string(row.string("total")),
                "requests": row.i64("calls"),
            })
        })
        .collect();
    let value = json!({
        "days": days,
        "limit": limit,
        "items": items,
    });
    cache_set(cache_key, value.clone(), TOP_CACHE_TTL);
    Ok(Json(value))
}
