//! Evaluator trait for scoring artifacts

use super::Artifact;
use async_trait::async_trait;
use ollama_rs::generation::chat::request::ChatMessageRequest;
use ollama_rs::generation::chat::{ChatMessage, MessageRole};

/// Result of evaluating an artifact
#[derive(Clone, Debug)]
pub struct EvalResult {
    pub score: f64,
    pub sharpened: Option<String>,
    pub feedback: String,
}

/// Trait for artifact evaluators
#[async_trait]
pub trait Evaluator: Send + Sync {
    async fn evaluate(&self, artifact: &Artifact) -> anyhow::Result<EvalResult>;
}

/// LLM-based judge evaluator
pub struct LlmJudgeEvaluator {
    model: String,
    rubric: String,
}

impl LlmJudgeEvaluator {
    pub fn new(model: impl Into<String>, rubric: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            rubric: rubric.into(),
        }
    }
}

#[async_trait]
impl Evaluator for LlmJudgeEvaluator {
    async fn evaluate(&self, artifact: &Artifact) -> anyhow::Result<EvalResult> {
        let ollama = ollama_rs::Ollama::new("http://localhost".to_string(), 11434);

        let system_prompt = format!(
            "You are a judge evaluator. Score the artifact 0-1 based on:\n{}",
            self.rubric
        );

        let user_prompt = format!(
            "Artifact to evaluate:\n```\n{}\n```\n\nProvide score and brief feedback.",
            artifact.content
        );

        let messages = vec![
            ChatMessage::new(MessageRole::System, system_prompt),
            ChatMessage::new(MessageRole::User, user_prompt),
        ];

        let request = ChatMessageRequest::new(self.model.clone(), messages);

        match ollama.send_chat_messages(request).await {
            Ok(response) => {
                let content = response.message.content;
                let mut score = 0.5;
                let mut feedback = "No feedback".to_string();

                for line in content.lines() {
                    if line.starts_with("SCORE:") {
                        if let Some(s) = line.split(':').nth(1) {
                            score = s.trim().parse::<f64>().unwrap_or(0.5).clamp(0.0, 1.0);
                        }
                    } else if line.starts_with("FEEDBACK:") {
                        feedback = line.split(':').nth(1).unwrap_or("").trim().to_string();
                    }
                }

                Ok(EvalResult {
                    score,
                    sharpened: None,
                    feedback,
                })
            }
            Err(e) => Ok(EvalResult {
                score: 0.5,
                sharpened: None,
                feedback: format!("Evaluation error: {}", e),
            }),
        }
    }
}

/// Keyword-based evaluator for quick checks
pub struct KeywordEvaluator {
    required: Vec<String>,
    forbidden: Vec<String>,
}

impl KeywordEvaluator {
    pub fn new(required: Vec<String>, forbidden: Vec<String>) -> Self {
        Self {
            required,
            forbidden,
        }
    }
}

#[async_trait]
impl Evaluator for KeywordEvaluator {
    async fn evaluate(&self, artifact: &Artifact) -> anyhow::Result<EvalResult> {
        let content_lower = artifact.content.to_lowercase();

        let mut score: f64 = 1.0;
        let mut feedback_parts = vec![];

        // Check required keywords
        for keyword in &self.required {
            if !content_lower.contains(&keyword.to_lowercase()) {
                score -= 0.2;
                feedback_parts.push(format!("Missing required: {}", keyword));
            }
        }

        // Check forbidden keywords
        for keyword in &self.forbidden {
            if content_lower.contains(&keyword.to_lowercase()) {
                score -= 0.3;
                feedback_parts.push(format!("Contains forbidden: {}", keyword));
            }
        }

        Ok(EvalResult {
            score: score.max(0.0),
            sharpened: None,
            feedback: if feedback_parts.is_empty() {
                "Passed all checks".to_string()
            } else {
                feedback_parts.join("; ")
            },
        })
    }
}

/// Composite evaluator combining multiple evaluators
pub struct CompositeEvaluator {
    evaluators: Vec<(Box<dyn Evaluator>, f64)>,
}

impl CompositeEvaluator {
    pub fn new() -> Self {
        Self { evaluators: vec![] }
    }

    pub fn add<E: Evaluator + 'static>(mut self, evaluator: E, weight: f64) -> Self {
        self.evaluators.push((Box::new(evaluator), weight));
        self
    }
}

#[async_trait]
impl Evaluator for CompositeEvaluator {
    async fn evaluate(&self, artifact: &Artifact) -> anyhow::Result<EvalResult> {
        let mut total_score = 0.0;
        let mut total_weight = 0.0;
        let mut all_feedback = vec![];

        for (evaluator, weight) in &self.evaluators {
            match evaluator.evaluate(artifact).await {
                Ok(result) => {
                    total_score += result.score * weight;
                    total_weight += weight;
                    all_feedback.push(result.feedback);
                }
                Err(_) => {
                    total_weight += weight;
                }
            }
        }

        let final_score = if total_weight > 0.0 {
            total_score / total_weight
        } else {
            0.0
        };

        Ok(EvalResult {
            score: final_score,
            sharpened: None,
            feedback: all_feedback.join(" | "),
        })
    }
}

/// Batched LLM evaluator for efficient parallel evaluation
pub struct BatchedLlmEvaluator {
    model: String,
    rubric: String,
    batch_size: usize,
    latency_budget_ms: u64,
}

impl BatchedLlmEvaluator {
    pub fn new(
        model: impl Into<String>,
        rubric: impl Into<String>,
        batch_size: usize,
        latency_budget_ms: u64,
    ) -> Self {
        Self {
            model: model.into(),
            rubric: rubric.into(),
            batch_size,
            latency_budget_ms,
        }
    }

    /// Evaluate a batch of artifacts in parallel with latency budget enforcement
    pub async fn evaluate_batch(&self, artifacts: &[Artifact]) -> Vec<anyhow::Result<EvalResult>> {
        use tokio::time::{timeout, Duration};

        let mut results = Vec::with_capacity(artifacts.len());

        // Process in batches
        for chunk in artifacts.chunks(self.batch_size) {
            let batch_futures: Vec<_> = chunk
                .iter()
                .map(|artifact| {
                    let evaluator = LlmJudgeEvaluator::new(self.model.clone(), self.rubric.clone());
                    let budget = Duration::from_millis(self.latency_budget_ms);

                    async move {
                        timeout(budget, evaluator.evaluate(artifact))
                            .await
                            .map_err(|_| {
                                anyhow::anyhow!("LLM evaluation timed out after {:?}", budget)
                            })
                            .and_then(|r| r)
                    }
                })
                .collect();

            let batch_results = futures::future::join_all(batch_futures).await;
            results.extend(batch_results);
        }

        results
    }
}
