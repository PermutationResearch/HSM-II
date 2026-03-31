//! Trace2Skill CLI: merge JSONL trajectories → merged skill JSON; optionally import eval artifacts.

use std::path::PathBuf;

use anyhow::Context;
use clap::{Parser, Subcommand};

use hyper_stigmergy::embedded_graph_store::EmbeddedGraphStore;
use hyper_stigmergy::trace2skill::{
    import_eval_artifacts_to_jsonl, load_merged, merge_pool, read_jsonl, save_merged,
};

#[derive(Parser)]
#[command(name = "hsm_trace2skill")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Read trajectory JSONL, run analysts, write merged skill JSON (`HSM_TRACE2SKILL_LLM=1` for LLM lessons).
    Merge {
        #[arg(long)]
        r#in: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Convert `hsm-eval` / meta-harness `--artifacts` dir → Trace2Skill JSONL (then merge with `merge`).
    ImportEval {
        #[arg(long)]
        artifacts: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Load `merged.json`, add/update a Proposed skill in the embedded world, save.
    Apply {
        #[arg(long)]
        merged: PathBuf,
    },
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .init();

    let cli = Cli::parse();
    match cli.command {
        Command::Merge { r#in, out } => {
            let rows = read_jsonl(&r#in).with_context(|| format!("read {}", r#in.display()))?;
            let merged = merge_pool(&rows);
            save_merged(&out, &merged).with_context(|| format!("write {}", out.display()))?;
            eprintln!(
                "wrote {} ({} trajectories → 1 proposal)",
                out.display(),
                rows.len()
            );
        }
        Command::ImportEval { artifacts, out } => {
            let n = import_eval_artifacts_to_jsonl(&artifacts, &out)
                .with_context(|| format!("import {}", artifacts.display()))?;
            eprintln!(
                "wrote {} trajectories to {} (eval → Trace2Skill JSONL)",
                n,
                out.display()
            );
        }
        Command::Apply { merged } => {
            let doc = load_merged(&merged).with_context(|| format!("read {}", merged.display()))?;
            let (mut world, rlm) = EmbeddedGraphStore::load_world()
                .context("EmbeddedGraphStore::load_world (is HSM-II data present?)")?;
            let skill = world.skill_bank.ingest_trace2skill_proposal(
                &doc.title,
                &doc.principle,
                &doc.trajectory_ids,
            );
            let bytes =
                EmbeddedGraphStore::save_world(&world, rlm.as_ref()).context("save_world")?;
            eprintln!(
                "ingested trace2skill skill id={} title={:?} ({} bytes written)",
                skill.id, skill.title, bytes
            );
        }
    }
    Ok(())
}
