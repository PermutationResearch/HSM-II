//! Frontier-to-local model routing and training data export.
//!
//! Routes queries between frontier models (Claude, GPT-4) for hard tasks
//! and local models (Ollama) for routine tasks. Supports exporting
//! validated run data for fine-tuning local models.

use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use crate::ollama_client::OllamaClient;

use super::Generation;

/// Model tier for routing decisions.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ModelTier {
    /// Cloud API (Claude, GPT-4) for hard problems
    Frontier {
        provider: String,
        model: String,
        api_key_env: String,
    },
    /// Local model (Ollama) for routine tasks
    Local { model: String },
    /// Auto-route based on confidence/complexity
    Auto,
}

/// Configuration for a frontier model provider.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FrontierConfig {
    /// Provider name: "anthropic", "openai"
    pub provider: String,
    /// Model identifier
    pub model: String,
    /// Environment variable holding the API key
    pub api_key_env: String,
}

/// A training example extracted from validated runs.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrainingExample {
    pub input: String,
    pub output: String,
    pub score: f64,
    pub scenario: String,
    pub generation_id: u64,
}

/// Routes queries between frontier and local models.
pub struct DistillationRouter {
    /// Local model name (for Ollama)
    pub local_model: String,
    /// Optional frontier model configuration
    pub frontier_config: Option<FrontierConfig>,
    /// Confidence threshold: below this → use frontier
    pub confidence_threshold: f64,
    /// Complexity threshold: above this → use frontier
    pub complexity_threshold: f64,
}

impl DistillationRouter {
    pub fn new(local_model: impl Into<String>) -> Self {
        Self {
            local_model: local_model.into(),
            frontier_config: None,
            confidence_threshold: 0.6,
            complexity_threshold: 0.7,
        }
    }

    pub fn with_frontier(mut self, config: FrontierConfig) -> Self {
        self.frontier_config = Some(config);
        self
    }

    pub fn with_thresholds(mut self, confidence: f64, complexity: f64) -> Self {
        self.confidence_threshold = confidence;
        self.complexity_threshold = complexity;
        self
    }

    /// Determine which tier to use for a given query.
    pub fn select_tier(&self, confidence: f64, complexity: f64) -> ModelTier {
        if self.frontier_config.is_none() {
            return ModelTier::Local {
                model: self.local_model.clone(),
            };
        }

        if confidence < self.confidence_threshold || complexity > self.complexity_threshold {
            if let Some(ref config) = self.frontier_config {
                return ModelTier::Frontier {
                    provider: config.provider.clone(),
                    model: config.model.clone(),
                    api_key_env: config.api_key_env.clone(),
                };
            }
        }

        ModelTier::Local {
            model: self.local_model.clone(),
        }
    }

    /// Route a query to the appropriate model tier.
    /// Falls back to local if frontier is unavailable.
    pub async fn route_query(
        &self,
        prompt: &str,
        confidence: f64,
        complexity: f64,
        local_llm: &OllamaClient,
    ) -> anyhow::Result<(String, ModelTier)> {
        let tier = self.select_tier(confidence, complexity);

        match &tier {
            ModelTier::Local { .. } => {
                debug!("Routing to local model (confidence={:.2}, complexity={:.2})", confidence, complexity);
                let result = local_llm.generate(prompt).await;
                Ok((result.text, tier))
            }
            ModelTier::Frontier {
                provider,
                model,
                api_key_env,
            } => {
                debug!(
                    "Routing to frontier model {}:{} (confidence={:.2}, complexity={:.2})",
                    provider, model, confidence, complexity
                );

                // Try frontier API
                match self.call_frontier(prompt, provider, model, api_key_env).await {
                    Ok(response) => Ok((response, tier)),
                    Err(e) => {
                        warn!("Frontier model failed, falling back to local: {}", e);
                        let result = local_llm.generate(prompt).await;
                        Ok((
                            result.text,
                            ModelTier::Local {
                                model: self.local_model.clone(),
                            },
                        ))
                    }
                }
            }
            ModelTier::Auto => {
                // Auto delegates to select_tier which never returns Auto
                let result = local_llm.generate(prompt).await;
                Ok((result.text, ModelTier::Local { model: self.local_model.clone() }))
            }
        }
    }

