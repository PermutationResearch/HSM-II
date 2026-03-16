//! MiroFish-inspired Scenario Simulator — parallel branch prediction engine.
//!
//! Runs multiple scenario branches in parallel using Ollama, each exploring
//! a different variant of a prediction topic. Branches are synthesized into
//! a PredictionReport with confidence scores and a unified synthesis.
//!
//! Architecture (inspired by MiroFish's OASIS multi-agent simulation):
//!   1. Seed Processing: parse topic + seed context + variables
//!   2. Branch Generation: create N scenario variants with LLM
//!   3. Parallel Simulation: run each branch independently via Ollama
//!   4. Synthesis: merge branch predictions into a coherent report

use serde::{Deserialize, Serialize};

use crate::ollama_client::{OllamaClient, OllamaConfig};

/// Configuration for the scenario simulator.
#[derive(Clone, Debug)]
pub struct ScenarioSimulatorConfig {
    /// Number of parallel scenario branches to explore
    pub num_branches: usize,
    /// Ollama host (e.g., "http://localhost")
    pub ollama_host: String,
    /// Ollama port
    pub ollama_port: u16,
    /// Model to use for simulation
    pub model: String,
    /// Max tokens per branch generation
    pub max_tokens: u32,
    /// Temperature for branch diversity (higher = more diverse branches)
    pub temperature: f64,
}

impl Default for ScenarioSimulatorConfig {
    fn default() -> Self {
        Self {
            num_branches: 3,
            ollama_host: std::env::var("OLLAMA_HOST")
                .unwrap_or_else(|_| "http://localhost".to_string()),
            ollama_port: std::env::var("OLLAMA_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(11434),
            model: crate::ollama_client::resolve_model_from_env("llama3.2"),
            max_tokens: 1024,
            temperature: 0.8,
        }
    }
}

/// A single scenario branch — one possible future trajectory.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScenarioBranch {
    /// Branch identifier (e.g., "optimistic", "pessimistic", "disruptive")
    pub variant: String,
    /// The prediction narrative for this branch
    pub prediction: String,
    /// Confidence score (0.0-1.0)
    pub confidence: f64,
    /// Key factors driving this scenario
    pub key_factors: Vec<String>,
    /// Potential risks or invalidation triggers
    pub risks: Vec<String>,
}

/// The full prediction report: synthesis of all branches.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PredictionReport {
    /// Original prediction topic
    pub topic: String,
    /// Seed context provided
    pub seeds: Vec<String>,
    /// Variables injected into simulation
    pub variables: Vec<String>,
    /// Individual scenario branches
    pub branches: Vec<ScenarioBranch>,
    /// Synthesized cross-branch analysis
    pub synthesis: String,
    /// Overall confidence (weighted average across branches)
    pub overall_confidence: f64,
    /// Timestamp of generation
    pub generated_at: u64,
}

/// MiroFish-inspired scenario simulator.
///
/// Generates parallel scenario branches using Ollama and synthesizes
/// them into a prediction report with confidence scoring.
pub struct ScenarioSimulator {
    config: ScenarioSimulatorConfig,
    client: OllamaClient,
}

impl ScenarioSimulator {
    /// Create a new simulator with the given configuration.
    pub fn new(config: ScenarioSimulatorConfig) -> Self {
        let ollama_config = OllamaConfig {
            host: config.ollama_host.clone(),
            port: config.ollama_port,
            model: config.model.clone(),
            max_tokens: config.max_tokens,
            temperature: config.temperature,
            latency_budget_ms: 60_000, // 60s per branch
            enable_batching: false,
            batch_size: 1,
            batch_timeout_ms: 1000,
        };
        let client = OllamaClient::new(ollama_config);
        Self { config, client }
    }

    /// Run scenario simulation: generate branches, simulate each, synthesize.
    pub async fn simulate(
        &self,
        topic: &str,
        seeds: &[String],
        variables: Option<&[String]>,
    ) -> Result<PredictionReport, String> {
        let vars = variables.unwrap_or(&[]);

        // Phase 1: Generate branch variants
        let variants = self.generate_variants(topic, seeds, vars).await?;

        // Phase 2: Simulate each branch in parallel
        let mut branches = Vec::new();
        for variant in &variants {
            let branch = self.simulate_branch(topic, seeds, vars, variant).await?;
            branches.push(branch);
        }

        // Phase 3: Synthesize
        let synthesis = self.synthesize(topic, &branches).await?;

        let overall_confidence = if branches.is_empty() {
            0.0
        } else {
            branches.iter().map(|b| b.confidence).sum::<f64>() / branches.len() as f64
        };

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Ok(PredictionReport {
            topic: topic.to_string(),
            seeds: seeds.to_vec(),
            variables: vars.to_vec(),
            branches,
            synthesis,
            overall_confidence,
            generated_at: now,
        })
    }

