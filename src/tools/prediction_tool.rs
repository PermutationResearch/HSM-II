//! Prediction tool — MiroFish-inspired scenario simulation tool.
//!
//! Allows agents to run scenario-based predictions on any topic by
//! generating parallel scenario branches and synthesizing results.

use super::{object_schema, Tool, ToolOutput};
use crate::scenario_simulator::{PredictionReport, ScenarioSimulator, ScenarioSimulatorConfig};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Tool that exposes MiroFish-inspired scenario simulation to agents.
pub struct PredictionTool {
    simulator: Arc<Mutex<ScenarioSimulator>>,
}

impl PredictionTool {
    pub fn new() -> Self {
        let config = ScenarioSimulatorConfig::default();
        Self {
            simulator: Arc::new(Mutex::new(ScenarioSimulator::new(config))),
        }
    }

    /// Create with custom configuration.
    pub fn with_config(config: ScenarioSimulatorConfig) -> Self {
        Self {
            simulator: Arc::new(Mutex::new(ScenarioSimulator::new(config))),
        }
    }

    fn format_report(report: &PredictionReport) -> String {
        let mut output = format!("## Prediction: {}\n\n", report.topic);

        output.push_str(&format!(
            "**Overall Confidence:** {:.0}%\n\n",
            report.overall_confidence * 100.0
        ));

        output.push_str("### Scenario Branches\n\n");
        for branch in &report.branches {
            output.push_str(&format!(
                "**[{}]** (confidence: {:.0}%)\n{}\n",
                branch.variant,
                branch.confidence * 100.0,
                branch.prediction
            ));

            if !branch.key_factors.is_empty() {
                output.push_str("  Key factors:\n");
                for f in &branch.key_factors {
                    output.push_str(&format!("  - {}\n", f));
                }
            }
            if !branch.risks.is_empty() {
                output.push_str("  Risks:\n");
                for r in &branch.risks {
                    output.push_str(&format!("  - {}\n", r));
                }
            }
            output.push('\n');
        }

        output.push_str(&format!("### Synthesis\n\n{}\n", report.synthesis));
        output
    }
}

#[async_trait::async_trait]
impl Tool for PredictionTool {
    fn name(&self) -> &str {
        "predict"
    }

    fn description(&self) -> &str {
        "Run scenario-based prediction (MiroFish-inspired). Generates parallel scenario \
         branches for a given topic with optional seed context and variables, then \
         synthesizes them into a prediction report with confidence scores."
    }

    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            (
                "topic",
                "Prediction topic (e.g. 'AI market growth 2025', 'impact of new regulation')",
                true,
            ),
            (
                "seeds",
                "JSON array of seed context strings to ground the prediction",
                false,
            ),
            (
                "variables",
                "JSON array of variables to inject (e.g. 'regulation=strict', 'growth_rate=high')",
                false,
            ),
        ])
    }

    async fn execute(&self, params: Value) -> ToolOutput {
        let topic = params
            .get("topic")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if topic.is_empty() {
            return ToolOutput::error("topic is required");
        }

        let seeds: Vec<String> = params
            .get("seeds")
            .and_then(|v| {
                // Accept either a JSON array or a string that parses as JSON array
                if let Some(arr) = v.as_array() {
                    Some(
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect(),
                    )
                } else if let Some(s) = v.as_str() {
                    serde_json::from_str::<Vec<String>>(s).ok()
                } else {
                    None
                }
            })
            .unwrap_or_default();

        let variables: Option<Vec<String>> = params
            .get("variables")
            .and_then(|v| {
                if let Some(arr) = v.as_array() {
                    Some(
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect(),
                    )
                } else if let Some(s) = v.as_str() {
                    serde_json::from_str::<Vec<String>>(s).ok()
                } else {
                    None
                }
            });

        let sim = self.simulator.lock().await;
        match sim
            .simulate(topic, &seeds, variables.as_deref())
            .await
        {
            Ok(report) => {
                let formatted = Self::format_report(&report);
                ToolOutput::success(formatted)
                    .with_metadata(serde_json::to_value(&report).unwrap_or(Value::Null))
            }
            Err(e) => ToolOutput::error(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prediction_tool_metadata() {
        let tool = PredictionTool::new();
        assert_eq!(tool.name(), "predict");
        assert!(tool.description().contains("MiroFish"));
        let schema = tool.parameters_schema();
        assert!(schema.get("properties").is_some());
        let props = schema["properties"].as_object().unwrap();
        assert!(props.contains_key("topic"));
        assert!(props.contains_key("seeds"));
        assert!(props.contains_key("variables"));
    }

    #[test]
    fn test_format_report() {
        let report = PredictionReport {
            topic: "Test topic".to_string(),
            seeds: vec![],
            variables: vec![],
            branches: vec![crate::scenario_simulator::ScenarioBranch {
                variant: "optimistic".to_string(),
                prediction: "Things go well".to_string(),
                confidence: 0.8,
                key_factors: vec!["factor1".to_string()],
                risks: vec!["risk1".to_string()],
            }],
            synthesis: "Overall positive outlook.".to_string(),
            overall_confidence: 0.8,
            generated_at: 0,
        };

        let formatted = PredictionTool::format_report(&report);
        assert!(formatted.contains("## Prediction: Test topic"));
        assert!(formatted.contains("[optimistic]"));
        assert!(formatted.contains("80%"));
        assert!(formatted.contains("factor1"));
        assert!(formatted.contains("risk1"));
        assert!(formatted.contains("Overall positive outlook"));
    }
}
