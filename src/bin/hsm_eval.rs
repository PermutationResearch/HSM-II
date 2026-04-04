//! hsm-eval — Comparative evaluation: HSM-II vs vanilla LLM baseline.
//!
//! Usage:
//!   hsm-eval                         # Run full suite (20 tasks)
//!   hsm-eval --tasks se              # Run only software engineering tasks
//!   hsm-eval --json results.json     # Export comparison JSON (per suite + combined)
//!   hsm-eval --artifacts runs/foo    # manifest + JSONL turns + optional traces

use std::path::PathBuf;

use clap::Parser;
use tracing_subscriber::EnvFilter;

use rusqlite::Connection;

use hyper_stigmergy::eval::{
    self, append_runs_index, calibration_report, compare, default_runs_index, eval_tasks_for_suite,
    filter_tasks, ingest_json_file, load_gold_labels, parse_weighted_suites,
    sync_index_line_to_sqlite, write_jsonl, write_manifest, write_turn_metrics_jsonl,
    ArtifactPaths, BaselineRunner, BipartiteMemoryGraph, ComparisonReport, EvalTask, HsmRunner,
    HsmRunnerConfig, RunManifest, RunnerMetrics, WeightedEvalSuite,
};
use hyper_stigmergy::eval::{init_memory_graph_sqlite_schema, upsert_memory_graph_sqlite};
use hyper_stigmergy::llm::client::LlmClient;

#[derive(Parser)]
#[command(name = "hsm-eval")]
#[command(about = "Comparative evaluation harness: HSM-II vs baseline LLM")]
struct Cli {
    #[arg(long)]
    tasks: Option<String>,

    /// Keep only tasks whose `EvalTask.domain` equals this (e.g. `software_engineering`, `data_science`).
    #[arg(long)]
    task_domain: Option<String>,

    #[arg(long)]
    json: Option<PathBuf>,

    /// Load HSM harness JSON (`HsmRunnerConfig`). CLI memory flags override fields after load.
    #[arg(long)]
    hsm_config: Option<PathBuf>,

    /// Ablation: disable cross-session memory injection (beliefs still accumulate).
    #[arg(long, default_value_t = false)]
    hsm_no_memory: bool,

    #[arg(long)]
    hsm_context_top_k: Option<usize>,

    #[arg(long)]
    hsm_context_budget: Option<usize>,

    #[arg(long)]
    hsm_belief_threshold: Option<f64>,

    #[arg(long)]
    hsm_summary_threshold: Option<f64>,

    /// Max lines for aggregate injected recall block (memdir `MEMORY.md` parity).
    #[arg(long)]
    hsm_memory_max_lines: Option<usize>,

    /// Max bytes for that block (UTF-8).
    #[arg(long)]
    hsm_memory_max_bytes: Option<usize>,

    /// Disable in-session snip / `<compact_boundary>` folding.
    #[arg(long, default_value_t = false)]
    hsm_no_session_compaction: bool,

    #[arg(long)]
    hsm_compaction_trigger_messages: Option<usize>,

    #[arg(long)]
    hsm_compaction_keep_tail: Option<usize>,

    /// After HSM run, write bipartite entity–fact JSON (`<artifacts>/<suite>/memory_graph.json`, or `memory_graph_<suite>.json` without `--artifacts`).
    #[arg(long, default_value_t = false)]
    memory_graph: bool,

    /// Upsert the same bipartite graph into SQLite (runs after eval; use with `--trace` to include `retrieval_turn` / `ranked_belief` facts). Relative paths resolve from cwd.
    #[arg(long)]
    memory_graph_sqlite: Option<PathBuf>,

    /// Load `memory_graph.json` into a SQLite DB and exit (no LLM run). Use with `--memory-graph-json` and `--memory-graph-sqlite-out`.
    #[arg(long)]
    memory_graph_json: Option<PathBuf>,

    /// Output DB path for `--memory-graph-json` ingest-only mode.
    #[arg(long)]
    memory_graph_sqlite_out: Option<PathBuf>,

    #[arg(long)]
    baseline_only: bool,

    #[arg(long)]
    hsm_only: bool,

    #[arg(long)]
    limit: Option<usize>,

    #[arg(short, long)]
    verbose: bool,

    #[arg(long)]
    suite: Option<String>,

    #[arg(long)]
    suites: Option<String>,

    #[arg(long)]
    artifacts: Option<PathBuf>,

    #[arg(long, default_value_t = false)]
    trace: bool,

    #[arg(long)]
    gold: Option<PathBuf>,

