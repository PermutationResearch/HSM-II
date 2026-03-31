//! Core harness state and outcome types (HarnessV1).

use serde::{Deserialize, Serialize};

/// High-level lifecycle state for one generator step / eval turn.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessState {
    Queued,
    Running,
    WaitingTool,
    Paused,
    Resumed,
    Completed,
    Failed,
}

/// Opaque resume handle for pause–resume semantics (checkpoint id, session id, etc.).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResumeToken(pub String);

impl ResumeToken {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskOutcome {
    Success,
    Error,
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorClass {
    Transient,
    Tool,
    Policy,
    Model,
    Fatal,
    Unknown,
}

/// Identifies one eval / harness step.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HarnessStepKey {
    pub task_id: String,
    pub turn_index: usize,
}

impl HarnessStepKey {
    pub fn new(task_id: impl Into<String>, turn_index: usize) -> Self {
        Self {
            task_id: task_id.into(),
            turn_index,
        }
    }
}
