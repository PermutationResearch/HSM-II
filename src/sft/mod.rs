//! SFT (Supervised Fine-Tuning) trace capture.
//!
//! Captures the full Hermes execution loop — system prompt, every
//! (assistant → tool call → tool result) turn with unredacted content —
//! and pairs it with Company OS task metadata.
//!
//! Output: append-only JSONL at `{home}/memory/sft_traces.jsonl`.
//! Enable with `HSM_SFT_CAPTURE=1`.
//!
//! Each record is a self-contained training example: the messages array is
//! the complete conversation a fine-tuned model should learn to reproduce.
//! Pair with the doc-workflow files (spec/analysis/plan) from the worktree
//! to get the full spec→execution dataset.

use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;
use uuid::Uuid;

pub const SCHEMA_V: u32 = 1;

// ── Message types ────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    System,
    User,
    Assistant,
    /// Tool result returned to the model after a tool call.
    Tool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SftMessage {
    pub role: Role,
    /// Full content — never redacted or summarised (unlike TrajectoryRecord).
    pub content: String,
    /// Set on Role::Tool — the name of the tool that produced this result.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    /// Set on Role::Tool — whether the tool call succeeded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_success: Option<bool>,
}

// ── Trace ────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SftTrace {
    /// Schema version — increment when the format changes incompatibly.
    pub schema_v: u32,
    /// Unique id for deduplication.
    pub id: String,
    pub ts: String,

    // ── Company OS context ────────────────────────────────────────────────
    /// `tasks.id` this run belongs to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    /// `agent_runs.id` for this execution.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    /// The DRI agent name (e.g. "ceo-agent").
    pub actor: String,
    /// `companies.id`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub company_id: Option<String>,

    // ── The training example ──────────────────────────────────────────────
    /// Full conversation in order: system → user → (assistant → tool)* → assistant.
    /// This is the primary training signal.
    pub messages: Vec<SftMessage>,

    // ── Outcome metadata ──────────────────────────────────────────────────
    /// "success" | "error" — from the agent_runs status.
    pub outcome: String,
    /// Number of tool calls made during the run.
    pub tool_calls_count: usize,
    /// True when `cargo test` passed on the resulting diff.
    /// Set externally by the verification pipeline; starts false.
    pub verified: bool,
    /// 1–5 quality score set by a post-run LLM eval (Aeon-style).
    /// None until scored.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality_score: Option<f32>,
}

impl SftTrace {
    pub fn new(actor: impl Into<String>) -> Self {
        Self {
            schema_v: SCHEMA_V,
            id: Uuid::new_v4().to_string(),
            ts: Utc::now().to_rfc3339(),
            task_id: None,
            run_id: None,
            actor: actor.into(),
            company_id: None,
            messages: Vec::new(),
            outcome: "pending".to_string(),
            tool_calls_count: 0,
            verified: false,
            quality_score: None,
        }
    }

    pub fn with_task(mut self, task_id: impl Into<String>) -> Self {
        self.task_id = Some(task_id.into());
        self
    }

    pub fn with_run(mut self, run_id: impl Into<String>) -> Self {
        self.run_id = Some(run_id.into());
        self
    }

    pub fn with_company(mut self, company_id: impl Into<String>) -> Self {
        self.company_id = Some(company_id.into());
        self
    }

    pub fn push_system(&mut self, content: impl Into<String>) {
        self.messages.push(SftMessage {
            role: Role::System,
            content: content.into(),
            tool_name: None,
            tool_success: None,
        });
    }

    pub fn push_user(&mut self, content: impl Into<String>) {
        self.messages.push(SftMessage {
            role: Role::User,
            content: content.into(),
            tool_name: None,
            tool_success: None,
        });
    }

    pub fn push_assistant(&mut self, content: impl Into<String>) {
        self.messages.push(SftMessage {
            role: Role::Assistant,
            content: content.into(),
            tool_name: None,
            tool_success: None,
        });
    }

    pub fn push_tool_result(
        &mut self,
        tool_name: impl Into<String>,
        content: impl Into<String>,
        success: bool,
    ) {
        self.tool_calls_count += 1;
        self.messages.push(SftMessage {
            role: Role::Tool,
            content: content.into(),
            tool_name: Some(tool_name.into()),
            tool_success: Some(success),
        });
    }
}

// ── Shared capture handle ────────────────────────────────────────────────────

/// Arc-wrapped trace shared between the dispatch context and the agent's
/// execution loop. Set on the agent before calling `handle_message`, read
/// back after it returns.
#[derive(Clone, Debug)]
pub struct SftCapture(pub Arc<Mutex<SftTrace>>);

impl SftCapture {
    pub fn new(actor: impl Into<String>) -> Self {
        Self(Arc::new(Mutex::new(SftTrace::new(actor))))
    }

    pub async fn push_system(&self, content: impl Into<String>) {
        self.0.lock().await.push_system(content);
    }

    pub async fn push_user(&self, content: impl Into<String>) {
        self.0.lock().await.push_user(content);
    }

    pub async fn push_assistant(&self, content: impl Into<String>) {
        self.0.lock().await.push_assistant(content);
    }

    pub async fn push_tool_result(
        &self,
        tool_name: impl Into<String>,
        content: impl Into<String>,
        success: bool,
    ) {
        self.0.lock().await.push_tool_result(tool_name, content, success);
    }

    /// Finalise and return a clone of the completed trace.
    pub async fn finish(&self, outcome: &str) -> SftTrace {
        let mut t = self.0.lock().await;
        t.outcome = outcome.to_string();
        t.clone()
    }
}

// ── JSONL writer ─────────────────────────────────────────────────────────────

/// Append one trace to the JSONL file at `path`.
/// Creates the file and parent directories if they don't exist.
pub async fn write_trace(path: &Path, trace: &SftTrace) -> Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let mut line = serde_json::to_string(trace)?;
    line.push('\n');
    let mut f = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await?;
    f.write_all(line.as_bytes()).await?;
    Ok(())
}

/// Returns true when `HSM_SFT_CAPTURE` is set to a truthy value.
pub fn capture_enabled() -> bool {
    std::env::var("HSM_SFT_CAPTURE")
        .map(|v| {
            let s = v.trim();
            s == "1" || s.eq_ignore_ascii_case("true") || s.eq_ignore_ascii_case("yes")
        })
        .unwrap_or(false)
}