    #[arg(long, default_value_t = true)]
    write_runs_index: bool,
}

fn resolve_suites(cli: &Cli) -> anyhow::Result<Vec<WeightedEvalSuite>> {
    let mut out = if let Some(ref spec) = cli.suites {
        parse_weighted_suites(spec).map_err(anyhow::Error::msg)?
    } else {
        let name = cli.suite.as_deref().unwrap_or("full").to_string();
        let tasks = eval_tasks_for_suite(&name).map_err(anyhow::Error::msg)?;
        vec![WeightedEvalSuite {
            name,
            weight: 1.0,
            tasks,
        }]
    };
    for s in &mut out {
        filter_tasks(&mut s.tasks, cli.tasks.as_deref(), cli.limit);
        if s.tasks.is_empty() {
            anyhow::bail!("suite {:?} empty after filters", s.name);
        }
    }
    if let Some(ref dom) = cli.task_domain {
        let dom = dom.trim();
        if dom.is_empty() {
            anyhow::bail!("--task-domain must not be empty");
        }
        for s in &mut out {
            s.tasks.retain(|t| t.domain == dom);
            if s.tasks.is_empty() {
                anyhow::bail!("suite {:?} empty after --task-domain {:?}", s.name, dom);
            }
        }
    }
    Ok(out)
}

