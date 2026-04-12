//! HSM-II Personal Agent - Main entry point
//!
//! A grounded, Hermes-like personal AI assistant powered by HSM-II's
//! advanced multi-agent coordination (stigmergy, DKS, CASS, Council).

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use tracing::{error, info, warn};

use hyper_stigmergy::api::{ApiState, HonchoApiState, SharedState};
use hyper_stigmergy::console::{console_router, ConsoleState};
use hyper_stigmergy::personal::{gateway, resolve_hsmii_home, EnhancedPersonalAgent, Heartbeat};
use hyper_stigmergy::tui_codex_style::{AutocompleteSuggestion, CodexEvent, CodexState};
use hyper_stigmergy::{ApprovalOutcome, ApprovalService};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex, RwLock};

// TUI imports for Codex-style interface
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;

fn local_cors_layer() -> tower_http::cors::CorsLayer {
    let origins_raw = std::env::var("HSM_API_ALLOWED_ORIGINS")
        .unwrap_or_else(|_| "http://127.0.0.1:3001,http://localhost:3001".to_string());
    let origins = origins_raw
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();
    let methods = [
        axum::http::Method::GET,
        axum::http::Method::POST,
        axum::http::Method::PUT,
        axum::http::Method::DELETE,
        axum::http::Method::OPTIONS,
    ];
    if origins.iter().any(|s| *s == "*") {
        return tower_http::cors::CorsLayer::new()
            .allow_origin(tower_http::cors::Any)
            .allow_methods(methods);
    }
    let parsed = origins
        .into_iter()
        .filter_map(|s| s.parse::<axum::http::HeaderValue>().ok())
        .collect::<Vec<_>>();
    tower_http::cors::CorsLayer::new()
        .allow_origin(parsed)
        .allow_methods(methods)
}

#[derive(Parser)]
#[command(name = "hsmii")]
#[command(about = "HSM-II Personal Agent - Your AI companion")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Custom config directory (overrides profile / HSMII_HOME)
    #[arg(short, long, global = true)]
    config: Option<PathBuf>,

    /// Profile name: data under ~/.hsmii/profiles/<name>/ (Hermes-style isolation)
    #[arg(short = 'p', long = "profile", global = true)]
    profile: Option<String>,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the agent (interactive mode)
    Start {
        /// Run in daemon mode (background)
        #[arg(short, long)]
        daemon: bool,
        /// Enable Discord gateway
        #[arg(long)]
        discord: bool,
        /// Enable Telegram gateway
        #[arg(long)]
        telegram: bool,
        /// Use TUI (Terminal UI) mode - Codex style
        #[arg(long)]
        tui: bool,
    },

    /// Chat with the agent
    Chat {
        /// Single message (non-interactive)
        #[arg(short, long)]
        message: Option<String>,
    },

    /// Execute a task
    Do {
        /// Task description
        task: String,
    },

    /// Configure the agent
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },

    /// View or edit memory
    Memory {
        #[command(subcommand)]
        action: MemoryAction,
    },

    /// Run heartbeat manually
    Heartbeat,

    /// Bootstrap new agent (first-time setup)
    Bootstrap,

    /// Onboard: teach HSM-II about your business
    Onboard,

    /// Ingest a document to extract business knowledge
    Ingest {
        /// Path to file (.txt, .md, .csv, .json, .html)
        file: String,
    },

    /// Run MiroFish trajectory prediction
    Predict {
        /// Scenario template: pricing, market-entry, growth, marketing, competitive, cost
        #[arg(short, long)]
        template: Option<String>,

        /// Custom topic (if not using a template)
        topic: Option<String>,

        /// List available templates
        #[arg(long)]
        list: bool,

        /// Show prediction history
        #[arg(long)]
        history: bool,

        /// Show calibration stats (back-testing results)
        #[arg(long)]
        backtest: bool,

        /// Record outcome for a past prediction: --outcome <prediction_id> <result> <correct|wrong>
        #[arg(long)]
        outcome: Option<String>,
    },

    /// Check agent status
    Status,
}

#[derive(Subcommand)]
enum ConfigAction {
    /// View current configuration
    Show,
    /// Edit personality (SOUL.md)
    Persona,
    /// Set a configuration value
    Set { key: String, value: String },
}

#[derive(Subcommand)]
enum MemoryAction {
    /// Show recent memories
    Show {
        /// Number of entries to show
        #[arg(short, long, default_value = "10")]
        n: usize,
    },
    /// Search memories
    Search { query: String },
    /// Add a fact
    Add {
        content: String,
        #[arg(short, long)]
        category: Option<String>,
    },
}

/// Load repo-root `.env` then cwd `.env` (same as `hsm_console`) so `HSM_COMPANY_OS_DATABASE_URL`
/// and friends resolve even when the binary is started from e.g. `web/company-console`.
fn load_repo_dotenv() {
    let repo_env = Path::new(env!("CARGO_MANIFEST_DIR")).join(".env");
    if repo_env.is_file() {
        if let Err(e) = dotenvy::from_path(&repo_env) {
            warn!(
                path = %repo_env.display(),
                error = %e,
                "failed to parse repo-root .env"
            );
        }
    }
    let _ = dotenvy::dotenv();
}

