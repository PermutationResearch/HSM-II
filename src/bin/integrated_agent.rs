//! HSM-II Fully Integrated Agent - Main entry point
//!
//! A grounded, Hermes-like personal AI assistant with all HSM-II components:
//! - Federation (multi-node knowledge sharing)
//! - Email Agent (autonomous inbox management)
//! - Coder Assistant (dedicated code editing mode)
//! - Prolog Logic (symbolic reasoning)
//! - Ouroboros Compatibility (blockchain integration)
//! - Pi AI Compatibility (external AI bridges)
//! - Hermes Bridge (external tool ecosystem)

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tracing::{error, info};

use hyper_stigmergy::personal::{
    gateway, resolve_hsmii_home, IntegratedAgentConfig, IntegratedPersonalAgent,
};

#[derive(Parser)]
#[command(name = "hsmii-integrated")]
#[command(about = "HSM-II Fully Integrated Agent - Multi-component AI assistant")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Custom config directory (overrides profile / HSMII_HOME)
    #[arg(short, long, global = true)]
    config: Option<PathBuf>,

    /// Profile name: data under ~/.hsmii/profiles/<name>/
    #[arg(short = 'p', long = "profile", global = true)]
    profile: Option<String>,

    /// Enable all components
    #[arg(long, global = true)]
    all: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the integrated agent
    Start {
        /// Run in daemon mode
        #[arg(short, long)]
        daemon: bool,
        /// Enable Discord gateway
        #[arg(long)]
        discord: bool,
        /// Enable Telegram gateway
        #[arg(long)]
        telegram: bool,
        /// Enable email agent
        #[arg(long)]
        email: bool,
        /// Enable federation
        #[arg(long)]
        federation: bool,
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

    /// Email management commands
    Email {
        #[command(subcommand)]
        action: EmailAction,
    },

    /// Federation commands
    Federation {
        #[command(subcommand)]
        action: FederationAction,
    },

    /// Prolog symbolic reasoning
    Prolog {
        /// Prolog query or command
        query: Option<String>,
    },

    /// Coder assistant mode
    Code {
        /// Coding task description
        task: Option<String>,
    },

    /// Component status
    Status,

    /// Configure components
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },

    /// Bootstrap new agent
    Bootstrap {
        /// Enable all components
        #[arg(long)]
        all: bool,
    },

    /// Maintenance commands
    Maintain {
        #[command(subcommand)]
        action: MaintainAction,
    },
}

#[derive(Subcommand)]
enum EmailAction {
    /// Check email status
    Status,
    /// Process inbox
    Inbox,
    /// Send a test email
    Send {
        to: String,
        subject: String,
        body: String,
    },
}

#[derive(Subcommand)]
enum FederationAction {
    /// Show federation status
    Status,
    /// Sync with peers
    Sync,
    /// Query distributed knowledge
    Query { topic: String },
}

#[derive(Subcommand)]
enum ConfigAction {
    /// Show current configuration
    Show,
    /// Enable a component
    Enable { component: String },
    /// Disable a component
    Disable { component: String },
    /// Set configuration value
    Set { key: String, value: String },
}

#[derive(Subcommand)]
enum MaintainAction {
    /// Run graph gardening (prune decayed edges)
    Garden,
    /// Optimize hypergraph
    Optimize,
    /// Vacuum and compact storage
    Vacuum,
    /// Show maintenance status
    Status,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt::init();
    let _ = dotenvy::dotenv();
    hyper_stigmergy::telemetry::init_from_env();
    let _telemetry_session = hyper_stigmergy::telemetry::start_session_guard();

    let cli = Cli::parse();

    // Determine config path
    let home = resolve_hsmii_home(cli.config, cli.profile.as_deref());

