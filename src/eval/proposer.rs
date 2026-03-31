//! Deterministic **proposer context** for a coding agent: harness file surface + past runs from SQLite.
//!
//! Does not call an LLM — it emits JSON you feed to Cursor/Codex/your proposer.

use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use super::run_store::{open_run_store, query_best_objective, query_recent, RunRow};
use super::try_git_head;

/// Paths under `src/eval/**` suitable for harness / rubric edits.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProposerContext {
    pub generated_unix: u64,
    pub git_head: Option<String>,
    pub workspace: PathBuf,
    pub harness_source_files: Vec<String>,
    pub recent_runs: Vec<RunRow>,
    pub top_objective_runs: Vec<RunRow>,
    /// Copy-paste instructions for an agent performing **code** search + edits.
    pub agent_instructions: String,
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// All `*.rs` files under `workspace/src/eval`, relative to `workspace`.
pub fn discover_harness_rust_sources(workspace: &Path) -> anyhow::Result<Vec<String>> {
    let base = workspace.join("src/eval");
    if !base.is_dir() {
        return Ok(Vec::new());
    }
    let mut out: Vec<String> = WalkDir::new(&base)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "rs"))
        .map(|e| {
            e.path()
                .strip_prefix(workspace)
                .unwrap_or(e.path())
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    out.sort();
    Ok(out)
}

#[derive(Clone, Debug)]
pub struct ProposerOptions {
    pub recent_limit: usize,
    pub top_objective_min: f64,
    pub top_objective_limit: usize,
}

impl Default for ProposerOptions {
    fn default() -> Self {
        Self {
            recent_limit: 20,
            top_objective_min: 0.0,
            top_objective_limit: 10,
        }
    }
}

/// Build a context document from workspace + SQLite run store.
pub fn build_proposer_context(
    workspace: &Path,
    db_path: &Path,
    opts: ProposerOptions,
) -> anyhow::Result<ProposerContext> {
    let conn = open_run_store(db_path).with_context(|| db_path.display().to_string())?;
    let recent_runs = query_recent(&conn, opts.recent_limit)?;
    let top_objective_runs = query_best_objective(&conn, opts.top_objective_min, opts.top_objective_limit)?;
    let harness_source_files = discover_harness_rust_sources(workspace)?;
    let file_list = harness_source_files.join("\n");
    let agent_instructions = format!(
        r#"You are improving the HSM-II **eval harness** (not task JSON), measurable via `hsm-eval` / `hsm_meta_harness`.

Editable Rust surface (search and patch only what matters):
{file_list}

Workflow:
1. Read `recent_runs` / `top_objective_runs` — note `run_dir`, `objective_score`, `git_commit`.
2. Inspect traces or `comparison_report.json` under those dirs if present.
3. Propose a **minimal diff** to `src/eval/*.rs` (judges, runner, metrics) or prompts embedded there.
4. Run `cargo run --bin hsm_outer_loop -- compile-check --workspace .` then a small eval.

Constraints: preserve public eval APIs where possible; extend with new modules rather than mega-rewrites unless necessary."#,
        file_list = file_list,
    );

    Ok(ProposerContext {
        generated_unix: unix_now(),
        git_head: try_git_head(),
        workspace: workspace.to_path_buf(),
        harness_source_files,
        recent_runs,
        top_objective_runs,
        agent_instructions,
    })
}
