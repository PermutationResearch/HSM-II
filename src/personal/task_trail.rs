//! Append-only JSONL trail: turns, tool denials, and hyperedges (stigmergy / graph-lite audit).

use anyhow::Result;
use serde::Serialize;
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;

const SCHEMA_V: u32 = 1;

#[derive(Clone, Debug)]
pub struct TaskTrail {
    path: PathBuf,
    enabled: bool,
}

#[derive(Serialize)]
struct TurnEvent {
    v: u32,
    kind: &'static str,
    ts: String,
    message_id: String,
    user_preview: String,
    assistant_preview: String,
    skills_used: Vec<String>,
    council_used: bool,
    tool_step_count: usize,
    world_edges: usize,
    world_beliefs: usize,
}

#[derive(Serialize)]
struct HyperedgeEvent {
    v: u32,
    kind: &'static str,
    ts: String,
    rel: String,
    participants: Vec<String>,
    payload: serde_json::Value,
}

#[derive(Serialize)]
struct ToolDeniedEvent {
    v: u32,
    kind: &'static str,
    ts: String,
    tool: String,
    reason: String,
}

impl TaskTrail {
    /// `HSM_TASK_TRAIL=0` disables; otherwise on (default: on).
    pub fn from_home(home: &Path) -> Self {
        let enabled = std::env::var("HSM_TASK_TRAIL")
            .map(|v| {
                let s = v.trim();
                !(s == "0" || s.eq_ignore_ascii_case("false") || s.eq_ignore_ascii_case("no"))
            })
            .unwrap_or(true);
        Self {
            path: home.join("memory/task_trail.jsonl"),
            enabled,
        }
    }

    async fn append_row<T: Serialize>(&self, row: &T) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }
        if let Some(parent) = self.path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let mut line = serde_json::to_string(row)?;
        line.push('\n');
        let mut f = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .await?;
        f.write_all(line.as_bytes()).await?;
        Ok(())
    }

    pub async fn append_turn(
        &self,
        message_id: &str,
        user_preview: &str,
        assistant_preview: &str,
        skills_used: &[String],
        council_used: bool,
        tool_step_count: usize,
        world_edges: usize,
        world_beliefs: usize,
    ) -> Result<()> {
        let ts = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        self.append_row(&TurnEvent {
            v: SCHEMA_V,
            kind: "turn",
            ts,
            message_id: message_id.to_string(),
            user_preview: clamp_preview(user_preview, 500),
            assistant_preview: clamp_preview(assistant_preview, 800),
            skills_used: skills_used.to_vec(),
            council_used,
            tool_step_count,
            world_edges,
            world_beliefs,
        })
        .await
    }

    pub async fn append_hyperedge(
        &self,
        rel: impl Into<String>,
        participants: Vec<String>,
        payload: serde_json::Value,
    ) -> Result<()> {
        let ts = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        self.append_row(&HyperedgeEvent {
            v: SCHEMA_V,
            kind: "hyperedge",
            ts,
            rel: rel.into(),
            participants,
            payload,
        })
        .await
    }

    pub async fn append_tool_denied(&self, tool: &str, reason: &str) -> Result<()> {
        let ts = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        self.append_row(&ToolDeniedEvent {
            v: SCHEMA_V,
            kind: "tool_denied",
            ts,
            tool: tool.to_string(),
            reason: clamp_preview(reason, 500),
        })
        .await
    }
}

fn clamp_preview(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    s.chars().take(max_chars).collect::<String>() + "…"
}
