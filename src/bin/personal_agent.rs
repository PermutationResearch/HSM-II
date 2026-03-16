//! HSM-II Personal Agent - Main entry point
//!
//! A grounded, Hermes-like personal AI assistant powered by HSM-II's
//! advanced multi-agent coordination (stigmergy, DKS, CASS, Council).

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tracing::{error, info};

use hyper_stigmergy::personal::{
    gateway, hsmii_home, EnhancedPersonalAgent,
};
use hyper_stigmergy::tui_codex_style::{AutocompleteSuggestion, CodexEvent, CodexState};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

// TUI imports for Codex-style interface
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;

#[derive(Parser)]
#[command(name = "hsmii")]
#[command(about = "HSM-II Personal Agent - Your AI companion")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Custom config directory
    #[arg(short, long, global = true)]
    config: Option<PathBuf>,
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

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    // Determine config path
    let home = cli.config.unwrap_or_else(hsmii_home);

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
        Commands::Status => {
            cmd_status(&home).await?;
        }
    }

    Ok(())
}

/// Start the agent
async fn cmd_start(home: &PathBuf, daemon: bool, discord: bool, telegram: bool) -> Result<()> {
    // Check if initialized (LadybugDB format)
    let initialized = hyper_stigmergy::embedded_graph_store::EmbeddedGraphStore::exists() 
        || home.join("config.json").exists();
    
    if !initialized {
        println!("Enhanced agent not initialized. Run `hsmii bootstrap` first.");
        return Ok(());
    }

    // Load enhanced agent with full HSM-II
    let mut agent = EnhancedPersonalAgent::initialize(home).await?;

    println!("🚀 Starting Enhanced HSM-II Personal Agent");
    println!("   Agents: {}", agent.world.agents.len());
    for (i, a) in agent.world.agents.iter().enumerate() {
        let jw = a.calculate_jw(agent.world.global_coherence(), 3);
        println!("     {}. {:?} (JW: {:.3})", i + 1, a.role, jw);
    }
    println!("   Coherence: {:.3}", agent.world.global_coherence());
    println!("   Council: {}", if agent.config.enable_council { "✓ enabled" } else { "✗ disabled" });
    println!("   CASS: {}", if agent.config.enable_cass { "✓ enabled" } else { "✗ disabled" });
    println!("   LadybugDB: ✓ active\n");

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
        
        // Start gateway and get message channel
        let rx = agent.start_gateway(gateway_config).await?;
        msg_rx = Some(rx);
        println!("Gateway(s) started. Telegram/Discord messages will be processed.\n");
    }

    if daemon {
        // Daemon mode - spawn message processing task
        info!("Running in daemon mode");

        // Spawn message processing loop
        if let Some(mut rx) = msg_rx {
            let home_msg = home.clone();
            tokio::spawn(async move {
                while let Some((msg, response_tx)) = rx.recv().await {
                    // Reload agent for each message to get latest state
                    let mut agent = match EnhancedPersonalAgent::initialize(&home_msg).await {
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

        // Start DKS evolution loop (replacing simple heartbeat)
        let home_clone = home.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(60));
            loop {
                interval.tick().await;
                if let Ok(mut agent) = EnhancedPersonalAgent::initialize(&home_clone).await {
                    // Run DKS tick
                    if agent.config.enable_dks {
                        let _ = agent.services.dks.tick();
                    }
                    // Auto-save
                    let _ = agent.save().await;
                }
            }
        });

        println!("✓ Agent is running in daemon mode");
        if telegram {
            println!("  Telegram bot: active (polling for messages)");
        }
        if discord {
            println!("  Discord bot: active");
        }
        println!("  DKS evolution: every 60s");
        println!("  Auto-save: enabled\n");
        println!("Press Ctrl+C to stop.");

        // Wait for shutdown signal
        tokio::signal::ctrl_c().await?;

        println!("\nShutting down...");
        agent.save().await?;
    } else {
        // Interactive mode
        println!("Interactive mode. Type 'exit' to quit.\n");

        use tokio::io::{stdin, AsyncBufReadExt, BufReader};

        let stdin = BufReader::new(stdin());
        let mut lines = stdin.lines();

        loop {
            // Handle both stdin and gateway messages concurrently
            tokio::select! {
                // Handle stdin input
                line_result = lines.next_line() => {
                    let line: String = line_result?.unwrap_or_default();
                    
                    match line.as_str() {
                        "exit" | "quit" => break,
                        "" => continue,
                        input => {
                            // Create gateway message
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

                            match agent.handle_message(msg).await {
                                Ok(response) => {
                                    // Show council usage info
                                    let stats = agent.get_stats();
                                    if stats.council_invocations > 0 {
                                        println!("\n[Coherence: {:.3} | Council used: {} | Agents: {}]", 
                                            stats.coherence, stats.council_invocations, stats.agent_count);
                                    }
                                    println!("\n{}", response);
                                }
                                Err(e) => error!("Error: {}", e),
                            }
                        }
                    }
                }
                // Handle gateway messages (if gateway is enabled)
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
    println!("Using {} agents with coherence {:.3}\n", 
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
            println!("\nCouncil: {}", if agent.config.enable_council { "enabled" } else { "disabled" });
            println!("CASS: {}", if agent.config.enable_cass { "enabled" } else { "disabled" });
            println!("DKS: {}", if agent.config.enable_dks { "enabled" } else { "disabled" });
            println!("JouleWork tracking: {}", if agent.config.track_joulework { "enabled" } else { "disabled" });
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
                println!("{} [{:.2}] {}", source_icon, belief.confidence, belief.content);
            }
            println!("\n# Hyperedges\n");
            for (i, edge) in agent.world.edges.iter().rev().take(n).enumerate() {
                println!("Edge {}: agents {:?}, weight={:.2}, emergent={}", 
                    i, edge.participants, edge.weight, edge.emergent);
            }
        }
        MemoryAction::Search { query } => {
            println!("# Vector search for: {}\n", query);
            let beliefs = agent.get_relevant_beliefs(&query).await?;
            for belief in beliefs {
                println!("[{:.2}] {}", belief.confidence, belief.content);
            }
        }
        MemoryAction::Add { content, category: _ } => {
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
            });
            agent.save().await?;
            println!("✓ Added to LadybugDB beliefs");
        }
    }
    Ok(())
}

