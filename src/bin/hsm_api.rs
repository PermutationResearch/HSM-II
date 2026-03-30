//! HSM-II REST API server binary.
//!
//! Launches an axum HTTP server exposing the full HSM-II API:
//! beliefs, skills, context ranking, predictions, trust, council, world.
//!
//! Usage:
//!   cargo run --bin hsm-api -- [--port 3000] [--host 0.0.0.0]

use std::net::SocketAddr;

use clap::Parser;
use tracing_subscriber::EnvFilter;

use hyper_stigmergy::api::{ApiState, SharedState};
use hyper_stigmergy::hyper_stigmergy::HyperStigmergicMorphogenesis;

#[derive(Parser, Debug)]
#[command(name = "hsm-api", about = "HSM-II REST API server")]
struct Args {
    /// Port to listen on
    #[arg(short, long, default_value = "3000")]
    port: u16,

    /// Host to bind to
    #[arg(long, default_value = "127.0.0.1")]
    host: String,

    /// Number of initial agents
    #[arg(long, default_value = "5")]
    agents: usize,

    /// Skip world initialization (API-only mode, no world/tick endpoints)
    #[arg(long)]
    no_world: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();

    // Build shared state
    let shared = if args.no_world {
        tracing::info!("Starting in API-only mode (no world)");
        SharedState::new()
    } else {
        tracing::info!("Initializing world with {} agents", args.agents);
        let world = HyperStigmergicMorphogenesis::new(args.agents);
        SharedState::with_world(world)
    };

    let state = ApiState::new(shared);
    let app = hyper_stigmergy::api::api_router(state);

    // Add CORS and tracing middleware
    let app = app.layer(
        tower_http::cors::CorsLayer::permissive()
    ).layer(
        tower_http::trace::TraceLayer::new_for_http()
    );

    let addr: SocketAddr = format!("{}:{}", args.host, args.port).parse()?;
    tracing::info!("HSM-II API server listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
