//! Playbook execution harness — runs Steps via ToolRegistry.
//!
//! Executes playbook steps sequentially with retry logic,
//! artifact collection, and success criteria validation.

use std::collections::HashMap;
use tokio::time::Instant;
use tracing::debug;

use crate::ollama_client::OllamaClient;
use crate::tools::{ToolCall, ToolRegistry};

use super::{ArtifactType, Playbook, RunArtifact, Step};

/// Executes playbooks using the ToolRegistry.
pub struct PlaybookHarness {
    /// Max retries per step (overridden by step.max_retries)
    pub default_max_retries: u32,
    /// Timeout per step in seconds
    pub step_timeout_secs: u64,
}

/// Result of executing a playbook.
#[derive(Clone, Debug)]
pub struct HarnessResult {
    pub success: bool,
    pub steps_completed: usize,
    pub steps_total: usize,
    pub artifacts: Vec<RunArtifact>,
    pub total_duration_ms: u64,
    pub error: Option<String>,
}

impl Default for PlaybookHarness {
    fn default() -> Self {
        Self {
            default_max_retries: 2,
            step_timeout_secs: 60,
        }
    }
}

impl PlaybookHarness {
    pub fn new() -> Self {
        Self::default()
    }

    /// Execute all steps in a playbook via the tool registry.
    pub async fn execute(
        &self,
        playbook: &Playbook,
        tool_registry: &mut ToolRegistry,
        llm: &OllamaClient,
        context: &HashMap<String, String>,
    ) -> HarnessResult {
        let start = Instant::now();
        let mut artifacts = Vec::new();
        let mut steps_completed = 0;

        for step in &playbook.steps {
            let max_retries = if step.max_retries > 0 {
                step.max_retries
            } else {
                self.default_max_retries
            };

            let mut last_error = None;
            let mut succeeded = false;

            for attempt in 0..=max_retries {
                if attempt > 0 {
                    debug!(
                        "Retrying step {} (attempt {}/{})",
                        step.index,
                        attempt + 1,
                        max_retries + 1
                    );
                }

                match self.execute_step(step, tool_registry, llm, context).await {
                    Ok(artifact) => {
                        // Check success criteria
                        if self.check_success_criteria(step, &artifact, llm).await {
                            artifacts.push(artifact);
                            succeeded = true;
                            break;
                        } else {
                            last_error =
                                Some(format!("Step {} success criteria not met", step.index));
                            artifacts.push(artifact);
                        }
                    }
                    Err(e) => {
                        last_error = Some(format!("Step {} failed: {}", step.index, e));
                    }
                }
            }

            if succeeded {
                steps_completed += 1;
            } else {
                // Record error artifact
                artifacts.push(RunArtifact {
                    step_index: step.index,
                    artifact_type: ArtifactType::ErrorLog,
                    content: last_error.clone().unwrap_or_default(),
                    metadata: HashMap::new(),
                });
                return HarnessResult {
                    success: false,
                    steps_completed,
                    steps_total: playbook.steps.len(),
                    artifacts,
                    total_duration_ms: start.elapsed().as_millis() as u64,
                    error: last_error,
                };
            }
        }

        HarnessResult {
            success: true,
            steps_completed,
            steps_total: playbook.steps.len(),
            artifacts,
            total_duration_ms: start.elapsed().as_millis() as u64,
            error: None,
        }
    }

    /// Execute a single step.
    async fn execute_step(
        &self,
        step: &Step,
        tool_registry: &mut ToolRegistry,
        llm: &OllamaClient,
        context: &HashMap<String, String>,
    ) -> anyhow::Result<RunArtifact> {
        if let Some(ref tool_name) = step.tool_name {
            // Tool step: execute via ToolRegistry
            let params = self.resolve_params(step.tool_params.as_ref(), context);
            let call = ToolCall {
                name: tool_name.clone(),
                parameters: params,
                call_id: format!("harness_step_{}", step.index),
                harness_run: None,
                idempotency_key: None,
            };

            let result = tool_registry.execute(call).await;
            let content = if result.output.success {
                result.output.result
            } else {
                return Err(anyhow::anyhow!(
                    "Tool {} failed: {}",
                    tool_name,
                    result.output.error.unwrap_or_default()
                ));
            };

            Ok(RunArtifact {
                step_index: step.index,
                artifact_type: ArtifactType::ToolOutput,
                content,
                metadata: {
                    let mut m = HashMap::new();
                    m.insert("tool".to_string(), tool_name.clone());
                    m.insert("duration_ms".to_string(), result.duration_ms.to_string());
                    m
                },
            })
        } else if let Some(ref prompt_template) = step.prompt_template {
            // LLM step: generate via OllamaClient
            let resolved_prompt = self.resolve_template(prompt_template, context);
            let result = llm.generate(&resolved_prompt).await;

            if result.timed_out {
                return Err(anyhow::anyhow!("LLM step timed out"));
            }

            Ok(RunArtifact {
                step_index: step.index,
                artifact_type: ArtifactType::LlmResponse,
                content: result.text,
                metadata: {
                    let mut m = HashMap::new();
                    m.insert("latency_ms".to_string(), result.latency_ms.to_string());
                    m
                },
            })
        } else {
            Err(anyhow::anyhow!(
                "Step {} has neither tool_name nor prompt_template",
                step.index
            ))
        }
    }

