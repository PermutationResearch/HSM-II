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

        match job.action.as_str() {
            "log" | "" => {
                tracing::info!(
                    target: "hsm.cron",
                    name = %job.name,
                    msg = %job.message,
                    "cron job fired"
                );
            }
            other => {
                tracing::info!(target: "hsm.cron", name = %job.name, action = %other, "cron job fired (no handler)");
            }
        }
        map.insert(job.name.clone(), now);
    }
}
