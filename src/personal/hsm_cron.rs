//! Optional file-driven cron jobs (`HSM_CRON_CONFIG` or `config/hsm_cron.json`).

use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::{LazyLock, Mutex};

use chrono::Utc;
use cron::Schedule;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct CronConfigFile {
    #[serde(default)]
    pub jobs: Vec<CronJobSpec>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct CronJobSpec {
    pub name: String,
    pub schedule: String,
    #[serde(default = "default_action")]
    pub action: String,
    #[serde(default)]
    pub message: String,
}

fn default_action() -> String {
    "log".into()
}

fn cron_script_actions_enabled() -> bool {
    std::env::var("HSM_CRON_ALLOW_SCRIPT_ACTIONS")
        .ok()
        .as_deref()
        == Some("1")
}

fn cron_script_dir() -> PathBuf {
    if let Ok(p) = std::env::var("HSM_CRON_SCRIPT_DIR") {
        let t = p.trim();
        if !t.is_empty() {
            return PathBuf::from(t);
        }
    }
    PathBuf::from("config/cron-scripts")
}

fn cron_allowed_scripts() -> Vec<String> {
    std::env::var("HSM_CRON_ALLOWED_SCRIPTS")
        .ok()
        .map(|s| {
            s.split(',')
                .map(|x| x.trim().to_string())
                .filter(|x| !x.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn parse_script_message(raw: &str) -> Option<String> {
    let script = raw.trim();
    if script.is_empty() {
        return None;
    }
    // Harden against command injection: no spaces/args, no path traversal, conservative charset.
    if script.contains(' ') || script.contains('\t') || script.contains(';') || script.contains('|') {
        return None;
    }
    if script.contains("..") || script.starts_with('/') {
        return None;
    }
    if script
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
    {
        Some(script.to_string())
    } else {
        None
    }
}

fn config_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("HSM_CRON_CONFIG") {
        let t = p.trim();
        if !t.is_empty() {
            return Some(PathBuf::from(t));
        }
    }
    let fallback = PathBuf::from("config/hsm_cron.json");
    if fallback.exists() {
        Some(fallback)
    } else {
        None
    }
}

/// True when a cron config file exists or `HSM_CRON_CONFIG` points to a path.
pub fn cron_file_configured() -> bool {
    config_path().is_some()
}

static LAST_FIRE: LazyLock<Mutex<HashMap<String, chrono::DateTime<Utc>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Run one tick: for each job, fire if the next schedule boundary passed since last fire.
pub async fn tick_daemon_jobs() {
    let Some(path) = config_path() else {
        return;
    };
    let raw = match tokio::fs::read_to_string(&path).await {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(target: "hsm.cron", path = %path.display(), "read failed: {}", e);
            return;
        }
    };
    let cfg: CronConfigFile = match serde_json::from_str(&raw) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(target: "hsm.cron", "parse {}: {}", path.display(), e);
            return;
        }
    };

    let now = Utc::now();
    let mut due_jobs: Vec<CronJobSpec> = Vec::new();
    {
        let Ok(mut map) = LAST_FIRE.lock() else {
            return;
        };
        for job in &cfg.jobs {
            let Ok(schedule) = Schedule::from_str(job.schedule.trim()) else {
                tracing::warn!(target: "hsm.cron", name = %job.name, "invalid schedule {:?}", job.schedule);
                continue;
            };
            let last = map
                .get(&job.name)
                .cloned()
                .unwrap_or_else(|| now - chrono::Duration::days(3650));
            let Some(next) = schedule.after(&last).next() else {
                continue;
            };
            if next > now {
                continue;
            }
            due_jobs.push(job.clone());
            map.insert(job.name.clone(), now);
        }
    }

    for job in due_jobs {
        match job.action.as_str() {
            "log" | "" => {
                tracing::info!(
                    target: "hsm.cron",
                    name = %job.name,
                    msg = %job.message,
                    "cron job fired"
                );
            }
            "self_improvement_weekly_nudge" => {
                let base = std::env::var("HSM_CONSOLE_URL")
                    .ok()
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or_else(|| "http://127.0.0.1:3847".to_string());
                let url = format!(
                    "{}/api/company/self-improvement/weekly-nudge",
                    base.trim_end_matches('/')
                );
                match reqwest::Client::new().post(url.clone()).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        tracing::info!(
                            target: "hsm.cron",
                            name = %job.name,
                            endpoint = %url,
                            "cron job fired (weekly nudge triggered)"
                        );
                    }
                    Ok(resp) => {
                        tracing::warn!(
                            target: "hsm.cron",
                            name = %job.name,
                            endpoint = %url,
                            status = %resp.status(),
                            "weekly nudge trigger returned non-success"
                        );
                    }
                    Err(e) => {
                        tracing::warn!(
                            target: "hsm.cron",
                            name = %job.name,
                            endpoint = %url,
                            error = %e,
                            "weekly nudge trigger failed"
                        );
                    }
                }
            }
            "script" => {
                if !cron_script_actions_enabled() {
                    tracing::warn!(
                        target: "hsm.cron",
                        name = %job.name,
                        "script action blocked; set HSM_CRON_ALLOW_SCRIPT_ACTIONS=1 to enable"
                    );
                    continue;
                }
                let Some(script_name) = parse_script_message(&job.message) else {
                    tracing::warn!(
                        target: "hsm.cron",
                        name = %job.name,
                        message = %job.message,
                        "invalid script name in cron message (blocked)"
                    );
                    continue;
                };
                let allowed = cron_allowed_scripts();
                if !allowed.is_empty() && !allowed.iter().any(|s| s == &script_name) {
                    tracing::warn!(
                        target: "hsm.cron",
                        name = %job.name,
                        script = %script_name,
                        "script not in HSM_CRON_ALLOWED_SCRIPTS allowlist"
                    );
                    continue;
                }
                let dir = cron_script_dir();
                let script_path = dir.join(&script_name);
                let Some(path_str) = script_path.to_str() else {
                    tracing::warn!(target: "hsm.cron", name = %job.name, "invalid script path");
                    continue;
                };
                match tokio::process::Command::new(path_str).output().await {
                    Ok(out) => {
                        if out.status.success() {
                            tracing::info!(
                                target: "hsm.cron",
                                name = %job.name,
                                script = %script_name,
                                "cron script executed"
                            );
                        } else {
                            tracing::warn!(
                                target: "hsm.cron",
                                name = %job.name,
                                script = %script_name,
                                status = %out.status,
                                "cron script failed"
                            );
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            target: "hsm.cron",
                            name = %job.name,
                            script = %script_name,
                            error = %e,
                            "cron script execution failed"
                        );
                    }
                }
            }
            other => {
                tracing::info!(target: "hsm.cron", name = %job.name, action = %other, "cron job fired (no handler)");
            }
        }
    }
}
