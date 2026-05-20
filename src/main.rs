mod admin;
mod api_keys;
mod app_state;
mod auth;
mod config;
mod config_wizard;
mod dashboard;
mod db;
mod error;
mod gateio;
mod ledger;
mod maintenance;
mod market_tunnel;
mod object_store;
mod pagination;
mod pricing;
mod process_lock;
mod provider;
mod proxy;
mod rate_limit;
mod router_account;
mod router_client;
mod router_notifications;
mod router_request_logs;
mod scheduling;
mod static_assets;
mod support;
mod topups;
mod types;
mod usage;
mod version;

use std::net::SocketAddr;

use anyhow::Context;
use axum::{
    Router,
    body::Body,
    extract::State,
    http::Request,
    middleware,
    middleware::Next,
    response::Response,
    routing::{delete, get, post, put},
};
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::{app_state::AppState, config::Config};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let env_file = Config::load_default_env_file()?;

    match args.first().map(String::as_str) {
        Some("help") | Some("--help") | Some("-h") => {
            print_help();
            return Ok(());
        }
        Some("config") => {
            match args.get(1).map(String::as_str) {
                Some("show") => {
                    let config = Config::from_env();
                    let masked = args.get(2).map(String::as_str) == Some("--masked");
                    for (key, value) in config.env_report(&env_file) {
                        let value = if masked {
                            mask_config_value(&key, &value)
                        } else {
                            value
                        };
                        println!("{key}={value}");
                    }
                }
                Some("path") => {
                    println!("{}", env_file.display());
                }
                Some(command) => {
                    eprintln!("unknown config command: {command}\n");
                    print_help();
                    std::process::exit(2);
                }
                None => {
                    config_wizard::run(&env_file)?;
                }
            }
            return Ok(());
        }
        Some("login") => {
            let config = Config::from_env();
            router_account::login(&config).await?;
            return Ok(());
        }
        Some("account") => {
            let config = Config::from_env();
            router_account::account(&config).await?;
            return Ok(());
        }
        Some("logout") => {
            process_lock::ServerLock::assert_not_running()?;
            router_account::logout()?;
            return Ok(());
        }
        Some(command) => {
            eprintln!("unknown command: {command}\n");
            print_help();
            std::process::exit(2);
        }
        None => {}
    }

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "cc_switch_market=info,tower_http=info,axum=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let config = Config::from_env();
    config.validate()?;
    let _server_lock = process_lock::ServerLock::acquire()?;
    tracing::info!(
        env_file = %env_file.display(),
        db_mode = if config.turso_database_url.is_some() { "turso" } else { "local_sqlite" },
        "loaded cc-switch-market configuration"
    );
    let state = AppState::new(config.clone()).await?;
    let pricing_summary = pricing::pricing_summary(state.db()).await.ok();
    let (session, market_registration) = router_account::register_market(&config, pricing_summary).await.with_context(|| {
        "router market registration failed. Run `cc-switch-market login` first and verify ROUTER_BASE_DOMAIN/ROUTER_MARKET_SUBDOMAIN"
    })?;
    update_market_runtime_from_registration(&state, &market_registration).await;
    tracing::info!(
        market_email = %session.email,
        public_base_url = %config.market_public_base_url,
        "router market registration is ready"
    );
    let _db_sync = db::spawn_turso_sync(config.clone(), state.db.clone());
    let _db_backup = db::spawn_turso_backup(config.clone(), state.db.clone());
    let _topup_expiry = topups::spawn_order_expiry(state.clone());
    let _maintenance = maintenance::spawn(state.clone());
    let _gateio_worker = admin::spawn_gateio_worker(state.clone());
    let _share_sync = router_client::spawn_share_sync(state.clone());
    let _request_log_sync = router_request_logs::spawn_sync(state.clone());
    let _market_pricing_sync = spawn_market_pricing_sync(state.clone());
    let app = build_router(state.clone());
    let addr: SocketAddr = config
        .market_http_addr
        .parse()
        .with_context(|| format!("invalid MARKET_HTTP_ADDR {}", config.market_http_addr))?;

    tracing::info!(%addr, "starting cc-switch-market");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let _market_tunnel = market_tunnel::spawn(config.clone());
    axum::serve(listener, app).await?;
    Ok(())
}

fn spawn_market_pricing_sync(state: AppState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            interval.tick().await;
            let pricing_summary = pricing::pricing_summary(state.db()).await.ok();
            match router_account::register_market(&state.config, pricing_summary).await {
                Ok((_session, registration)) => {
                    update_market_runtime_from_registration(&state, &registration).await;
                }
                Err(err) => {
                    tracing::warn!(error = %err, "sync market pricing summary to router failed");
                }
            }
        }
    })
}

