//! Agent console API for the Next.js dashboard (`web/agent-console`).
//!
//! ```text
//! cargo run -p hyper-stigmergy --bin hsm_console -- --port 3847
//! # Optional: export HSM_COMPANY_OS_DATABASE_URL=postgres://user:pass@localhost:5432/db
//! cd web/agent-console && npm run dev   # NEXT_PUBLIC_API_BASE=http://127.0.0.1:3847
//! ```
//!
//! Profile: env `HSMII_PROFILE`, or `--hsm-profile` / `-R`. Avoid spelling the long flag `--profile`
//! — clap 4.5+ infers a `-p` short from that name and collides with `--port -p`.

use std::net::SocketAddr;

use clap::Parser;
use tracing_subscriber::EnvFilter;

use hyper_stigmergy::console::{console_router, ConsoleState};
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

    let args = Args::parse();
    let home = resolve_hsmii_home(args.config, args.hsm_profile.as_deref());

    let company_db = hyper_stigmergy::company_os::connect_optional().await?;
    if company_db.is_some() {
        tracing::info!("Company OS: PostgreSQL connected and migrations applied");
    } else {
        tracing::info!("Company OS: disabled (set HSM_COMPANY_OS_DATABASE_URL to enable /api/company/*)");
    }

    let state = ConsoleState::new(home.clone(), company_db);
    let app = console_router(state);

    let addr: SocketAddr = format!("{}:{}", args.bind_host, args.listen_port).parse()?;
    tracing::info!(%addr, home = %home.display(), "HSM agent console API");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
