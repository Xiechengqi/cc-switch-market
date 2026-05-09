use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};

use chrono::Utc;

use crate::error::ApiError;

static LIMITS: OnceLock<Mutex<HashMap<String, Window>>> = OnceLock::new();

#[derive(Clone, Copy)]
struct Window {
    window_start: i64,
    count: u32,
}

pub fn check(scope: &str, subject: &str, max_per_minute: u32) -> Result<(), ApiError> {
    let now_minute = Utc::now().timestamp() / 60;
    let key = format!("{scope}:{subject}");
    let limits = LIMITS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut limits = limits
        .lock()
        .map_err(|_| ApiError::service_unavailable("rate limit lock poisoned"))?;
    if limits.len() > 10_000 {
        limits.retain(|_, window| now_minute - window.window_start <= 10);
    }
    let entry = limits.entry(key).or_insert(Window {
        window_start: now_minute,
        count: 0,
    });
    if entry.window_start != now_minute {
        *entry = Window {
            window_start: now_minute,
            count: 0,
        };
    }
    entry.count = entry.count.saturating_add(1);
    if entry.count > max_per_minute {
        return Err(ApiError::bad_request(
            "rate_limited",
            "too many requests; retry later",
        ));
    }
    Ok(())
}
