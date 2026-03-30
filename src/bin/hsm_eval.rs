//! hsm-eval — Comparative evaluation: HSM-II vs vanilla LLM baseline.
//!
//! Usage:
//!   hsm-eval                         # Run full suite (20 tasks)
//!   hsm-eval --tasks se              # Run only software engineering tasks
//!   hsm-eval --tasks ds,biz          # Run data science + business tasks
//!   hsm-eval --json results.json     # Export results as JSON
//!   hsm-eval --baseline-only         # Run baseline only (debug)
//!   hsm-eval --hsm-only              # Run HSM-II only (debug)

use clap::Parser;
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

use hyper_stigmergy::eval::{
    self, compare, load_eval_suite, print_report, suite_council_vs_single, suite_memory_retrieval,
    suite_tool_routing, BaselineRunner, HsmRunner,
};
use hyper_stigmergy::llm::client::LlmClient;

#[derive(Parser)]
#[command(name = "hsm-eval")]
#[command(about = "Comparative evaluation harness: HSM-II vs baseline LLM")]
struct Cli {
    /// Filter tasks by domain prefix (comma-separated: se,ds,biz,rw,stress)
    #[arg(long)]
    tasks: Option<String>,

    /// Export results to JSON file
    #[arg(long)]
    json: Option<PathBuf>,

    /// Run baseline only (skip HSM-II)
    #[arg(long)]
    baseline_only: bool,

    /// Run HSM-II only (skip baseline)
    #[arg(long)]
    hsm_only: bool,

    /// Limit number of tasks (for quick testing)
    #[arg(long)]
    limit: Option<usize>,

    /// Verbose: print each turn's response
    #[arg(short, long)]
    verbose: bool,

    /// Pre-registered suite: full (default), memory, tool, council
    #[arg(long)]
    suite: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("warn,hsm_eval=info")),
        )
        .compact()
        .init();

    let cli = Cli::parse();

    // Load and filter tasks
    let mut tasks = match cli.suite.as_deref() {
        Some("memory") => suite_memory_retrieval(),
        Some("tool") | Some("tools") => suite_tool_routing(),
        Some("council") => suite_council_vs_single(),
        Some("full") | None => load_eval_suite(),
        Some(other) => {
            eprintln!(
                "Unknown --suite {:?}; use full | memory | tool | council",
                other
            );
            std::process::exit(2);
        }
    };
    if let Some(ref filter) = cli.tasks {
        let prefixes: Vec<&str> = filter.split(',').map(|s| s.trim()).collect();
        tasks.retain(|t| prefixes.iter().any(|p| t.id.starts_with(p)));
    }
    if let Some(limit) = cli.limit {
        tasks.truncate(limit);
    }

    let total_turns: usize = tasks.iter().map(|t| t.turns.len()).sum();
    let recall_turns: usize = tasks.iter().flat_map(|t| &t.turns).filter(|t| t.requires_recall).count();

    println!("\n╔══════════════════════════════════════════════════════════╗");
    println!("║         HSM-II Comparative Evaluation Harness           ║");
    println!("╠══════════════════════════════════════════════════════════╣");
    println!("║  Tasks: {:>3}                                            ║", tasks.len());
    println!("║  Total turns: {:>3}                                      ║", total_turns);
    println!("║  Recall turns: {:>3} ({:.0}% of total)                   ║",
        recall_turns, (recall_turns as f64 / total_turns as f64) * 100.0);
    println!("╚══════════════════════════════════════════════════════════╝\n");

    // Verify LLM client can be created (fail fast)
    let _client = match LlmClient::new() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("ERROR: Failed to initialize LLM client: {}", e);
            eprintln!("Set one of: OPENAI_API_KEY, ANTHROPIC_API_KEY, or OLLAMA_URL");
            std::process::exit(1);
        }
    };

    // ── RUN BASELINE ──
    let baseline_metrics = if !cli.hsm_only {
        println!("━━━ Running BASELINE (vanilla LLM, no memory) ━━━");
        let runner = BaselineRunner::new(LlmClient::new()?);
        let metrics = runner.run(&tasks).await;
        println!(
            "  ✓ Baseline complete: {} turns, {:.1}% avg keyword score, {} total tokens\n",
            metrics.turns.len(),
            metrics.avg_keyword_score() * 100.0,
            metrics.total_tokens()
        );

        if cli.verbose {
            print_turn_details(&metrics);
        }

        Some(metrics)
    } else {
        None
    };

    // ── RUN HSM-II ──
    let hsm_metrics = if !cli.baseline_only {
        println!("━━━ Running HSM-II (persistent memory + context ranking + reputation) ━━━");
        let mut runner = HsmRunner::new(LlmClient::new()?);
        let metrics = runner.run(&tasks).await;
        println!(
            "  ✓ HSM-II complete: {} turns, {:.1}% avg keyword score, {} total tokens\n",
            metrics.turns.len(),
            metrics.avg_keyword_score() * 100.0,
            metrics.total_tokens()
        );

        if cli.verbose {
            print_turn_details(&metrics);
        }

        Some(metrics)
    } else {
        None
    };

    // ── COMPARISON REPORT ──
    if let (Some(ref baseline), Some(ref hsm)) = (&baseline_metrics, &hsm_metrics) {
        let report = compare(baseline, hsm, &tasks);
        print_report(&report);

        // Export JSON if requested
        if let Some(ref path) = cli.json {
            let json = serde_json::to_string_pretty(&report)?;
            std::fs::write(path, &json)?;
            println!("Results exported to: {}", path.display());
        }
    } else if let Some(ref metrics) = baseline_metrics {
        println!("\n--- Baseline-only results ---");
        println!("Avg keyword score: {:.1}%", metrics.avg_keyword_score() * 100.0);
        println!("Avg recall score:  {:.1}%", metrics.avg_recall_score() * 100.0);
        println!("Total tokens:      {}", metrics.total_tokens());
        println!("Total LLM calls:   {}", metrics.total_llm_calls());
    } else if let Some(ref metrics) = hsm_metrics {
        println!("\n--- HSM-II-only results ---");
        println!("Avg keyword score: {:.1}%", metrics.avg_keyword_score() * 100.0);
        println!("Avg recall score:  {:.1}%", metrics.avg_recall_score() * 100.0);
        println!("Total tokens:      {}", metrics.total_tokens());
        println!("Total LLM calls:   {}", metrics.total_llm_calls());
    }

    Ok(())
}

fn print_turn_details(metrics: &eval::RunnerMetrics) {
    for turn in &metrics.turns {
        let recall_marker = if turn.requires_recall { " [RECALL]" } else { "" };
        println!(
            "  [{} T{}S{}{}] score={:.0}% tokens={} latency={}ms",
            turn.task_id,
            turn.turn_index,
            turn.session,
            recall_marker,
            turn.keyword_score * 100.0,
            turn.prompt_tokens + turn.completion_tokens,
            turn.latency_ms,
        );
        if let Some(ref err) = turn.error {
            println!("    ERROR: {}", err);
        }
    }
    println!();
}
