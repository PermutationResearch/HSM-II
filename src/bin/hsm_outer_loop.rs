//! Outer-loop scaffolding: compile gate, proposer context, SQLite run index, external benchmarks.
//!
//! ```text
//! hsm_outer_loop compile-check --workspace .
//! hsm_outer_loop index-db --jsonl runs/runs_index.jsonl --db runs/runs.sqlite
//! hsm_outer_loop query-db --db runs/runs.sqlite --harness hsm_meta_harness
//! hsm_outer_loop propose --workspace . --db runs/runs.sqlite --out proposals/context.json
//! hsm_outer_loop external-batch --spec config/external_suite.example.json
//! hsm_outer_loop list-runs --index runs/runs_index.jsonl
//! ```

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;
use clap::{Parser, Subcommand};
use serde::Serialize;
use tracing_subscriber::EnvFilter;

use hyper_stigmergy::eval::{
    build_proposer_context, ingest_jsonl, open_run_store, query_best_objective, query_by_harness,
    query_by_run_dir_contains, query_recent, rebuild_fts, run_external_batch_sync,
    run_external_sync, search_fts, try_git_head, write_manifest, ArtifactPaths,
    ExternalBenchmarkBatch, ExternalBenchmarkSpec, ProposerOptions, RunManifest,
};

#[derive(Parser)]
#[command(name = "hsm_outer_loop")]
#[command(about = "Compile gates, run DB, proposer context, external benchmark batches")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run `cargo check` on the workspace.
    CompileCheck {
        #[arg(long, default_value = ".", alias = "manifest-path")]
        workspace: PathBuf,
    },
    /// Run one external benchmark spec (JSON).
    External {
        #[arg(long)]
        spec: PathBuf,
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Run several external benchmarks from one JSON file `{ "benchmarks": [ ... ] }`.
    ExternalBatch {
        #[arg(long)]
        spec: PathBuf,
        #[arg(long)]
        out: Option<PathBuf>,
        /// Stop after first failing benchmark (rest skipped).
        #[arg(long, default_value_t = false)]
        fail_fast: bool,
    },
    /// Append JSONL lines into SQLite for richer queries than `list-runs`.
    IndexDb {
        #[arg(long, default_value = "runs/runs_index.jsonl")]
        jsonl: PathBuf,
        #[arg(long, default_value = "runs/runs.sqlite")]
        db: PathBuf,
        #[arg(long, default_value_t = false)]
        reset: bool,
        /// Rebuild FTS index from `runs` after ingest (needed for older DBs missing FTS rows).
        #[arg(long, default_value_t = false)]
        rebuild_fts: bool,
    },
    /// Query the SQLite run store (JSON rows to stdout).
    QueryDb {
        #[arg(long, default_value = "runs/runs.sqlite")]
        db: PathBuf,
        /// [FTS5](https://www.sqlite.org/fts5.html#full_text_query_syntax) query over denormalized text + raw JSON.
        #[arg(long)]
        fts: Option<String>,
        #[arg(long)]
        harness: Option<String>,
        #[arg(long, default_value_t = 25)]
        limit: usize,
        #[arg(long)]
        min_objective: Option<f64>,
        #[arg(long)]
        run_dir_contains: Option<String>,
    },
    /// Emit `ProposerContext` JSON for a coding agent (harness paths + past runs).
    Propose {
        #[arg(long, default_value = ".")]
        workspace: PathBuf,
        #[arg(long, default_value = "runs/runs.sqlite")]
        db: PathBuf,
        /// Ingest this JSONL into `db` before building context (append-only).
        #[arg(long)]
        sync_jsonl: Option<PathBuf>,
        #[arg(long, default_value_t = 20)]
        recent_limit: usize,
        #[arg(long, default_value_t = 0.0)]
        top_objective_min: f64,
        #[arg(long, default_value_t = 10)]
        top_objective_limit: usize,
        #[arg(long)]
        out: Option<PathBuf>,
    },
    SuggestArtifacts {
        #[arg(long)]
        label: String,
        #[arg(long, default_value = "runs")]
        parent: PathBuf,
    },
    ListRuns {
        #[arg(long, default_value = "runs/runs_index.jsonl")]
        index: PathBuf,
        #[arg(long, default_value_t = 30)]
        limit: usize,
    },
}

