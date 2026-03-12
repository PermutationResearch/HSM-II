//! Optimize Anything - Declarative optimization for text artifacts
//!
//! A Rust implementation inspired by GEPA's optimize_anything API.
//! Optimizes any artifact representable as text (code, prompts, configs, etc.)
//! using LLM-driven evolutionary search.

use serde::{Deserialize, Serialize};

pub mod evaluator;
pub mod types;

pub use evaluator::{EvalResult, Evaluator, KeywordEvaluator, LlmJudgeEvaluator};
pub use types::{Artifact, Candidate, OptimizationMode, ASI};

/// Configuration for optimization session
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OptimizationConfig {
    pub max_iterations: usize,
    pub population_size: usize,
    pub temperature: f32,
    pub model: String,
    pub enable_reflection: bool,
    pub early_stopping_patience: usize,
    pub improvement_threshold: f64,
}

impl Default for OptimizationConfig {
    fn default() -> Self {
        Self {
            max_iterations: 10,
            population_size: 5,
            temperature: 0.7,
            model: "hf.co/DavidAU/OpenAi-GPT-oss-20b-HERETIC-uncensored-NEO-Imatrix-gguf:IQ4_NL"
                .to_string(),
            enable_reflection: true,
            early_stopping_patience: 3,
            improvement_threshold: 0.01,
        }
    }
}

/// Active optimization session
#[derive(Clone, Debug)]
pub struct OptimizationSession {
    pub id: String,
    pub config: OptimizationConfig,
    pub mode: OptimizationMode,
    pub objective: String,
    pub candidates: Vec<Candidate>,
    pub best_score: f64,
    pub iteration: usize,
}

