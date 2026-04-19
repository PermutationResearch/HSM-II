//! Instruction-file hygiene: detect bloated .md files and LLM-distil them.
//!
//! ## The problem
//! Agents accumulate instruction text over time: YC-bench heuristics, brand guidelines,
//! mechanical if/then trees, survival checklists, agent briefings… Each new rule gets
//! appended. Past a threshold the additional bytes are *noise* — they dilute the signal,
//! push high-value rules past the context cap, and degrade decision quality.
//!
//! ## What this module does
//! On a configurable cadence (default every 6 hours) it:
//!   1. Scans the instruction .md files for an agent home (`AGENTS.md`, `VISION.md`,
//!      `CEO_BOOTSTRAP.md`, `prompt.template.md`, and anything under `skills/`).
//!   2. Builds a [`HygieneReport`] — per-file sizes, total bytes, noise score.
//!   3. If total instruction bytes exceed `HSM_HYGIENE_TOTAL_BYTES` (default 16 KB), it
//!      distils every file over `HSM_HYGIENE_FILE_BYTES` (default 3 KB) using the LLM:
//!      *"Compress to essential rules only. Remove prose explanations. Keep decision trees,
//!      specific numbers, and code blocks verbatim."*
//!   4. Writes the distilled content back, saves the original as `<file>.hygiene.bak`.
//!   5. Appends a record to `memory/.hygiene_log.jsonl`.
//!
//! ## Controls
//! | env var | default | meaning |
//! |---------|---------|---------|
//! | `HSM_MD_HYGIENE` | `0` | set `1` to enable |
//! | `HSM_HYGIENE_INTERVAL_SECS` | `21600` (6 h) | how often to run |
//! | `HSM_HYGIENE_TOTAL_BYTES` | `16384` (16 KB) | trigger threshold: total instruction bytes |
//! | `HSM_HYGIENE_FILE_BYTES` | `3072` (3 KB) | per-file distillation threshold |
//! | `HSM_HYGIENE_DRY_RUN` | `0` | set `1` to log without writing changes |

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tracing::{info, warn};

// ── Configuration ────────────────────────────────────────────────────────────

fn enabled() -> bool {
    env_flag("HSM_MD_HYGIENE")
}

fn dry_run() -> bool {
    env_flag("HSM_HYGIENE_DRY_RUN")
}

fn env_flag(var: &str) -> bool {
    std::env::var(var)
        .map(|v| {
            let s = v.trim().to_ascii_lowercase();
            s == "1" || s == "true" || s == "yes"
        })
        .unwrap_or(false)
}

fn interval_secs() -> u64 {
    std::env::var("HSM_HYGIENE_INTERVAL_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|&n: &u64| n > 0)
        .unwrap_or(21_600) // 6 h
}

fn total_threshold() -> usize {
    std::env::var("HSM_HYGIENE_TOTAL_BYTES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(16_384) // 16 KB
}

fn file_threshold() -> usize {
    std::env::var("HSM_HYGIENE_FILE_BYTES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3_072) // 3 KB
}

// ── State persistence ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HygieneState {
    pub last_run_unix: u64,
    /// Total instruction bytes measured on last run.
    pub last_total_bytes: usize,
    /// Files distilled on last run: path → bytes saved.
    #[serde(default)]
    pub last_distilled: Vec<(String, i64)>,
}

fn state_path(home: &Path) -> PathBuf {
    home.join("memory/.hygiene_state.json")
}

fn load_state(home: &Path) -> HygieneState {
    std::fs::read(state_path(home))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save_state(home: &Path, state: &HygieneState) -> Result<()> {
    let p = state_path(home);
    if let Some(par) = p.parent() {
        std::fs::create_dir_all(par)?;
    }
    crate::fs_atomic::write_atomic(&p, &serde_json::to_vec_pretty(state)?)?;
    Ok(())
}

// ── Log ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct HygieneLogEntry {
    pub ts_unix: u64,
    pub total_bytes_before: usize,
    pub total_bytes_after: usize,
    pub dry_run: bool,
    pub files: Vec<FileHygieneResult>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FileHygieneResult {
    pub rel_path: String,
    pub bytes_before: usize,
    pub bytes_after: usize,
    /// Positive = bytes saved; negative = expanded (shouldn't happen).
    pub bytes_saved: i64,
    pub action: String, // "distilled" | "skipped" | "unchanged"
}

fn append_log(home: &Path, entry: &HygieneLogEntry) -> Result<()> {
    let p = home.join("memory/.hygiene_log.jsonl");
    if let Some(par) = p.parent() {
        std::fs::create_dir_all(par)?;
    }
    let line = serde_json::to_string(entry)? + "\n";
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&p)?;
    f.write_all(line.as_bytes())?;
    Ok(())
}

// ── File discovery ───────────────────────────────────────────────────────────

/// Returns `(rel_path, abs_path)` for every instruction .md file in `home`.
pub fn instruction_files(home: &Path) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();

    // Top-level structural files
    for name in &[
        "AGENTS.md",
        "VISION.md",
        "CEO_BOOTSTRAP.md",
        "prompt.template.md",
        "HEARTBEAT.md",
    ] {
        let p = home.join(name);
        if p.exists() {
            out.push((name.to_string(), p));
        }
    }

    // skills/*.md
    let skills_dir = home.join("skills");
    if let Ok(rd) = std::fs::read_dir(&skills_dir) {
        let mut skill_files: Vec<_> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("md"))
            .collect();
        skill_files.sort();
        for p in skill_files {
            if let Some(name) = p.file_name() {
                out.push((format!("skills/{}", name.to_string_lossy()), p));
            }
        }
    }

    // agents/<slug>/AGENTS.md  (per-agent briefings)
    let agents_dir = home.join("agents");
    if let Ok(rd) = std::fs::read_dir(&agents_dir) {
        for entry in rd.flatten() {
            let slug_path = entry.path();
            if !slug_path.is_dir() {
                continue;
            }
            let briefing = slug_path.join("AGENTS.md");
            if briefing.exists() {
                let rel = format!(
                    "agents/{}/AGENTS.md",
                    slug_path.file_name().unwrap_or_default().to_string_lossy()
                );
                out.push((rel, briefing));
            }
        }
    }

    out
}

