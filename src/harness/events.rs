//! Append-only harness events (trace envelope).

use serde::{Deserialize, Serialize};

use super::types::{HarnessState, TaskOutcome};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HarnessEvent {
    pub schema: String,
    pub trace_id: String,
    pub agent_id: String,
    pub runner_name: String,
    pub task_id: String,
    pub turn_index: usize,
    pub from_state: HarnessState,
    pub to_state: HarnessState,
    pub unix_ms: u128,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome: Option<TaskOutcome>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl HarnessEvent {
    pub const SCHEMA_V1: &'static str = "hsm.harness.event.v1";

    pub fn transition(
        trace_id: impl Into<String>,
        agent_id: impl Into<String>,
        runner_name: impl Into<String>,
        task_id: impl Into<String>,
        turn_index: usize,
        from_state: HarnessState,
        to_state: HarnessState,
        duration_ms: Option<u64>,
        outcome: Option<TaskOutcome>,
        detail: Option<String>,
    ) -> Self {
        let unix_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        Self {
            schema: Self::SCHEMA_V1.to_string(),
            trace_id: trace_id.into(),
            agent_id: agent_id.into(),
            runner_name: runner_name.into(),
            task_id: task_id.into(),
            turn_index,
            from_state,
            to_state,
            unix_ms,
            duration_ms,
            outcome,
            detail,
        }
    }
}