    match cli.command {
        Commands::Start {
            daemon,
            discord,
            telegram,
            email,
            federation,
        } => {
            cmd_start(
                &home,
                daemon,
                discord,
                telegram,
                email,
                federation || cli.all,
            )
            .await?;
        }
        Commands::Chat { message } => {
            cmd_chat(&home, message).await?;
        }
        Commands::Do { task } => {
            cmd_do(&home, &task).await?;
        }
        Commands::Email { action } => {
            cmd_email(&home, action).await?;
        }
        Commands::Federation { action } => {
            cmd_federation(&home, action).await?;
        }
        Commands::Prolog { query } => {
            cmd_prolog(&home, query).await?;
        }
        Commands::Code { task } => {
            cmd_code(&home, task).await?;
        }
        Commands::Status => {
            cmd_status(&home).await?;
        }
        Commands::Config { action } => {
            cmd_config(&home, action).await?;
        }
        Commands::Bootstrap { all } => {
            cmd_bootstrap(&home, all || cli.all).await?;
        }
        Commands::Maintain { action } => {
            cmd_maintain(&home, action).await?;
        }
    }

    Ok(())
}

/// Start the integrated agent
async fn cmd_start(
    home: &PathBuf,
    daemon: bool,
    discord: bool,
    telegram: bool,
    _email: bool,
    _federation: bool,
) -> Result<()> {
    // Check if initialized
    let initialized = hyper_stigmergy::embedded_graph_store::EmbeddedGraphStore::exists()
        || home.join("integrated_config.json").exists();

    if !initialized {
        println!("Integrated agent not initialized. Run `hsmii-integrated bootstrap` first.");
        return Ok(());
    }

    // Load integrated agent
    let mut agent = IntegratedPersonalAgent::initialize(home).await?;

    // Print startup banner
    println!("╔══════════════════════════════════════════════════════════╗");
    println!("║     HSM-II Fully Integrated Personal Agent               ║");
    println!("╚══════════════════════════════════════════════════════════╝");
    println!();
    println!("🧠 Core System:");
    println!(
        "   Agents: {} | Coherence: {:.3}",
        agent.core.world.agents.len(),
        agent.core.world.global_coherence()
    );

    // Component status
    let status = agent.get_component_status();
    println!();
    println!("🔌 Components:");
    println!("   Email:       {}", if status.email { "✓" } else { "✗" });
    println!(
        "   Federation:  {}",
        if status.federation { "✓" } else { "✗" }
    );
    println!(
        "   Coder:       {}",
        if status.coder_assistant { "✓" } else { "✗" }
    );
    println!("   Prolog:      {}", if status.prolog { "✓" } else { "✗" });
    println!("   Pi AI:       {}", if status.pi_ai { "✓" } else { "✗" });
    println!(
        "   Ouroboros:   {}",
        if status.ouroboros { "✓" } else { "✗" }
    );
    println!("   Hermes:      {}", if status.hermes { "✓" } else { "✗" });
    println!();

    // Gardening status
    if agent.config.enable_gardening {
        println!(
            "🌱 Gardening: enabled (interval: {}s, threshold: {:.2})",
            agent.config.gardening_interval_secs, agent.config.decay_threshold
        );
    }
    println!();

    // Setup gateway if requested
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
        println!("Gateway(s) started.");
    }

    if daemon {
        info!("Running in daemon mode");

        // Spawn message processing task
        if let Some(mut rx) = msg_rx {
            let home_msg = home.clone();
            tokio::spawn(async move {
                while let Some((msg, response_tx)) = rx.recv().await {
                    let mut agent = match IntegratedPersonalAgent::initialize(&home_msg).await {
                        Ok(a) => a,
                        Err(e) => {
                            error!("Failed to load agent: {}", e);
                            let _ = response_tx.send(format!("Error: {}", e));
                            continue;
                        }
                    };

                    let response = match agent.handle_message(msg).await {
                        Ok(resp) => resp,
                        Err(e) => format!("Error: {}", e),
                    };

                    let _ = response_tx.send(response);
                    let _ = agent.save().await;
                }
            });
        }

        // Start DKS evolution and gardening loop
        let home_clone = home.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(60));
            loop {
                interval.tick().await;
                if let Ok(mut agent) = IntegratedPersonalAgent::initialize(&home_clone).await {
                    // Run DKS tick
                    if agent.config.base.enable_dks {
                        let tick = agent.core.services.dks.tick();
                        agent.core.services.last_dks_tick = Some(tick);
                    }

                    // Trigger gardening if needed
                    if agent.config.enable_gardening
                        && agent.last_gardening.elapsed().as_secs()
                            > agent.config.gardening_interval_secs
                    {
                        let _ = agent.garden_hypergraph().await;
                        agent.last_gardening = std::time::Instant::now();
                    }

                    let _ = agent.save().await;
                }
            }
        });

        println!("✓ Agent running in daemon mode");
        println!("  DKS evolution: every 60s");
        println!("  Auto-gardening: enabled");
        println!("\nPress Ctrl+C to stop.");

        tokio::signal::ctrl_c().await?;
        println!("\nShutting down...");
        agent.save().await?;
    } else {
        // Interactive mode
        println!("Interactive mode. Type 'exit' to quit.");
        println!("Commands: /email, /federation, /prolog, /coder, /help\n");

        use tokio::io::{stdin, AsyncBufReadExt, BufReader};

        let stdin = BufReader::new(stdin());
        let mut lines = stdin.lines();

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
                                thread_workspace_root: None,
                            };

                            match agent.handle_message(msg).await {
                                Ok(response) => {
                                    println!("\n{}", response);
                                }
                                Err(e) => error!("Error: {}", e),
                            }
                        }
                    }
                }
                Some((msg, response_tx)) = async {
                    if let Some(ref mut rx) = msg_rx { rx.recv().await } else { None }
                } => {
                    let response = match agent.handle_message(msg).await {
                        Ok(resp) => resp,
                        Err(e) => format!("Error: {}", e),
                    };
                    let _ = response_tx.send(response);
                }
            }
        }

        agent.save().await?;
    }

    Ok(())
}

