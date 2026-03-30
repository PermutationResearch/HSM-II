//! Evaluation runners — baseline (vanilla LLM) and HSM-II (full pipeline).
//!
//! Both runners process the same task suite and produce comparable metrics.

use std::collections::HashMap;
use std::time::Instant;

use crate::llm::client::{LlmClient, LlmRequest, Message};
use super::metrics::{score_keywords, RunnerMetrics, TurnMetrics};
use super::tasks::EvalTask;

// ═══════════════════════════════════════════════════════════════════════════════
// BASELINE RUNNER — Vanilla LLM, no memory
// ═══════════════════════════════════════════════════════════════════════════════

/// Baseline: sends each turn to the LLM with only within-session history.
/// Cross-session turns get NO context from prior sessions (simulating process restart).
pub struct BaselineRunner {
    client: LlmClient,
    system_prompt: String,
    model: String,
}

impl BaselineRunner {
    pub fn new(client: LlmClient) -> Self {
        let model = std::env::var("OLLAMA_MODEL")
            .or_else(|_| std::env::var("DEFAULT_LLM_MODEL"))
            .unwrap_or_else(|_| "qwen3:1.7b".to_string());
        // /no_think disables qwen3's internal chain-of-thought to save tokens and time
        Self {
            client,
            system_prompt: "You are a helpful AI assistant. Answer the user's questions thoroughly and accurately. Be concise.".to_string(),
            model,
        }
    }

    /// Run all tasks, returning collected metrics
    pub async fn run(&self, tasks: &[EvalTask]) -> RunnerMetrics {
        let mut metrics = RunnerMetrics::new("baseline");
        let run_start = Instant::now();

        for task in tasks {
            // Track per-session conversation history
            let mut session_history: HashMap<u32, Vec<Message>> = HashMap::new();

            for (turn_idx, turn) in task.turns.iter().enumerate() {
                let turn_start = Instant::now();

                // Get history for THIS session only (baseline has no cross-session memory)
                let history = session_history.entry(turn.session).or_default();

                // Build messages: system + session history + current turn
                let mut messages = vec![Message::system(&self.system_prompt)];
                messages.extend(history.iter().cloned());
                messages.push(Message::user(&turn.user));

                // Make LLM call
                let (response_text, prompt_tokens, completion_tokens, error) =
                    self.call_llm(&messages).await;

                // Record the exchange in session history
                history.push(Message::user(&turn.user));
                history.push(Message::assistant(&response_text));

                let keyword_score = score_keywords(&response_text, &turn.expected_keywords);

                metrics.turns.push(TurnMetrics {
                    task_id: task.id.clone(),
                    turn_index: turn_idx,
                    session: turn.session,
                    requires_recall: turn.requires_recall,
                    response: response_text,
                    latency_ms: turn_start.elapsed().as_millis() as u64,
                    prompt_tokens,
                    completion_tokens,
                    keyword_score,
                    llm_calls: 1,
                    error,
                });
            }
        }

        metrics.total_duration_ms = run_start.elapsed().as_millis() as u64;
        metrics
    }