    /// Generate N scenario variant labels using LLM.
    async fn generate_variants(
        &self,
        topic: &str,
        seeds: &[String],
        variables: &[String],
    ) -> Result<Vec<String>, String> {
        let seed_text = if seeds.is_empty() {
            "No additional context.".to_string()
        } else {
            seeds.join("\n- ")
        };

        let var_text = if variables.is_empty() {
            "None.".to_string()
        } else {
            variables.join(", ")
        };

        let prompt = format!(
            "You are a scenario planning analyst. Given a prediction topic, generate exactly {} distinct scenario variant labels.\n\n\
             Topic: {}\n\
             Context:\n- {}\n\
             Variables: {}\n\n\
             Output ONLY the variant labels, one per line, no numbering, no explanation.\n\
             Examples of good labels: optimistic, pessimistic, disruptive, status-quo, accelerated, delayed\n\
             Labels:",
            self.config.num_branches, topic, seed_text, var_text
        );

        let result = self.client.generate(&prompt).await;
        if result.timed_out {
            return Err(format!("Failed to generate variants: {}", result.text));
        }

        let variants: Vec<String> = result
            .text
            .lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty())
            .map(|l| {
                // Strip leading dash/bullet/number
                let s = l.trim_start_matches(|c: char| c == '-' || c == '•' || c == '*' || c.is_ascii_digit() || c == '.');
                s.trim().to_string()
            })
            .filter(|s| !s.is_empty())
            .take(self.config.num_branches)
            .collect();