/// Measure total instruction bytes currently on disk.
pub fn measure_total_bytes(home: &Path) -> usize {
    instruction_files(home)
        .iter()
        .filter_map(|(_, p)| std::fs::metadata(p).ok().map(|m| m.len() as usize))
        .sum()
}

// ── Noise scoring ────────────────────────────────────────────────────────────

/// Simple noise proxy: large files with many section headers accumulate the most redundancy.
/// Returns a score in [0, ∞). Higher = more likely to benefit from distillation.
pub fn noise_score(content: &str) -> f32 {
    let bytes = content.len() as f32;
    let section_headers = content.lines().filter(|l| l.starts_with('#')).count() as f32;
    let repetition_penalty = {
        // Rough repetition: count how many 8-word n-grams appear ≥ 2 times
        let words: Vec<&str> = content.split_whitespace().collect();
        let mut seen = std::collections::HashSet::new();
        let mut dupes = 0usize;
        for w in words.windows(8) {
            let key = w.join(" ");
            if !seen.insert(key.clone()) {
                dupes += 1;
            }
        }
        dupes as f32
    };
    (bytes / 1000.0) * (1.0 + section_headers / 10.0) * (1.0 + repetition_penalty / 50.0)
}

// ── LLM distillation ─────────────────────────────────────────────────────────

const DISTILL_SYSTEM: &str = "\
You are an expert at restructuring and compressing AI agent instruction files.

## Canonical output format
Every distilled file must follow this exact structure:

```
# <Agent or file name>

<One tight paragraph — the holistic description. What this agent IS, what it exists to do,
and its defining character. No bullet points. No hedging. One paragraph maximum.>

## DO
- <Specific, actionable, positive rule>
- <Another DO>
...

## DON'T
- <Specific, actionable, negative rule — what to never do>
- <Another DON'T>
...
```

If the file has hard decision rules (IF/THEN trees, numeric thresholds, survival rules),
add a third section after DON'T:

```
## RULES
- IF <condition>: <action>
- IF <condition> AND <condition>: <action>
...
```

## How to distil

KEEP:
- All IF/THEN decision trees and numeric thresholds verbatim (they are load-bearing)
- Specific numbers, dollar amounts, percentages, counts
- Code blocks and YAML blocks unchanged
- The agent's distinct voice and identity

REMOVE:
- Verbose prose explanations that restate what the rule already says
- Worked examples and scenario walkthroughs (keep the rule, drop the story)
- Aphorisms, philosophy, and motivational text
- Redundant restatements of the same rule under different headings
- Any section whose content is already captured by a bullet elsewhere

Target: 40-60% size reduction while preserving 100% of the decision logic.

Output ONLY the distilled file content. No preamble, no explanation, no commentary.";