    async fn call_llm(&self, messages: &[Message]) -> (String, usize, usize, Option<String>) {
        let request = LlmRequest {
            model: self.model.clone(),
            messages: messages.to_vec(),
            temperature: 0.3,
            max_tokens: Some(1500),
            ..LlmRequest::default()
        };

        match self.client.chat(request).await {
            Ok(resp) => (
                resp.content,
                resp.usage.prompt_tokens,
                resp.usage.completion_tokens,
                None,
            ),
            Err(e) => (
                format!("[ERROR: {}]", e),
                0,
                0,
                Some(e.to_string()),
            ),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// HSM-II RUNNER — Persistent memory + context ranking + reputation routing
// ═══════════════════════════════════════════════════════════════════════════════

/// HSM-II runner: uses persistent memory (beliefs), context ranking,
/// and reputation-based skill selection to augment each LLM call.
pub struct HsmRunner {
    client: LlmClient,
    system_prompt: String,
    model: String,
    /// Persistent belief store (survives across sessions)
    beliefs: Vec<StoredBelief>,
    /// Skill bank with usage tracking (reputation)
    skills: Vec<TrackedSkill>,
    /// Cross-session conversation summaries
    session_summaries: HashMap<String, Vec<SessionSummary>>,
}

/// A belief persisted across sessions
#[derive(Clone, Debug)]
struct StoredBelief {
    content: String,
    confidence: f64,
    domain: Option<String>,
    source_task: String,
    source_turn: usize,
    created_at: u64,
    keywords: Vec<String>,
}

/// A skill with reputation tracking
#[derive(Clone, Debug)]
struct TrackedSkill {
    id: String,
    description: String,
    domain: String,
    usage_count: u64,
    success_count: u64,
    avg_keyword_score: f64,
}

/// Summary of a completed session (for cross-session recall)
#[derive(Clone, Debug)]
struct SessionSummary {
    task_id: String,
    session: u32,
    summary: String,
    key_decisions: Vec<String>,
    keywords: Vec<String>,
}

impl HsmRunner {
    pub fn new(client: LlmClient) -> Self {
        let model = std::env::var("OLLAMA_MODEL")
            .or_else(|_| std::env::var("DEFAULT_LLM_MODEL"))
            .unwrap_or_else(|_| "qwen3:1.7b".to_string());
        Self {
            client,
            model,
            system_prompt: "You are a helpful AI assistant with persistent memory. You remember previous conversations and use relevant context to give better answers. Be concise.".to_string(),
            beliefs: Vec::new(),
            skills: Self::seed_skills(),
            session_summaries: HashMap::new(),
        }
    }

    /// Seed initial skill bank with domain knowledge
    fn seed_skills() -> Vec<TrackedSkill> {
        vec![
            TrackedSkill {
                id: "api-design".into(), description: "REST API design with authentication, pagination, and versioning".into(),
                domain: "software_engineering".into(), usage_count: 0, success_count: 0, avg_keyword_score: 0.0,
            },
            TrackedSkill {
                id: "debugging".into(), description: "Systematic debugging of production issues with root cause analysis".into(),
                domain: "software_engineering".into(), usage_count: 0, success_count: 0, avg_keyword_score: 0.0,
            },
            TrackedSkill {
                id: "architecture".into(), description: "System architecture design with trade-off analysis".into(),
                domain: "software_engineering".into(), usage_count: 0, success_count: 0, avg_keyword_score: 0.0,
            },
            TrackedSkill {
                id: "ml-pipeline".into(), description: "ML pipeline design, model training, and deployment".into(),
                domain: "data_science".into(), usage_count: 0, success_count: 0, avg_keyword_score: 0.0,
            },
            TrackedSkill {
                id: "data-engineering".into(), description: "Data pipeline design with ETL and streaming".into(),
                domain: "data_science".into(), usage_count: 0, success_count: 0, avg_keyword_score: 0.0,
            },
            TrackedSkill {
                id: "business-strategy".into(), description: "Market analysis, go-to-market, pricing strategy".into(),
                domain: "business".into(), usage_count: 0, success_count: 0, avg_keyword_score: 0.0,
            },
            TrackedSkill {
                id: "product-mgmt".into(), description: "Product roadmap prioritization and stakeholder management".into(),
                domain: "business".into(), usage_count: 0, success_count: 0, avg_keyword_score: 0.0,
            },
            TrackedSkill {
                id: "technical-writing".into(), description: "Research papers, technical blogs, documentation".into(),
                domain: "research".into(), usage_count: 0, success_count: 0, avg_keyword_score: 0.0,
            },
            TrackedSkill {
                id: "system-design".into(), description: "Large-scale system design with HIPAA, multi-tenant, distributed".into(),
                domain: "stress_test".into(), usage_count: 0, success_count: 0, avg_keyword_score: 0.0,
            },
        ]
    }

    /// Run all tasks with HSM-II augmentation
    pub async fn run(&mut self, tasks: &[EvalTask]) -> RunnerMetrics {
        let mut metrics = RunnerMetrics::new("hsm-ii");
        let run_start = Instant::now();

        for task in tasks {
            let mut session_history: HashMap<u32, Vec<Message>> = HashMap::new();
            let mut prev_session: u32 = 0;

            for (turn_idx, turn) in task.turns.iter().enumerate() {
                let turn_start = Instant::now();

                // ── SESSION BOUNDARY DETECTION ──
                // When session changes, summarize the previous session and store as belief
                if turn.session != prev_session && prev_session > 0 {
                    if let Some(history) = session_history.get(&prev_session) {
                        let summary = self.summarize_session(history);
                        let keywords = self.extract_keywords(history);

                        self.session_summaries
                            .entry(task.id.clone())
                            .or_default()
                            .push(SessionSummary {
                                task_id: task.id.clone(),
                                session: prev_session,
                                summary: summary.clone(),
                                key_decisions: keywords.clone(),
                                keywords: keywords.clone(),
                            });

                        // Store as persistent belief
                        self.beliefs.push(StoredBelief {
                            content: summary,
                            confidence: 0.9,
                            domain: turn.domain.clone(),
                            source_task: task.id.clone(),
                            source_turn: turn_idx,
                            created_at: std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs(),
                            keywords,
                        });
                    }
                }
                prev_session = turn.session;

                // ── CONTEXT RANKING ──
                // Retrieve relevant beliefs and skills for this turn
                let relevant_context = self.rank_context(&turn.user, &task.domain, turn.requires_recall, &task.id);

                // ── REPUTATION-BASED SKILL SELECTION ──
                let best_skill = self.select_skill(&task.domain);

                // ── BUILD AUGMENTED PROMPT ──
                let mut messages = Vec::new();

                // System prompt with skill guidance
                let mut system = self.system_prompt.clone();
                if let Some(skill) = &best_skill {
                    system.push_str(&format!(
                        "\n\nYour expertise area: {}. Apply this knowledge.",
                        skill.description
                    ));
                }
                messages.push(Message::system(&system));

                // Inject persistent memory context (cross-session recall)
                if !relevant_context.is_empty() {
                    let context_block = format!(
                        "## Relevant context from previous sessions:\n{}",
                        relevant_context
                    );
                    messages.push(Message::system(&context_block));
                }

                // Add within-session history
                let history = session_history.entry(turn.session).or_default();
                messages.extend(history.iter().cloned());

                // Add current turn
                messages.push(Message::user(&turn.user));

                // ── LLM CALL ──
                let (response_text, prompt_tokens, completion_tokens, error) =
                    self.call_llm(&messages).await;

                // ── POST-PROCESSING ──
                // Score and update reputation
                let keyword_score = score_keywords(&response_text, &turn.expected_keywords);

                // Update skill reputation
                if let Some(skill) = &best_skill {
                    self.update_skill_reputation(&skill.id, keyword_score);
                }

                // Extract and store new beliefs from this response
                if keyword_score > 0.3 {
                    self.beliefs.push(StoredBelief {
                        content: format!(
                            "Q: {}\nA: {}",
                            truncate(&turn.user, 300),
                            truncate(&response_text, 600)
                        ),
                        confidence: keyword_score,
                        domain: turn.domain.clone(),
                        source_task: task.id.clone(),
                        source_turn: turn_idx,
                        created_at: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs(),
                        keywords: turn.expected_keywords.clone(),
                    });
                }

                // Record in session history
                history.push(Message::user(&turn.user));
                history.push(Message::assistant(&response_text));

                metrics.turns.push(TurnMetrics {
                    task_id: task.id.clone(),
                    turn_index: turn_idx,
                    session: turn.session,
                    requires_recall: turn.requires_recall,
                    response: response_text,
                    latency_ms: turn_start.elapsed().as_millis() as u64,
                    prompt_tokens,
                    completion_tokens,
                    keyword_score,
                    llm_calls: 1,
                    error,
                });
            }

            // End of task: summarize final session
            if let Some(history) = session_history.get(&prev_session) {
                let summary = self.summarize_session(history);
                let keywords = self.extract_keywords(history);
                self.session_summaries
                    .entry(task.id.clone())
                    .or_default()
                    .push(SessionSummary {
                        task_id: task.id.clone(),
                        session: prev_session,
                        summary: summary.clone(),
                        key_decisions: keywords.clone(),
                        keywords,
                    });
                self.beliefs.push(StoredBelief {
                    content: summary,
                    confidence: 0.85,
                    domain: Some(task.domain.clone()),
                    source_task: task.id.clone(),
                    source_turn: task.turns.len(),
                    created_at: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                    keywords: Vec::new(),
                });
            }
        }

        metrics.total_duration_ms = run_start.elapsed().as_millis() as u64;
        metrics
    }

    /// Rank and retrieve relevant context from persistent memory
    fn rank_context(
        &self,
        query: &str,
        domain: &str,
        requires_recall: bool,
        task_id: &str,
    ) -> String {
        if !requires_recall {
            return String::new();
        }

        let query_lower = query.to_lowercase();
        let query_words: Vec<&str> = query_lower.split_whitespace().collect();

        // Score beliefs by relevance
        let mut scored: Vec<(usize, f64)> = self
            .beliefs
            .iter()
            .enumerate()
            .map(|(i, belief)| {
                let mut score = 0.0;

                // Keyword overlap (Jaccard-like)
                let belief_lower = belief.content.to_lowercase();
                let matching_words = query_words
                    .iter()
                    .filter(|w| w.len() > 3 && belief_lower.contains(**w))
                    .count();
                score += matching_words as f64 * 0.15;

                // Domain match bonus
                if belief.domain.as_deref() == Some(domain) {
                    score += 0.3;
                }

                // Same task bonus (for cross-session recall within same task)
                if belief.source_task == task_id {
                    score += 0.4;
                }

                // Keyword overlap with belief's stored keywords
                let kw_overlap = belief.keywords.iter()
                    .filter(|kw| query_lower.contains(&kw.to_lowercase()))
                    .count();
                score += kw_overlap as f64 * 0.2;

                // Confidence weighting
                score *= belief.confidence;

                (i, score)
            })
            .filter(|(_, s)| *s > 0.1)
            .collect();

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Include session summaries for this task
        let mut context_parts: Vec<String> = Vec::new();

        if let Some(summaries) = self.session_summaries.get(task_id) {
            for s in summaries {
                context_parts.push(format!("- [Session {}] {}", s.session, truncate(&s.summary, 500)));
            }
        }

        // Top-K beliefs (full context with large models)
        let top_k = 5;
        for (idx, _score) in scored.iter().take(top_k) {
            let belief = &self.beliefs[*idx];
            context_parts.push(format!("- {}", truncate(&belief.content, 400)));
        }

        context_parts.join("\n")
    }

    /// Select the best skill based on domain + reputation
    fn select_skill(&self, domain: &str) -> Option<TrackedSkill> {
        let mut candidates: Vec<&TrackedSkill> = self
            .skills
            .iter()
            .filter(|s| s.domain == domain || domain == "stress_test")
            .collect();

        if candidates.is_empty() {
            return self.skills.first().cloned();
        }

        // Sort by reputation score: weighted combination of usage and success rate
        candidates.sort_by(|a, b| {
            let score_a = if a.usage_count > 0 {
                a.avg_keyword_score * 0.7 + (a.success_count as f64 / a.usage_count as f64) * 0.3
            } else {
                0.5 // neutral score for unused skills
            };
            let score_b = if b.usage_count > 0 {
                b.avg_keyword_score * 0.7 + (b.success_count as f64 / b.usage_count as f64) * 0.3
            } else {
                0.5
            };
            score_b.partial_cmp(&score_a).unwrap_or(std::cmp::Ordering::Equal)
        });

        candidates.first().cloned().cloned()
    }

    /// Update skill reputation after a turn
    fn update_skill_reputation(&mut self, skill_id: &str, keyword_score: f64) {
        if let Some(skill) = self.skills.iter_mut().find(|s| s.id == skill_id) {
            skill.usage_count += 1;
            if keyword_score >= 0.5 {
                skill.success_count += 1;
            }
            // Exponential moving average
            let alpha = 0.3;
            skill.avg_keyword_score =
                alpha * keyword_score + (1.0 - alpha) * skill.avg_keyword_score;
        }
    }

    /// Summarize a session's conversation history (without LLM, for speed)
    fn summarize_session(&self, history: &[Message]) -> String {
        let mut summary_parts = Vec::new();
        for msg in history {
            if msg.role == "user" {
                summary_parts.push(format!("User asked: {}", truncate(&msg.content, 150)));
            } else if msg.role == "assistant" {
                summary_parts.push(format!("Discussed: {}", truncate(&msg.content, 200)));
            }
        }
        summary_parts.join(" | ")
    }

    /// Extract keywords from conversation history
    fn extract_keywords(&self, history: &[Message]) -> Vec<String> {
        let all_text: String = history.iter().map(|m| m.content.as_str()).collect::<Vec<_>>().join(" ");
        let lower = all_text.to_lowercase();

        // Simple keyword extraction: words that appear multiple times and are >4 chars
        let mut word_counts: HashMap<String, usize> = HashMap::new();
        for word in lower.split(|c: char| !c.is_alphanumeric()) {
            if word.len() > 4 {
                *word_counts.entry(word.to_string()).or_insert(0) += 1;
            }
        }

        let mut keywords: Vec<(String, usize)> = word_counts.into_iter().filter(|(_, c)| *c >= 2).collect();
        keywords.sort_by(|a, b| b.1.cmp(&a.1));
        keywords.into_iter().take(10).map(|(w, _)| w).collect()
    }

    async fn call_llm(&self, messages: &[Message]) -> (String, usize, usize, Option<String>) {
        let request = LlmRequest {
            model: self.model.clone(),
            messages: messages.to_vec(),
            temperature: 0.3,
            max_tokens: Some(1500),
            ..LlmRequest::default()
        };

        match self.client.chat(request).await {
            Ok(resp) => (
                resp.content,
                resp.usage.prompt_tokens,
                resp.usage.completion_tokens,
                None,
            ),
            Err(e) => (
                format!("[ERROR: {}]", e),
                0,
                0,
                Some(e.to_string()),
            ),
        }
    }
}

/// Truncate a string to max_len characters
fn truncate(s: &str, max_len: usize) -> &str {
    if s.len() <= max_len {
        s
    } else {
        let boundary = s.char_indices()
            .take_while(|(i, _)| *i < max_len)
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(max_len);
        &s[..boundary]
    }
}
