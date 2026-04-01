//! hsm-autoreason — Autoreason multi-agent refinement (Author → Strawman → B/AB → blind Borda).
//!
//! ```bash
//! cargo run -p hyper-stigmergy --bin hsm-autoreason -- --prompt "Design a minimal REST API for todos"
//! ```
//!
//! JSONL: each line JSON with `prompt`, `user`, or `task` string field (first match wins).

use std::path::PathBuf;

use clap::Parser;
use serde::Deserialize;
use tracing_subscriber::EnvFilter;

use hyper_stigmergy::eval::autoreason::{AutoreasonConfig, run_autoreason};
use hyper_stigmergy::llm::client::LlmClient;

#[derive(Parser, Debug)]
#[command(name = "hsm-autoreason")]
#[command(about = "Autoreason: adversarial loop + blind Borda judges (several LLM calls per round)")]
struct Cli {
    /// Task / user message (if not using --prompt-file / --jsonl)
    #[arg(long)]
    prompt: Option<String>,

    #[arg(long)]
    prompt_file: Option<PathBuf>,

    /// JSONL with a string field per line (see --jsonl-field)
    #[arg(long)]
    jsonl: Option<PathBuf>,

    #[arg(long, default_value = "prompt,user,task")]
    jsonl_fields: String,

    #[arg(long, default_value_t = 1)]
    jsonl_limit: usize,

    #[arg(long, default_value_t = 2)]
    streak: u32,

    #[arg(long, default_value_t = 8)]
    max_rounds: u32,

    #[arg(long, default_value_t = 3)]
    judges: u32,

    /// Write full JSON result (rounds, tokens, final text)
    #[arg(long)]
    output_json: Option<PathBuf>,

    #[arg(long, default_value_t = false)]
    quiet: bool,
}

#[derive(Debug, Deserialize)]
struct JsonlRow {
    prompt: Option<String>,
    user: Option<String>,
    task: Option<String>,
}

fn extract_prompt(row: &JsonlRow, fields: &[&str]) -> Option<String> {
    for f in fields {
        match *f {
            "prompt" => {
                if let Some(ref s) = row.prompt {
                    return Some(s.clone());
                }
            }
            "user" => {
                if let Some(ref s) = row.user {
                    return Some(s.clone());
                }
            }
            "task" => {
                if let Some(ref s) = row.task {
                    return Some(s.clone());
                }
            }
            _ => {}
        }
    }
    None
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("warn,hsm_autoreason=info")),
        )
        .compact()
        .init();

    let cli = Cli::parse();

    if LlmClient::new().is_err() {
        eprintln!("Configure one of: OPENAI_API_KEY, OPENROUTER_API_KEY, ANTHROPIC_API_KEY, OLLAMA_URL");
        std::process::exit(1);
    }
    let client = LlmClient::new()?;
    let model = std::env::var("OLLAMA_MODEL")
        .or_else(|_| std::env::var("DEFAULT_LLM_MODEL"))
        .unwrap_or_else(|_| "gpt-4o-mini".to_string());

    let fields: Vec<&str> = cli.jsonl_fields.split(',').map(|s| s.trim()).collect();

    let mut jobs: Vec<String> = Vec::new();
    if let Some(ref p) = cli.prompt {
        jobs.push(p.clone());
    }
    if let Some(ref path) = cli.prompt_file {
        jobs.push(std::fs::read_to_string(path)?);
    }
    if let Some(ref path) = cli.jsonl {
        let text = std::fs::read_to_string(path)?;
        for line in text.lines().take(cli.jsonl_limit) {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let row: JsonlRow = serde_json::from_str(line)?;
            if let Some(s) = extract_prompt(&row, &fields) {
                jobs.push(s);
            }
        }
    }

    if jobs.is_empty() {
        anyhow::bail!("Provide --prompt, --prompt-file, or --jsonl");
    }

    let cfg = AutoreasonConfig {
        convergence_streak: cli.streak,
        max_rounds: cli.max_rounds,
        num_judges: cli.judges,
        ..AutoreasonConfig::default()
    };

    for (i, task) in jobs.iter().enumerate() {
        eprintln!(
            "━━━ Autoreason job {} / {} ━━━",
            i + 1,
            jobs.len()
        );
        let out = run_autoreason(&client, &model, task, &cfg).await?;
        if !cli.quiet {
            println!("--- final ({}) ---", out.stop_reason);
            println!("{}", out.final_text);
            println!(
                "--- tokens: prompt {} completion {} | llm_calls {} | rounds {} ---",
                out.total_prompt_tokens,
                out.total_completion_tokens,
                out.llm_calls,
                out.rounds.len()
            );
        }
        if let Some(ref path) = cli.output_json {
            let p = if jobs.len() > 1 {
                path.with_file_name(format!(
                    "{}_{}",
                    path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(),
                    i
                ))
                .with_extension(path.extension().unwrap_or_default())
            } else {
                path.clone()
            };
            std::fs::write(&p, serde_json::to_string_pretty(&out)?)?;
            eprintln!("wrote {}", p.display());
        }
    }

    Ok(())
}