        if variants.is_empty() {
            // Fallback: generate default variants
            Ok(vec![
                "optimistic".to_string(),
                "pessimistic".to_string(),
                "disruptive".to_string(),
            ]
            .into_iter()
            .take(self.config.num_branches)
            .collect())
        } else {
            Ok(variants)
        }
    }

    /// Simulate a single scenario branch.
    async fn simulate_branch(
        &self,
        topic: &str,
        seeds: &[String],
        variables: &[String],
        variant: &str,
    ) -> Result<ScenarioBranch, String> {
        let seed_text = if seeds.is_empty() {
            "No additional context.".to_string()
        } else {
            seeds.join("\n- ")
        };

        let var_text = if variables.is_empty() {
            "None.".to_string()
        } else {
            variables.join(", ")
        };

        let prompt = format!(
            "You are a scenario simulation engine exploring a '{}' scenario.\n\n\
             Topic: {}\n\
             Context:\n- {}\n\
             Variables: {}\n\n\
             Provide a detailed prediction for this scenario variant. Include:\n\
             1. PREDICTION: A 2-3 sentence prediction narrative\n\
             2. CONFIDENCE: A number between 0.0 and 1.0\n\
             3. KEY_FACTORS: 2-3 driving factors (one per line, prefixed with '- ')\n\
             4. RISKS: 1-2 risks that could invalidate this scenario (one per line, prefixed with '- ')\n\n\
             Format exactly as shown above with section headers.",
            variant, topic, seed_text, var_text
        );

        let result = self.client.generate(&prompt).await;
        if result.timed_out {
            return Err(format!("Branch '{}' failed: {}", variant, result.text));
        }

        let text = &result.text;
        let branch = Self::parse_branch(text, variant);
        Ok(branch)
    }

    /// Parse LLM output into a ScenarioBranch.
    fn parse_branch(text: &str, variant: &str) -> ScenarioBranch {
        let mut prediction = String::new();
        let mut confidence = 0.5;
        let mut key_factors = Vec::new();
        let mut risks = Vec::new();
        let mut current_section = "";

        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            let upper = trimmed.to_uppercase();
            if upper.starts_with("PREDICTION:") {
                current_section = "prediction";
                let rest = trimmed.splitn(2, ':').nth(1).unwrap_or("").trim();
                if !rest.is_empty() {
                    prediction = rest.to_string();
                }
            } else if upper.starts_with("CONFIDENCE:") {
                current_section = "confidence";
                let rest = trimmed.splitn(2, ':').nth(1).unwrap_or("").trim();
                if let Some(val) = rest
                    .chars()
                    .take_while(|c| c.is_ascii_digit() || *c == '.')
                    .collect::<String>()
                    .parse::<f64>()
                    .ok()
                {
                    confidence = val.clamp(0.0, 1.0);
                }
            } else if upper.starts_with("KEY_FACTORS:") || upper.starts_with("KEY FACTORS:") {
                current_section = "factors";
            } else if upper.starts_with("RISKS:") || upper.starts_with("RISK:") {
                current_section = "risks";
            } else {
                match current_section {
                    "prediction" => {
                        if !prediction.is_empty() {
                            prediction.push(' ');
                        }
                        prediction.push_str(trimmed);
                    }
                    "factors" if trimmed.starts_with('-') || trimmed.starts_with('•') => {
                        key_factors.push(trimmed.trim_start_matches(|c: char| c == '-' || c == '•' || c == ' ').to_string());
                    }
                    "risks" if trimmed.starts_with('-') || trimmed.starts_with('•') => {
                        risks.push(trimmed.trim_start_matches(|c: char| c == '-' || c == '•' || c == ' ').to_string());
                    }
                    _ => {}
                }
            }
        }

        // Fallback if parsing found nothing
        if prediction.is_empty() {
            prediction = text.lines().take(3).collect::<Vec<_>>().join(" ");
        }

        ScenarioBranch {
            variant: variant.to_string(),
            prediction,
            confidence,
            key_factors,
            risks,
        }
    }

    /// Synthesize all branches into a unified analysis.
    async fn synthesize(
        &self,
        topic: &str,
        branches: &[ScenarioBranch],
    ) -> Result<String, String> {
        if branches.is_empty() {
            return Ok("No branches to synthesize.".to_string());
        }

        let branch_summaries: Vec<String> = branches
            .iter()
            .map(|b| {
                format!(
                    "- {} (confidence: {:.2}): {}",
                    b.variant, b.confidence, b.prediction
                )
            })
            .collect();

        let prompt = format!(
            "You are a strategic analyst synthesizing multiple scenario predictions.\n\n\
             Topic: {}\n\n\
             Scenario branches:\n{}\n\n\
             Provide a 3-4 sentence synthesis that:\n\
             1. Identifies the most likely trajectory\n\
             2. Notes key uncertainties\n\
             3. Recommends a strategic posture\n\n\
             Synthesis:",
            topic,
            branch_summaries.join("\n")
        );

        let result = self.client.generate(&prompt).await;
        if result.timed_out {
            return Err(format!("Synthesis failed: {}", result.text));
        }

        Ok(result.text.trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_defaults() {
        let cfg = ScenarioSimulatorConfig::default();
        assert_eq!(cfg.num_branches, 3);
        assert_eq!(cfg.ollama_port, 11434);
        assert!(cfg.temperature > 0.0);
    }

    #[test]
    fn test_parse_branch_structured() {
        let text = "\
PREDICTION: The market will see moderate growth driven by AI adoption.
CONFIDENCE: 0.72
KEY_FACTORS:
- Increasing enterprise AI budgets
- Regulatory clarity in major markets
RISKS:
- Economic downturn could reduce spending
- Open-source alternatives may commoditize";

        let branch = ScenarioSimulator::parse_branch(text, "optimistic");
        assert_eq!(branch.variant, "optimistic");
        assert!(branch.prediction.contains("moderate growth"));
        assert!((branch.confidence - 0.72).abs() < 0.01);
        assert_eq!(branch.key_factors.len(), 2);
        assert_eq!(branch.risks.len(), 2);
        assert!(branch.key_factors[0].contains("enterprise AI"));
    }

    #[test]
    fn test_parse_branch_unstructured_fallback() {
        let text = "This is an unstructured response without section headers.\nIt should still produce a branch.";
        let branch = ScenarioSimulator::parse_branch(text, "fallback");
        assert_eq!(branch.variant, "fallback");
        assert!(!branch.prediction.is_empty());
        assert!((branch.confidence - 0.5).abs() < 0.01); // default
    }

    #[test]
    fn test_prediction_report_serde() {
        let report = PredictionReport {
            topic: "AI market trends".to_string(),
            seeds: vec!["recent growth data".to_string()],
            variables: vec!["regulation_level=moderate".to_string()],
            branches: vec![ScenarioBranch {
                variant: "optimistic".to_string(),
                prediction: "Growth continues at 25% CAGR".to_string(),
                confidence: 0.7,
                key_factors: vec!["Enterprise adoption".to_string()],
                risks: vec!["Recession risk".to_string()],
            }],
            synthesis: "Overall trajectory is positive with moderate uncertainty.".to_string(),
            overall_confidence: 0.7,
            generated_at: 1234567890,
        };

        let json = serde_json::to_string(&report).unwrap();
        let restored: PredictionReport = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.topic, "AI market trends");
        assert_eq!(restored.branches.len(), 1);
        assert!((restored.overall_confidence - 0.7).abs() < 0.01);
    }

    #[test]
    fn test_scenario_branch_serde() {
        let branch = ScenarioBranch {
            variant: "disruptive".to_string(),
            prediction: "A new paradigm emerges".to_string(),
            confidence: 0.45,
            key_factors: vec!["technology breakthrough".to_string()],
            risks: vec!["adoption barriers".to_string()],
        };

        let json = serde_json::to_string(&branch).unwrap();
        let restored: ScenarioBranch = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.variant, "disruptive");
        assert!((restored.confidence - 0.45).abs() < 0.01);
    }
}
