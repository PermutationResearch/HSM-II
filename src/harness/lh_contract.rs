//! Phase 0 — long-horizon gateway contract types (JSON-serde friendly).

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Identifies a conversation / run thread for the long-horizon harness.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RunIdentity {
    pub thread_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_run_id: Option<String>,
    /// One id for billing / tracing across coordinator steps and tool checkpoints (layer 12).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
}

/// Who is acting in this step (lead orchestrator vs delegated sub-agent).
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum LhActorRole {
    #[default]
    Lead,
    Subagent,
}

/// How this actor may see or mutate shared harness state.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum LhContextScope {
    /// Sub-agent sees only its own context blob; stricter tool policy applies.
    #[default]
    Isolated,
    SharedRead,
    SharedWrite,
}

/// Reference to an artifact produced or consumed by the harness (path, URL, or opaque id).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct LhArtifactRef {
    pub uri: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

/// Uploaded or staged input (companion to artifacts).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct LhUploadRef {
    pub uri: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime: Option<String>,
}

/// One tool invocation with optional provenance for auditing / tracing.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct LhToolInvocationEnvelope {
    pub tool_name: String,
    pub parameters: Value,
    pub call_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<String>,
}

/// Per-turn harness envelope: attach to HTTP/gateway requests or set on [`crate::tools::ToolRegistry`].
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct HarnessRunEnvelope {
    pub run: RunIdentity,
    #[serde(default)]
    pub actor: LhActorRole,
    #[serde(default)]
    pub context_scope: LhContextScope,
    #[serde(default)]
    pub artifacts: Vec<LhArtifactRef>,
    #[serde(default)]
    pub uploads: Vec<LhUploadRef>,
}

impl HarnessRunEnvelope {
    pub fn lead_thread(thread_id: impl Into<String>) -> Self {
        Self {
            run: RunIdentity {
                thread_id: thread_id.into(),
                turn_id: None,
                parent_run_id: None,
                correlation_id: None,
            },
            actor: LhActorRole::Lead,
            context_scope: LhContextScope::SharedWrite,
            artifacts: Vec::new(),
            uploads: Vec::new(),
        }
    }

    /// Isolated subagent run linked to `parent.run.thread_id` (coordinator / layer-12 handoff).
    pub fn subagent_delegate(parent: &HarnessRunEnvelope, sub_slug: impl Into<String>) -> Self {
        let mut slug: String = sub_slug
            .into()
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        slug = slug.trim_matches('_').to_string();
        if slug.is_empty() {
            slug = "task".into();
        }
        let sub_thread = format!("{}:sub:{}", parent.run.thread_id, slug);
        Self {
            run: RunIdentity {
                thread_id: sub_thread,
                turn_id: None,
                parent_run_id: Some(parent.run.thread_id.clone()),
                correlation_id: parent.run.correlation_id.clone(),
            },
            actor: LhActorRole::Subagent,
            context_scope: LhContextScope::Isolated,
            artifacts: Vec::new(),
            uploads: Vec::new(),
        }
    }
}
