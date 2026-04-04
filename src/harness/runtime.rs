//! Minimal harness runtime: turn-scoped transitions + optional JSONL sink.

use std::io;
use std::path::PathBuf;
use std::time::Instant;

use super::events::HarnessEvent;
use super::store::HarnessStore;
use super::types::{HarnessState, TaskOutcome};

pub struct HarnessRuntime {
    pub trace_id: String,
    pub agent_id: String,
    pub runner_name: String,
    store: Option<HarnessStore>,
}

impl HarnessRuntime {
    /// No-op runtime (no file I/O).
    pub fn noop() -> Self {
        Self {
            trace_id: String::new(),
            agent_id: "eval".to_string(),
            runner_name: "noop".to_string(),
            store: None,
        }
    }

    /// Build from env:
    /// - `HSM_HARNESS_LOG` — JSONL log path (if unset, runtime is noop).
    /// - `HSM_HARNESS_TRACE_ID` — optional trace id (default: random uuid).
    /// - `HSM_HARNESS_AGENT_ID` — optional agent id (default: `eval`).
    /// - `HSM_HARNESS_CHECKPOINT_DIR` — optional checkpoint directory.
    pub fn from_env(runner_name: impl Into<String>) -> io::Result<Self> {
        let runner_name = runner_name.into();
        match std::env::var("HSM_HARNESS_LOG") {
            Ok(log_path) => {
                let trace_id = std::env::var("HSM_HARNESS_TRACE_ID")
                    .unwrap_or_else(|_| uuid::Uuid::new_v4().to_string());
                let agent_id =
                    std::env::var("HSM_HARNESS_AGENT_ID").unwrap_or_else(|_| "eval".to_string());
                let checkpoint_dir = std::env::var("HSM_HARNESS_CHECKPOINT_DIR")
                    .ok()
                    .map(PathBuf::from);
                let store = HarnessStore::new(PathBuf::from(log_path), checkpoint_dir)?;
                Ok(Self {
                    trace_id,
                    agent_id,
                    runner_name,
                    store: Some(store),
                })
            }
            Err(_) => Ok(Self {
                trace_id: String::new(),
                agent_id: "eval".to_string(),
                runner_name,
                store: None,
            }),
        }
    }

    fn emit(
        &mut self,
        task_id: &str,
        turn_index: usize,
        from_state: HarnessState,
        to_state: HarnessState,
        duration_ms: Option<u64>,
        outcome: Option<TaskOutcome>,
        detail: Option<String>,
    ) {
        let Some(ref store) = self.store else {
            return;
        };
        let ev = HarnessEvent::transition(
            self.trace_id.clone(),
            self.agent_id.clone(),
            self.runner_name.clone(),
            task_id,
            turn_index,
            from_state,
            to_state.clone(),
            duration_ms,
            outcome,
            detail,
        );
        if let Err(e) = store.append_event(&ev) {
            tracing::warn!(target: "harness", "harness append_event failed: {}", e);
        }
    }

    /// Mark start of an eval turn: `Queued -> Running`.
    pub fn turn_begin(&mut self, task_id: &str, turn_index: usize) {
        self.emit(
            task_id,
            turn_index,
            HarnessState::Queued,
            HarnessState::Running,
            None,
            None,
            None,
        );
    }

    /// Mark end of turn after work finished.
    pub fn turn_end(
        &mut self,
        task_id: &str,
        turn_index: usize,
        step_start: Instant,
        error: Option<&str>,
    ) {
        let duration_ms = step_start.elapsed().as_millis() as u64;
        let (to, outcome, detail) = if error.is_none() {
            (HarnessState::Completed, Some(TaskOutcome::Success), None)
        } else {
            (
                HarnessState::Failed,
                Some(TaskOutcome::Error),
                error.map(|s| s.lines().next().unwrap_or(s).to_string()),
            )
        };
        self.emit(
            task_id,
            turn_index,
            HarnessState::Running,
            to,
            Some(duration_ms),
            outcome,
            detail,
        );
    }
}
