//! Investigation CLI - Entry point for recursive investigation agent
//!
//! Usage:
//!   investigate --new "Campaign Finance Analysis" --workspace ./cases/case_001
//!   investigate --list
//!   investigate --resume <session_id>
//!   investigate --repl

use clap::{Parser, Subcommand};
use hyper_stigmergy::investigation_engine::{InvestigationEngine, InvestigationSession, SessionId};
use hyper_stigmergy::harness::{ResumeSessionMap, RuntimeConfig};
// use hyper_stigmergy::investigation_tools::InvestigationToolRegistry;
use std::path::PathBuf;
use tokio::io::{self, AsyncBufReadExt, BufReader};

#[derive(Parser)]
#[command(name = "investigate")]
#[command(about = "Recursive Investigation Agent for heterogeneous datasets")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Workspace directory for investigations
    #[arg(short, long, global = true)]
    workspace: Option<PathBuf>,

    /// Verbose output
    #[arg(short, long, global = true)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Create a new investigation
    New {
        /// Investigation title
        title: String,

        /// Investigation description
        #[arg(short, long)]
        description: Option<String>,
    },

    /// List all investigations
    List {
        /// Show all details
        #[arg(short, long)]
        detailed: bool,
    },

    /// Resume an investigation
    Resume {
        /// Session ID to resume
        session_id: String,
    },

    /// Start interactive REPL
    Repl {
        /// Optional session to load
        #[arg(short, long)]
        session: Option<String>,
    },

    /// Run a single query
    Query {
        /// Investigation query
        query: String,

        /// Title for new investigation
        #[arg(short, long)]
        title: Option<String>,
    },

    /// Show investigation status
    Status {
        /// Session ID (defaults to latest)
        session_id: Option<String>,
    },

    /// Export investigation report
    Export {
        /// Session ID
        session_id: String,

        /// Output format
        #[arg(short, long, value_enum, default_value = "markdown")]
        format: ExportFormat,

        /// Output file
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
enum ExportFormat {
    Markdown,
    Json,
    Html,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Determine workspace
    let workspace = cli
        .workspace
        .or_else(|| dirs::home_dir().map(|h| h.join(".hsm_investigations")))
        .unwrap_or_else(|| PathBuf::from("./investigations"));

    // Ensure workspace exists
    tokio::fs::create_dir_all(&workspace).await?;

    match cli.command {
        Some(Commands::New { title, description }) => {
            cmd_new(&workspace, title, description, cli.verbose).await?;
        }
        Some(Commands::List { detailed }) => {
            cmd_list(&workspace, detailed).await?;
        }
        Some(Commands::Resume { session_id }) => {
            cmd_resume(&workspace, &session_id).await?;
        }
        Some(Commands::Repl { session }) => {
            cmd_repl(&workspace, session).await?;
        }
        Some(Commands::Query { query, title }) => {
            cmd_query(&workspace, query, title).await?;
        }
        Some(Commands::Status { session_id }) => {
            cmd_status(&workspace, session_id).await?;
        }
        Some(Commands::Export {
            session_id,
            format,
            output,
        }) => {
            cmd_export(&workspace, &session_id, format, output).await?;
        }
        None => {
            // Default to REPL mode
            cmd_repl(&workspace, None).await?;
        }
    }

    Ok(())
}

async fn cmd_new(
    workspace: &PathBuf,
    title: String,
    description: Option<String>,
    verbose: bool,
) -> anyhow::Result<()> {
    let desc = description.unwrap_or_default();
    let session = InvestigationSession::new(&title, &desc, workspace.clone());
    let session_id = session.id;

    // Save initial session
    session.save().await?;

    if verbose {
        println!("✓ Created new investigation");
        println!("  Title: {}", title);
        println!("  ID: {}", session_id.0);
        println!("  Workspace: {}", workspace.display());
        println!("  Description: {}", desc);
    } else {
        println!("{}", session_id.0);
    }
    persist_resume_alias(&session_id.0.to_string(), &session_id.0.to_string())?;

    Ok(())
}

async fn cmd_list(workspace: &PathBuf, detailed: bool) -> anyhow::Result<()> {
    let sessions = InvestigationSession::list_sessions(workspace).await?;

    if sessions.is_empty() {
        println!("No investigations found in {}", workspace.display());
        return Ok(());
    }

    if detailed {
        println!("Investigations in {}:\n", workspace.display());
        for session in sessions {
            println!("  ID:          {}", session.id.0);
            println!("  Title:       {}", session.title);
            println!("  Description: {}", session.description);
            println!("  Status:      {:?}", session.status);
            println!("  Findings:    {}", session.finding_count);
            println!(
                "  Created:     {}",
                session.created_at.format("%Y-%m-%d %H:%M")
            );
            println!(
                "  Updated:     {}",
                session.updated_at.format("%Y-%m-%d %H:%M")
            );
            println!();
        }
    } else {
        println!(
            "{:<36} {:<20} {:<10} {}",
            "ID", "Title", "Findings", "Status"
        );
        println!("{}", "-".repeat(80));
        for session in sessions {
            let title = if session.title.len() > 18 {
                format!("{}...", &session.title[..15])
            } else {
                session.title.clone()
            };
            println!(
                "{} {:<20} {:<10} {:?}",
                session.id.0, title, session.finding_count, session.status
            );
        }
    }

    Ok(())
}

async fn cmd_resume(workspace: &PathBuf, session_id: &str) -> anyhow::Result<()> {
    let session_id = SessionId(uuid::Uuid::parse_str(session_id)?);

    let session = InvestigationSession::load(workspace, session_id).await?;
    println!("✓ Resumed investigation: {}", session.title);
    println!("  Datasets: {}", session.datasets.len());
    println!("  Entities: {}", session.entities.len());
    println!("  Findings: {}", session.findings.len());
    persist_resume_alias(&session.id.0.to_string(), &session.id.0.to_string())?;

    // Start REPL with this session
    start_repl(workspace, Some(session)).await?;

    Ok(())
}

async fn cmd_repl(workspace: &PathBuf, session: Option<String>) -> anyhow::Result<()> {
    let existing_session = if let Some(session_id) = session {
        let session_id = SessionId(uuid::Uuid::parse_str(&session_id)?);
        Some(InvestigationSession::load(workspace, session_id).await?)
    } else {
        None
    };

    start_repl(workspace, existing_session).await?;
    Ok(())
}

async fn start_repl(
    workspace: &PathBuf,
    existing_session: Option<InvestigationSession>,
) -> anyhow::Result<()> {
    let (session, engine) = if let Some(session) = existing_session {
        let engine = InvestigationEngine::new(session.clone());
        (session, engine)
    } else {
        // Create new session interactively using blocking stdin
        println!("Creating new investigation...");

        print!("Title: ");
        std::io::Write::flush(&mut std::io::stdout())?;
        let mut title = String::new();
        std::io::stdin().read_line(&mut title)?;

        print!("Description (optional): ");
        std::io::Write::flush(&mut std::io::stdout())?;
        let mut desc = String::new();
        std::io::stdin().read_line(&mut desc)?;

        let session = InvestigationSession::new(title.trim(), desc.trim(), workspace.clone());

        // Save initial session
        session.save().await?;

        let engine = InvestigationEngine::new(session.clone());
        (session, engine)
    };

    println!("\n🔍 Investigation Agent REPL");
    println!("Session: {} ({})", session.title, session.id.0);
    println!("Type 'help' for commands, 'exit' to quit\n");

    let stdin = io::stdin();
    let reader = BufReader::new(stdin);
    let mut lines = reader.lines();

    loop {
        print!("> ");
        std::io::Write::flush(&mut std::io::stdout())?;

        if let Some(line) = lines.next_line().await? {
            let line = line.trim();

            if line.is_empty() {
                continue;
            }

            match line {
                "exit" | "quit" => {
                    println!("Saving session...");
                    engine.save().await?;
                    println!("Goodbye!");
                    break;
                }
                "help" => {
                    print_repl_help();
                }
                "status" => {
                    let summary = engine.get_summary().await;
                    println!("Status: {:?}", summary.status);
                    println!("  Datasets: {}", summary.dataset_count);
                    println!("  Entities: {}", summary.entity_count);
                    println!("  Findings: {}", summary.finding_count);
                    println!(
                        "  Subtasks: {}/{}",
                        summary.completed_subtasks, summary.subtask_count
                    );
                }
                "save" => {
                    engine.save().await?;
                    println!("✓ Session saved");
                }
                cmd if cmd.starts_with("investigate ") => {
                    let query = &cmd[12..];
                    println!("🔍 Investigating: {}", query);

                    match engine.investigate(query).await {
                        Ok(response) => {
                            println!("\n{}", response);
                        }
                        Err(e) => {
                            eprintln!("Error: {}", e);
                        }
                    }
                }
                cmd if cmd.starts_with("load ") => {
                    let path = &cmd[5..];
                    println!("📊 Loading dataset: {}", path);
                    // Would implement dataset loading
                }
                cmd if cmd.starts_with("delegate ") => {
                    let desc = &cmd[9..];
                    let criteria = vec!["Complete analysis".to_string()];

                    match engine.delegate_subtask(desc, criteria, None).await {
                        Ok(result) => {
                            println!("✓ {}", result);
                        }
                        Err(e) => {
                            eprintln!("Error: {}", e);
                        }
                    }
                }
                _ => {
                    // Treat as investigation query
                    println!("🔍 Investigating: {}", line);

                    match engine.investigate(line).await {
                        Ok(response) => {
                            println!("\n{}", response);
                        }
                        Err(e) => {
                            eprintln!("Error: {}", e);
                        }
                    }
                }
            }
        } else {
            break;
        }
    }

    Ok(())
}

fn print_repl_help() {
    println!("Commands:");
    println!("  investigate <query>  - Start an investigation");
    println!("  load <path>          - Load a dataset");
    println!("  delegate <desc>      - Delegate a subtask");
    println!("  status               - Show investigation status");
    println!("  save                 - Save session");
    println!("  help                 - Show this help");
    println!("  exit/quit            - Exit REPL");
    println!();
    println!("Or just type your question to investigate.");
}

async fn cmd_query(
    workspace: &PathBuf,
    query: String,
    title: Option<String>,
) -> anyhow::Result<()> {
    let title = title.unwrap_or_else(|| "Ad-hoc Query".to_string());
    let session = InvestigationSession::new(&title, &query, workspace.clone());

    println!("🔍 Starting investigation: {}", query);

    let engine = InvestigationEngine::new(session);

    match engine.investigate(&query).await {
        Ok(response) => {
            println!("\n{}", response);

            // Save session
            engine.save().await?;

            let summary = engine.get_summary().await;
            persist_resume_alias(&summary.id.0.to_string(), &summary.id.0.to_string())?;
            println!("\n✓ Investigation complete");
            println!("  Session ID: {}", summary.id.0);
            println!("  Findings: {}", summary.finding_count);
        }
        Err(e) => {
            eprintln!("Investigation failed: {}", e);
        }
    }

    Ok(())
}

fn persist_resume_alias(task_id: &str, resume_id: &str) -> anyhow::Result<()> {
    let cfg = RuntimeConfig::from_env();
    let mut map = ResumeSessionMap::load(&cfg.resume.session_map_path)?;
    map.task_to_resume
        .insert(task_id.to_string(), resume_id.to_string());
    map.save(&cfg.resume.session_map_path)
}

async fn cmd_status(workspace: &PathBuf, session_id: Option<String>) -> anyhow::Result<()> {
    if let Some(session_id) = session_id {
        let session_id = SessionId(uuid::Uuid::parse_str(&session_id)?);
        let session = InvestigationSession::load(workspace, session_id).await?;

        println!("Investigation: {}", session.title);
        println!("  ID:          {}", session.id.0);
        println!("  Status:      {:?}", session.status);
        println!("  Datasets:    {}", session.datasets.len());
        println!("  Entities:    {}", session.entities.len());
        println!("  Findings:    {}", session.findings.len());
        println!(
            "  Subtasks:    {}/{}",
            session
                .subtasks
                .iter()
                .filter(|s| matches!(
                    s.status,
                    hyper_stigmergy::investigation_engine::SubtaskStatus::Completed
                ))
                .count(),
            session.subtasks.len()
        );
        println!("  Tool Calls:  {}", session.tool_calls.len());
        println!("  Created:     {}", session.created_at);
        println!("  Updated:     {}", session.updated_at);
    } else {
        // Show latest session
        let sessions = InvestigationSession::list_sessions(workspace).await?;
        if let Some(session) = sessions.first() {
            println!("Latest investigation: {}", session.title);
            println!("  ID:      {}", session.id.0);
            println!("  Status:  {:?}", session.status);
            println!("  Updated: {}", session.updated_at);
        } else {
            println!("No investigations found");
        }
    }

    Ok(())
}

async fn cmd_export(
    workspace: &PathBuf,
    session_id: &str,
    format: ExportFormat,
    output: Option<PathBuf>,
) -> anyhow::Result<()> {
    let session_id = SessionId(uuid::Uuid::parse_str(session_id)?);
    let session = InvestigationSession::load(workspace, session_id).await?;

    let output = output.unwrap_or_else(|| {
        PathBuf::from(format!(
            "{}_report.{}",
            session_id.0,
            match format {
                ExportFormat::Markdown => "md",
                ExportFormat::Json => "json",
                ExportFormat::Html => "html",
            }
        ))
    });

    let report = generate_report(&session, format)?;
    tokio::fs::write(&output, report).await?;

    println!("✓ Report exported to: {}", output.display());

    Ok(())
}

fn generate_report(session: &InvestigationSession, format: ExportFormat) -> anyhow::Result<String> {
    match format {
        ExportFormat::Markdown => {
            let mut md = format!("# Investigation Report: {}\n\n", session.title);
            md.push_str(&format!("**ID:** {}\n\n", session.id.0));
            md.push_str(&format!("**Description:** {}\n\n", session.description));
            md.push_str(&format!("**Status:** {:?}\n\n", session.status));
            md.push_str(&format!(
                "**Date:** {}\n\n",
                session.updated_at.format("%Y-%m-%d %H:%M")
            ));

            md.push_str("## Datasets\n\n");
            for dataset in &session.datasets {
                md.push_str(&format!(
                    "- **{}**: {} records\n",
                    dataset.name, dataset.record_count
                ));
            }
            md.push_str("\n");

            md.push_str("## Findings\n\n");
            for finding in &session.findings {
                md.push_str(&format!("### {}\n", finding.title));
                md.push_str(&format!(
                    "**Severity:** {:?} | **Confidence:** {:.0}%\n\n",
                    finding.severity,
                    finding.confidence * 100.0
                ));
                md.push_str(&format!("{}\n\n", finding.description));
            }

            md.push_str("## Entities\n\n");
            for entity in &session.entities {
                md.push_str(&format!(
                    "- **{}** ({}) - Confidence: {:.0}%\n",
                    entity.name,
                    entity.entity_type,
                    entity.confidence * 100.0
                ));
            }

            Ok(md)
        }
        ExportFormat::Json => Ok(serde_json::to_string_pretty(session)?),
        ExportFormat::Html => {
            // Simple HTML report
            let mut html = format!(
                r#"<!DOCTYPE html>
<html>
<head>
    <title>Investigation Report: {}</title>
    <style>
        body {{ font-family: sans-serif; max-width: 800px; margin: 0 auto; padding: 20px; }}
        h1 {{ color: #333; }}
        .finding {{ border: 1px solid #ddd; padding: 15px; margin: 10px 0; border-radius: 5px; }}
        .severity-critical {{ border-left: 4px solid #d32f2f; }}
        .severity-high {{ border-left: 4px solid #f57c00; }}
        .severity-medium {{ border-left: 4px solid #fbc02d; }}
        .severity-low {{ border-left: 4px solid #388e3c; }}
    </style>
</head>
<body>
    <h1>{}</h1>
    <p><strong>ID:</strong> {}</p>
    <p><strong>Status:</strong> {:?}</p>
    <p><strong>Date:</strong> {}</p>
    
    <h2>Findings</h2>
"#,
                session.title,
                session.title,
                session.id.0,
                session.status,
                session.updated_at.format("%Y-%m-%d %H:%M")
            );

            for finding in &session.findings {
                let severity_class = match finding.severity {
                    hyper_stigmergy::investigation_engine::FindingSeverity::Critical => {
                        "severity-critical"
                    }
                    hyper_stigmergy::investigation_engine::FindingSeverity::High => "severity-high",
                    hyper_stigmergy::investigation_engine::FindingSeverity::Medium => {
                        "severity-medium"
                    }
                    hyper_stigmergy::investigation_engine::FindingSeverity::Low => "severity-low",
                    _ => "severity-low",
                };

                html.push_str(&format!(
                    r#"    <div class="finding {}">
        <h3>{}</h3>
        <p><strong>Severity:</strong> {:?} | <strong>Confidence:</strong> {:.0}%</p>
        <p>{}</p>
    </div>
"#,
                    severity_class,
                    finding.title,
                    finding.severity,
                    finding.confidence * 100.0,
                    finding.description
                ));
            }

            html.push_str("</body>\n</html>");
            Ok(html)
        }
    }
}
