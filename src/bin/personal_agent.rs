//! HSM-II Personal Agent - Main entry point
//!
//! A grounded, Hermes-like personal AI assistant powered by HSM-II's
//! advanced multi-agent coordination (stigmergy, DKS, CASS, Council).

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tracing::{error, info};

use hyper_stigmergy::personal::{
    gateway, hsmii_home, Heartbeat, Persona, PersonalAgent, PersonalMemory,
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
        Commands::Status => {
            cmd_status(&home).await?;
        }
    }

    Ok(())
}

/// Start the agent
async fn cmd_start(home: &PathBuf, daemon: bool, discord: bool, telegram: bool) -> Result<()> {
    // Check if initialized
    if !home.join("SOUL.md").exists() {
        println!("Agent not initialized. Run `hsmii bootstrap` first.");
        return Ok(());
    }

    // Load agent
    let mut agent = PersonalAgent::initialize(home).await?;

    println!("🚀 Starting {}...", agent.persona.name);

    if daemon {
        // Daemon mode - start gateway and heartbeat
        info!("Running in daemon mode");

        // Configure gateway
        let mut gateway_config = gateway::Config::default();
        if discord {
            gateway_config.discord_token = std::env::var("DISCORD_TOKEN").ok();
        }
        if telegram {
            gateway_config.telegram_token = std::env::var("TELEGRAM_TOKEN").ok();
        }

        // Start gateway
        agent.start_gateway(gateway_config).await?;

        // Start heartbeat loop
        let heartbeat = agent.heartbeat.clone();
        let home_clone = home.clone();
        tokio::spawn(async move {
            heartbeat.run_loop(home_clone).await;
        });

        println!("Agent is running. Press Ctrl+C to stop.");

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
            print!("{}> ", agent.persona.name);
            // Would need to flush stdout here in real implementation

            let line: String = lines.next_line().await?.unwrap_or_default();

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
                        Ok(response) => println!("\n{}", response),
                        Err(e) => error!("Error: {}", e),
                    }
                }
            }
        }

        agent.save().await?;
    }

    Ok(())
}

/// Chat with agent
async fn cmd_chat(home: &PathBuf, message: Option<String>) -> Result<()> {
    let mut agent = PersonalAgent::initialize(home).await?;

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
    let mut agent = PersonalAgent::initialize(home).await?;

    println!("Executing: {}\n", task);

    let result = agent.execute_task(task).await?;

    if result.success {
        println!("✓ Success\n{}\n", result.output);
    } else {
        println!("✗ Failed\n{}\n", result.output);
    }

    agent.save().await?;
    Ok(())
}

/// Configuration commands
async fn cmd_config(home: &PathBuf, action: ConfigAction) -> Result<()> {
    match action {
        ConfigAction::Show => {
            let persona = Persona::load(home).await?;
            println!("# Configuration\n");
            println!("Name: {}", persona.name);
            println!("Proactivity: {:.0}%", persona.proactivity * 100.0);
            println!("Capabilities:");
            for cap in &persona.capabilities {
                let status = if cap.enabled { "✓" } else { "✗" };
                println!("  {} {}", status, cap.name);
            }
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

/// Memory commands
async fn cmd_memory(home: &PathBuf, action: MemoryAction) -> Result<()> {
    let mut memory = PersonalMemory::load(home).await?;

    match action {
        MemoryAction::Show { n } => {
            println!("# Recent Memories (showing {} facts)\n", n);
            for fact in memory.memory_md.facts.iter().rev().take(n) {
                println!("- [{}] {}", fact.category, fact.content);
            }
        }
        MemoryAction::Search { query } => {
            println!("# Searching for: {}\n", query);
            // TODO: Implement search
        }
        MemoryAction::Add { content, category } => {
            let cat = category.unwrap_or_else(|| "general".to_string());
            memory.add_fact(&content, &cat);
            memory.save(home).await?;
            println!("✓ Added to memory");
        }
    }
    Ok(())
}

/// Run heartbeat
async fn cmd_heartbeat(home: &PathBuf) -> Result<()> {
    let mut agent = PersonalAgent::initialize(home).await?;

    println!("Running heartbeat...\n");

    let results = agent.heartbeat().await?;

    for result in results {
        let icon = if result.success { "✓" } else { "✗" };
        println!("{} {}: {}", icon, result.action, result.message);
    }

    Ok(())
}

/// Bootstrap new agent
async fn cmd_bootstrap(home: &PathBuf) -> Result<()> {
    if home.join("SOUL.md").exists() {
        println!("Agent already initialized at {}", home.display());
        println!("Run `hsmii config persona` to edit personality.");
        return Ok(());
    }

    let _agent = PersonalAgent::bootstrap(home).await?;

    println!("\n✨ Agent initialized!");
    println!("\nNext steps:");
    println!("  - Run `hsmii start` to chat with your agent");
    println!(
        "  - Edit {} to customize personality",
        home.join("SOUL.md").display()
    );
    println!(
        "  - Edit {} to add your details",
        home.join("USER.md").display()
    );

    Ok(())
}

/// Check status
async fn cmd_status(home: &PathBuf) -> Result<()> {
    if !home.join("SOUL.md").exists() {
        println!("Agent not initialized.");
        println!("Run `hsmii bootstrap` to set up your AI companion.");
        return Ok(());
    }

    let persona = Persona::load(home).await?;
    let memory = PersonalMemory::load(home).await?;
    let heartbeat = Heartbeat::load(home).await?;

    println!("# {} Status\n", persona.name);

    println!("## Memory");
    println!("  Facts: {}", memory.memory_md.facts.len());
    println!("  Projects: {}", memory.memory_md.projects.len());
    println!("  User: {}\n", memory.user_md.name);

    println!("## Health");
    println!(
        "  Last heartbeat: {} minutes ago",
        (chrono::Utc::now() - heartbeat.last_beat).num_minutes()
    );
    println!("  Cron jobs: {}", heartbeat.cron_jobs.len());

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

    // Load agent
    let agent = Arc::new(Mutex::new(PersonalAgent::initialize(home).await?));
    let mut state = CodexState::new(&agent.lock().await.persona.name);

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Update state with agent info - use actual LLM model name
    state.model = agent.lock().await.current_model();
    state.current_dir = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "~".to_string());

    // Welcome message
    let welcome_msg = format!(
        "Hi! I'm {}. {}\n\nWhat would you like to build today?",
        agent.lock().await.persona.name,
        agent.lock().await.persona.identity
    );
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