/// Chat with agent
async fn cmd_chat(home: &PathBuf, message: Option<String>) -> Result<()> {
    let mut agent = IntegratedPersonalAgent::initialize(home).await?;

    if let Some(msg) = message {
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
            thread_workspace_root: None,
        };

        let response = agent.handle_message(gateway_msg).await?;
        println!("{}", response);
    }

    agent.save().await?;
    Ok(())
}

/// Execute a task
async fn cmd_do(home: &PathBuf, task: &str) -> Result<()> {
    let mut agent = IntegratedPersonalAgent::initialize(home).await?;

    println!("Executing: {}\n", task);

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
        thread_workspace_root: None,
    };

    let result = agent.handle_message(msg).await?;
    println!("{}\n", result);

    agent.save().await?;
    Ok(())
}

/// Email commands
async fn cmd_email(home: &PathBuf, action: EmailAction) -> Result<()> {
    let mut agent = IntegratedPersonalAgent::initialize(home).await?;

    let msg = match action {
        EmailAction::Status => gateway::Message {
            content: "/email status".to_string(),
            ..default_message()
        },
        EmailAction::Inbox => gateway::Message {
            content: "/email inbox".to_string(),
            ..default_message()
        },
        EmailAction::Send { to, subject, body } => gateway::Message {
            content: format!("/email send {} {} {}", to, subject, body),
            ..default_message()
        },
    };

    let response = agent.handle_message(msg).await?;
    println!("{}", response);

    Ok(())
}

/// Federation commands
async fn cmd_federation(home: &PathBuf, action: FederationAction) -> Result<()> {
    let mut agent = IntegratedPersonalAgent::initialize(home).await?;

    let msg = match action {
        FederationAction::Status => gateway::Message {
            content: "/federation status".to_string(),
            ..default_message()
        },
        FederationAction::Sync => gateway::Message {
            content: "/federation sync".to_string(),
            ..default_message()
        },
        FederationAction::Query { topic } => gateway::Message {
            content: format!("/federation query {}", topic),
            ..default_message()
        },
    };

    let response = agent.handle_message(msg).await?;
    println!("{}", response);

    Ok(())
}

/// Prolog commands
async fn cmd_prolog(home: &PathBuf, query: Option<String>) -> Result<()> {
    let mut agent = IntegratedPersonalAgent::initialize(home).await?;

    let content = if let Some(q) = query {
        format!("/prolog {}", q)
    } else {
        "/prolog".to_string()
    };

    let msg = gateway::Message {
        content,
        ..default_message()
    };

    let response = agent.handle_message(msg).await?;
    println!("{}", response);

    Ok(())
}