    /// Check if a step's success criteria are met.
    async fn check_success_criteria(
        &self,
        step: &Step,
        artifact: &RunArtifact,
        llm: &OllamaClient,
    ) -> bool {
        if step.success_criteria.is_empty() || step.success_criteria == "any" {
            return true;
        }

        // Quick keyword check first
        let criteria_lower = step.success_criteria.to_lowercase();
        let _content_lower = artifact.content.to_lowercase();

        // Simple heuristic: if criteria mentions "non-empty" or "results found"
        if criteria_lower.contains("non-empty") || criteria_lower.contains("not empty") {
            return !artifact.content.trim().is_empty();
        }

        if criteria_lower.contains("results found") || criteria_lower.contains("output produced") {
            return artifact.content.len() > 10;
        }

        // For complex criteria, use LLM judge
        let prompt = format!(
            "Evaluate if this output meets the success criteria.\n\
             \nCriteria: {}\n\
             \nOutput (first 500 chars):\n{}\n\
             \nRespond with only YES or NO.",
            step.success_criteria,
            &artifact.content[..artifact.content.len().min(500)]
        );

        let result = llm.generate(&prompt).await;
        let answer = result.text.trim().to_uppercase();
        answer.contains("YES")
    }

    /// Resolve {{placeholders}} in a template string.
    fn resolve_template(&self, template: &str, context: &HashMap<String, String>) -> String {
        let mut result = template.to_string();
        for (key, value) in context {
            result = result.replace(&format!("{{{{{}}}}}", key), value);
        }
        result
    }

    /// Resolve parameter placeholders in tool params.
    fn resolve_params(
        &self,
        params: Option<&serde_json::Value>,
        context: &HashMap<String, String>,
    ) -> serde_json::Value {
        match params {
            Some(v) => {
                let json_str = serde_json::to_string(v).unwrap_or_default();
                let resolved = self.resolve_template(&json_str, context);
                serde_json::from_str(&resolved).unwrap_or_else(|_| v.clone())
            }
            None => serde_json::json!({}),
        }
    }
}

/// Builds test scenarios for validation.
pub struct ScenarioBuilder;

impl ScenarioBuilder {
    /// Generate test scenarios for a playbook using LLM.
    pub async fn build_scenarios(
        playbook: &Playbook,
        llm: &OllamaClient,
        count: usize,
    ) -> Vec<HashMap<String, String>> {
        let prompt = format!(
            "Generate {} different test scenarios for this playbook:\n\
             Name: {}\nDescription: {}\nPattern: {}\n\
             Steps: {}\n\n\
             For each scenario, provide key-value pairs as JSON objects, one per line.\n\
             Example: {{\"query\": \"test input\", \"expected\": \"test output\"}}\n\
             Generate {} scenarios:",
            count,
            playbook.name,
            playbook.description,
            playbook.scenario_pattern,
            playbook
                .steps
                .iter()
                .map(|s| s.description.as_str())
                .collect::<Vec<_>>()
                .join(" -> "),
            count
        );

        let result = llm.generate(&prompt).await;

        // Parse JSON objects from response
        let mut scenarios = Vec::new();
        for line in result.text.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('{') && trimmed.ends_with('}') {
                if let Ok(map) = serde_json::from_str::<HashMap<String, String>>(trimmed) {
                    scenarios.push(map);
                }
            }
        }

        // Ensure we have at least one scenario
        if scenarios.is_empty() {
            let mut default = HashMap::new();
            default.insert("query".to_string(), playbook.scenario_pattern.clone());
            scenarios.push(default);
        }

        scenarios.truncate(count);
        scenarios
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_template() {
        let harness = PlaybookHarness::new();
        let mut ctx = HashMap::new();
        ctx.insert("query".to_string(), "Rust news".to_string());
        ctx.insert("path".to_string(), "src/".to_string());

        let result = harness.resolve_template("Search for {{query}} in {{path}}", &ctx);
        assert_eq!(result, "Search for Rust news in src/");
    }

    #[test]
    fn test_resolve_params() {
        let harness = PlaybookHarness::new();
        let mut ctx = HashMap::new();
        ctx.insert("query".to_string(), "test".to_string());

        let params = serde_json::json!({"pattern": "{{query}}", "path": "src/"});
        let resolved = harness.resolve_params(Some(&params), &ctx);
        assert_eq!(resolved["pattern"], "test");
        assert_eq!(resolved["path"], "src/");
    }

    #[test]
    fn test_harness_defaults() {
        let h = PlaybookHarness::new();
        assert_eq!(h.default_max_retries, 2);
        assert_eq!(h.step_timeout_secs, 60);
    }
}