/// When true (default), `personal_agent start` binds `HSM_CONSOLE_PORT` (default 3847) with the same
/// `/api/company/*` + `/api/console/*` stack as `hsm_console`, sharing the in-process Paperclip layer.
fn embed_company_console_api_enabled() -> bool {
    match std::env::var("HSM_EMBED_CONSOLE_API")
        .map(|s| s.to_lowercase())
        .unwrap_or_default()
        .as_str()
    {
        "0" | "false" | "no" | "off" => false,
        _ => true,
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    load_repo_dotenv();
    hyper_stigmergy::telemetry::init_from_env();

    let cli = Cli::parse();

    let home = resolve_hsmii_home(cli.config, cli.profile.as_deref());

    match cli.command {
        Commands::Start {
            daemon,
            discord,
            telegram,
            tui,
        } => {
            if tui {
                cmd_start_tui(&home).await?;
            } else {
                cmd_start(&home, daemon, discord, telegram).await?;
            }
        }
        Commands::Chat { message } => {
            cmd_chat(&home, message).await?;
        }
        Commands::Do { task } => {
            cmd_do(&home, &task).await?;
        }
        Commands::Config { action } => {
            cmd_config(&home, action).await?;
        }
        Commands::Memory { action } => {
            cmd_memory(&home, action).await?;
        }
        Commands::Heartbeat => {
            cmd_heartbeat(&home).await?;
        }
        Commands::Bootstrap => {
            cmd_bootstrap(&home).await?;
        }
        Commands::Onboard => {
            cmd_onboard(&home).await?;
        }
        Commands::Ingest { file } => {
            cmd_ingest(&home, &file).await?;
        }
        Commands::Predict {
            template,
            topic,
            list,
            history,
            backtest,
            outcome,
        } => {
            cmd_predict(&home, template, topic, list, history, backtest, outcome).await?;
        }
        Commands::Status => {
            cmd_status(&home).await?;
        }
    }

    Ok(())
}

/// Start the agent — unified runtime (the “OS” for everything else in this process).
///
/// Everything runs in one process:
///  - the personal agent (kept in memory in an Arc<Mutex<>>)
///  - the Axum REST API server (in-process tokio task, `HSM_API_PORT` / default 3000)
///  - optional embedded **company console API** (`HSM_CONSOLE_PORT` / default 3847): same routes as
///    `hsm_console` (`/api/company/*`, `/api/console/*`), sharing this process’s Paperclip layer.
///    Disable with `HSM_EMBED_CONSOLE_API=0` if you run `hsm_console` separately.
///  - the Paperclip **Intelligence Layer** (goals, DRI registry, capability tracking) — **runtime state**, not a company pack
///  - the DKS evolution heartbeat (in-process tokio task)
///  - gateway message processing (Discord / Telegram)
///
/// Company packs (e.g. on-disk agent/skill trees imported into Postgres) supply **content**;
/// goals, DRIs, and capabilities for work inside HSM-II come from this **shared** Intelligence Layer
/// attached to `ApiState`. Rebuild and **restart** this binary (same flags/profile as before) so the
/// API serves the code you compiled — long-lived processes keep the old in-memory layer until replaced.
///
/// The agent's world is synced into the shared API state after every turn so the
/// dashboard always reflects the live in-memory state rather than a stale snapshot.
async fn cmd_start(home: &PathBuf, daemon: bool, discord: bool, telegram: bool) -> Result<()> {
    // Check if initialized (LadybugDB format)
    let initialized = hyper_stigmergy::embedded_graph_store::EmbeddedGraphStore::exists()
        || home.join("config.json").exists();

    if !initialized {
        println!("Enhanced agent not initialized. Run `hsmii bootstrap` first.");
        return Ok(());
    }

    // ── 1. Load the agent once; keep it in memory for the process lifetime ──
    let mut agent = EnhancedPersonalAgent::initialize(home).await?;

    // Enable memory journal by default so Honcho can read session transcripts.
    agent.config.memory_journal = true;

    println!("🚀 Starting Enhanced HSM-II Personal Agent (unified runtime)");
    println!("   Agents: {}", agent.world.agents.len());
    for (i, a) in agent.world.agents.iter().enumerate() {
        let jw = a.calculate_jw(agent.world.global_coherence(), 3);
        println!("     {}. {:?} (JW: {:.3})", i + 1, a.role, jw);
    }
    println!("   Coherence: {:.3}", agent.world.global_coherence());
    println!(
        "   Council: {}",
        if agent.config.enable_council {
            "✓ enabled"
        } else {
            "✗ disabled"
        }
    );
    println!(
        "   CASS: {}",
        if agent.config.enable_cass {
            "✓ enabled"
        } else {
            "✗ disabled"
        }
    );
    println!("   LadybugDB: ✓ active");
    println!("   Memory journal: ✓ enabled");
    println!("   Honcho inference: ✓ enabled\n");

    // ── 2. Build shared API state seeded from the live world ────────────────
    let shared_inner: Arc<RwLock<SharedState>> =
        Arc::new(RwLock::new(SharedState::with_world(agent.world.clone())));

    let honcho_api_state = HonchoApiState {
        honcho_home: agent.base_path.join("honcho"),
        hybrid_memory: Arc::clone(&agent.honcho_memory),
    };

    // ── 2b. Shared runtime Intelligence Layer (not company-pack data) ─────
    let mut intelligence = hyper_stigmergy::paperclip::IntelligenceLayer::new();
    // Optional template seeds default capabilities / DRIs / role shape for this process
    let template_path = home.join("config").join("paperclip_template.json");
    if template_path.exists() {
        match hyper_stigmergy::paperclip::template::CompanyTemplate::load(&template_path) {
            Ok(tpl) => {
                tpl.apply_to(&mut intelligence);
                info!(
                    "Loaded Paperclip template: {} capabilities, {} DRIs",
                    intelligence.capabilities.len(),
                    intelligence.dri_registry.len()
                );
            }
            Err(e) => warn!("Failed to load Paperclip template: {e}"),
        }
    } else {
        // Apply default template
        let tpl = hyper_stigmergy::paperclip::template::CompanyTemplate::paperclip_default();
        tpl.apply_to(&mut intelligence);
    }
    let intelligence = Arc::new(Mutex::new(intelligence));

    let api_state = ApiState::from_shared(Arc::clone(&shared_inner))
        .with_honcho(honcho_api_state)
        .with_intelligence(Arc::clone(&intelligence));

    // ── 3. Start the Axum API server as an in-process task ──────────────────
    let api_port: u16 = std::env::var("HSM_API_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3000);

    let app = hyper_stigmergy::api::api_router(api_state)
        .layer(local_cors_layer())
        .layer(tower_http::trace::TraceLayer::new_for_http());

    let addr: std::net::SocketAddr = format!("127.0.0.1:{api_port}").parse()?;
    tokio::spawn(async move {
        match tokio::net::TcpListener::bind(addr).await {
            Ok(listener) => {
                info!("HSM-II API server listening on {addr}");
                if let Err(e) = axum::serve(listener, app).await {
                    warn!("API server error: {e}");
                }
            }
            Err(e) => warn!("API server bind failed ({addr}): {e}"),
        }
    });

    println!("   REST API: http://127.0.0.1:{api_port}");
    println!("   Web UI:   http://127.0.0.1:3001 (run `cd web && npm run dev`)");

    // ── 3b. Company OS Postgres pool (shared by console + intelligence heartbeat)
    let company_db: Option<sqlx::PgPool> = match hyper_stigmergy::company_os::connect_optional().await {
        Ok(db) => db,
        Err(e) => {
            warn!(
                error = %e,
                "Company OS: PostgreSQL unavailable (set HSM_COMPANY_OS_DATABASE_URL)"
            );
            None
        }
    };

    // ── 3c. Embedded company console API (parity with `hsm_console`) ───────
    if embed_company_console_api_enabled() {
        let console_port: u16 = std::env::var("HSM_CONSOLE_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(3847);
        let console_addr: SocketAddr = format!("127.0.0.1:{console_port}").parse()?;

        if let Some(ref pool) = company_db {
            hyper_stigmergy::company_os::start_automation_worker(pool.clone());
            info!("Company OS automation worker started (embedded console)");
        }

        let console_state =
            ConsoleState::with_paperclip_layer(home.clone(), company_db.clone(), Arc::clone(&intelligence));
        let console_app = console_router(console_state)
            .layer(local_cors_layer())
            .layer(tower_http::trace::TraceLayer::new_for_http());

        tokio::spawn(async move {
            match tokio::net::TcpListener::bind(console_addr).await {
                Ok(listener) => {
                    info!("HSM company console API (embedded) listening on {console_addr}");
                    if let Err(e) = axum::serve(listener, console_app).await {
                        warn!("Embedded company console API error: {e}");
                    }
                }
                Err(e) => warn!(
                    "Embedded company console bind failed ({console_addr}): {e} — set HSM_EMBED_CONSOLE_API=0 or free the port (e.g. stop `hsm_console`)"
                ),
            }
        });

        println!("   Company console API: http://127.0.0.1:{console_port}  (set NEXT_PUBLIC_API_BASE for web/company-console)");
    } else {
        println!("   Company console API: off (HSM_EMBED_CONSOLE_API=0) — run `hsm_console` for /api/company");
    }
    println!();

    // ── 4. Helper: sync agent world into the shared API state ───────────────
    let sync_world = {
        let shared_inner = Arc::clone(&shared_inner);
        move |world: hyper_stigmergy::hyper_stigmergy::HyperStigmergicMorphogenesis| {
            let shared = Arc::clone(&shared_inner);
            tokio::spawn(async move {
                shared.write().await.world = Some(world);
            });
        }
    };

    // ── 5. Setup gateway if requested ────────────────────────────────────────
    let mut msg_rx = None;
    if discord || telegram {
        let mut gateway_config = gateway::Config::default();
        if discord {
            gateway_config.discord_token = std::env::var("DISCORD_TOKEN").ok();
        }
        if telegram {
            gateway_config.telegram_token = std::env::var("TELEGRAM_TOKEN").ok();
        }
        let rx = agent.start_gateway(gateway_config).await?;
        msg_rx = Some(rx);
        println!("Gateway(s) started. Telegram/Discord messages will be processed.\n");
    }

    // Wrap agent in Arc<Mutex<>> for shared ownership across tasks
    let agent = Arc::new(Mutex::new(agent));

    if daemon {
        // ── 6a. Daemon mode ──────────────────────────────────────────────────
        info!("Running in daemon mode");

        // Gateway message processing
        if let Some(mut rx) = msg_rx {
            let agent_clone = Arc::clone(&agent);
            let sync = sync_world.clone();
            tokio::spawn(async move {
                while let Some((msg, response_tx)) = rx.recv().await {
                    let mut ag = agent_clone.lock().await;
                    let response = match ag.handle_message(msg).await {
                        Ok(resp) => resp,
                        Err(e) => {
                            error!("handle_message error: {e}");
                            format!("Error: {e}")
                        }
                    };
                    sync(ag.world.clone());
                    let _ = ag.save().await;
                    let _ = response_tx.send(response);
                }
            });
        }

        // DKS + Intelligence Layer heartbeat (uses the shared agent — no disk reload)
        {
            let agent_clone = Arc::clone(&agent);
            let intelligence_clone = Arc::clone(&intelligence);
            let sync = sync_world.clone();
            let heartbeat_pool = company_db.clone();
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(60));
                loop {
                    interval.tick().await;
                    let mut ag = agent_clone.lock().await;
                    let heartbeat_company_id = if let Some(ref pool) = heartbeat_pool {
                        if let Ok(raw) = std::env::var("HSM_PRIMARY_COMPANY_ID") {
                            uuid::Uuid::parse_str(raw.trim()).ok()
                        } else {
                            sqlx::query_scalar::<_, uuid::Uuid>(
                                "SELECT id FROM companies ORDER BY created_at LIMIT 1",
                            )
                            .fetch_optional(pool)
                            .await
                            .ok()
                            .flatten()
                        }
                    } else {
                        None
                    };

                    // DKS tick
                    if ag.config.enable_dks {
                        let _ = ag.services.dks.tick();
                    }

                    // Intelligence Layer: build snapshot → scan world → tick → persist
                    {
                        use hyper_stigmergy::paperclip::intelligence::CompanyOsSnapshot;

                        // 1. Build a Company OS snapshot (Postgres → Paperclip inbound)
                        let snapshot = if let (Some(ref pool), Some(cid)) =
                            (heartbeat_pool.as_ref(), heartbeat_company_id)
                        {
                            match hyper_stigmergy::company_os::intelligence_signals::build_snapshot(
                                pool, cid,
                            )
                            .await
                            {
                                Ok(s) => s,
                                Err(e) => {
                                    warn!("intelligence heartbeat: snapshot failed: {e}");
                                    CompanyOsSnapshot::default()
                                }
                            }
                        } else {
                            CompanyOsSnapshot::default()
                        };

                        // 2. Scan world with live snapshot + tick
                        let mut il = intelligence_clone.lock().await;
                        il.scan_world(ag.world.global_coherence(), ag.world.tick_count, &snapshot);
                        let results = il.tick();

                        // 3. Log failures + policy/direction post-checks
                        for (ref sig, ref r) in &results {
                            if !r.success {
                                info!(
                                    goal_id = ?r.goal_id,
                                    escalated_to = ?r.escalated_to,
                                    "intelligence: {}",
                                    r.message
                                );
                            }
                            // 3a. Policy check: if composition created a goal, evaluate policy rules
                            if r.success {
                                if let (Some(ref pool), Some(cid)) =
                                    (heartbeat_pool.as_ref(), heartbeat_company_id)
                                {
                                        use hyper_stigmergy::company_os::intelligence_signals::{evaluate_policy, check_direction_alignment, query_dri_memory_context};

                                        let kind_str = format!("{:?}", sig.kind);
                                        let policy = evaluate_policy(pool, cid, &kind_str, sig.severity).await;

                                        if let Ok(ref pd) = policy {
                                            if !pd.allowed {
                                                // Blocked by policy — remove goal from Paperclip
                                                if let Some(ref gid) = r.goal_id {
                                                    il.goals.remove(gid);
                                                    info!(goal_id = %gid, reason = %pd.reason, "intelligence: goal blocked by policy");
                                                }
                                            } else if pd.requires_human {
                                                // Mark goal metadata for requires_human escalation
                                                if let Some(ref gid) = r.goal_id {
                                                    if let Some(goal) = il.goals.get_mut(gid) {
                                                        goal.metadata.insert("requires_human".into(), serde_json::json!(true));
                                                        goal.metadata.insert("policy_reason".into(), serde_json::json!(pd.reason));
                                                    }
                                                    info!(goal_id = %gid, "intelligence: goal requires human approval per policy");
                                                }
                                            }
                                        }

                                        // 3b. Direction alignment check
                                        if let Some(ref gid) = r.goal_id {
                                            if let Some(goal) = il.goals.get_mut(gid) {
                                                if let Ok((aligned, dir_excerpt)) = check_direction_alignment(pool, cid, &goal.title).await {
                                                    goal.metadata.insert("direction_aligned".into(), serde_json::json!(aligned));
                                                    if let Some(excerpt) = dir_excerpt {
                                                        goal.metadata.insert("direction_excerpt".into(), serde_json::json!(excerpt));
                                                    }
                                                    if !aligned {
                                                        info!(goal_id = %gid, title = %goal.title, "intelligence: goal may not align with company direction");
                                                    }
                                                }
                                            }
                                        }

                                        // 3c. Memory-driven DRI routing refinement
                                        if let Some(ref gid) = r.goal_id {
                                            if let Some(goal) = il.goals.get_mut(gid) {
                                                let domains: Vec<String> = goal.required_capabilities.clone();
                                                if let Ok(mem_ctx) = query_dri_memory_context(pool, cid, &domains, 5).await {
                                                    if !mem_ctx.is_empty() {
                                                        let hints: Vec<serde_json::Value> = mem_ctx.iter().map(|(t, b)| {
                                                            serde_json::json!({"title": t, "body": b})
                                                        }).collect();
                                                        goal.metadata.insert("dri_memory_context".into(), serde_json::json!(hints));
                                                    }
                                                }
                                            }
                                        }
                                }
                            }
                        }

                        // 4. Persist signals + sync goals/DRIs to Postgres
                        if let (Some(ref pool), Some(cid)) =
                            (heartbeat_pool.as_ref(), heartbeat_company_id)
                        {
                                // 4a. Persist signals
                                let processed: Vec<hyper_stigmergy::company_os::intelligence_signals::ProcessedSignal> =
                                    results.iter().map(|(sig, cr)| {
                                        hyper_stigmergy::company_os::intelligence_signals::ProcessedSignal {
                                            signal: sig.clone(),
                                            composition_success: Some(cr.success),
                                            composed_goal_pg_id: None,
                                            composed_task_pg_id: None,
                                            escalated_to: cr.escalated_to.clone(),
                                        }
                                    }).collect();
                                if !processed.is_empty() {
                                    match hyper_stigmergy::company_os::intelligence_signals::persist_signals(pool, cid, &processed).await {
                                        Ok(n) => {
                                            if n > 0 {
                                                info!(count = n, "intelligence: persisted {n} signals to Postgres");
                                            }
                                        }
                                        Err(e) => warn!("intelligence: persist_signals failed: {e}"),
                                    }
                                }

                                // 4b. Sync Paperclip goals → Postgres (round-trip via paperclip_goal_id)
                                let goals: Vec<hyper_stigmergy::paperclip::goal::Goal> =
                                    il.list_goals().iter().map(|g| (*g).clone()).collect();
                                if !goals.is_empty() {
                                    match hyper_stigmergy::company_os::paperclip_sync::sync_paperclip_goals(pool, cid, goals).await {
                                        Ok(report) => {
                                            let ins = report.get("inserted").and_then(|v| v.as_u64()).unwrap_or(0);
                                            let upd = report.get("updated").and_then(|v| v.as_u64()).unwrap_or(0);
                                            if ins + upd > 0 {
                                                info!(inserted = ins, updated = upd, "intelligence: synced goals to Postgres");
                                            }
                                        }
                                        Err(e) => warn!("intelligence: goal sync failed: {e}"),
                                    }
                                }

                                // 4c. Sync Paperclip DRIs → Postgres
                                let dris: Vec<hyper_stigmergy::paperclip::dri::DriEntry> =
                                    il.dri_registry.all().cloned().collect();
                                if !dris.is_empty() {
                                    match hyper_stigmergy::company_os::paperclip_sync::sync_paperclip_dris(pool, cid, dris).await {
                                        Ok(_) => {}
                                        Err(e) => warn!("intelligence: DRI sync failed: {e}"),
                                    }
                                }
                        }
                    }

                    sync(ag.world.clone());
                    let _ = ag.save().await;
                }
            });
        }

        // KAIROS-style idle maintenance: autoDream + HEARTBEAT.md without inbound messages
        if hyper_stigmergy::personal::kairos::kairos_enabled() {
            let kairos_secs = hyper_stigmergy::personal::kairos::tick_interval_secs();
            let agent_k = Arc::clone(&agent);
            let sync_k = sync_world.clone();
            tokio::spawn(async move {
                let mut interval =
                    tokio::time::interval(tokio::time::Duration::from_secs(kairos_secs));
                loop {
                    interval.tick().await;
                    let mut ag = agent_k.lock().await;
                    hyper_stigmergy::personal::kairos::run_idle_maintenance(&mut *ag).await;
                    sync_k(ag.world.clone());
                    let _ = ag.save().await;
                }
            });
        }

        if hyper_stigmergy::personal::hsm_cron::cron_file_configured() {
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(30));
                loop {
                    interval.tick().await;
                    hyper_stigmergy::personal::hsm_cron::tick_daemon_jobs().await;
                }
            });
        }

        println!("✓ Agent is running in daemon mode");
        if telegram {
            println!("  Telegram bot: active");
        }
        if discord {
            println!("  Discord bot: active");
        }
        println!("  DKS evolution: every 60s");
        println!("  Intelligence Layer: every 60s");
        if hyper_stigmergy::personal::kairos::kairos_enabled() {
            println!(
                "  KAIROS idle maintenance: every {}s (autoDream + HEARTBEAT.md when due)",
                hyper_stigmergy::personal::kairos::tick_interval_secs()
            );
        }
        if hyper_stigmergy::personal::hsm_cron::cron_file_configured() {
            println!("  File cron: every 30s (config/hsm_cron.json or HSM_CRON_CONFIG)");
        }
        println!("  Auto-save: enabled\n");
        println!("Press Ctrl+C to stop.");

        tokio::signal::ctrl_c().await?;
        println!("\nShutting down...");
        agent.lock().await.save().await?;
    } else {
        // ── 6b. Interactive (stdin) mode ─────────────────────────────────────
        println!("Interactive mode. Type 'exit' to quit.\n");

        use tokio::io::{stdin, AsyncBufReadExt, BufReader};
        let stdin_reader = BufReader::new(stdin());
        let mut lines = stdin_reader.lines();

        loop {
            tokio::select! {
                line_result = lines.next_line() => {
                    let line: String = line_result?.unwrap_or_default();
                    match line.as_str() {
                        "exit" | "quit" => break,
                        "" => continue,
                        input => {
                            let msg = gateway::Message {
                                id: uuid::Uuid::new_v4().to_string(),
                                platform: gateway::Platform::Cli,
                                channel_id: "cli".to_string(),
                                channel_name: None,
                                user_id: "user".to_string(),
                                user_name: "User".to_string(),
                                content: input.to_string(),
                                timestamp: chrono::Utc::now(),
                                attachments: vec![],
                                reply_to: None,
                            };

                            let mut ag = agent.lock().await;
                            match ag.handle_message(msg).await {
                                Ok(response) => {
                                    let stats = ag.get_stats();
                                    if stats.council_invocations > 0 {
                                        println!(
                                            "\n[Coherence: {:.3} | Council used: {} | Agents: {}]",
                                            stats.coherence,
                                            stats.council_invocations,
                                            stats.agent_count
                                        );
                                    }
                                    println!("\n{}", response);
                                }
                                Err(e) => error!("Error: {e}"),
                            }
                            sync_world(ag.world.clone());
                        }
                    }
                }
                Some((msg, response_tx)) = async {
                    if let Some(ref mut rx) = msg_rx { rx.recv().await } else { None }
                } => {
                    let mut ag = agent.lock().await;
                    let response = match ag.handle_message(msg).await {
                        Ok(resp) => resp,
                        Err(e) => format!("Error: {e}"),
                    };
                    sync_world(ag.world.clone());
                    let _ = response_tx.send(response);
                }
            }
        }

        agent.lock().await.save().await?;
    }

    Ok(())
}