/// Run DKS evolution tick
async fn cmd_heartbeat(home: &PathBuf) -> Result<()> {
    let mut agent = EnhancedPersonalAgent::initialize(home).await?;

    println!("Running DKS evolution tick...\n");

    let tick = agent.services.dks.tick();
    let stats = agent.services.dks.stats();
    println!("Generation: {}", tick.generation);
    println!("Population: {}", stats.size);
    println!("Avg persistence: {:.3}", stats.average_persistence);
    
    agent.save().await?;
    println!("\n✓ State saved to LadybugDB");

    Ok(())
}

/// Bootstrap new enhanced agent
async fn cmd_bootstrap(home: &PathBuf) -> Result<()> {
    if home.join("config.json").exists() || hyper_stigmergy::embedded_graph_store::EmbeddedGraphStore::exists() {
        println!("Enhanced agent already initialized at {}", home.display());
        println!("Run `hsmii config` to view configuration.");
        return Ok(());
    }

    println!("🌱 Bootstrapping Enhanced HSM-II Personal Agent\n");
    
    let mut agent = EnhancedPersonalAgent::initialize(home).await?;
    
    // Calculate initial JW scores for display
    let coherence = agent.world.global_coherence();
    println!("✨ Created {} agents (coherence: {:.3}):", agent.world.agents.len(), coherence);
    for (i, agent_info) in agent.world.agents.iter().enumerate() {
        let jw = agent_info.calculate_jw(coherence, 3);
        println!("  {}. {:?} - JW: {:.3}", 
            i + 1, agent_info.role, jw);
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
    println!("  LadybugDB: {}", 
        if hyper_stigmergy::embedded_graph_store::EmbeddedGraphStore::exists() { "✓ active" } else { "✗ not found" });
    println!("  Council: {}", if agent.config.enable_council { "✓ enabled" } else { "✗ disabled" });
    println!("  CASS: {}", if agent.config.enable_cass { "✓ enabled" } else { "✗ disabled" });
    println!("  DKS: {}", if agent.config.enable_dks { "✓ enabled" } else { "✗ disabled" });

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
            if a.config.enable_council { "✓" } else { "✗" },
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
