//! CLI: multi-agent harness — parallel drafts, cross-review, optional synthesis.
//!
//! Uses `LlmClient` from the environment unless `HSM_CC_AGENT_ENDPOINTS` lists one HTTP URL
//! per configured agent (see `harness::cc_orchestrator`).

use std::path::PathBuf;

use anyhow::Context;
use clap::Parser;
use hyper_stigmergy::harness::{
    CcCrossReviewMode, CcOrchestrator, CcOrchestratorConfig, CcTask, HarnessRuntime,
};
use hyper_stigmergy::llm::client::LlmClient;
use serde_json::json;
use tracing_subscriber::EnvFilter;
use uuid::Uuid;

#[derive(Parser)]
#[command(name = "hsm_cc_orchestrate")]
#[command(about = "Dispatch a task to multiple CC-style agents with cross-review (harness layer)")]
struct Cli {
    /// Task instruction (or use --task-file).
    #[arg(long)]
    task: Option<String>,

    #[arg(long)]
    task_file: Option<PathBuf>,

    #[arg(long)]
    context_file: Option<PathBuf>,

    /// `round_robin` (default) or `full_mesh`.
    #[arg(long, default_value = "round_robin")]
    review_mode: String,

    /// Skip final synthesizer LLM pass.
    #[arg(long, default_value_t = false)]
    no_synthesize: bool,

    /// Optional `HSM_HARNESS_LOG` receives turn events when set in the environment.
    #[arg(long, default_value_t = false)]
    json: bool,

    /// Write full run JSON to this path.
    #[arg(long)]
    out_json: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("warn,hyper_stigmergy=info")),
        )
        .compact()
        .init();

    let cli = Cli::parse();

    let instruction = if let Some(p) = &cli.task_file {
        std::fs::read_to_string(p).with_context(|| format!("read {}", p.display()))?
    } else if let Some(t) = &cli.task {
        t.clone()
    } else {
        anyhow::bail!("provide --task or --task-file");
    };

    let context = match &cli.context_file {
        Some(p) => Some(
            std::fs::read_to_string(p).with_context(|| format!("read {}", p.display()))?,
        ),
        None => None,
    };

    let review_mode = match cli.review_mode.as_str() {
        "full_mesh" => CcCrossReviewMode::FullMesh,
        "round_robin" | _ => CcCrossReviewMode::RoundRobin,
    };

    let mut cfg = CcOrchestratorConfig::default();
    cfg.review_mode = review_mode;
    cfg.synthesize = !cli.no_synthesize;

    let llm = LlmClient::new().context("LlmClient (set OPENAI_API_KEY, OLLAMA_URL, etc.)")?;
    let orch = CcOrchestrator::new(llm, cfg)?;

    let task = CcTask {
        id: format!("cc_{}", Uuid::new_v4()),
        instruction,
        context,
    };

    let mut harness = HarnessRuntime::from_env("hsm_cc_orchestrate").ok();
    let run = orch.run(task, &mut harness).await;

    if cli.json {
        println!("{}", serde_json::to_string_pretty(&run)?);
    } else {
        println!("Task ID: {}", run.task.id);
        for d in &run.drafts {
            println!("\n--- Draft {} ({} ms) ---\n{}", d.agent_id, d.latency_ms, d.text);
        }
        for r in &run.reviews {
            println!(
                "\n--- Review {}→{} score={:.2} approve={} ({} ms) ---\n{}",
                r.reviewer_id,
                r.subject_agent_id,
                r.score,
                r.approve,
                r.latency_ms,
                r.critique
            );
        }
        if let Some(ref s) = run.synthesized_answer {
            println!("\n=== Synthesized ===\n{s}");
        }
        if let Some(ref id) = run.chosen_draft_agent_id {
            println!(
                "\n(chosen draft by review scores: {} — use synthesized output when present)",
                id
            );
        }
        for e in &run.errors {
            eprintln!("error: {e}");
        }
    }

    if let Some(p) = cli.out_json {
        let summary = json!({
            "run": run,
            "cli": { "review_mode": cli.review_mode, "synthesize": !cli.no_synthesize },
        });
        std::fs::write(&p, serde_json::to_vec_pretty(&summary)?)
            .with_context(|| format!("write {}", p.display()))?;
    }

    if run.drafts.is_empty() && run.errors.iter().any(|e| e.contains("draft")) {
        anyhow::bail!("all draft workers failed");
    }

    Ok(())
}