fn hsm_runner_config(cli: &Cli) -> anyhow::Result<HsmRunnerConfig> {
    let mut cfg = if let Some(ref path) = cli.hsm_config {
        let text = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("read {}: {}", path.display(), e))?;
        serde_json::from_str(&text)
            .map_err(|e| anyhow::anyhow!("parse HSM config {}: {}", path.display(), e))?
    } else {
        HsmRunnerConfig::default()
    };
    if cli.hsm_no_memory {
        cfg.inject_memory_context = false;
    }
    if let Some(k) = cli.hsm_context_top_k {
        cfg.context_top_k = k;
    }
    if let Some(b) = cli.hsm_context_budget {
        cfg.context_char_budget = b;
    }
    if let Some(t) = cli.hsm_belief_threshold {
        cfg.context_score_threshold = t;
    }
    if let Some(t) = cli.hsm_summary_threshold {
        cfg.summary_score_threshold = t;
    }
    if let Some(n) = cli.hsm_memory_max_lines {
        cfg.memory_entrypoint_max_lines = n;
    }
    if let Some(n) = cli.hsm_memory_max_bytes {
        cfg.memory_entrypoint_max_bytes = n;
    }
    if cli.hsm_no_session_compaction {
        cfg.session_compaction_enabled = false;
    }
    if let Some(n) = cli.hsm_compaction_trigger_messages {
        cfg.session_compaction_trigger_messages = n;
    }
    if let Some(n) = cli.hsm_compaction_keep_tail {
        cfg.session_compaction_keep_tail_messages = n;
    }
    Ok(cfg)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("warn,hsm_eval=info")),
        )
        .compact()
        .init();

    let cli = Cli::parse();

    if let Some(ref json_path) = cli.memory_graph_json {
        let db_out = cli.memory_graph_sqlite_out.as_ref().ok_or_else(|| {
            anyhow::anyhow!("--memory-graph-sqlite-out is required with --memory-graph-json")
        })?;
        ingest_json_file(db_out, json_path)?;
        println!("Ingested {} → {}", json_path.display(), db_out.display());
        return Ok(());
    }

    let hsm_cfg = hsm_runner_config(&cli)?;
    let suites = resolve_suites(&cli)?;

    let suite_names: Vec<String> = suites.iter().map(|s| s.name.clone()).collect();
    let suite_weights: Vec<f64> = suites.iter().map(|s| s.weight).collect();
    let total_tasks: usize = suites.iter().map(|s| s.tasks.len()).sum();
    let total_turns: usize = suites
        .iter()
        .map(|s| s.tasks.iter().map(|t| t.turns.len()).sum::<usize>())
        .sum();

    println!("\n╔══════════════════════════════════════════════════════════╗");
    println!("║         HSM-II Comparative Evaluation Harness           ║");
    println!("╠══════════════════════════════════════════════════════════╣");
    println!("║  Suites: {:<47}║", suite_names.join(", "));
    println!(
        "║  Tasks: {:>3}                                              ║",
        total_tasks
    );
    println!(
        "║  Total turns: {:>3}                                      ║",
        total_turns
    );
    println!("╚══════════════════════════════════════════════════════════╝\n");

    if LlmClient::new().is_err() {
        eprintln!("ERROR: Failed to initialize LLM client");
        eprintln!("Set oneof: OPENAI_API_KEY, ANTHROPIC_API_KEY, or OLLAMA_URL");
        std::process::exit(1);
    }

    let gold = if let Some(ref p) = cli.gold {
        Some(load_gold_labels(p)?)
    } else {
        None
    };

    let mut agg_baseline: Option<RunnerMetrics> = None;
    let mut agg_hsm: Option<RunnerMetrics> = None;
    let mut agg_tasks: Vec<EvalTask> = Vec::new();
    let mut per_suite_reports: Vec<(String, ComparisonReport)> = Vec::new();

    let art_dir = cli.artifacts.clone();
    if let Some(ref root) = art_dir {
        std::fs::create_dir_all(root)?;
    }

    for ws in &suites {
        println!(
            "━━━ Suite: {} ({} tasks, weight {}) ━━━",
            ws.name,
            ws.tasks.len(),
            ws.weight
        );

        let baseline_metrics = if !cli.hsm_only {
            println!("  Baseline…");
            let mut runner = BaselineRunner::new(LlmClient::new()?);
            let m = runner.run(&ws.tasks).await;
            println!(
                "    {} turns | {:.1}% kw | {} tokens",
                m.turns.len(),
                m.avg_keyword_score() * 100.0,
                m.total_tokens()
            );
            Some(m)
        } else {
            None
        };

        let hsm_metrics = if !cli.baseline_only {
            println!("  HSM-II…");
            let mut runner = HsmRunner::with_config(LlmClient::new()?, hsm_cfg.clone());
            if cli.trace {
                runner.set_collect_traces(true);
            }
            let m = runner.run(&ws.tasks).await;
            let traces = if cli.trace {
                runner.take_traces()
            } else {
                vec![]
            };

            let write_graph = cli.memory_graph || cli.memory_graph_sqlite.is_some();
            if write_graph {
                let mut graph =
                    BipartiteMemoryGraph::project_from_snapshot(&runner.export_memory_snapshot());
                if !traces.is_empty() {
                    graph.append_traces(&traces);
                }
                if cli.memory_graph {
                    let path = if let Some(ref root) = art_dir {
                        root.join(&ws.name).join("memory_graph.json")
                    } else {
                        PathBuf::from(format!("memory_graph_{}.json", ws.name))
                    };
                    if let Some(parent) = path.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    std::fs::write(&path, serde_json::to_string_pretty(&graph)?)?;
                    eprintln!("    wrote bipartite memory graph → {}", path.display());
                }
                if let Some(ref dbpath) = cli.memory_graph_sqlite {
                    let mut conn = Connection::open(dbpath)?;
                    init_memory_graph_sqlite_schema(&conn)?;
                    upsert_memory_graph_sqlite(&mut conn, &graph)?;
                    eprintln!(
                        "    upserted bipartite memory graph (SQLite) → {}",
                        dbpath.display()
                    );
                }
            }
            println!(
                "    {} turns | {:.1}% kw | {} tokens",
                m.turns.len(),
                m.avg_keyword_score() * 100.0,
                m.total_tokens()
            );

            if let Some(ref root) = art_dir {
                let sd = root.join(&ws.name);
                std::fs::create_dir_all(&sd)?;
                if let Some(ref b) = baseline_metrics {
                    write_turn_metrics_jsonl(&sd.join("turns_baseline.jsonl"), &b.turns)?;
                }
                write_turn_metrics_jsonl(&sd.join("turns_hsm.jsonl"), &m.turns)?;
                if cli.trace && !traces.is_empty() {
                    write_jsonl(&sd.join("hsm_trace.jsonl"), &traces)?;
                }
                if let Some(ref g) = gold {
                    let cal = calibration_report(&m.turns, g);
                    std::fs::write(
                        sd.join("calibration.json"),
                        serde_json::to_string_pretty(&cal)?,
                    )?;
                    eprintln!(
                        "    calibration | {} labeled | agreement {:.1}%",
                        cal.labeled_turns,
                        cal.agreement_with_gold * 100.0
                    );
                }
            }

            if cli.verbose {
                print_turn_details(&m);
            }
            Some(m)
        } else {
            None
        };

        if let (Some(ref b), Some(ref h)) = (&baseline_metrics, &hsm_metrics) {
            let report = compare(b, h, &ws.tasks);
            eval::print_report(&report);
            per_suite_reports.push((ws.name.clone(), report.clone()));

            match (&mut agg_baseline, &mut agg_hsm) {
                (None, None) => {
                    let mut nb = RunnerMetrics::new("baseline");
                    nb.turns = b.turns.clone();
                    nb.total_duration_ms = b.total_duration_ms;
                    let mut nh = RunnerMetrics::new("hsm-ii");
                    nh.turns = h.turns.clone();
                    nh.total_duration_ms = h.total_duration_ms;
                    agg_baseline = Some(nb);
                    agg_hsm = Some(nh);
                    agg_tasks = ws.tasks.clone();
                }
                (Some(cb), Some(ch)) => {
                    cb.turns.extend(b.turns.iter().cloned());
                    ch.turns.extend(h.turns.iter().cloned());
                    cb.total_duration_ms += b.total_duration_ms;
                    ch.total_duration_ms += h.total_duration_ms;
                    agg_tasks.extend(ws.tasks.clone());
                }
                _ => {}
            }

            if let Some(ref path) = cli.json {
                let base = path.file_stem().and_then(|s| s.to_str()).unwrap_or("eval");
                let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("json");
                let jp = path.with_file_name(format!("{}_{}.{}", base, ws.name, ext));
                std::fs::write(&jp, serde_json::to_string_pretty(&report)?)?;
                println!("Exported {}", jp.display());
            }

            if let Some(ref root) = art_dir {
                let sd = root.join(&ws.name);
                std::fs::write(
                    sd.join("comparison_report.json"),
                    serde_json::to_string_pretty(&report)?,
                )?;
                std::fs::write(
                    sd.join("baseline_metrics.json"),
                    serde_json::to_string_pretty(b)?,
                )?;
                std::fs::write(
                    sd.join("hsm_metrics.json"),
                    serde_json::to_string_pretty(h)?,
                )?;
            }
        } else if let Some(ref b) = baseline_metrics {
            if cli.verbose {
                print_turn_details(b);
            }
        } else if let Some(ref h) = hsm_metrics {
            if cli.verbose {
                print_turn_details(h);
            }
        }
    }

    if suites.len() > 1 {
        if let (Some(ref b), Some(ref h)) = (&agg_baseline, &agg_hsm) {
            println!("\n━━━ Combined (all suites) ━━━");
            let report = compare(b, h, &agg_tasks);
            eval::print_report(&report);
            if let Some(ref path) = cli.json {
                let base = path.file_stem().and_then(|s| s.to_str()).unwrap_or("eval");
                let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("json");
                let comb = path.with_file_name(format!("{}_combined.{}", base, ext));
                std::fs::write(&comb, serde_json::to_string_pretty(&report)?)?;
                println!("Exported {}", comb.display());
            }
        }
    }

    if let Some(ref root) = art_dir {
        let manifest = RunManifest::new(
            "hsm-eval",
            root,
            suite_names.clone(),
            Some(suite_weights.clone()),
            cli.tasks.clone(),
            total_tasks,
            total_turns,
            std::env::var("HSM_PARENT_RUN_ID").ok(),
            ArtifactPaths {
                manifest: "manifest.json".into(),
                turns_baseline_jsonl: Some("<suite>/turns_baseline.jsonl".into()),
                turns_hsm_jsonl: Some("<suite>/turns_hsm.jsonl".into()),
                hsm_trace_jsonl: if cli.trace {
                    Some("<suite>/hsm_trace.jsonl".into())
                } else {
                    None
                },
                comparison_json: Some("<suite>/comparison_report.json".into()),
                per_suite_json: None,
            },
        );
        write_manifest(root, &manifest)?;
        if cli.write_runs_index {
            if let Some((name, rep)) = per_suite_reports.first() {
                let idx_path = default_runs_index(root);
                let line = serde_json::json!({
                    "harness": "hsm-eval",
                    "run_dir": root.display().to_string(),
                    "created_unix": manifest.created_unix,
                    "git_commit": manifest.git_commit,
                    "first_suite": name,
                    "keyword_delta": rep.improvement.keyword_score_delta,
                });
                append_runs_index(&idx_path, &line)?;
                if let Ok(db) = std::env::var("HSM_RUNS_SQLITE") {
                    if let Err(e) = sync_index_line_to_sqlite(std::path::Path::new(&db), &line) {
                        eprintln!("warning: HSM_RUNS_SQLITE sync failed: {}", e);
                    }
                }
            }
        }
        println!("Artifacts written to {}", root.display());
    }

    Ok(())
}

fn print_turn_details(metrics: &RunnerMetrics) {
    for turn in &metrics.turns {
        let recall_marker = if turn.requires_recall {
            " [RECALL]"
        } else {
            ""
        };
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