async fn distil_file(
    content: &str,
    rel_path: &str,
    llm: &crate::ollama_client::OllamaClient,
) -> Result<String> {
    let user = format!(
        "Compress this agent instruction file (`{}`):\n\n```markdown\n{}\n```",
        rel_path, content
    );
    let result = llm.chat(DISTILL_SYSTEM, &user, &[]).await;
    // Strip markdown fences if the LLM wrapped the output
    let text = result.text.trim().to_string();
    let text = if text.starts_with("```markdown") && text.ends_with("```") {
        text[11..text.len() - 3].trim().to_string()
    } else if text.starts_with("```") && text.ends_with("```") {
        text[3..text.len() - 3].trim().to_string()
    } else {
        text
    };
    if text.is_empty() {
        anyhow::bail!("LLM returned empty distillation for {}", rel_path);
    }
    Ok(text)
}

// ── Main entry point ─────────────────────────────────────────────────────────

/// Public report type — returned even on dry-run so callers can log/display it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HygieneReport {
    pub total_bytes_before: usize,
    pub total_bytes_after: usize,
    pub files_scanned: usize,
    pub files_distilled: usize,
    pub bytes_saved: i64,
    pub dry_run: bool,
    pub skipped_reason: Option<String>,
}

/// Run hygiene analysis if enabled and interval has elapsed.
///
/// Returns `None` if the feature is disabled or the interval has not elapsed.
pub async fn maybe_run_hygiene(
    home: &Path,
    last_local_tick: &mut Instant,
    llm: &crate::ollama_client::OllamaClient,
) -> Option<HygieneReport> {
    if !enabled() {
        return None;
    }
    let interval = interval_secs();
    if last_local_tick.elapsed().as_secs() < interval {
        return None;
    }

    let now_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let mut state = load_state(home);
    if now_unix.saturating_sub(state.last_run_unix) < interval {
        *last_local_tick = Instant::now();
        return None;
    }

    info!(target: "hsm_md_hygiene", "starting instruction-file hygiene scan");

    match run_hygiene_inner(home, now_unix, llm).await {
        Ok(report) => {
            state.last_run_unix = now_unix;
            state.last_total_bytes = report.total_bytes_after;
            *last_local_tick = Instant::now();
            if let Err(e) = save_state(home, &state) {
                warn!(target: "hsm_md_hygiene", "failed to save state: {e}");
            }
            info!(
                target: "hsm_md_hygiene",
                total_before = report.total_bytes_before,
                total_after = report.total_bytes_after,
                files_distilled = report.files_distilled,
                bytes_saved = report.bytes_saved,
                "hygiene scan complete"
            );
            Some(report)
        }
        Err(e) => {
            warn!(target: "hsm_md_hygiene", "hygiene scan failed: {e:#}");
            *last_local_tick = Instant::now();
            None
        }
    }
}

