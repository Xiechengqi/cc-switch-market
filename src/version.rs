use std::{
    path::{Path, PathBuf},
    process::Stdio,
};

use axum::{Json, extract::State};
use serde::Serialize;
use tokio::{fs, process::Command};
use uuid::Uuid;

use crate::{app_state::AppState, auth::AdminPrincipal, error::ApiError};

const RELEASE_BINARY_URL: &str = "https://github.com/Xiechengqi/cc-switch-market/releases/download/latest/cc-switch-market-linux-amd64";
const DEFAULT_BINARY_PATH: &str = "/usr/local/bin/cc-switch-market";
const DEFAULT_LOG_PATH: &str = "/var/log/cc-switch-market.log";
const SERVICE_NAME: &str = "cc-switch-market";

#[derive(Serialize)]
pub struct VersionInfo {
    version: &'static str,
    git_sha: &'static str,
    git_ref: &'static str,
    build_time: &'static str,
    target: &'static str,
    pid: u32,
    uptime_seconds: u64,
    current_exe: String,
    binary_path: String,
    log_path: &'static str,
    service_name: &'static str,
    service_exists: bool,
    release_binary_url: &'static str,
}

#[derive(Serialize)]
pub struct ActionResult {
    ok: bool,
    action: &'static str,
    mode: String,
    message: String,
}

pub async fn admin_version(
    State(state): State<AppState>,
    _admin: AdminPrincipal,
) -> Result<Json<VersionInfo>, ApiError> {
    Ok(Json(version_info(&state).await))
}

pub async fn admin_restart(
    State(state): State<AppState>,
    AdminPrincipal(admin): AdminPrincipal,
) -> Result<Json<ActionResult>, ApiError> {
    let mode = schedule_restart().await?;
    write_audit(
        &state,
        &admin.email,
        "version.restart",
        serde_json::json!({ "mode": mode }),
    )
    .await?;
    Ok(Json(ActionResult {
        ok: true,
        action: "restart",
        mode,
        message: "restart scheduled".to_string(),
    }))
}

pub async fn admin_update(
    State(state): State<AppState>,
    AdminPrincipal(admin): AdminPrincipal,
) -> Result<Json<ActionResult>, ApiError> {
    let target = binary_path();
    let temp_path = PathBuf::from("/tmp/cc-switch-market-linux-amd64.new");
    let backup_path = backup_path_for(&target);

    let response = reqwest::get(RELEASE_BINARY_URL).await.map_err(|err| {
        ApiError::service_unavailable(format!("download latest binary failed: {err}"))
    })?;
    if !response.status().is_success() {
        return Err(ApiError::service_unavailable(format!(
            "download latest binary returned {}",
            response.status()
        )));
    }
    let bytes = response.bytes().await.map_err(|err| {
        ApiError::service_unavailable(format!("read latest binary failed: {err}"))
    })?;
    if bytes.len() < 1024 * 1024 {
        return Err(ApiError::service_unavailable(
            "downloaded binary is unexpectedly small",
        ));
    }

    fs::write(&temp_path, &bytes).await.map_err(io_error)?;
    let chmod_status = Command::new("chmod")
        .arg("755")
        .arg(&temp_path)
        .status()
        .await
        .map_err(io_error)?;
    if !chmod_status.success() {
        return Err(ApiError::service_unavailable("chmod latest binary failed"));
    }

    if target.exists() {
        fs::copy(&target, &backup_path).await.map_err(io_error)?;
    }
    if fs::rename(&temp_path, &target).await.is_err() {
        fs::copy(&temp_path, &target).await.map_err(io_error)?;
        fs::remove_file(&temp_path).await.map_err(io_error)?;
    }

    let mode = schedule_restart().await?;
    write_audit(
        &state,
        &admin.email,
        "version.update",
        serde_json::json!({
            "mode": mode,
            "target": target.display().to_string(),
            "backup": backup_path.display().to_string(),
            "url": RELEASE_BINARY_URL,
            "bytes": bytes.len(),
        }),
    )
    .await?;
    Ok(Json(ActionResult {
        ok: true,
        action: "update",
        mode,
        message: "latest binary installed and restart scheduled".to_string(),
    }))
}

