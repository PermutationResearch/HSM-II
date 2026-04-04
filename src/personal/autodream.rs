//! autoDream-style scheduled consolidation: roll up memory extracts + staleness markers into
//! `memory/consolidated/autodream-*.md`, and persist mtimes in `memory/.autodream_state.json`.
//!
//! Enable with `HSM_AUTODREAM=1`. Interval: `HSM_AUTODREAM_INTERVAL_SECS` (default `3600`), or
//! `HSM_AUTODREAM_USE_HEARTBEAT_INTERVAL=1` to reuse `HSM_HEARTBEAT_INTERVAL_SECS` / config.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AutoDreamState {
    pub last_run_unix: u64,
    #[serde(default)]
    pub watched_mtimes: HashMap<String, u64>,
}

fn enabled() -> bool {
    std::env::var("HSM_AUTODREAM")
        .map(|v| {
            let s = v.trim();
            s == "1" || s.eq_ignore_ascii_case("true") || s.eq_ignore_ascii_case("yes")
        })
        .unwrap_or(false)
}

pub fn effective_interval_secs(fallback_heartbeat: Option<u64>) -> u64 {
    if std::env::var("HSM_AUTODREAM_USE_HEARTBEAT_INTERVAL")
        .map(|v| {
            let s = v.trim();
            s == "1" || s.eq_ignore_ascii_case("true") || s.eq_ignore_ascii_case("yes")
        })
        .unwrap_or(false)
    {
        if let Some(h) = fallback_heartbeat.filter(|&s| s > 0) {
            return h;
        }
    }
    std::env::var("HSM_AUTODREAM_INTERVAL_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|&n| n > 0)
        .unwrap_or(3600)
}

fn state_path(home: &Path) -> PathBuf {
    home.join("memory/.autodream_state.json")
}

pub fn load_state(home: &Path) -> AutoDreamState {
    let p = state_path(home);
    std::fs::read(&p)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save_state(home: &Path, state: &AutoDreamState) -> Result<()> {
    let p = state_path(home);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec_pretty(state)?;
    crate::fs_atomic::write_atomic(&p, &bytes)?;
    Ok(())
}

fn file_mtime_unix(path: &Path) -> Option<u64> {
    std::fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
}

pub fn watched_files(home: &Path) -> Vec<(String, PathBuf)> {
    let rels = [
        "AGENTS.md",
        "MEMORY.md",
        "USER.md",
        "prompt.template.md",
        "config/prompt_routes.yaml",
    ];
    rels.iter().map(|r| (r.to_string(), home.join(r))).collect()
}

/// Files whose mtime increased vs `state.watched_mtimes` (or missing prior entry and file exists).
pub fn stale_watched_paths(home: &Path, state: &AutoDreamState) -> Vec<String> {
    let mut out = Vec::new();
    for (rel, path) in watched_files(home) {
        let Some(m) = file_mtime_unix(&path) else {
            continue;
        };
        match state.watched_mtimes.get(&rel) {
            Some(&prev) if prev >= m => {}
            _ => out.push(rel),
        }
    }
    out
}

fn list_recent_extracts(home: &Path, max: usize) -> Vec<(String, String)> {
    let root = home.join("memory/extracts");
    if !root.is_dir() {
        return Vec::new();
    }
    let mut files: Vec<PathBuf> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&root) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) == Some("md") {
                files.push(p);
            }
        }
    }
    files.sort_by(|a, b| {
        let ta = std::fs::metadata(a)
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        let tb = std::fs::metadata(b)
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        tb.cmp(&ta)
    });
    files
        .into_iter()
        .take(max)
        .filter_map(|p| {
            let name = p.file_name()?.to_string_lossy().into_owned();
            let snippet = std::fs::read_to_string(&p)
                .ok()?
                .chars()
                .take(400)
                .collect::<String>()
                .replace('\n', " ");
            Some((name, snippet))
        })
        .collect()
}

/// Run consolidation if enabled, interval elapsed (local + persisted), and there is work
/// (stale instruction files or recent extracts).
pub async fn maybe_consolidate(
    home: &Path,
    last_local_tick: &mut Instant,
    interval_secs: u64,
) -> Result<()> {
    if !enabled() {
        return Ok(());
    }
    if last_local_tick.elapsed().as_secs() < interval_secs {
        return Ok(());
    }

    let now_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let mut state = load_state(home);
    if now_unix.saturating_sub(state.last_run_unix) < interval_secs {
        *last_local_tick = Instant::now();
        return Ok(());
    }

    let stale = stale_watched_paths(home, &state);
    let extracts = list_recent_extracts(home, 12);
    if stale.is_empty() && extracts.is_empty() {
        *last_local_tick = Instant::now();
        state.last_run_unix = now_unix;
        for (rel, path) in watched_files(home) {
            if let Some(m) = file_mtime_unix(&path) {
                state.watched_mtimes.insert(rel, m);
            }
        }
        save_state(home, &state)?;
        return Ok(());
    }

    *last_local_tick = Instant::now();

    let out_dir = home.join("memory/consolidated");
    std::fs::create_dir_all(&out_dir)?;
    let label = chrono::Utc::now().format("%Y%m%dT%H%M%SZ");
    let out_path = out_dir.join(format!("autodream-{label}.md"));

    let mut body = String::new();
    body.push_str("# autoDream consolidation\n\n");
    body.push_str(&format!(
        "- generated_at: `{}` (unix {})\n",
        label, now_unix
    ));
    if !stale.is_empty() {
        body.push_str("\n## Stale / updated instruction files\n\n");
        for s in &stale {
            body.push_str(&format!("- `{}`\n", s));
        }
    }
    if !extracts.is_empty() {
        body.push_str("\n## Recent memory extracts (snapshots)\n\n");
        for (name, snip) in &extracts {
            body.push_str(&format!("### `{}`\n> {}\n\n", name, snip));
        }
    }
    body.push_str(
        "\n---\n_Merge noteworthy lines into MEMORY.md or your living prompt as you see fit._\n",
    );

    tokio::fs::write(&out_path, &body).await?;

    state.last_run_unix = now_unix;
    for (rel, path) in watched_files(home) {
        if let Some(m) = file_mtime_unix(&path) {
            state.watched_mtimes.insert(rel, m);
        }
    }
    save_state(home, &state)?;

    info!(
        target: "hsm_autodream",
        path = %out_path.display(),
        stale = ?stale,
        n_extracts = extracts.len(),
        "autoDream consolidation written"
    );
    Ok(())
}

/// Summarize staleness for console API (union of extract mtimes vs state not tracked — keep simple).
pub fn staleness_snapshot(home: &Path) -> serde_json::Value {
    let state = load_state(home);
    let stale = stale_watched_paths(home, &state);
    let watched: HashMap<String, Option<u64>> = watched_files(home)
        .into_iter()
        .map(|(rel, p)| (rel, file_mtime_unix(&p)))
        .collect();
    serde_json::json!({
        "autodream_enabled": enabled(),
        "last_run_unix": state.last_run_unix,
        "stale_watched": stale,
        "watched_mtimes": watched,
        "interval_secs": effective_interval_secs(None),
    })
}