async fn run_hygiene_inner(
    home: &Path,
    now_unix: u64,
    llm: &crate::ollama_client::OllamaClient,
) -> Result<HygieneReport> {
    let total_threshold = total_threshold();
    let file_threshold = file_threshold();
    let is_dry_run = dry_run();

    let files = instruction_files(home);
    let files_scanned = files.len();

    // --- Phase 1: measure ---
    let mut file_contents: Vec<(String, PathBuf, String)> = Vec::new();
    let mut total_bytes_before: usize = 0;

    for (rel, abs) in &files {
        match std::fs::read_to_string(abs) {
            Ok(content) => {
                total_bytes_before += content.len();
                file_contents.push((rel.clone(), abs.clone(), content));
            }
            Err(e) => {
                warn!(target: "hsm_md_hygiene", rel = %rel, "could not read file: {e}");
            }
        }
    }

    // --- Phase 2: decide whether to act ---
    if total_bytes_before < total_threshold {
        let report = HygieneReport {
            total_bytes_before,
            total_bytes_after: total_bytes_before,
            files_scanned,
            files_distilled: 0,
            bytes_saved: 0,
            dry_run: is_dry_run,
            skipped_reason: Some(format!(
                "total {total_bytes_before}B < threshold {total_threshold}B"
            )),
        };
        return Ok(report);
    }

    // --- Phase 3: rank and distil ---
    // Sort by noise score descending — worst offenders first
    let mut ranked: Vec<_> = file_contents
        .iter()
        .map(|(rel, abs, content)| {
            let score = noise_score(content);
            (score, rel.clone(), abs.clone(), content.clone())
        })
        .collect();
    ranked.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    let mut file_results: Vec<FileHygieneResult> = Vec::new();
    let mut total_bytes_after = total_bytes_before;

    for (_score, rel, abs, content) in &ranked {
        if content.len() < file_threshold {
            file_results.push(FileHygieneResult {
                rel_path: rel.clone(),
                bytes_before: content.len(),
                bytes_after: content.len(),
                bytes_saved: 0,
                action: "skipped".to_string(),
            });
            continue;
        }

        info!(
            target: "hsm_md_hygiene",
            file = %rel,
            bytes = content.len(),
            "distilling"
        );

        match distil_file(content, rel, llm).await {
            Ok(distilled) => {
                let bytes_after = distilled.len();
                let bytes_saved = content.len() as i64 - bytes_after as i64;

                // Sanity check: reject if distillation made it larger or removed too much
                let ratio = bytes_after as f64 / content.len() as f64;
                if ratio > 1.1 {
                    warn!(
                        target: "hsm_md_hygiene",
                        file = %rel,
                        "distilled version is larger than original — skipping"
                    );
                    file_results.push(FileHygieneResult {
                        rel_path: rel.clone(),
                        bytes_before: content.len(),
                        bytes_after: content.len(),
                        bytes_saved: 0,
                        action: "skipped_expanded".to_string(),
                    });
                    continue;
                }
                if ratio < 0.1 {
                    warn!(
                        target: "hsm_md_hygiene",
                        file = %rel,
                        "distilled version is suspiciously tiny — skipping"
                    );
                    file_results.push(FileHygieneResult {
                        rel_path: rel.clone(),
                        bytes_before: content.len(),
                        bytes_after: content.len(),
                        bytes_saved: 0,
                        action: "skipped_too_small".to_string(),
                    });
                    continue;
                }

                if !is_dry_run {
                    // Backup original
                    let bak = abs.with_extension("hygiene.bak");
                    if let Err(e) = std::fs::copy(abs, &bak) {
                        warn!(target: "hsm_md_hygiene", "backup failed for {rel}: {e}");
                    }
                    // Write distilled
                    if let Err(e) = std::fs::write(abs, distilled.as_bytes()) {
                        warn!(target: "hsm_md_hygiene", "write failed for {rel}: {e}");
                        file_results.push(FileHygieneResult {
                            rel_path: rel.clone(),
                            bytes_before: content.len(),
                            bytes_after: content.len(),
                            bytes_saved: 0,
                            action: "write_failed".to_string(),
                        });
                        continue;
                    }
                }

                total_bytes_after = (total_bytes_after as i64 - bytes_saved) as usize;
                file_results.push(FileHygieneResult {
                    rel_path: rel.clone(),
                    bytes_before: content.len(),
                    bytes_after: bytes_after,
                    bytes_saved,
                    action: if is_dry_run {
                        "dry_run".to_string()
                    } else {
                        "distilled".to_string()
                    },
                });
            }
            Err(e) => {
                warn!(target: "hsm_md_hygiene", file = %rel, "distillation error: {e:#}");
                file_results.push(FileHygieneResult {
                    rel_path: rel.clone(),
                    bytes_before: content.len(),
                    bytes_after: content.len(),
                    bytes_saved: 0,
                    action: "error".to_string(),
                });
            }
        }
    }

    let files_distilled = file_results.iter().filter(|r| r.action == "distilled").count();
    let bytes_saved: i64 = file_results.iter().map(|r| r.bytes_saved).sum();

    // --- Phase 4: log ---
    let entry = HygieneLogEntry {
        ts_unix: now_unix,
        total_bytes_before,
        total_bytes_after,
        dry_run: is_dry_run,
        files: file_results,
    };
    if let Err(e) = append_log(home, &entry) {
        warn!(target: "hsm_md_hygiene", "log append failed: {e}");
    }

    Ok(HygieneReport {
        total_bytes_before,
        total_bytes_after,
        files_scanned,
        files_distilled,
        bytes_saved,
        dry_run: is_dry_run,
        skipped_reason: None,
    })
}

// ── Standalone analysis (no write) ───────────────────────────────────────────

/// Synchronously scan instruction files and return a size report.
/// Does not require LLM access. Useful for logging context pressure.
pub fn snapshot(home: &Path) -> Vec<(String, usize, f32)> {
    instruction_files(home)
        .into_iter()
        .filter_map(|(rel, abs)| {
            let content = std::fs::read_to_string(&abs).ok()?;
            let bytes = content.len();
            let score = noise_score(&content);
            Some((rel, bytes, score))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_score_increases_with_size() {
        let small = "# Rules\n\nIF x > 0: do y.\n";
        let large = small.repeat(50);
        assert!(noise_score(&large) > noise_score(small));
    }

    #[test]
    fn noise_score_increases_with_repetition() {
        let unique = (0..20)
            .map(|i| format!("Rule {i}: unique content item {i} different text.\n"))
            .collect::<String>();
        let repeated = "IF funds < payroll * 3: ENTER SURVIVAL MODE.\n".repeat(20);
        // Both are similar size but repeated has duplicate n-grams
        assert!(noise_score(&repeated) >= noise_score(&unique));
    }
}