async fn update_market_runtime_from_registration(
    state: &AppState,
    registration: &router_account::MarketRegistration,
) {
    let mut runtime = state.market_runtime.write().await;
    runtime.owner_email = Some(registration.email.trim().to_ascii_lowercase());
    runtime.maintenance_enabled = registration.maintenance_enabled;
    runtime.maintenance_message = registration.maintenance_message.clone();
}

fn print_help() {
    println!(
        r#"cc-switch-market

Usage:
  cc-switch-market            Start HTTP server
  cc-switch-market help       Show help
  cc-switch-market login      Login to router with email verification code
  cc-switch-market account    Show current router login and market endpoint
  cc-switch-market logout     Logout router account; refuses while server is running
  cc-switch-market config     Configure environment interactively
  cc-switch-market config show
                              Print effective environment configuration
  cc-switch-market config show --masked
                              Print effective environment configuration with secrets masked
  cc-switch-market config path
                              Print env file path

Default env file:
  $HOME/.config/cc-switch-market/.env
"#
    );
}

fn mask_config_value(key: &str, value: &str) -> String {
    if !is_secret_env_key(key) || value.is_empty() {
        return value.to_string();
    }
    let chars = value.chars().collect::<Vec<_>>();
    if chars.len() <= 8 {
        return "configured".to_string();
    }
    let prefix = chars.iter().take(4).collect::<String>();
    let suffix = chars
        .iter()
        .skip(chars.len().saturating_sub(4))
        .collect::<String>();
    format!("{prefix}****{suffix}")
}

fn is_secret_env_key(key: &str) -> bool {
    key.contains("SECRET")
        || key.contains("TOKEN")
        || key.contains("API_KEY")
        || key.contains("ACCESS_KEY")
        || key == "TURSO_AUTH_TOKEN"
}