/// Chat with agent
async fn cmd_chat(home: &PathBuf, message: Option<String>) -> Result<()> {
    let mut agent = EnhancedPersonalAgent::initialize(home).await?;

    if let Some(msg) = message {
        // Single message mode
        let gateway_msg = gateway::Message {
            id: uuid::Uuid::new_v4().to_string(),
            platform: gateway::Platform::Cli,
            channel_id: "cli".to_string(),
            channel_name: None,
            user_id: "user".to_string(),
            user_name: "User".to_string(),
            content: msg,
            timestamp: chrono::Utc::now(),
            attachments: vec![],
            reply_to: None,
        };

        let response = agent.handle_message(gateway_msg).await?;
        println!("{}", response);
    } else {
        // Interactive mode (same as start without daemon)
        // ...implementation similar to cmd_start
    }

    agent.save().await?;
    Ok(())
}

/// Execute a task
async fn cmd_do(home: &PathBuf, task: &str) -> Result<()> {
    let mut agent = EnhancedPersonalAgent::initialize(home).await?;

    println!("Executing: {}\n", task);
    println!(
        "Using {} agents with coherence {:.3}\n",
        agent.world.agents.len(),
        agent.world.global_coherence()
    );

    // Create message for the task
    let msg = gateway::Message {
        id: uuid::Uuid::new_v4().to_string(),
        platform: gateway::Platform::Cli,
        channel_id: "cli".to_string(),
        channel_name: None,
        user_id: "user".to_string(),
        user_name: "User".to_string(),
        content: task.to_string(),
        timestamp: chrono::Utc::now(),
        attachments: vec![],
        reply_to: None,
    };

    let result = agent.handle_message(msg).await;
    let output = match result {
        Ok(resp) => resp,
        Err(e) => format!("Error: {}", e),
    };

    println!("{}\n", output);

    agent.save().await?;
    Ok(())
}