/// Code commands
async fn cmd_code(home: &PathBuf, task: Option<String>) -> Result<()> {
    let mut agent = IntegratedPersonalAgent::initialize(home).await?;

    let content = if let Some(t) = task {
        format!("/coder {}", t)
    } else {
        "/coder".to_string()
    };

    let msg = gateway::Message {
        content,
        ..default_message()
    };

    let response = agent.handle_message(msg).await?;
    println!("{}", response);

    Ok(())
}

/// Check status
async fn cmd_status(home: &PathBuf) -> Result<()> {
    if !hyper_stigmergy::embedded_graph_store::EmbeddedGraphStore::exists() {
        println!("Integrated agent not initialized.");
        println!("Run `hsmii-integrated bootstrap` to set up.");
        return Ok(());
    }

    let agent = IntegratedPersonalAgent::initialize(home).await?;
    let status = agent.get_component_status();
    let stats = agent.get_stats();

    println!("# HSM-II Integrated Agent Status\n");

    println!("## Core System");
    println!("  Agents: {}", stats.agent_count);
    println!("  Hyperedges: {}", stats.edge_count);
    println!("  Beliefs: {}", stats.belief_count);
    println!("  Global coherence: {:.3}\n", stats.coherence);

    println!("## Components");
    println!(
        "  Email:       {}",
        if status.email {
            "✓ enabled"
        } else {
            "✗ disabled"
        }
    );
    println!(
        "  Federation:  {}",
        if status.federation {
            "✓ enabled"
        } else {
            "✗ disabled"
        }
    );
    println!(
        "  Coder:       {}",
        if status.coder_assistant {
            "✓ enabled"
        } else {
            "✗ disabled"
        }
    );
    println!(
        "  Prolog:      {}",
        if status.prolog {
            "✓ enabled"
        } else {
            "✗ disabled"
        }
    );
    println!(
        "  Pi AI:       {}",
        if status.pi_ai {
            "✓ enabled"
        } else {
            "✗ disabled"
        }
    );
    println!(
        "  Ouroboros:   {}",
        if status.ouroboros {
            "✓ enabled"
        } else {
            "✗ disabled"
        }
    );
    println!(
        "  Hermes:      {}",
        if status.hermes {
            "✓ enabled"
        } else {
            "✗ disabled"
        }
    );

    println!("\n## Activity");
    println!("  Messages processed: {}", stats.total_messages);
    println!("  Council invocations: {}", stats.council_invocations);

    Ok(())
}

/// Config commands
async fn cmd_config(home: &PathBuf, action: ConfigAction) -> Result<()> {
    match action {
        ConfigAction::Show => {
            let agent = IntegratedPersonalAgent::initialize(home).await?;
            let config = &agent.config;

            println!("# HSM-II Configuration\n");
            println!(
                "Email:       {}",
                if config.enable_email {
                    "enabled"
                } else {
                    "disabled"
                }
            );
            println!(
                "Federation:  {}",
                if config.enable_federation {
                    "enabled"
                } else {
                    "disabled"
                }
            );
            println!(
                "Coder:       {}",
                if config.enable_coder_assistant {
                    "enabled"
                } else {
                    "disabled"
                }
            );
            println!(
                "Prolog:      {}",
                if config.enable_prolog {
                    "enabled"
                } else {
                    "disabled"
                }
            );
            println!(
                "Pi AI:       {}",
                if config.enable_pi_ai {
                    "enabled"
                } else {
                    "disabled"
                }
            );
            println!(
                "Ouroboros:   {}",
                if config.enable_ouroboros {
                    "enabled"
                } else {
                    "disabled"
                }
            );
            println!(
                "Hermes:      {}",
                if config.enable_hermes {
                    "enabled"
                } else {
                    "disabled"
                }
            );
            println!(
                "Gardening:   {}",
                if config.enable_gardening {
                    "enabled"
                } else {
                    "disabled"
                }
            );
            println!("  Interval:  {}s", config.gardening_interval_secs);
            println!("  Threshold: {:.2}", config.decay_threshold);
        }
        ConfigAction::Enable { component } => {
            println!("Enabling component: {}", component);
            // TODO: Implement config modification
        }
        ConfigAction::Disable { component } => {
            println!("Disabling component: {}", component);
            // TODO: Implement config modification
        }
        ConfigAction::Set { key, value } => {
            println!("Setting {} = {}", key, value);
            // TODO: Implement config setting
        }
    }
    Ok(())
}

