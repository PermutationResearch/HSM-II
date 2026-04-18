//! Company console API for the Next.js dashboard (`web/company-console`).
//!
//! ```text
//! cargo run -p hyper-stigmergy --bin hsm_console -- --port 3847
//! # Optional: export HSM_COMPANY_OS_DATABASE_URL=postgres://user:pass@localhost:5432/db
//! cd web/company-console && npm run dev   # NEXT_PUBLIC_API_BASE=http://127.0.0.1:3847
//! ```
//!
//! Profile: env `HSMII_PROFILE`, or `--hsm-profile` / `-R`. Avoid spelling the long flag `--profile`
//! — clap 4.5+ infers a `-p` short from that name and collides with `--port -p`.

use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

use clap::Parser;
use tokio::sync::Mutex;
use tracing_subscriber::EnvFilter;

use hyper_stigmergy::console::{console_router, ConsoleState};
use hyper_stigmergy::paperclip::IntelligenceLayer;
use hyper_stigmergy::personal::resolve_hsmii_home;

#[derive(Parser, Debug)]
#[command(name = "hsm-console")]
struct Args {
    /// HTTP listen port (`-p` / `--port`).
    #[arg(short = 'p', long = "port", default_value = "3847")]
    listen_port: u16,
    #[arg(long = "host", default_value = "127.0.0.1")]
    bind_host: String,
    #[arg(short = 'c', long = "config")]
    config: Option<std::path::PathBuf>,
    /// HSMII profile (`~/.hsmii/profiles/<name>/`). Same as other bins’ `--profile`, but the flag is
    /// spelled `--hsm-profile` here so `-p` can stay the HTTP port.
    #[arg(short = 'R', long = "hsm-profile")]
    hsm_profile: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    // Load `.env` from the package root (workspace root), not the shell cwd — so
    // `HSM_COMPANY_OS_DATABASE_URL` is found even when the binary is started from e.g. `web/company-console`.
    let repo_env = Path::new(env!("CARGO_MANIFEST_DIR")).join(".env");
    if repo_env.is_file() {
        if let Err(e) = dotenvy::from_path(&repo_env) {
            tracing::warn!(path = %repo_env.display(), error = %e, "failed to parse repo-root .env");
        }
    }
    // Optional: `.env` in the current working directory (only fills vars still unset).
    let _ = dotenvy::dotenv();

    hyper_stigmergy::policy_config::ensure_loaded();
    hyper_stigmergy::telemetry::init_from_env();

    let args = Args::parse();
    let home = resolve_hsmii_home(args.config, args.hsm_profile.as_deref());

    let company_db = hyper_stigmergy::company_os::connect_optional().await?;
    if company_db.is_some() {
        tracing::info!("Company OS: PostgreSQL connected and migrations applied");
    } else {
        tracing::info!(
            repo_env_path = %repo_env.display(),
            repo_env_exists = repo_env.is_file(),
            "Company OS: disabled — set non-empty HSM_COMPANY_OS_DATABASE_URL in repo-root .env (see .env.example) or export it, then restart hsm_console"
        );
    }

    let paperclip = Arc::new(Mutex::new(IntelligenceLayer::new()));
    let state = ConsoleState::with_paperclip_layer(home.clone(), company_db, paperclip);
    if let Some(pool) = state.company_db.clone() {
        hyper_stigmergy::company_os::start_automation_worker(pool, home.clone());
        tracing::info!("Company OS automation worker started");
    }
    let app = console_router(state);

    let addr: SocketAddr = format!("{}:{}", args.bind_host, args.listen_port).parse()?;
    tracing::info!(%addr, home = %home.display(), "HSM company console API");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