/// Configuration commands
async fn cmd_config(home: &PathBuf, action: ConfigAction) -> Result<()> {
    match action {
        ConfigAction::Show => {
            let agent = EnhancedPersonalAgent::initialize(home).await?;
            println!("# HSM-II Configuration\n");
            println!("Agents: {}", agent.world.agents.len());
            println!("Coherence: {:.3}", agent.world.global_coherence());
            println!("Beliefs: {}", agent.world.beliefs.len());
            println!("Edges: {}", agent.world.edges.len());
            println!(
                "\nCouncil: {}",
                if agent.config.enable_council {
                    "enabled"
                } else {
                    "disabled"
                }
            );
            println!(
                "CASS: {}",
                if agent.config.enable_cass {
                    "enabled"
                } else {
                    "disabled"
                }
            );
            println!(
                "DKS: {}",
                if agent.config.enable_dks {
                    "enabled"
                } else {
                    "disabled"
                }
            );
            println!(
                "JouleWork tracking: {}",
                if agent.config.track_joulework {
                    "enabled"
                } else {
                    "disabled"
                }
            );
        }
        ConfigAction::Persona => {
            // Open SOUL.md in editor
            let path = home.join("SOUL.md");
            let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vim".to_string());
            std::process::Command::new(editor).arg(&path).status()?;
        }
        ConfigAction::Set { key, value } => {
            println!("Setting {} = {}", key, value);
            // TODO: Implement config setting
        }
    }
    Ok(())
}

/// Memory commands - now uses LadybugDB beliefs
async fn cmd_memory(home: &PathBuf, action: MemoryAction) -> Result<()> {
    let mut agent = EnhancedPersonalAgent::initialize(home).await?;

    match action {
        MemoryAction::Show { n } => {
            use hyper_stigmergy::hyper_stigmergy::BeliefSource;
            println!("# LadybugDB Beliefs (showing {})\n", n);
            for belief in agent.world.beliefs.iter().rev().take(n) {
                let source_icon = match belief.source {
                    BeliefSource::UserProvided => "👤",
                    BeliefSource::Observation => "👁️",
                    BeliefSource::Reflection => "💭",
                    BeliefSource::Inference => "🔗",
                    BeliefSource::Prediction => "🔮",
                };
                println!(
                    "{} [{:.2}] {}",
                    source_icon, belief.confidence, belief.content
                );
            }
            println!("\n# Hyperedges\n");
            for (i, edge) in agent.world.edges.iter().rev().take(n).enumerate() {
                println!(
                    "Edge {}: agents {:?}, weight={:.2}, emergent={}",
                    i, edge.participants, edge.weight, edge.emergent
                );
            }
        }
        MemoryAction::Search { query } => {
            println!("# Vector search for: {}\n", query);
            let beliefs = agent.get_relevant_beliefs(&query).await?;
            for belief in beliefs {
                println!("[{:.2}] {}", belief.confidence, belief.content);
            }
        }
        MemoryAction::Add {
            content,
            category: _,
        } => {
            use hyper_stigmergy::hyper_stigmergy::{Belief, BeliefSource};
            // Add as a belief to the world
            let id = agent.world.beliefs.len();
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let (l0, l1) = hyper_stigmergy::memory::derive_hierarchy(&content);
            agent.world.beliefs.push(Belief {
                id,
                content: content.clone(),
                abstract_l0: Some(l0),
                overview_l1: Some(l1),
                confidence: 0.9,
                source: BeliefSource::UserProvided,
                supporting_evidence: vec!["User added via CLI".to_string()],
                contradicting_evidence: vec![],
                created_at: now,
                updated_at: now,
                update_count: 0,
                owner_namespace: None,
                supersedes_belief_id: None,
                evidence_belief_ids: Vec::new(),
                human_committed: true,
            });
            agent.save().await?;
            println!("✓ Added to LadybugDB beliefs");
        }
    }
    Ok(())
}

/// Run DKS evolution tick
async fn cmd_heartbeat(home: &PathBuf) -> Result<()> {
    let mut heartbeat = Heartbeat::load(home).await?;
    let hb_results = heartbeat.tick(home).await?;
    if hb_results.is_empty() {
        println!("Heartbeat: no checklist/routine actions due.");
    } else {
        println!("Heartbeat actions:");
        for row in &hb_results {
            let status = if row.success { "ok" } else { "error" };
            println!("- [{}] {}: {}", status, row.action, row.message);
        }
    }

    let mut agent = EnhancedPersonalAgent::initialize(home).await?;
    println!("\nRunning DKS evolution tick...\n");
    let tick = agent.services.dks.tick();
    let stats = agent.services.dks.stats();
    println!("Generation: {}", tick.generation);
    println!("Population: {}", stats.size);
    println!("Avg persistence: {:.3}", stats.average_persistence);
    agent.save().await?;
    println!("\nState saved to LadybugDB");

    Ok(())
}