    /// Call a frontier model API.
    async fn call_frontier(
        &self,
        prompt: &str,
        provider: &str,
        model: &str,
        api_key_env: &str,
    ) -> anyhow::Result<String> {
        let api_key = std::env::var(api_key_env).map_err(|_| {
            anyhow::anyhow!("API key env var {} not set", api_key_env)
        })?;

        let (url, body) = match provider {
            "anthropic" => {
                let url = "https://api.anthropic.com/v1/messages";
                let body = serde_json::json!({
                    "model": model,
                    "max_tokens": 4096,
                    "messages": [{"role": "user", "content": prompt}]
                });
                (url, body)
            }
            "openai" => {
                let url = "https://api.openai.com/v1/chat/completions";
                let body = serde_json::json!({
                    "model": model,
                    "messages": [{"role": "user", "content": prompt}],
                    "max_tokens": 4096
                });
                (url, body)
            }
            _ => return Err(anyhow::anyhow!("Unknown provider: {}", provider)),
        };

        let client = reqwest::Client::new();
        let mut request = client.post(url).json(&body);

        // Add auth headers based on provider
        request = match provider {
            "anthropic" => request
                .header("x-api-key", &api_key)
                .header("anthropic-version", "2023-06-01")
                .header("content-type", "application/json"),
            "openai" => request
                .header("Authorization", format!("Bearer {}", api_key))
                .header("content-type", "application/json"),
            _ => request,
        };

        let response = request.send().await?;
        let status = response.status();
        let body: serde_json::Value = response.json().await?;

        if !status.is_success() {
            return Err(anyhow::anyhow!(
                "Frontier API returned {}: {}",
                status,
                serde_json::to_string_pretty(&body).unwrap_or_default()
            ));
        }

        // Extract text based on provider format
        let text = match provider {
            "anthropic" => body["content"][0]["text"]
                .as_str()
                .unwrap_or("")
                .to_string(),
            "openai" => body["choices"][0]["message"]["content"]
                .as_str()
                .unwrap_or("")
                .to_string(),
            _ => String::new(),
        };

        Ok(text)
    }

    /// Export training data from validated generations.
    /// Extracts high-quality input/output pairs for fine-tuning.
    pub fn export_training_data(
        &self,
        generations: &[Generation],
        min_score: f64,
    ) -> Vec<TrainingExample> {
        let mut examples = Vec::new();

        for gen in generations {
            for run in &gen.run_records {
                if run.composite_score < min_score {
                    continue;
                }

                // Extract input from strategy description + step descriptions
                let input = format!(
                    "Scenario: {}\nTask: {}",
                    gen.scenario, run.strategy.description
                );

                // Extract output from successful artifacts
                let output: String = run
                    .artifacts
                    .iter()
                    .filter(|a| {
                        matches!(
                            a.artifact_type,
                            super::ArtifactType::ToolOutput | super::ArtifactType::LlmResponse
                        )
                    })
                    .map(|a| a.content.as_str())
                    .collect::<Vec<_>>()
                    .join("\n---\n");

                if !output.is_empty() {
                    examples.push(TrainingExample {
                        input,
                        output,
                        score: run.composite_score,
                        scenario: gen.scenario.clone(),
                        generation_id: gen.id,
                    });
                }
            }
        }

        info!(
            "Exported {} training examples from {} generations (min_score={:.2})",
            examples.len(),
            generations.len(),
            min_score
        );
        examples
    }

    /// Save router config to disk.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "local_model": self.local_model,
            "confidence_threshold": self.confidence_threshold,
            "complexity_threshold": self.complexity_threshold,
            "frontier": self.frontier_config.as_ref().map(|c| serde_json::json!({
                "provider": c.provider,
                "model": c.model,
                "api_key_env": c.api_key_env,
            })),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_select_tier_no_frontier() {
        let router = DistillationRouter::new("llama3");
        let tier = router.select_tier(0.3, 0.9);
        assert!(matches!(tier, ModelTier::Local { .. }));
    }

    #[test]
    fn test_select_tier_with_frontier() {
        let router = DistillationRouter::new("llama3").with_frontier(FrontierConfig {
            provider: "anthropic".to_string(),
            model: "claude-sonnet-4-6".to_string(),
            api_key_env: "ANTHROPIC_API_KEY".to_string(),
        });

        // Low confidence → frontier
        let tier = router.select_tier(0.3, 0.5);
        assert!(matches!(tier, ModelTier::Frontier { .. }));

        // High complexity → frontier
        let tier = router.select_tier(0.8, 0.9);
        assert!(matches!(tier, ModelTier::Frontier { .. }));

        // High confidence + low complexity → local
        let tier = router.select_tier(0.9, 0.3);
        assert!(matches!(tier, ModelTier::Local { .. }));
    }

    #[test]
    fn test_export_training_data() {
        use super::super::{ArtifactType, RunArtifact, RunRecord, Strategy, StrategySource};
        use std::collections::HashMap;

        let router = DistillationRouter::new("llama3");

        let mut run = RunRecord::new(Strategy::new(
            "Search and summarize",
            vec![],
            StrategySource::Proposed,
        ));
        run.composite_score = 0.85;
        run.artifacts.push(RunArtifact {
            step_index: 0,
            artifact_type: ArtifactType::ToolOutput,
            content: "Found 5 results about Rust".to_string(),
            metadata: HashMap::new(),
        });

        let mut gen = super::super::Generation::new(1, "search news");
        gen.run_records.push(run);

        let examples = router.export_training_data(&[gen], 0.7);
        assert_eq!(examples.len(), 1);
        assert!(examples[0].output.contains("Found 5 results"));
    }

    #[test]
    fn test_router_config_json() {
        let router = DistillationRouter::new("llama3")
            .with_frontier(FrontierConfig {
                provider: "anthropic".to_string(),
                model: "claude-sonnet-4-6".to_string(),
                api_key_env: "ANTHROPIC_API_KEY".to_string(),
            })
            .with_thresholds(0.5, 0.8);

        let json = router.to_json();
        assert_eq!(json["local_model"], "llama3");
        assert_eq!(json["confidence_threshold"], 0.5);
    }
}