#[derive(Serialize)]
struct LoopManifest {
    pub created_unix: u64,
    pub git_commit: Option<String>,
    pub label: String,
    pub suggested_commands: Vec<String>,
}

fn stamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("warn,hsm_outer_loop=info")),
        )
        .compact()
        .init();

    let cli = Cli::parse();
    match cli.command {
        Commands::CompileCheck { workspace } => {
            let st = std::process::Command::new("cargo")
                .args(["check", "--workspace"])
                .current_dir(&workspace)
                .status()
                .context("spawn cargo check")?;
            if !st.success() {
                anyhow::bail!("cargo check failed with {:?}", st.code());
            }
            println!("cargo check --workspace OK (cwd {})", workspace.display());
        }
        Commands::External { spec, out } => {
            let text = std::fs::read_to_string(&spec)
                .with_context(|| format!("read {}", spec.display()))?;
            let b: ExternalBenchmarkSpec = serde_json::from_str(&text)
                .context("parse ExternalBenchmarkSpec JSON")?;
            let result = run_external_sync(&b)?;
            let path = out.unwrap_or_else(|| {
                PathBuf::from(format!("runs/external_{}_{}.json", b.name, stamp()))
            });
            if let Some(p) = path.parent() {
                std::fs::create_dir_all(p)?;
            }
            std::fs::write(&path, serde_json::to_string_pretty(&result)?)?;
            println!(
                "External {} | pass={} score={:.3} timeout={} | {}",
                result.name,
                result.passed,
                result.score,
                result.timed_out,
                path.display()
            );
            if !result.passed {
                std::process::exit(1);
            }
        }
        Commands::ExternalBatch { spec, out, fail_fast } => {
            let text = std::fs::read_to_string(&spec)
                .with_context(|| format!("read {}", spec.display()))?;
            let batch: ExternalBenchmarkBatch =
                serde_json::from_str(&text).context("parse ExternalBenchmarkBatch JSON")?;
            let batch_result = run_external_batch_sync(&batch, fail_fast)?;
            let path = out.unwrap_or_else(|| {
                PathBuf::from(format!("runs/external_batch_{}.json", stamp()))
            });
            if let Some(p) = path.parent() {
                std::fs::create_dir_all(p)?;
            }
            std::fs::write(&path, serde_json::to_string_pretty(&batch_result)?)?;
            println!(
                "Batch | mean_score={:.3} all_passed={} stopped_early={} | {}",
                batch_result.mean_score,
                batch_result.all_passed,
                batch_result.stopped_early,
                path.display()
            );
            if !batch_result.all_passed {
                std::process::exit(1);
            }
        }
        Commands::IndexDb {
            jsonl,
            db,
            reset,
            rebuild_fts: do_rebuild_fts,
        } => {
            let conn = open_run_store(&db)?;
            if reset {
                let n = hyper_stigmergy::eval::clear_runs(&conn)?;
                println!("reset: deleted {} rows", n);
            }
            if jsonl.exists() {
                let (seen, inserted) = ingest_jsonl(&conn, &jsonl)?;
                let total = hyper_stigmergy::eval::row_count(&conn)?;
                println!(
                    "jsonl {} | lines_seen={} inserted_new={} (duplicates skipped) | total_rows={}",
                    jsonl.display(),
                    seen,
                    inserted,
                    total
                );
            } else {
                println!(
                    "(no jsonl at {}, schema ready at {})",
                    jsonl.display(),
                    db.display()
                );
            }
            if do_rebuild_fts {
                let n = rebuild_fts(&conn)?;
                println!("rebuild_fts: indexed {} runs", n);
            }
        }
        Commands::QueryDb {
            db,
            fts,
            harness,
            limit,
            min_objective,
            run_dir_contains,
        } => {
            let conn = open_run_store(&db)?;
            let rows = if let Some(ref q) = fts {
                search_fts(&conn, q, limit)?
            } else if let Some(sub) = run_dir_contains {
                query_by_run_dir_contains(&conn, &sub, limit)?
            } else if let Some(mo) = min_objective {
                query_best_objective(&conn, mo, limit)?
            } else if let Some(ref h) = harness {
                query_by_harness(&conn, h, limit)?
            } else {
                query_recent(&conn, limit)?
            };
            println!("{}", serde_json::to_string_pretty(&rows)?);
        }
        Commands::Propose {
            workspace,
            db,
            sync_jsonl,
            recent_limit,
            top_objective_min,
            top_objective_limit,
            out,
        } => {
            let conn = open_run_store(&db)?;
            if let Some(ref jl) = sync_jsonl {
                if jl.exists() {
                    let (seen, inserted) = ingest_jsonl(&conn, jl)?;
                    println!(
                        "sync_jsonl {} | seen={} inserted_new={}",
                        jl.display(),
                        seen,
                        inserted
                    );
                }
            }
            let ctx = build_proposer_context(
                &workspace,
                &db,
                ProposerOptions {
                    recent_limit,
                    top_objective_min,
                    top_objective_limit,
                },
            )?;
            let path = out.unwrap_or_else(|| {
                PathBuf::from(format!(
                    "proposals/proposer_context_{}.json",
                    stamp()
                ))
            });
            if let Some(p) = path.parent() {
                std::fs::create_dir_all(p)?;
            }
            std::fs::write(&path, serde_json::to_string_pretty(&ctx)?)?;
            println!(
                "Wrote {} ({} harness files, {} recent runs)",
                path.display(),
                ctx.harness_source_files.len(),
                ctx.recent_runs.len()
            );
        }
        Commands::ListRuns { index, limit } => {
            if !index.exists() {
                println!("(no index file at {})", index.display());
                return Ok(());
            }
            let text = std::fs::read_to_string(&index).with_context(|| index.display().to_string())?;
            let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
            let start = lines.len().saturating_sub(limit);
            for line in &lines[start..] {
                let v: serde_json::Value = serde_json::from_str(line).with_context(|| {
                    format!(
                        "bad JSONL line: {}",
                        line.chars().take(120).collect::<String>()
                    )
                })?;
                println!("{}", serde_json::to_string_pretty(&v)?);
                println!("---");
            }
        }
        Commands::SuggestArtifacts { label, parent } => {
            std::fs::create_dir_all(&parent)?;
            let run_dir = parent.join(format!("outer_{}_{}", label, stamp()));
            std::fs::create_dir_all(&run_dir)?;
            let manifest = LoopManifest {
                created_unix: stamp(),
                git_commit: try_git_head(),
                label: label.clone(),
                suggested_commands: vec![
                    "cargo run --bin hsm_outer_loop -- compile-check --workspace .".into(),
                    "cargo run --bin hsm_outer_loop -- index-db --jsonl runs/runs_index.jsonl --db runs/runs.sqlite --rebuild-fts"
                        .into(),
                    "cargo run --bin hsm_outer_loop -- propose --workspace . --db runs/runs.sqlite --sync-jsonl runs/runs_index.jsonl"
                        .into(),
                    "cargo run --bin hsm_meta_harness -- --trace --suites memory:1,tool:1 --candidates 4".into(),
                    "cargo run --bin hsm-eval -- --artifacts <dir> --trace --suites memory:1,tool:1".into(),
                ],
            };
            std::fs::write(
                run_dir.join("loop_manifest.json"),
                serde_json::to_string_pretty(&manifest)?,
            )?;
            let rm = RunManifest::new(
                "hsm_outer_loop",
                &run_dir,
                vec![format!("outer_{}", label)],
                None,
                None,
                0,
                0,
                std::env::var("HSM_PARENT_RUN_ID").ok(),
                ArtifactPaths {
                    manifest: "manifest.json".into(),
                    turns_baseline_jsonl: None,
                    turns_hsm_jsonl: None,
                    hsm_trace_jsonl: None,
                    comparison_json: None,
                    per_suite_json: None,
                },
            );
            write_manifest(&run_dir, &rm)?;
            println!("Wrote {} and manifest.json", run_dir.display());
        }
    }
    Ok(())
}