/// Bootstrap new enhanced agent
async fn cmd_bootstrap(home: &PathBuf) -> Result<()> {
    if home.join("config.json").exists()
        || hyper_stigmergy::embedded_graph_store::EmbeddedGraphStore::exists()
    {
        println!("Enhanced agent already initialized at {}", home.display());
        println!("Run `hsmii config` to view configuration.");
        return Ok(());
    }

    println!("🌱 Bootstrapping Enhanced HSM-II Personal Agent\n");

    let mut agent = EnhancedPersonalAgent::initialize(home).await?;

    // Calculate initial JW scores for display
    let coherence = agent.world.global_coherence();
    println!(
        "✨ Created {} agents (coherence: {:.3}):",
        agent.world.agents.len(),
        coherence
    );
    for (i, agent_info) in agent.world.agents.iter().enumerate() {
        let jw = agent_info.calculate_jw(coherence, 3);
        println!("  {}. {:?} - JW: {:.3}", i + 1, agent_info.role, jw);
    }

    // Save the initial world state
    agent.save().await?;

    println!("\n✓ LadybugDB initialized and saved");
    println!("✓ CASS skill system ready");
    println!("✓ Council deliberation enabled");
    println!("✓ DKS evolution active");
    println!("✓ JouleWork tracking on");

    println!("\nNext steps:");
    println!("  - Run `hsmii start` to chat with your multi-agent system");
    println!("  - Run `hsmii start --telegram` to enable Telegram bot");
    println!("  - Use `hsmii memory` to view/query beliefs");

    Ok(())
}

/// Onboard: guided questionnaire to teach HSM-II about the business
async fn cmd_onboard(home: &PathBuf) -> Result<()> {
    if !hyper_stigmergy::embedded_graph_store::EmbeddedGraphStore::exists() {
        println!("Agent not initialized. Run `hsmii bootstrap` first.");
        return Ok(());
    }

    let mut agent = EnhancedPersonalAgent::initialize(home).await?;
    let beliefs_before = agent.world.beliefs.len();

    let _result = hyper_stigmergy::onboard::run_onboard_interactive(
        &mut agent.world,
        &mut agent.living_prompt,
    )
    .await?;

    agent.save().await?;

    println!(
        "\n✓ Saved to LadybugDB ({} → {} beliefs)",
        beliefs_before,
        agent.world.beliefs.len()
    );
    println!("  Run `hsmii memory show` to review your beliefs");
    println!("  Run `hsmii start` to chat with business-aware HSM-II");

    Ok(())
}