fn build_router(state: AppState) -> Router {
    let csrf_state = state.clone();
    Router::new()
        .route("/v1/healthz", get(types::healthz))
        .route("/v1/version", get(types::version))
        .route("/v1/public/info", get(types::public_info))
        .route("/v1/public/config", get(types::public_config))
        .route("/v1/public/dashboard/kpis", get(dashboard::kpis))
        .route("/v1/public/dashboard/trend", get(dashboard::trend))
        .route("/v1/public/dashboard/breakdown", get(dashboard::breakdown))
        .route(
            "/v1/public/dashboard/top-models",
            get(dashboard::top_models),
        )
        .route(
            "/v1/public/dashboard/top-providers",
            get(dashboard::top_providers),
        )
        .route("/v1/public/dashboard/top-users", get(dashboard::top_users))
        .route("/v1/metrics", get(types::metrics))
        .route("/docs", get(types::docs))
        .route(
            "/market-api/object-upload/{*object_key}",
            put(object_store::upload_object),
        )
        .route(
            "/market-api/object-download/{*object_key}",
            get(object_store::download_object),
        )
        .route(
            "/v1/auth/email/request-code",
            post(auth::request_email_code),
        )
        .route(
            "/market-api/auth/email/request-code",
            post(auth::request_email_code),
        )
        .route("/v1/auth/email/verify-code", post(auth::verify_email_code))
        .route(
            "/market-api/auth/email/verify-code",
            post(auth::verify_email_code),
        )
        .route("/v1/auth/logout", post(auth::logout))
        .route("/market-api/auth/logout", post(auth::logout))
        .route("/v1/me", get(auth::me))
        .route("/v1/session/status", get(auth::session_status))
        .route("/market-api/session/status", get(auth::session_status))
        .route(
            "/v1/api-keys",
            post(api_keys::create_api_key).get(api_keys::list_api_keys),
        )
        .route(
            "/v1/api-keys/{id}",
            post(api_keys::rename_api_key).delete(api_keys::delete_api_key_endpoint),
        )
        .route(
            "/v1/api-keys/{id}/status",
            post(api_keys::set_api_key_status_endpoint),
        )
        .route(
            "/v1/api-keys/{id}/limits",
            post(api_keys::update_api_key_limits),
        )
        .route(
            "/v1/api-keys/{id}/share-allowlist",
            get(api_keys::get_api_key_share_allowlist_endpoint)
                .post(api_keys::set_api_key_share_allowlist_endpoint),
        )
        .route(
            "/v1/me/available-shares",
            get(api_keys::available_shares_endpoint),
        )
        .route(
            "/v1/api-key-secrets",
            get(api_keys::list_api_key_secrets_endpoint)
                .post(api_keys::create_api_key_secret_endpoint),
        )
        .route("/v1/topups/checkout", post(topups::create_checkout))
        .route("/v1/topups/{id}", get(topups::get_topup))
        .route("/v1/webhooks/dodo", post(topups::dodo_webhook))
        .route("/v1/prices", get(pricing::list_prices))
        .route("/v1/admin/prices", get(pricing::admin_list_prices))
        .route("/v1/admin/prices/{id}", put(pricing::upsert_price))
        .route("/v1/admin/price-changes", get(pricing::price_changes))
        .route(
            "/v1/admin/models",
            get(pricing::admin_list_models).post(pricing::admin_create_model),
        )
        .route(
            "/v1/admin/models/route-preview",
            post(pricing::admin_route_preview),
        )
        .route(
            "/v1/admin/model-vendor-discounts/{app_type}",
            put(pricing::admin_put_model_vendor_discount),
        )
        .route(
            "/v1/admin/models/{id}",
            get(pricing::admin_get_model)
                .patch(pricing::admin_patch_model)
                .delete(pricing::admin_delete_model),
        )
        .route(
            "/v1/admin/models/{id}/activate",
            post(pricing::admin_activate_model),
        )
        .route(
            "/v1/admin/models/{id}/deactivate",
            post(pricing::admin_deactivate_model),
        )
        .route(
            "/v1/admin/models/{id}/price",
            put(pricing::admin_put_model_price),
        )
        .route(
            "/v1/admin/models/{id}/price-changes",
            get(pricing::admin_model_price_changes),
        )
        .route(
            "/v1/admin/models/{id}/routing",
            put(pricing::admin_put_model_routing),
        )
        .route(
            "/v1/admin/models/{id}/routing/shares",
            put(pricing::admin_put_model_routing_shares),
        )
        .route("/v1/wallet/ledger", get(ledger::wallet_ledger))
        .route("/v1/wallet/summary", get(ledger::wallet_summary))
        .route("/v1/money-events", get(ledger::money_events))
        .route("/v1/usage", get(proxy::usage))
        .route("/v1/usage/{id}/report", post(support::report_usage))
        .route("/v1/chat/completions", post(proxy::chat_completions))
        .route("/v1/responses", post(proxy::responses))
        .route("/responses", post(proxy::responses))
        .route("/v1/messages", post(proxy::messages))
        .route("/v1beta/models/{*path}", post(proxy::gemini_models_v1beta))
        .route("/v1/models/{*path}", post(proxy::gemini_models_v1))
        .route("/v1/provider/earnings", get(provider::earnings))
        .route("/v1/provider/claim/summary", get(provider::claim_summary))
        .route(
            "/v1/provider/claim/payout-preview",
            get(provider::payout_preview),
        )
        .route(
            "/v1/provider/claim/payout",
            post(provider::create_gateio_payout),
        )
        .route(
            "/v1/provider/claim/payout-ticket",
            post(provider::create_manual_payout_ticket),
        )
        .route(
            "/v1/provider/claim/convert-to-balance",
            post(provider::convert_to_balance),
        )
        .route(
            "/v1/provider/claim/transfer-provider",
            post(provider::transfer_provider_earnings),
        )
        .route("/v1/provider/claim/payouts", get(provider::payouts))
        .route(
            "/v1/provider/claim/payouts/{id}",
            get(provider::payout_detail),
        )
        .route(
            "/v1/ticket-attachments/presign",
            post(support::presign_attachment),
        )
        .route(
            "/v1/tickets",
            post(support::create_ticket).get(support::list_tickets),
        )
        .route(
            "/v1/tickets/{id}",
            get(support::get_ticket).delete(support::delete_ticket),
        )
        .route("/v1/tickets/{id}/close", post(support::close_ticket))
        .route(
            "/v1/tickets/{id}/messages",
            post(support::add_ticket_message),
        )
        .route("/v1/admin/users", get(admin::users))
        .route("/v1/admin/users/{id}", get(admin::user))
        .route("/v1/admin/users/{id}/ledger", get(admin::user_ledger))
        .route("/v1/admin/users/{id}/adjust", post(admin::adjust_user))
        .route("/v1/admin/topups", get(admin::topups))
        .route("/v1/admin/topups/{id}", get(admin::topup))
        .route("/v1/admin/topups/{id}/refund", post(admin::refund_topup))
        .route("/v1/admin/webhooks/dodo", get(admin::webhooks))
        .route("/v1/admin/shares", get(admin::shares))
        .route("/v1/admin/shares/sync", post(admin::sync_shares))
        .route(
            "/v1/admin/share-capability-blocks",
            get(admin::share_capability_blocks).post(admin::create_share_capability_block),
        )
        .route(
            "/v1/admin/share-capability-blocks/{router_id}/{share_id}/{capability}",
            delete(admin::delete_share_capability_block),
        )
        .route("/v1/admin/charges", get(admin::charges))
        .route(
            "/v1/admin/charges/{id}/review-context",
            get(admin::charge_review_context),
        )
        .route(
            "/v1/admin/charges/{id}/settle-manual",
            post(admin::settle_charge_manual),
        )
        .route(
            "/v1/admin/charges/{id}/release",
            post(admin::release_charge),
        )
        .route("/v1/admin/earnings", get(admin::earnings))
        .route("/v1/admin/money/overview", get(admin::money_overview))
        .route("/v1/admin/ledger", get(admin::ledger))
        .route("/v1/admin/ledger/check", get(admin::ledger_check))
        .route("/v1/admin/money-events", get(admin::money_events))
        .route("/v1/admin/settlements", get(admin::settlements))
        .route("/v1/admin/payout-requests", get(admin::payout_requests))
        .route("/v1/admin/payout-requests/{id}", get(admin::payout_request))
        .route(
            "/v1/admin/payout-requests/{id}/execute-gateio",
            post(admin::execute_gateio),
        )
        .route(
            "/v1/admin/payout-requests/{id}/mark-paid",
            post(admin::mark_payout_paid),
        )
        .route(
            "/v1/admin/payout-requests/{id}/mark-failed",
            post(admin::mark_payout_failed),
        )
        .route(
            "/v1/admin/payout-requests/{id}/cancel",
            post(admin::cancel_payout),
        )
        .route("/v1/admin/tickets", get(admin::tickets))
        .route("/v1/admin/tickets/{id}", get(admin::ticket))
        .route("/v1/admin/tickets/{id}/assign", post(admin::assign_ticket))
        .route(
            "/v1/admin/tickets/{id}/messages",
            post(admin::admin_ticket_message),
        )
        .route("/v1/admin/tickets/{id}/status", post(admin::ticket_status))
        .route(
            "/v1/admin/tickets/{id}/link-payout",
            post(admin::link_payout),
        )
        .route(
            "/v1/admin/tickets/{id}/complete-manual-payout",
            post(admin::complete_manual_payout),
        )
        .route(
            "/v1/admin/tickets/{id}/adjust-provider-payable",
            post(admin::adjust_provider_payable),
        )
        .route(
            "/v1/admin/settings",
            get(admin::settings).put(admin::update_settings),
        )
        .route("/v1/admin/settings/env", put(admin::update_env_settings))
        .route(
            "/v1/admin/settings/footer-links",
            put(admin::update_footer_links),
        )
        .route("/v1/admin/version", get(version::admin_version))
        .route("/v1/admin/version/restart", post(version::admin_restart))
        .route("/v1/admin/version/update", post(version::admin_update))
        .route("/v1/admin/audit", get(admin::audit))
        .fallback(static_assets::serve)
        .with_state(state)
        .layer(middleware::from_fn_with_state(
            csrf_state.clone(),
            metrics_middleware,
        ))
        .layer(middleware::from_fn_with_state(
            csrf_state,
            auth::csrf_middleware,
        ))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
}

async fn metrics_middleware(
    State(state): State<AppState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let endpoint = metrics_endpoint(request.uri().path());
    let started = std::time::Instant::now();
    let response = next.run(request).await;
    state.metrics.record(
        &endpoint,
        response.status(),
        started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
    );
    response
}

fn metrics_endpoint(path: &str) -> String {
    if path.starts_with("/v1/admin/") {
        "/v1/admin/*".to_string()
    } else if path.starts_with("/v1/provider/") {
        "/v1/provider/*".to_string()
    } else if path.starts_with("/v1/auth/") || path.starts_with("/market-api/auth/") {
        "/v1/auth/*".to_string()
    } else if path.starts_with("/v1beta/models/") {
        "/v1beta/models/*".to_string()
    } else if path.starts_with("/v1/models/") {
        "/v1/models/*".to_string()
    } else if path.starts_with("/_next/") {
        "/_next/*".to_string()
    } else {
        path.to_string()
    }
}
