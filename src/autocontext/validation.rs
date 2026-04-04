//! Staged validation pipeline for autocontext playbooks.
//!
//! Stages: Unit → Integration → Staged
//! Each stage is progressively stricter. Failed validations
//! prevent persistence. Rollback support for staged failures.

use std::collections::HashMap;
use tracing::{debug, info};

use crate::ollama_client::OllamaClient;
use crate::tools::ToolRegistry;

use super::harness::{PlaybookHarness, ScenarioBuilder};
use super::Playbook;

/// Validation stages in order of strictness.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub enum ValidationStage {
    /// Quick check: format, score threshold, well-formed steps
    Unit,
    /// Run against one test scenario
    Integration,
    /// Run against multiple generated scenarios
    Staged,
}

impl std::fmt::Display for ValidationStage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ValidationStage::Unit => write!(f, "Unit"),
            ValidationStage::Integration => write!(f, "Integration"),
            ValidationStage::Staged => write!(f, "Staged"),
        }
    }
}

impl Default for ValidationStage {
    fn default() -> Self {
        ValidationStage::Unit
    }
}

/// Result of running a single validation stage.
#[derive(Clone, Debug)]
pub struct StageResult {
    pub stage: ValidationStage,
    pub passed: bool,
    pub score: f64,
    pub details: String,
}

/// Overall result of the validation pipeline.
#[derive(Clone, Debug)]
pub struct ValidationResult {
    pub passed: bool,
    pub stage_reached: ValidationStage,
    pub stage_results: Vec<StageResult>,
    pub rollback_needed: bool,
}

/// Configuration for the validation pipeline.
#[derive(Clone, Debug)]
pub struct ValidationConfig {
    /// Minimum quality score to pass Unit stage
    pub unit_score_threshold: f64,
    /// Minimum success rate for Staged validation
    pub staged_success_rate: f64,
    /// Number of scenarios for Staged validation
    pub staged_scenario_count: usize,
}

impl Default for ValidationConfig {
    fn default() -> Self {
        Self {
            unit_score_threshold: 0.6,
            staged_success_rate: 0.7,
            staged_scenario_count: 3,
        }
    }
}

/// The validation pipeline executor.
pub struct ValidationPipeline {
    pub stages: Vec<ValidationStage>,
    pub config: ValidationConfig,
    harness: PlaybookHarness,
}

impl ValidationPipeline {
    pub fn new(stages: Vec<ValidationStage>) -> Self {
        Self {
            stages,
            config: ValidationConfig::default(),
            harness: PlaybookHarness::new(),
        }
    }

    pub fn with_config(mut self, config: ValidationConfig) -> Self {
        self.config = config;
        self
    }

    /// Default pipeline: Unit + Integration.
    pub fn default_pipeline() -> Self {
        Self::new(vec![ValidationStage::Unit, ValidationStage::Integration])
    }

    /// Full pipeline: Unit + Integration + Staged.
    pub fn full_pipeline() -> Self {
        Self::new(vec![
            ValidationStage::Unit,
            ValidationStage::Integration,
            ValidationStage::Staged,
        ])
    }

    /// Run playbook through all configured stages.
    /// Stops at first failure.
    pub async fn validate(
        &self,
        playbook: &Playbook,
        tool_registry: &mut ToolRegistry,
        llm: &OllamaClient,
    ) -> ValidationResult {
        let mut stage_results = Vec::new();
        let mut last_stage = ValidationStage::Unit;

        for stage in &self.stages {
            last_stage = stage.clone();
            let result = match stage {
                ValidationStage::Unit => self.validate_unit(playbook, tool_registry),
                ValidationStage::Integration => {
                    self.validate_integration(playbook, tool_registry, llm)
                        .await
                }
                ValidationStage::Staged => self.validate_staged(playbook, tool_registry, llm).await,
            };

            let passed = result.passed;
            stage_results.push(result);

            if !passed {
                info!("Playbook '{}' failed {} validation", playbook.name, stage);
                return ValidationResult {
                    passed: false,
                    stage_reached: last_stage,
                    stage_results,
                    rollback_needed: *stage == ValidationStage::Staged,
                };
            }

            debug!("Playbook '{}' passed {} validation", playbook.name, stage);
        }

        info!(
            "Playbook '{}' passed all {} validation stages",
            playbook.name,
            self.stages.len()
        );
        ValidationResult {
            passed: true,
            stage_reached: last_stage,
            stage_results,
            rollback_needed: false,
        }
    }

    /// Unit validation: format checks, score threshold, tool name validity.
    fn validate_unit(&self, playbook: &Playbook, tool_registry: &ToolRegistry) -> StageResult {
        let mut issues = Vec::new();
        let mut score: f64 = 1.0;

        // Must have at least one step
        if playbook.steps.is_empty() {
            issues.push("No steps defined".to_string());
            score -= 0.5;
        }

        // Quality score threshold
        if playbook.quality_score < self.config.unit_score_threshold {
            issues.push(format!(
                "Quality score {:.2} below threshold {:.2}",
                playbook.quality_score, self.config.unit_score_threshold
            ));
            score -= 0.3;
        }

        // All steps must have descriptions and success criteria
        for step in &playbook.steps {
            if step.description.is_empty() {
                issues.push(format!("Step {} has empty description", step.index));
                score -= 0.1;
            }
            if step.success_criteria.is_empty() {
                issues.push(format!("Step {} has empty success criteria", step.index));
                score -= 0.1;
            }
            // Validate tool names exist in registry
            if let Some(ref tool_name) = step.tool_name {
                if !tool_registry.has(tool_name) {
                    issues.push(format!(
                        "Step {} references unknown tool '{}'",
                        step.index, tool_name
                    ));
                    score -= 0.2;
                }
            }
            // Must have either tool or prompt
            if step.tool_name.is_none() && step.prompt_template.is_none() {
                issues.push(format!("Step {} has neither tool nor prompt", step.index));
                score -= 0.2;
            }
        }

        let score: f64 = score.max(0.0);
        let passed = issues.is_empty() && score >= 0.5;

        StageResult {
            stage: ValidationStage::Unit,
            passed,
            score,
            details: if issues.is_empty() {
                "All unit checks passed".to_string()
            } else {
                issues.join("; ")
            },
        }
    }