/// Run MiroFish trajectory prediction
async fn cmd_predict(
    home: &PathBuf,
    template_id: Option<String>,
    topic: Option<String>,
    list_templates: bool,
    show_history: bool,
    show_backtest: bool,
    outcome_arg: Option<String>,
) -> Result<()> {
    use hyper_stigmergy::mirofish::{builtin_templates, MiroFishEngine, PredictionStore};
    use std::collections::HashMap;

    // List templates mode
    if list_templates {
        println!("\n📊 Available MiroFish Scenario Templates\n");
        for t in builtin_templates() {
            println!("  {} — {}", t.id, t.name);
            println!("    Domain: {}", t.domain);
            println!(
                "    Required: {}",
                t.required_variables
                    .iter()
                    .map(|v| v.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            println!("    Variants: {}", t.suggested_variants.join(", "));
            println!();
        }
        println!("Usage: hsmii predict --template pricing_strategy");
        println!("       hsmii predict \"What happens if we raise prices 20%?\"");
        println!("       hsmii predict --history              (show past predictions)");
        println!("       hsmii predict --backtest             (calibration stats)");
        println!("       hsmii predict --outcome \"pred_123 actual_result correct\"");
        return Ok(());
    }

    // History mode: show past predictions
    if show_history {
        let store = PredictionStore::load(home);
        println!(
            "\n📜 PREDICTION HISTORY ({} predictions, {} analyses)\n",
            store.records.len(),
            store.analyses.len()
        );

        let pending = store.pending_outcomes();
        if !pending.is_empty() {
            println!("⏳ AWAITING OUTCOMES ({}):", pending.len());
            println!("{}", "─".repeat(50));
            for p in &pending {
                println!(
                    "  ID: {}  | Topic: {} | Confidence: {:.0}%",
                    p.id,
                    p.topic,
                    p.predicted_confidence * 100.0
                );
                println!("  Predicted: {}", p.predicted_outcome);
                println!();
            }
        }

        let analyses = store.list_analyses();
        if !analyses.is_empty() {
            println!("📊 STORED ANALYSES (last 10):");
            println!("{}", "─".repeat(50));
            for a in analyses.iter().take(10) {
                let recal = if a.analysis.recalibrated {
                    " [recalibrated]"
                } else {
                    ""
                };
                println!(
                    "  {} | {} | Impact: {:.1} | Confidence: {:.0}%{}",
                    a.id,
                    a.analysis.domain,
                    a.analysis.expected_impact,
                    a.analysis.scenario_report.overall_confidence * 100.0,
                    recal
                );
                if !a.notes.is_empty() {
                    println!("    Notes: {}", a.notes);
                }
            }
        }

        if store.records.is_empty() && store.analyses.is_empty() {
            println!("  No predictions yet. Run: hsmii predict --template pricing_strategy");
        }
        return Ok(());
    }

    // Backtest mode: show calibration stats
    if show_backtest {
        let store = PredictionStore::load(home);
        let stats = store.calibration_stats();

        println!("\n🎯 CALIBRATION & BACK-TESTING REPORT\n");
        println!("{}", "═".repeat(50));
        println!("  Total predictions evaluated: {}", stats.total_evaluated);
        println!("  Correct predictions:         {}", stats.correct);
        println!(
            "  Actual accuracy:             {:.1}%",
            stats.actual_accuracy * 100.0
        );
        println!(
            "  Avg predicted confidence:    {:.1}%",
            stats.avg_predicted_confidence * 100.0
        );
        println!(
            "  Calibration error:           {:.2}",
            stats.calibration_error
        );
        println!("  Direction:                   {}", stats.direction);
        println!(
            "  Adjustment factor:           {:.2}×",
            stats.adjustment_factor
        );
        println!();

        if stats.direction == "overconfident" {
            println!("  ⚠ Your model predicts with higher confidence than warranted.");
            println!(
                "    Confidence scores are being adjusted down by {:.0}%.",
                (1.0 - stats.adjustment_factor) * 100.0
            );
        } else if stats.direction == "underconfident" {
            println!("  💡 Your model is more accurate than it thinks.");
            println!(
                "    Confidence scores are being adjusted up by {:.0}%.",
                (stats.adjustment_factor - 1.0) * 100.0
            );
        } else if stats.direction == "well-calibrated" {
            println!("  ✅ Model is well-calibrated. Confidence scores are accurate.");
        }

        let synthetic_count = store
            .records
            .iter()
            .filter(|r| r.id.starts_with("synthetic_"))
            .count();
        let real_count = stats.total_evaluated - synthetic_count;
        if synthetic_count > 0 {
            println!(
                "\n  📊 Data sources: {} synthetic bootstrap + {} real outcomes",
                synthetic_count, real_count
            );
            if real_count < 10 {
                println!("  💡 Record more outcomes to improve calibration accuracy:");
                println!("     hsmii predict --outcome \"<prediction_id> <what_happened> correct|wrong\"");
            }
        }
        return Ok(());
    }

    // Outcome recording mode
    if let Some(outcome_str) = outcome_arg {
        let parts: Vec<&str> = outcome_str.splitn(3, ' ').collect();
        if parts.len() < 3 {
            println!(
                "Usage: hsmii predict --outcome \"<prediction_id> <actual_result> correct|wrong\""
            );
            println!("Example: hsmii predict --outcome \"pred_1710547200 revenue_grew correct\"");
            return Ok(());
        }
        let pred_id = parts[0];
        let actual = parts[1];
        let was_correct = parts[2].to_lowercase() == "correct";

        let mut store = PredictionStore::load(home);
        store.record_outcome(pred_id, actual, was_correct);
        println!(
            "✅ Recorded outcome for '{}': {} ({})",
            pred_id,
            actual,
            if was_correct { "correct" } else { "wrong" }
        );

        let stats = store.calibration_stats();
        println!(
            "   Updated calibration: {:.1}% accuracy over {} predictions ({})",
            stats.actual_accuracy * 100.0,
            stats.total_evaluated,
            stats.direction
        );
        return Ok(());
    }

    // ── Main prediction flow ─────────────────────────────────────────────

    if !hyper_stigmergy::embedded_graph_store::EmbeddedGraphStore::exists() {
        println!("Agent not initialized. Run `hsmii bootstrap` first.");
        return Ok(());
    }

    let agent = EnhancedPersonalAgent::initialize(home).await?;
    let mut llm_config = hyper_stigmergy::ollama_client::OllamaConfig::default();
    if llm_config.model == "auto" {
        llm_config.model = hyper_stigmergy::ollama_client::OllamaConfig::detect_model(
            &llm_config.host,
            llm_config.port,
        )
        .await;
    }
    let llm = hyper_stigmergy::ollama_client::OllamaClient::new(llm_config);
    let mut engine = MiroFishEngine::new(llm, home);

    if let Some(tid) = template_id {
        // Template-based prediction
        let templates = builtin_templates();
        let template = templates.iter().find(|t| t.id == tid).ok_or_else(|| {
            anyhow::anyhow!(
                "Template '{}' not found. Use --list to see available templates.",
                tid
            )
        })?;

        println!("\n🐟 MiroFish Trajectory Analysis: {}\n", template.name);

        // Collect required variables interactively
        use tokio::io::{stdin, AsyncBufReadExt, BufReader};
        let stdin_reader = BufReader::new(stdin());
        let mut lines = stdin_reader.lines();
        let mut variables = HashMap::new();

        for var in &template.required_variables {
            println!("  {} (e.g., {})", var.description, var.example);
            print!("  > ");
            std::io::Write::flush(&mut std::io::stdout()).ok();
            let answer = lines.next_line().await?.unwrap_or_default();
            variables.insert(var.name.clone(), answer.trim().to_string());
        }

        for var in &template.optional_variables {
            let default = var.default.as_deref().unwrap_or("skip");
            println!("  {} [default: {}]", var.description, default);
            print!("  > ");
            std::io::Write::flush(&mut std::io::stdout()).ok();
            let answer = lines.next_line().await?.unwrap_or_default();
            let value = if answer.trim().is_empty() {
                var.default.clone().unwrap_or_default()
            } else {
                answer.trim().to_string()
            };
            if !value.is_empty() {
                variables.insert(var.name.clone(), value);
            }
        }

        println!("\n⏳ Running trajectory analysis...\n");

        let analysis = engine
            .analyze_with_template(template, &variables, &agent.world.beliefs)
            .await?;

        // Display results
        println!("📈 TRAJECTORY ANALYSIS RESULTS");
        println!("{}\n", "═".repeat(60));

        println!("🎯 Most Likely Outcome: {}", analysis.most_likely_outcome);
        println!("📊 Expected Impact: {:.1}/10", analysis.expected_impact);
        if analysis.recalibrated {
            println!("🔧 Confidence recalibrated (adjustment factor applied from {} historical predictions)",
                analysis.calibration.as_ref().map(|c| c.total_evaluated).unwrap_or(0));
        }
        println!();

        println!(
            "🔀 PROBABILITY FLOW ({} time steps)",
            analysis.flow_network.time_steps
        );
        println!("{}", "─".repeat(40));
        for state in &analysis.flow_network.states {
            if state.probability > 0.01 {
                let bar_len = (state.probability * 30.0) as usize;
                let bar = "█".repeat(bar_len);
                println!(
                    "  {:30} {:5.1}% {}",
                    state.description,
                    state.probability * 100.0,
                    bar
                );
            }
        }
        println!();

        println!(
            "📋 ACTION TRAJECTORY ({} steps)",
            analysis.trajectory.steps.len()
        );
        println!("{}", "─".repeat(40));
        for (i, step) in analysis.trajectory.steps.iter().enumerate() {
            let recal_note = if analysis.recalibrated && i < analysis.step_scores.len() {
                format!(" → recal: {:.0}%", analysis.step_scores[i] * 100.0)
            } else {
                String::new()
            };
            println!(
                "  Step {}: {} (p={:.0}%{}, {})",
                step.step,
                step.action,
                step.success_probability * 100.0,
                recal_note,
                step.time_horizon
            );
            if !step.risks.is_empty() {
                println!("    ⚠ Risks: {}", step.risks.join(", "));
            }
        }
        println!(
            "  → Cumulative probability: {:.0}%",
            analysis.trajectory.cumulative_probability * 100.0
        );
        println!();

        if !analysis.scenario_report.branches.is_empty() {
            println!("🌿 SCENARIO BRANCHES");
            println!("{}", "─".repeat(40));
            for branch in &analysis.scenario_report.branches {
                println!(
                    "  {} ({:.0}%): {}",
                    branch.variant,
                    branch.confidence * 100.0,
                    &branch.prediction[..branch.prediction.len().min(120)]
                );
            }
            println!();
        }

        if !analysis.scenario_report.synthesis.is_empty() {
            println!("🧩 SYNTHESIS");
            println!("{}", "─".repeat(40));
            println!("  {}", analysis.scenario_report.synthesis);
        }

        // Validation warnings
        if let Some(ref validation) = analysis.validation {
            if !validation.warnings.is_empty() {
                println!("\n⚠ VARIABLE WARNINGS:");
                for w in &validation.warnings {
                    println!("  - {}", w);
                }
            }
        }

        println!("\n💾 Auto-saved to prediction history (use --history to view)");
    } else if let Some(topic) = topic {
        // Free-form prediction (uses base scenario simulator)
        println!("\n🐟 MiroFish Prediction: {}\n", topic);

        let seeds: Vec<String> = agent
            .world
            .beliefs
            .iter()
            .take(5)
            .map(|b| b.content.clone())
            .collect();

        let config = hyper_stigmergy::scenario_simulator::ScenarioSimulatorConfig::default();
        let simulator = hyper_stigmergy::scenario_simulator::ScenarioSimulator::new(config);
        let report = simulator
            .simulate(&topic, &seeds, None)
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        println!(
            "📊 Overall confidence: {:.0}%\n",
            report.overall_confidence * 100.0
        );
        for branch in &report.branches {
            println!(
                "  {} ({:.0}%): {}",
                branch.variant,
                branch.confidence * 100.0,
                &branch.prediction[..branch.prediction.len().min(120)]
            );
        }
        println!("\n🧩 Synthesis: {}", report.synthesis);
    } else {
        println!("Usage: hsmii predict --template <id>     (template-based analysis)");
        println!("       hsmii predict \"topic question\"     (free-form prediction)");
        println!("       hsmii predict --list               (show available templates)");
        println!("       hsmii predict --history             (show past predictions)");
        println!("       hsmii predict --backtest            (calibration stats)");
        println!("       hsmii predict --outcome \"<id> <result> correct|wrong\"");
    }

    Ok(())
}

/// Ingest a document to extract business knowledge
async fn cmd_ingest(home: &PathBuf, file_path: &str) -> Result<()> {
    if !hyper_stigmergy::embedded_graph_store::EmbeddedGraphStore::exists() {
        println!("Agent not initialized. Run `hsmii bootstrap` first.");
        return Ok(());
    }

    let mut agent = EnhancedPersonalAgent::initialize(home).await?;
    let beliefs_before = agent.world.beliefs.len();

    let config = hyper_stigmergy::onboard::IngestConfig::default();
    let _result = hyper_stigmergy::onboard::ingest_file(
        &agent.llm,
        &mut agent.world,
        &mut agent.living_prompt,
        file_path,
        &config,
    )
    .await?;

    agent.save().await?;

    println!(
        "\n✓ Saved to LadybugDB ({} → {} beliefs)",
        beliefs_before,
        agent.world.beliefs.len()
    );

    Ok(())
}

/// Check status
async fn cmd_status(home: &PathBuf) -> Result<()> {
    if !hyper_stigmergy::embedded_graph_store::EmbeddedGraphStore::exists() {
        println!("Enhanced agent not initialized.");
        println!("Run `hsmii bootstrap` to set up your HSM-II multi-agent system.");
        return Ok(());
    }

    let agent = EnhancedPersonalAgent::initialize(home).await?;
    let stats = agent.get_stats();

    println!("# HSM-II Enhanced Agent Status\n");

    println!("## Multi-Agent World");
    println!("  Agents: {}", stats.agent_count);
    println!("  Hyperedges: {}", stats.edge_count);
    println!("  Beliefs: {}", stats.belief_count);
    println!("  Global coherence: {:.3}", stats.coherence);
    println!("  Tick count: {}\n", stats.tick_count);

    println!("## Activity");
    println!("  Messages processed: {}", stats.total_messages);
    println!("  Council invocations: {}\n", stats.council_invocations);

    println!("## Agent Roles");
    for agent_info in &agent.world.agents {
        let jw = agent_info.calculate_jw(stats.coherence, 3);
        println!("  {:?}: JW={:.3}", agent_info.role, jw);
    }

    println!("\n## System");
    println!(
        "  LadybugDB: {}",
        if hyper_stigmergy::embedded_graph_store::EmbeddedGraphStore::exists() {
            "✓ active"
        } else {
            "✗ not found"
        }
    );
    println!(
        "  Council: {}",
        if agent.config.enable_council {
            "✓ enabled"
        } else {
            "✗ disabled"
        }
    );
    println!(
        "  CASS: {}",
        if agent.config.enable_cass {
            "✓ enabled"
        } else {
            "✗ disabled"
        }
    );
    println!(
        "  DKS: {}",
        if agent.config.enable_dks {
            "✓ enabled"
        } else {
            "✗ disabled"
        }
    );

    Ok(())
}

/// Start the agent with Codex-style TUI
async fn cmd_start_tui(home: &PathBuf) -> Result<()> {
    use hyper_stigmergy::tui_codex_style::{draw_codex_interface, CodexEvent, CodexState};

    // Check if initialized
    if !home.join("SOUL.md").exists() {
        println!("Agent not initialized. Run `hsmii bootstrap` first.");
        return Ok(());
    }

    // Load enhanced agent
    let agent = Arc::new(Mutex::new(EnhancedPersonalAgent::initialize(home).await?));
    let agent_name = {
        let a = agent.lock().await;
        format!("HSM-II Agent ({} agents)", a.world.agents.len())
    };
    let mut state = CodexState::new(&agent_name);

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Update state with agent info - use model from LLM
    state.model = agent.lock().await.llm.model().to_string();
    state.current_dir = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "~".to_string());

    // Welcome message with HSM-II stats
    let welcome_msg = {
        let a = agent.lock().await;
        let stats = a.get_stats();
        format!(
            "🌌 HSM-II Enhanced Agent Online\n\n\
            Active agents: {} | Coherence: {:.3}\n\
            Council: {} | CASS: {} | DKS: {}\n\n\
            What would you like to explore today?",
            stats.agent_count,
            stats.coherence,
            if a.config.enable_council {
                "✓"
            } else {
                "✗"
            },
            if a.config.enable_cass { "✓" } else { "✗" },
            if a.config.enable_dks { "✓" } else { "✗" }
        )
    };
    state.push_message("agent", &welcome_msg);

    // Channel for receiving agent responses asynchronously
    type AgentResponse = Result<String>;
    let (response_tx, mut response_rx): (
        mpsc::UnboundedSender<AgentResponse>,
        mpsc::UnboundedReceiver<AgentResponse>,
    ) = mpsc::unbounded_channel();

    let mut last_tick = tokio::time::Instant::now();
    let tick_rate = tokio::time::Duration::from_millis(100);
    let result: Result<()> = loop {
        // Update autocomplete suggestions based on input
        if state.input.starts_with('/') {
            let suggestions = get_autocomplete_suggestions(&state.input);
            let autocomplete_suggestions: Vec<AutocompleteSuggestion> = suggestions
                .into_iter()
                .map(|(cmd, desc)| AutocompleteSuggestion {
                    command: cmd.to_string(),
                    description: desc.to_string(),
                })
                .collect();
            state.update_autocomplete(autocomplete_suggestions);
        } else {
            state.show_autocomplete = false;
        }

        // Draw UI
        terminal.draw(|f| {
            draw_codex_interface(
                f,
                f.size(),
                &state.agent_name,
                &state.version,
                &state.model,
                &state.current_dir,
                &state.input,
                &state.messages,
                Some(&state),
            );
        })?;

        // Check for pending agent response
        if let Ok(response) = response_rx.try_recv() {
            state.set_thinking(false);
            match response {
                Ok(agent_response) => {
                    state.push_message("agent", &agent_response);
                }
                Err(e) => {
                    state.push_message("agent", &format!("Error: {}", e));
                }
            }
        }

        // Advance thinking animation if thinking
        if state.is_thinking {
            state.advance_thinking_animation();
        }

        // Handle events with timeout
        let timeout = tick_rate.saturating_sub(last_tick.elapsed());

        if crossterm::event::poll(timeout)? {
            match handle_tui_event(&state).await? {
                CodexEvent::Quit => break Ok(()),
                CodexEvent::Submit => {
                    if !state.input.is_empty() && !state.is_thinking {
                        let user_input = state.input.clone();
                        state.push_message("user", &user_input);
                        state.clear_input();

                        // Handle slash commands
                        if user_input.starts_with("/") {
                            handle_slash_command(&user_input, &mut state).await;
                        } else {
                            // Set thinking state immediately
                            state.set_thinking(true);

                            // Spawn agent handling in a separate task
                            let agent_clone = Arc::clone(&agent);
                            let response_tx_clone = response_tx.clone();
                            let msg = gateway::Message {
                                id: uuid::Uuid::new_v4().to_string(),
                                platform: gateway::Platform::Cli,
                                channel_id: "tui".to_string(),
                                channel_name: None,
                                user_id: "user".to_string(),
                                user_name: "User".to_string(),
                                content: user_input,
                                timestamp: chrono::Utc::now(),
                                attachments: vec![],
                                reply_to: None,
                            };

                            tokio::spawn(async move {
                                let mut agent = agent_clone.lock().await;
                                let result = agent
                                    .handle_message(msg)
                                    .await
                                    .map_err(|e| anyhow::anyhow!(e));
                                let _ = response_tx_clone.send(result);
                            });
                        }
                    }
                }
                CodexEvent::Input(c) => {
                    state.input.push(c);
                }
                CodexEvent::Backspace => {
                    state.input.pop();
                    if state.input.is_empty() {
                        state.show_autocomplete = false;
                    }
                }
                CodexEvent::AutocompleteNext => {
                    state.autocomplete_next();
                }
                CodexEvent::AutocompletePrev => {
                    state.autocomplete_prev();
                }
                CodexEvent::AutocompleteSelect => {
                    state.apply_autocomplete();
                    // After applying autocomplete, also submit the command
                    if !state.input.is_empty() {
                        let user_input = state.input.clone();
                        state.push_message("user", &user_input);
                        state.clear_input();

                        // Handle slash commands
                        if user_input.starts_with("/") {
                            handle_slash_command(&user_input, &mut state).await;
                        }
                    }
                }
                CodexEvent::ChangeModel => {
                    // Toggle between simple display modes
                    state.model = if state.model == "compact" {
                        "full".to_string()
                    } else {
                        "compact".to_string()
                    };
                }
                CodexEvent::NoOp => {
                    // Close autocomplete if it was showing (Esc pressed)
                    if state.show_autocomplete {
                        state.show_autocomplete = false;
                    }
                }
            }
        }

        if last_tick.elapsed() >= tick_rate {
            last_tick = tokio::time::Instant::now();
        }
    };

    // Restore terminal
    disable_raw_mode()?;
    terminal.backend_mut().execute(LeaveAlternateScreen)?;

    // Save agent state
    agent.lock().await.save().await?;

    result
}

/// Model information with metadata
#[derive(Clone)]
struct ModelInfo {
    name: &'static str,
    provider: &'static str,
    description: &'static str,
    context_window: &'static str,
    cost_in: &'static str,
    cost_out: &'static str,
    reasoning: bool,
}

const AVAILABLE_MODELS: &[ModelInfo] = &[
    ModelInfo {
        name: "qwen3-coder:480b-cloud",
        provider: "ollama",
        description: "QwEncoder 480B Cloud - default model",
        context_window: "128k tokens",
        cost_in: "Cloud",
        cost_out: "Cloud",
        reasoning: false,
    },
    ModelInfo {
        name: "qwen3.5-35b-a3b",
        provider: "ollama",
        description: "Qwen 3.5 35B A3B - local model",
        context_window: "128k tokens",
        cost_in: "Free",
        cost_out: "Free",
        reasoning: false,
    },
    ModelInfo {
        name: "llama3.2",
        provider: "ollama",
        description: "Local model - your data stays on your machine",
        context_window: "128k tokens",
        cost_in: "Free",
        cost_out: "Free",
        reasoning: false,
    },
    ModelInfo {
        name: "claude-3.5-sonnet",
        provider: "anthropic",
        description: "Excels at analysis, reasoning, and code review",
        context_window: "200k tokens",
        cost_in: "$3.00",
        cost_out: "$15.00",
        reasoning: true,
    },
    ModelInfo {
        name: "kimi-k1.5",
        provider: "moonshot",
        description: "Large context window for long documents",
        context_window: "2M tokens",
        cost_in: "$3.00",
        cost_out: "$3.00",
        reasoning: true,
    },
    ModelInfo {
        name: "gpt-4",
        provider: "openai",
        description: "Great for creative tasks and general assistance",
        context_window: "128k tokens",
        cost_in: "$30.00",
        cost_out: "$60.00",
        reasoning: true,
    },
    ModelInfo {
        name: "qwen3-8b",
        provider: "alibaba",
        description: "Efficient coding model",
        context_window: "128k tokens",
        cost_in: "Free / API",
        cost_out: "Free / API",
        reasoning: false,
    },
];

fn get_model_info(name: &str) -> Option<&'static ModelInfo> {
    AVAILABLE_MODELS.iter().find(|m| {
        m.name.eq_ignore_ascii_case(name) || m.name.to_lowercase().contains(&name.to_lowercase())
    })
}

