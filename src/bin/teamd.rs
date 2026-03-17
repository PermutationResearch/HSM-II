//! teamd — Multi-Tenant Autonomous Business Team Server
//!
//! Serves the HSM-II Team API on port 8788, providing multi-tenant
//! management of autonomous business agent teams.
//!
//! Usage:
//!     cargo run --bin teamd -- --bind 127.0.0.1:8788
//!
//! Environment:
//!     HSM_ROODB_URL — Optional MySQL connection for persistent storage
//!     HSM_DATA_DIR  — Base directory for file storage (default: ~/.hsmii)

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    middleware,
    routing::{get, post, put},
    Router,
};
use clap::Parser;
use tower_http::cors::{Any, CorsLayer};
use tracing::info;

use hyper_stigmergy::auth::{PersistentAuthManager, TenantAuthState};
use hyper_stigmergy::team_api::{self, TeamAppState};
use hyper_stigmergy::tenant::TenantRegistry;
use hyper_stigmergy::usage_tracker::UsageTracker;

// ═══════════════════════════════════════════════════════════════════
// CLI Arguments
// ═══════════════════════════════════════════════════════════════════

#[derive(Parser, Debug)]
#[command(
    name = "teamd",
    about = "HSM-II Multi-Tenant Autonomous Business Team Server"
)]
struct Args {
    /// Address to bind the server to.
    #[arg(long, default_value = "127.0.0.1:8788")]
    bind: String,

    /// Base directory for tenant data and auth persistence.
    /// Defaults to ~/.hsmii if not specified.
    #[arg(long)]
    data_dir: Option<String>,

    /// Usage flush interval in seconds.
    #[arg(long, default_value_t = 300)]
    flush_interval: u64,
}

// ═══════════════════════════════════════════════════════════════════
// Main
// ═══════════════════════════════════════════════════════════════════

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialise tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "teamd=info,hyper_stigmergy=info,tower_http=info".into()),
        )
        .init();

    let args = Args::parse();

    // ── Resolve data directory ──────────────────────────────────────
    let base_dir = match &args.data_dir {
        Some(d) => PathBuf::from(d),
        None => {
            let env_dir = std::env::var("HSM_DATA_DIR").ok();
            match env_dir {
                Some(d) => PathBuf::from(d),
                None => dirs::home_dir()
                    .map(|h| h.join(".hsmii"))
                    .unwrap_or_else(|| PathBuf::from(".hsmii")),
            }
        }
    };
    std::fs::create_dir_all(&base_dir)?;
    info!(data_dir = %base_dir.display(), "Data directory resolved");

    // ── Initialise services ─────────────────────────────────────────
    let auth_dir = base_dir.join("auth");
    std::fs::create_dir_all(&auth_dir)?;

    let auth = Arc::new(
        PersistentAuthManager::load(&auth_dir).unwrap_or_else(|e| {
            tracing::warn!(error = %e, "Failed to load auth state, starting fresh");
            PersistentAuthManager::new(&auth_dir)
        }),
    );
    info!("Auth manager initialised");

    let registry = Arc::new(TenantRegistry::new(&base_dir));
    info!(
        tenant_count = registry.tenant_count().await,
        "Tenant registry loaded"
    );

    let usage = Arc::new(UsageTracker::new(&base_dir));
    info!("Usage tracker initialised");

    // Start background flush loop for usage metrics
    usage.start_flush_loop(args.flush_interval);
    info!(
        interval_secs = args.flush_interval,
        "Usage flush loop started"
    );

    // ── Build application state ─────────────────────────────────────
    let app_state = TeamAppState {
        registry: registry.clone(),
        auth: auth.clone(),
        usage: usage.clone(),
    };

    let tenant_auth_state = TenantAuthState {
        auth: auth.clone(),
    };

    // ── Build router ────────────────────────────────────────────────
    // Public routes (no auth required)
    let public_routes = Router::new()
        .route("/api/v1/auth/register", post(team_api::register_tenant))
        .route("/api/v1/auth/token", post(team_api::get_token))
        .route("/health", get(team_api::health))
        .with_state(app_state.clone());

    // Protected routes (require tenant auth)
    let protected_routes = Router::new()
        // Team management
        .route("/api/v1/team", get(team_api::list_team))
        .route("/api/v1/team/:role", get(team_api::get_member))
        .route(
            "/api/v1/team/:role/status",
            put(team_api::update_member_status),
        )
        // Brand context
        .route(
            "/api/v1/brand",
            get(team_api::get_brand).put(team_api::update_brand),
        )
        // Tasks
        .route("/api/v1/tasks", post(team_api::submit_task))
        // Campaigns
        .route(
            "/api/v1/campaigns",
            post(team_api::create_campaign).get(team_api::list_campaigns),
        )
        .route("/api/v1/campaigns/:id", get(team_api::get_campaign))
        .route(
            "/api/v1/campaigns/:id/patterns",
            get(team_api::get_dream_patterns),
        )
        // Task outcomes (dream feedback loop)
        .route(
            "/api/v1/tasks/:id/outcome",
            post(team_api::record_task_outcome),
        )
        // Usage / billing
        .route("/api/v1/usage", get(team_api::get_usage))
        // Apply auth middleware to all protected routes
        .route_layer(middleware::from_fn_with_state(
            tenant_auth_state,
            hyper_stigmergy::auth::require_tenant_auth,
        ))
        .with_state(app_state);

    // CORS layer — permissive for development
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .merge(public_routes)
        .merge(protected_routes)
        .layer(cors);

    // ── Start server ────────────────────────────────────────────────
    let addr: SocketAddr = args.bind.parse()?;
    info!(
        bind = %addr,
        "Starting teamd — Multi-Tenant Autonomous Business Team Server"
    );

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app.into_make_service()).await?;

    Ok(())
}