    /// Integration validation: execute playbook against one test scenario.
    async fn validate_integration(
        &self,
        playbook: &Playbook,
        tool_registry: &mut ToolRegistry,
        llm: &OllamaClient,
    ) -> StageResult {
        // Build a simple test context from the scenario pattern
        let mut context = HashMap::new();
        context.insert("query".to_string(), playbook.scenario_pattern.clone());

        let result = self
            .harness
            .execute(playbook, tool_registry, llm, &context)
            .await;

        let score = if result.success {
            result.steps_completed as f64 / result.steps_total.max(1) as f64
        } else {
            result.steps_completed as f64 / (result.steps_total.max(1) as f64 * 2.0)
        };

        StageResult {
            stage: ValidationStage::Integration,
            passed: result.success,
            score,
            details: if result.success {
                format!(
                    "All {} steps completed in {}ms",
                    result.steps_completed, result.total_duration_ms
                )
            } else {
                format!(
                    "Failed at step {}/{}: {}",
                    result.steps_completed,
                    result.steps_total,
                    result.error.unwrap_or_default()
                )
            },
        }
    }

    /// Staged validation: execute against multiple LLM-generated scenarios.
    async fn validate_staged(
        &self,
        playbook: &Playbook,
        tool_registry: &mut ToolRegistry,
        llm: &OllamaClient,
    ) -> StageResult {
        let scenarios =
            ScenarioBuilder::build_scenarios(playbook, llm, self.config.staged_scenario_count)
                .await;

        let mut successes = 0;
        let mut total_score = 0.0;
        let mut details_parts = Vec::new();

        for (i, scenario) in scenarios.iter().enumerate() {
            let result = self
                .harness
                .execute(playbook, tool_registry, llm, scenario)
                .await;

            if result.success {
                successes += 1;
                total_score += 1.0;
                details_parts.push(format!(
                    "Scenario {}: PASS ({}ms)",
                    i + 1,
                    result.total_duration_ms
                ));
            } else {
                total_score += result.steps_completed as f64 / result.steps_total.max(1) as f64;
                details_parts.push(format!(
                    "Scenario {}: FAIL ({})",
                    i + 1,
                    result.error.unwrap_or_default()
                ));
            }
        }

        let success_rate = successes as f64 / scenarios.len().max(1) as f64;
        let avg_score = total_score / scenarios.len().max(1) as f64;
        let passed = success_rate >= self.config.staged_success_rate;

        StageResult {
            stage: ValidationStage::Staged,
            passed,
            score: avg_score,
            details: format!(
                "{}/{} scenarios passed ({:.0}%): {}",
                successes,
                scenarios.len(),
                success_rate * 100.0,
                details_parts.join("; ")
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::autocontext::Step;

    #[test]
    fn test_validation_stage_ordering() {
        assert!(ValidationStage::Unit < ValidationStage::Integration);
        assert!(ValidationStage::Integration < ValidationStage::Staged);
    }

    #[test]
    fn test_unit_validation_empty_playbook() {
        let mut registry = ToolRegistry::new();
        let pipeline = ValidationPipeline::default_pipeline();
        let pb = Playbook::new("Empty", "desc", "pattern");
        let result = pipeline.validate_unit(&pb, &registry);
        assert!(!result.passed);
        assert!(result.details.contains("No steps"));
    }

    #[test]
    fn test_unit_validation_valid_playbook() {
        let mut registry = ToolRegistry::new();
        // Register a test tool
        use std::sync::Arc;
        registry.register(Arc::new(crate::tools::shell_tools::GrepTool));
        let pipeline = ValidationPipeline::default_pipeline();

        let pb = Playbook::new("Search", "desc", "search code").with_steps(vec![Step::tool_step(
            0,
            "Grep for pattern",
            "grep",
            serde_json::json!({"pattern": "test", "path": "src/"}),
            "results found",
        )]);
        let mut pb = pb;
        pb.quality_score = 0.8;

        let result = pipeline.validate_unit(&pb, &registry);
        assert!(result.passed, "Failed: {}", result.details);
    }

    #[test]
    fn test_unit_validation_unknown_tool() {
        let registry = ToolRegistry::new();
        let pipeline = ValidationPipeline::default_pipeline();
        let mut pb = Playbook::new("Test", "desc", "pattern").with_steps(vec![Step::tool_step(
            0,
            "Use nonexistent",
            "nonexistent_tool",
            serde_json::json!({}),
            "ok",
        )]);
        pb.quality_score = 0.8;

        let result = pipeline.validate_unit(&pb, &registry);
        assert!(!result.passed);
        assert!(result.details.contains("unknown tool"));
    }
}