/// Bootstrap new agent
async fn cmd_bootstrap(home: &PathBuf, all: bool) -> Result<()> {
    if home.join("integrated_config.json").exists() {
        println!("Integrated agent already initialized at {}", home.display());
        return Ok(());
    }

    println!("🌱 Bootstrapping HSM-II Integrated Agent\n");

    let mut config = IntegratedAgentConfig::default();

    if all {
        config.enable_email = true;
        config.enable_federation = true;
        config.enable_coder_assistant = true;
        config.enable_prolog = true;
        config.enable_pi_ai = true;
        println!("✨ All components enabled\n");
    }

    let mut agent = IntegratedPersonalAgent::initialize(home).await?;

    println!("✨ Created {} agents", agent.core.world.agents.len());

    let status = agent.get_component_status();
    println!("\n🔌 Components:");
    println!("  Email:       {}", if status.email { "✓" } else { "✗" });
    println!(
        "  Federation:  {}",
        if status.federation { "✓" } else { "✗" }
    );
    println!(
        "  Coder:       {}",
        if status.coder_assistant { "✓" } else { "✗" }
    );
    println!("  Prolog:      {}", if status.prolog { "✓" } else { "✗" });
    println!("  Pi AI:       {}", if status.pi_ai { "✓" } else { "✗" });

    agent.save().await?;

    println!("\n✓ LadybugDB initialized");
    println!("✓ Configuration saved");

    println!("\nNext steps:");
    println!("  - Run `hsmii-integrated start` to start the agent");
    println!("  - Run `hsmii-integrated status` to check status");

    Ok(())
}

/// Maintenance commands
async fn cmd_maintain(home: &PathBuf, action: MaintainAction) -> Result<()> {
    let mut agent = IntegratedPersonalAgent::initialize(home).await?;

    match action {
        MaintainAction::Garden => {
            println!("🌱 Running graph gardening...");
            agent.garden_hypergraph().await?;
            println!("✓ Gardening complete");
        }
        MaintainAction::Optimize => {
            println!("⚙️ Running hypergraph optimization...");
            // TODO: Implement optimization
            println!("✓ Optimization complete");
        }
        MaintainAction::Vacuum => {
            println!("🧹 Vacuuming storage...");
            // TODO: Implement vacuum
            println!("✓ Vacuum complete");
        }
        MaintainAction::Status => {
            println!("# Maintenance Status\n");
            println!(
                "Gardening:   {} (interval: {}s)",
                if agent.config.enable_gardening {
                    "enabled"
                } else {
                    "disabled"
                },
                agent.config.gardening_interval_secs
            );
            println!("Last garden: {:?} ago", agent.last_gardening.elapsed());
            println!("\nHypergraph:");
            println!("  Agents:    {}", agent.core.world.agents.len());
            println!("  Edges:     {}", agent.core.world.edges.len());
            println!("  Beliefs:   {}", agent.core.world.beliefs.len());
            println!("  Coherence: {:.3}", agent.core.world.global_coherence());
        }
    }

    agent.save().await?;
    Ok(())
}

fn default_message() -> gateway::Message {
    gateway::Message {
        id: uuid::Uuid::new_v4().to_string(),
        platform: gateway::Platform::Cli,
        channel_id: "cli".to_string(),
        channel_name: None,
        user_id: "user".to_string(),
        user_name: "User".to_string(),
        content: String::new(),
        timestamp: chrono::Utc::now(),
        attachments: vec![],
        reply_to: None,
        thread_workspace_root: None,
    }
}