fn format_current_model_info(current_model: &str) -> String {
    // Extract short name from long model path (e.g., hf.co/mradermacher/Llama-3.3-8B... -> Llama-3.3-8B...)
    let short_name = current_model.split('/').last().unwrap_or(current_model);

    if let Some(info) = get_model_info(current_model) {
        format!("## Current Model\n\n**Name:** {}\n**Provider:** {}\n**Description:** {}\n\n**Specifications:**\n  • Context window: {}\n  • Input cost: {}/1M tokens\n  • Output cost: {}/1M tokens\n  • Reasoning: {}\n\nUse `/model list` to see all available models or `/model <name>` to switch.",
            info.name,
            info.provider,
            info.description,
            info.context_window,
            info.cost_in,
            info.cost_out,
            if info.reasoning { "Yes" } else { "No" }
        )
    } else {
        format!("## Current Model\n\n**Name:** {}\n**Full path:** {}\n\nThis is a custom/local Ollama model.\n\nUse `/model list` to see built-in models or `/model <name>` to switch.", short_name, current_model)
    }
}

fn format_model_list(current_model: &str) -> String {
    let mut output = String::from("## Available Models\n\n");

    // Extract short name from current model path for comparison
    let current_short = current_model.split('/').last().unwrap_or(current_model);

    for info in AVAILABLE_MODELS {
        let is_current = info.name == current_model
            || info.name == current_short
            || current_model.contains(&info.name);
        let marker = if is_current { "▶ " } else { "  " };
        output.push_str(&format!(
            "{}**{}** ({})\n",
            marker, info.name, info.provider
        ));
        output.push_str(&format!("   {}\n", info.description));
        output.push_str(&format!(
            "   Context: {} | Cost: {} in / {} out\n\n",
            info.context_window, info.cost_in, info.cost_out
        ));
    }

    output.push_str("Use `/model <name>` to switch models.\n");
    output.push_str("\nTo use external models, set your API key:\n");
    output.push_str("  export ANTHROPIC_API_KEY=sk-...\n");
    output.push_str("  export OPENAI_API_KEY=sk-...\n");
    output.push_str("  export MOONSHOT_API_KEY=sk-...");

    output
}