impl OptimizationSession {
    pub fn new(
        objective: impl Into<String>,
        config: OptimizationConfig,
        mode: OptimizationMode,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            config,
            mode,
            objective: objective.into(),
            candidates: vec![],
            best_score: 0.0,
            iteration: 0,
        }
    }

    pub fn add_candidate(&mut self, candidate: Candidate) {
        if candidate.score > self.best_score {
            self.best_score = candidate.score;
        }
        self.candidates.push(candidate);
    }

    /// Run the full GEPA-inspired evolutionary optimization loop.
    ///
    /// Each iteration:
    /// 1. Mutate / crossover top candidates to produce a new population
    /// 2. Evaluate every candidate via LLM-judge (Ollama)
    /// 3. Keep the top-`population_size` survivors (elitism)
    /// 4. Broadcast progress events through `tx`
    /// 5. Apply early-stopping when improvement stalls
    ///
    /// Progress events are JSON strings:
    ///   `{"iter":N,"best":0.87,"improved":true,"feedback":"…"}`
    pub async fn run(&mut self, tx: tokio::sync::broadcast::Sender<String>) {
        use ollama_rs::generation::chat::request::ChatMessageRequest;
        use ollama_rs::{
            generation::chat::{ChatMessage, MessageRole},
            Ollama,
        };

        let ollama = Ollama::new("http://localhost".to_string(), 11434);

        // ── emit helper ────────────────────────────────────────────────────────
        let emit = |payload: String| {
            let _ = tx.send(payload);
        };

        // ── bootstrap: if no seed candidate yet, treat objective itself as seed ─
        if self.candidates.is_empty() {
            let seed = Artifact::new(self.objective.clone());
            let eval = Self::evaluate_candidate_internal(
                &ollama,
                &self.config.model,
                &self.objective,
                &seed.content,
            )
            .await;
            self.best_score = eval.score;
            self.candidates.push(Candidate::new(
                seed,
                eval.score,
                ASI::new().log(eval.feedback.clone()),
                0,
            ));
            emit(format!(
                r#"{{"iter":0,"best":{:.4},"improved":true,"feedback":"{}"}}"#,
                eval.score,
                eval.feedback.replace('"', "\\\"")
            ));
        }

        let mut no_improvement_streak = 0usize;

        for iter in 1..=self.config.max_iterations {
            self.iteration = iter;

            // ── select parents (top-k by score) ────────────────────────────────
            let mut ranked = self.candidates.clone();
            ranked.sort_by(|a, b| {
                b.score
                    .partial_cmp(&a.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            let elite_count = (self.config.population_size / 2).max(1);
            let parents: Vec<Candidate> = ranked.into_iter().take(elite_count).collect();

            let mut new_candidates: Vec<Candidate> = Vec::new();

            // ── mutation offspring ──────────────────────────────────────────────
            for parent in &parents {
                let feedback = parent
                    .asi
                    .text
                    .last()
                    .cloned()
                    .unwrap_or_else(|| "Improve quality and specificity.".into());

                let mutated = Self::mutate_internal(
                    &ollama,
                    &self.config.model,
                    &self.objective,
                    &parent.artifact.content,
                    &feedback,
                )
                .await;

                let mut eval = Self::evaluate_candidate_internal(
                    &ollama,
                    &self.config.model,
                    &self.objective,
                    &mutated,
                )
                .await;

                let final_content = eval.sharpened.take().unwrap_or(mutated);
                let artifact = Artifact::new(final_content)
                    .with_metadata("mode", self.mode.to_string())
                    .with_metadata("iter", iter.to_string());
                let asi = ASI::new()
                    .log(eval.feedback.clone())
                    .with_score("llm_judge", eval.score);

                new_candidates.push(
                    Candidate::new(artifact, eval.score, asi, iter)
                        .with_parents(vec![parent.artifact.id.clone()]),
                );
            }

            // ── crossover for MultiTask / Generalization modes ──────────────────
            if self.mode != OptimizationMode::SingleTask && parents.len() >= 2 {
                let a = &parents[0].artifact.content;
                let b = &parents[1].artifact.content;
                let crossover_prompt = format!(
                    "Merge the best parts of these two artifacts to satisfy: {}\n\
                     Artifact A:\n```\n{a}\n```\n\nArtifact B:\n```\n{b}\n```\n\n\
                     Return only the merged artifact.",
                    self.objective
                );
                let msgs = vec![ChatMessage::new(MessageRole::User, crossover_prompt)];
                let req = ChatMessageRequest::new(self.config.model.clone(), msgs);
                if let Ok(resp) = ollama.send_chat_messages(req).await {
                    let crossed = resp.message.content.trim().to_string();
                    let eval = Self::evaluate_candidate_internal(
                        &ollama,
                        &self.config.model,
                        &self.objective,
                        &crossed,
                    )
                    .await;
                    let artifact = Artifact::new(crossed)
                        .with_metadata("mode", "crossover")
                        .with_metadata("iter", iter.to_string());
                    let asi = ASI::new()
                        .log(eval.feedback.clone())
                        .with_score("llm_judge", eval.score);
                    new_candidates.push(
                        Candidate::new(artifact, eval.score, asi, iter).with_parents(
                            parents
                                .iter()
                                .take(2)
                                .map(|p| p.artifact.id.clone())
                                .collect(),
                        ),
                    );
                }
            }

            // ── merge offspring into pool, then prune ───────────────────────────
            for c in new_candidates {
                self.add_candidate(c);
            }
            self.candidates.sort_by(|a, b| {
                b.score
                    .partial_cmp(&a.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            self.candidates.truncate(self.config.population_size * 2);

            let current_best = self.candidates.first().map(|c| c.score).unwrap_or(0.0);
            let improved = current_best > self.best_score + self.config.improvement_threshold;

            if improved {
                self.best_score = current_best;
                no_improvement_streak = 0;
            } else {
                no_improvement_streak += 1;
            }

            let best_feedback = self
                .candidates
                .first()
                .and_then(|c| c.asi.text.last())
                .cloned()
                .unwrap_or_default();

            emit(format!(
                r#"{{"iter":{iter},"best":{:.4},"improved":{improved},"feedback":"{}"}}"#,
                self.best_score,
                best_feedback.replace('"', "\\\"")
            ));

            // ── early stopping ─────────────────────────────────────────────────
            if no_improvement_streak >= self.config.early_stopping_patience {
                emit(format!(
                    r#"{{"iter":{iter},"status":"early_stop","reason":"no improvement for {} iters","best":{:.4}}}"#,
                    self.config.early_stopping_patience, self.best_score
                ));
                break;
            }
        }

        // ── final summary ──────────────────────────────────────────────────────
        let winner_id = self
            .candidates
            .first()
            .map(|c| c.artifact.id.clone())
            .unwrap_or_else(|| "none".into());
        emit(format!(
            r#"{{"status":"done","iterations":{},"best":{:.4},"winner_id":"{}"}}"#,
            self.iteration, self.best_score, winner_id
        ));
    }

    // ── private LLM helpers ────────────────────────────────────────────────────

    async fn evaluate_candidate_internal(
        ollama: &ollama_rs::Ollama,
        model: &str,
        objective: &str,
        content: &str,
    ) -> EvalResult {
        use ollama_rs::generation::chat::request::ChatMessageRequest;
        use ollama_rs::generation::chat::{ChatMessage, MessageRole};

        let system = "You are a strict quality judge.\n\
            Score the artifact 0.0-1.0 on specificity, correctness, and clarity.\n\
            Reply ONLY in:\nSCORE: <float>\nFEEDBACK: <one line>\nSHARPENED: <improved version or NONE>";
        let user = format!("Objective: {objective}\n\nArtifact:\n```\n{content}\n```\n\nEvaluate.");
        let msgs = vec![
            ChatMessage::new(MessageRole::System, system.to_string()),
            ChatMessage::new(MessageRole::User, user),
        ];
        let req = ChatMessageRequest::new(model.to_string(), msgs);
        match ollama.send_chat_messages(req).await {
            Ok(r) => parse_eval_response(&r.message.content, content).unwrap_or(EvalResult {
                score: 0.5,
                sharpened: None,
                feedback: "parse error".into(),
            }),
            Err(e) => EvalResult {
                score: 0.5,
                sharpened: None,
                feedback: format!("eval error: {e}"),
            },
        }
    }

    async fn mutate_internal(
        ollama: &ollama_rs::Ollama,
        model: &str,
        objective: &str,
        content: &str,
        feedback: &str,
    ) -> String {
        use ollama_rs::generation::chat::request::ChatMessageRequest;
        use ollama_rs::generation::chat::{ChatMessage, MessageRole};

        let system = format!(
            "You are an expert optimizer. Your task: {objective}\n\
             Improve the artifact based on the feedback. \
             Return ONLY the improved artifact, no preamble."
        );
        let user = format!(
            "Feedback: {feedback}\n\nCurrent artifact:\n```\n{content}\n```\n\n\
             Rewrite it to fix the issues. Reply with the improved artifact only."
        );
        let msgs = vec![
            ChatMessage::new(MessageRole::System, system),
            ChatMessage::new(MessageRole::User, user),
        ];
        let req = ChatMessageRequest::new(model.to_string(), msgs);
        match ollama.send_chat_messages(req).await {
            Ok(r) => r.message.content.trim().to_string(),
            Err(_) => content.to_string(),
        }
    }
}

/// Create session from JSON configuration
pub fn session_from_json(json: &str) -> anyhow::Result<OptimizationSession> {
    let config: OptimizationConfig = serde_json::from_str(json)?;
    Ok(OptimizationSession::new(
        "Session from config",
        config,
        OptimizationMode::SingleTask,
    ))
}

/// Evaluate a council synthesis for quality without iteration.
/// Returns (score 0-1, sharpened_text if improved).
pub async fn evaluate_synthesis(
    synthesis: &str,
    question: &str,
    model: &str,
) -> anyhow::Result<EvalResult> {
    use ollama_rs::generation::chat::request::ChatMessageRequest;
    use ollama_rs::{
        generation::chat::{ChatMessage, MessageRole},
        Ollama,
    };

    let ollama = Ollama::new("http://localhost".to_string(), 11434);

    let system_prompt = "You are a quality evaluator for council syntheses.\n\
        Score the synthesis on:\n\
        1. Specificity (0-0.25): Does it make concrete claims with numbers/citations?\n\
        2. Falsifiability (0-0.25): Could it be proven wrong? Or is it vague fluff?\n\
        3. Groundedness (0-0.25): Does it cite actual evidence from the world data?\n\
        4. Actionability (0-0.25): Does it give a clear recommendation?\n\
        \n\
        Respond ONLY in this format:\n\
        SCORE: [0.0-1.0]\n\
        SHARPENED: [rewritten, more specific version if score < 0.8, else 'NONE']\n\
        FEEDBACK: [brief explanation of score]";

    let user_prompt = format!(
        "Original question: {}\n\n\
         Synthesis to evaluate:\n{}\n\n\
         Provide your evaluation.",
        question, synthesis
    );

    let messages = vec![
        ChatMessage::new(MessageRole::System, system_prompt.to_string()),
        ChatMessage::new(MessageRole::User, user_prompt.to_string()),
    ];

    let request = ChatMessageRequest::new(model.to_string(), messages);

    match ollama.send_chat_messages(request).await {
        Ok(response) => {
            let content = response.message.content;
            parse_eval_response(&content, synthesis)
        }
        Err(e) => Ok(EvalResult {
            score: 0.65,
            sharpened: None,
            feedback: format!("Evaluation failed ({}), using default score", e),
        }),
    }
}

fn parse_eval_response(content: &str, original: &str) -> anyhow::Result<EvalResult> {
    let mut score = 0.65;
    let mut sharpened = None;
    let mut feedback = "No feedback provided".to_string();

    for line in content.lines() {
        if line.starts_with("SCORE:") {
            if let Some(s) = line.split(':').nth(1) {
                score = s.trim().parse::<f64>().unwrap_or(0.65).clamp(0.0, 1.0);
            }
        } else if line.starts_with("SHARPENED:") {
            let s = line.split(':').nth(1).unwrap_or("").trim();
            if !s.is_empty() && s != "NONE" && s.len() > 10 {
                sharpened = Some(s.to_string());
            }
        } else if line.starts_with("FEEDBACK:") {
            feedback = line.split(':').nth(1).unwrap_or("").trim().to_string();
        }
    }

    if let Some(ref s) = sharpened {
        if s == original || s.len() < original.len() / 2 {
            sharpened = None;
        }
    }

    Ok(EvalResult {
        score,
        sharpened,
        feedback,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = OptimizationConfig::default();
        assert_eq!(config.max_iterations, 10);
        assert_eq!(config.population_size, 5);
    }

    #[test]
    fn test_parse_eval_response() {
        let response = "SCORE: 0.85\nSHARPENED: Better text\nFEEDBACK: Good";
        let result = parse_eval_response(response, "original").unwrap();
        assert!((result.score - 0.85).abs() < 0.01);
        assert_eq!(result.sharpened, Some("Better text".to_string()));
    }
}