async fn version_info(state: &AppState) -> VersionInfo {
    let current_exe =
        std::env::current_exe().unwrap_or_else(|_| PathBuf::from(DEFAULT_BINARY_PATH));
    let binary_path = binary_path();
    VersionInfo {
        version: env!("CARGO_PKG_VERSION"),
        git_sha: option_env!("CC_SWITCH_MARKET_GIT_SHA").unwrap_or("unknown"),
        git_ref: option_env!("CC_SWITCH_MARKET_GIT_REF").unwrap_or("unknown"),
        build_time: option_env!("CC_SWITCH_MARKET_BUILD_TIME").unwrap_or("unknown"),
        target: "linux-amd64",
        pid: std::process::id(),
        uptime_seconds: state.started_at.elapsed().as_secs(),
        current_exe: current_exe.display().to_string(),
        binary_path: binary_path.display().to_string(),
        log_path: DEFAULT_LOG_PATH,
        service_name: SERVICE_NAME,
        service_exists: service_exists().await,
        release_binary_url: RELEASE_BINARY_URL,
    }
}

async fn schedule_restart() -> Result<String, ApiError> {
    if service_exists().await {
        let script = format!("sleep 1; systemctl restart {SERVICE_NAME}");
        spawn_detached_shell(&script)?;
        return Ok("systemd".to_string());
    }

    let pid = std::process::id();
    let binary = shell_quote(&binary_path().display().to_string());
    let log = shell_quote(DEFAULT_LOG_PATH);
    let script = format!(
        "sleep 1; kill -TERM {pid} 2>/dev/null || true; \
         for i in 1 2 3 4 5; do kill -0 {pid} 2>/dev/null || break; sleep 1; done; \
         kill -KILL {pid} 2>/dev/null || true; \
         nohup {binary} > {log} 2>&1 &"
    );
    spawn_detached_shell(&script)?;
    Ok("manual".to_string())
}

fn spawn_detached_shell(script: &str) -> Result<(), ApiError> {
    std::process::Command::new("nohup")
        .arg("sh")
        .arg("-c")
        .arg(script)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(io_error)?;
    Ok(())
}

async fn service_exists() -> bool {
    if let Ok(output) = Command::new("systemctl")
        .arg("cat")
        .arg(format!("{SERVICE_NAME}.service"))
        .output()
        .await
    {
        if output.status.success() {
            return true;
        }
    }
    let Ok(output) = Command::new("systemctl")
        .arg("list-unit-files")
        .arg(format!("{SERVICE_NAME}.service"))
        .arg("--no-legend")
        .output()
        .await
    else {
        return false;
    };
    output.status.success() && String::from_utf8_lossy(&output.stdout).contains(SERVICE_NAME)
}

fn binary_path() -> PathBuf {
    PathBuf::from(DEFAULT_BINARY_PATH)
}

fn backup_path_for(target: &Path) -> PathBuf {
    let timestamp = chrono::Utc::now().format("%Y%m%d%H%M%S");
    let file_name = target
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(SERVICE_NAME);
    target.with_file_name(format!("{file_name}.bak.{timestamp}"))
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn io_error(err: std::io::Error) -> ApiError {
    ApiError::service_unavailable(err.to_string())
}

async fn write_audit(
    state: &AppState,
    admin: &str,
    action: &str,
    metadata: serde_json::Value,
) -> Result<(), ApiError> {
    state
        .db()
        .execute(
            "INSERT INTO admin_audit (id, admin_actor, action, reference_type, reference_id, metadata_json, created_at) VALUES (?1,?2,?3,'version',?4,?5,?6)",
            vec![
                crate::db::uuid_val(Uuid::new_v4()),
                crate::db::val(admin),
                crate::db::val(action),
                crate::db::uuid_val(Uuid::nil()),
                crate::db::json_val(metadata),
                crate::db::val(crate::db::now_string()),
            ],
        )
        .await?;
    Ok(())
}