/// Available slash commands for autocomplete
const SLASH_COMMANDS: &[(&str, &str)] = &[
    ("/help", "Show available commands"),
    ("/model", "Show or switch LLM model"),
    ("/model list", "List all available models"),
    ("/approve list", "List pending tool approvals"),
    ("/approve allow <key>", "Approve a pending tool key"),
    ("/approve deny <key>", "Deny a pending tool key"),
    ("/clear", "Clear conversation history"),
    ("/exit", "Exit the TUI"),
];

/// Handle slash commands in TUI
async fn handle_slash_command(cmd: &str, state: &mut CodexState) {
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    let command = parts.get(0).map(|s| *s).unwrap_or("");
    let subcommand = parts.get(1).map(|s| *s).unwrap_or("");

    match command {
        "/help" => {
            let help_text = format!(
                r#"## Available Commands

| Command | Description |
|---------|-------------|
| /help | Show this help message |
| /model | Show current model info |
| /model list | List all available models |
| /model <name> | Switch to a different model |
| /approve list | Show pending approvals |
| /approve allow <key> | Approve an action key |
| /approve deny <key> | Deny an action key |
| /clear | Clear conversation history |
| /exit | Exit the TUI |

---

{}
"#,
                format_model_list(&state.model)
            );
            state.push_message("system", &help_text);
        }

        "/model" => {
            match subcommand {
                "" => {
                    // Show detailed current model info
                    let info = format_current_model_info(&state.model);
                    state.push_message("system", &info);
                }
                "list" => {
                    state.push_message("system", &format_model_list(&state.model));
                }
                "claude-3.5-sonnet" | "claude" => {
                    state.model = "claude-3.5-sonnet".to_string();
                    state.push_message("system", "✓ Switched to **Claude 3.5 Sonnet**\n\nThis model excels at analysis, reasoning, and code review.");
                }
                "kimi" | "kimi-k1.5" => {
                    state.model = "kimi-k1.5".to_string();
                    state.push_message("system", "✓ Switched to **Kimi K1.5**\n\nLarge context window (2M tokens) perfect for long documents.");
                }
                "gpt-4" | "gpt" => {
                    state.model = "gpt-4".to_string();
                    state.push_message("system", "✓ Switched to **GPT-4**\n\nGreat for creative tasks and general assistance.");
                }
                "llama" | "llama3.2" | "local" => {
                    state.model = "llama3.2".to_string();
                    state.push_message("system", "✓ Switched to **Llama 3.2**\n\nLocal model - your data stays on your machine. Fully private!");
                }
                "qwencoder" | "qwen3-coder" | "qwen3-coder:480b-cloud" | "480b-cloud" => {
                    state.model = "qwen3-coder:480b-cloud".to_string();
                    state.push_message(
                        "system",
                        "✓ Switched to **QwEncoder 480B Cloud**\n\nLarge cloud model.",
                    );
                }
                "qwen3.5" | "qwen3.5-35b" | "qwen3.5-35b-a3b" => {
                    state.model = "qwen3.5-9b-q4km".to_string();
                    state.push_message(
                        "system",
                        "✓ Switched to **Qwen 3.5 35B A3B**\n\nDefault local model - efficient, capable.",
                    );
                }
                "qwen" | "qwen3-8b" => {
                    state.model = "qwen3-8b".to_string();
                    state.push_message(
                        "system",
                        "✓ Switched to **Qwen 3 8B**\n\nEfficient coding model.",
                    );
                }
                _ => {
                    state.push_message(
                        "system",
                        &format!(
                            "❌ Unknown model: `{}`\n\nType `/model list` to see available models.",
                            subcommand
                        ),
                    );
                }
            }
        }

        "/clear" => {
            state.messages.clear();
            state.push_message("system", "🗑️ Conversation cleared.");
        }

        "/approve" => {
            let svc = ApprovalService::from_env();
            match subcommand {
                "list" | "" => match svc.list_pending() {
                    Ok(items) if items.is_empty() => {
                        state.push_message("system", "No pending approvals.");
                    }
                    Ok(items) => {
                        let mut out = String::from("## Pending Approvals\n\n");
                        for p in items {
                            out.push_str(&format!("- `{}`\n  - {}\n", p.key, p.summary));
                        }
                        state.push_message("system", &out);
                    }
                    Err(e) => state.push_message("system", &format!("Approval error: {}", e)),
                },
                "allow" | "deny" => {
                    let key = parts.get(2).copied().unwrap_or_default();
                    if key.is_empty() {
                        state.push_message("system", "Usage: /approve allow|deny <key>");
                    } else {
                        let outcome = if subcommand == "allow" {
                            ApprovalOutcome::Allow
                        } else {
                            ApprovalOutcome::Deny
                        };
                        match svc.decide(key, outcome, "tui_user") {
                            Ok(()) => state.push_message(
                                "system",
                                &format!("Recorded decision for `{}`.", key),
                            ),
                            Err(e) => {
                                state.push_message("system", &format!("Approval error: {}", e))
                            }
                        }
                    }
                }
                _ => state.push_message(
                    "system",
                    "Usage: /approve list | /approve allow <key> | /approve deny <key>",
                ),
            }
        }

        "/exit" => {
            // This will be handled by the main loop to exit
            state.push_message("system", "Press **Esc** or **Ctrl+C** to exit.");
        }

        _ => {
            state.push_message(
                "system",
                &format!(
                    "❌ Unknown command: `{}`\n\nType `/help` to see available commands.",
                    command
                ),
            );
        }
    }
}

/// Get autocomplete suggestions for partial input
fn get_autocomplete_suggestions(input: &str) -> Vec<(&'static str, &'static str)> {
    if input.is_empty() || !input.starts_with('/') {
        return vec![];
    }

    let query = input.trim_start_matches('/').to_lowercase();

    // If query is empty (just "/"), return all commands
    if query.is_empty() {
        return SLASH_COMMANDS.to_vec();
    }

    SLASH_COMMANDS
        .iter()
        .filter(|(cmd, _)| cmd.to_lowercase().contains(&query))
        .copied()
        .collect()
}

async fn handle_tui_event(state: &CodexState) -> Result<CodexEvent> {
    use hyper_stigmergy::tui_codex_style::CodexEvent;

    if let Event::Key(key) = event::read()? {
        // Handle autocomplete navigation when autocomplete is showing
        if state.show_autocomplete && !state.autocomplete_suggestions.is_empty() {
            match key.code {
                KeyCode::Down | KeyCode::Tab => return Ok(CodexEvent::AutocompleteNext),
                KeyCode::Up => return Ok(CodexEvent::AutocompletePrev),
                KeyCode::Enter | KeyCode::Right => return Ok(CodexEvent::AutocompleteSelect),
                KeyCode::Esc => {
                    // Just close autocomplete, don't quit
                    return Ok(CodexEvent::NoOp);
                }
                _ => {}
            }
        }

        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Ok(CodexEvent::Quit)
            }
            KeyCode::Char('q') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Ok(CodexEvent::Quit)
            }
            KeyCode::Esc => Ok(CodexEvent::Quit),
            KeyCode::Enter => Ok(CodexEvent::Submit),
            KeyCode::Char('m') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Ok(CodexEvent::ChangeModel)
            }
            KeyCode::Backspace => Ok(CodexEvent::Backspace),
            KeyCode::Char(c) => Ok(CodexEvent::Input(c)),
            _ => Ok(CodexEvent::NoOp),
        }
    } else {
        Ok(CodexEvent::NoOp)
    }
}
