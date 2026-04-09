//! Integrated Tool Executor
//!
//! Connects the tool system to all HSM-II subsystems:
//! - Social memory (records deliveries, updates reputation)
//! - Stigmergic traces (records tool executions in the field)

use std::sync::Arc;
use tracing::debug;

use crate::agent::AgentId;
use crate::social_memory::DataSensitivity;
use crate::TraceKind;

use super::{ToolCall, ToolCallResult, ToolRegistry};

/// Tool execution integrated with HSM-II systems
pub struct IntegratedToolExecutor {
    registry: ToolRegistry,
    agent_id: AgentId,
    /// Link to world for social memory, traces
    world: Option<Arc<tokio::sync::Mutex<crate::HyperStigmergicMorphogenesis>>>,
}

impl IntegratedToolExecutor {
    pub fn new(agent_id: AgentId) -> Self {
        let mut registry = ToolRegistry::new();
        crate::tools::register_all_tools(&mut registry);
        Self {
            registry,
            agent_id,
            world: None,
        }
    }

    pub fn with_world(
        mut self,
        world: Arc<tokio::sync::Mutex<crate::HyperStigmergicMorphogenesis>>,
    ) -> Self {
        self.world = Some(world);
        self
    }

    /// Execute a tool with full HSM-II integration
    pub async fn execute(
        &mut self,
        call: ToolCall,
        task_key: &str,
        sensitivity: DataSensitivity,
    ) -> ToolCallResult {
        let started_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // 1. Record promise in social memory (if world connected)
        let promise_id = if let Some(world_arc) = &self.world {
            let mut world = world_arc.lock().await;
            let pid = world.record_agent_promise(
                self.agent_id,
                None, // beneficiary is the user/system
                task_key,
                &format!("Execute tool {} for task {}", call.name, task_key),
                sensitivity.clone(),
                Some(started_at + 300), // 5 min deadline
            );
            Some(pid)
        } else {
            None
        };

        // 2. Record stigmergic trace (tool execution starting)
        if let Some(world_arc) = &self.world {
            let mut world = world_arc.lock().await;
            world.record_agent_trace(
                self.agent_id,
                "local-agent",
                Some(task_key),
                TraceKind::QueryPlanned,
                &format!("Tool {} invoked", call.name),
                None,
                Some(0.5), // initial confidence
                sensitivity.clone(),
            );
        }

        // 3. Execute the tool
        debug!("Executing tool: {} for task {}", call.name, task_key);
        let result = self.registry.execute(call.clone()).await;

        let completed_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // 4. Record delivery in social memory
        if let Some(pid) = promise_id {
            if let Some(world_arc) = &self.world {
                let mut world = world_arc.lock().await;

                let success = result.output.success;
                let quality = if success { 0.8 } else { 0.2 };
                let on_time = completed_at < started_at + 300;
                let safe = !result.output.result.contains("password")
                    && !result.output.result.contains("secret");

                world.resolve_agent_promise(
                    &pid,
                    if success {
                        crate::social_memory::PromiseStatus::Kept
                    } else {
                        crate::social_memory::PromiseStatus::Broken
                    },
                    Some(self.agent_id),
                    Some(quality),
                    Some(on_time),
                    Some(safe),
                    &[], // no collaborators
                );
                world.record_agent_delivery(
                    self.agent_id,
                    &format!("tool:{}", call.name),
                    success,
                    quality,
                    on_time,
                    safe,
                    &[], // no collaborators
                );
            }
        }

        // 5. Record stigmergic trace (completion) + optional web → experience/belief ingest
        if let Some(world_arc) = &self.world {
            let mut world = world_arc.lock().await;
            let trace_kind = if result.output.success {
                TraceKind::DeliveryRecorded
            } else {
                TraceKind::Observation
            };

            world.record_agent_trace(
                self.agent_id,
                "local-agent",
                Some(task_key),
                trace_kind,
                &format!(
                    "Tool {} completed: success={}",
                    call.name, result.output.success
                ),
                Some(result.output.success),
                Some(if result.output.success { 0.9 } else { 0.3 }),
                sensitivity.clone(),
            );

            if result.output.success {
                super::web_ingest::ingest_web_tool_success(
                    &mut *world,
                    &call.name,
                    &call.parameters,
                    &result.output,
                );
            }
        }

        crate::runtime_control::publish_completion(
            crate::runtime_control::CompletionEvent::background_completion(
                task_key,
                result.output.success,
                if result.output.success {
                    format!("task {task_key} completed")
                } else {
                    format!(
                        "task {task_key} failed: {}",
                        result
                            .output
                            .error
                            .as_deref()
                            .unwrap_or("unknown error")
                    )
                },
            ),
        );

        result
    }

    /// Get tool registry for direct access
    pub fn registry(&self) -> &ToolRegistry {
        &self.registry
    }

    /// Get mutable registry
    pub fn registry_mut(&mut self) -> &mut ToolRegistry {
        &mut self.registry
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_basic_execution() {
        let agent_id: AgentId = 1;
        let mut executor = IntegratedToolExecutor::new(agent_id);

        let call = ToolCall {
            name: "bash".to_string(),
            parameters: serde_json::json!({"command": "echo hello"}),
            call_id: "test-1".to_string(),
            harness_run: None,
            idempotency_key: None,
        };

        let result = executor
            .execute(call, "test-task", DataSensitivity::Internal)
            .await;
        assert!(result.output.success);
    }
}
