//! Evaluation runners — baseline (vanilla LLM) and HSM-II (full pipeline).
//!
//! Both runners process the same task suite and produce comparable metrics.

use std::collections::HashMap;
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::llm::client::{LlmClient, LlmRequest, Message};
use crate::personal::prompt_defaults::LIVING_PROMPT_SEED;
use super::judges;
use super::metrics::{score_keywords, turn_rubric_composite, RunnerMetrics, TurnMetrics};
use super::tasks::{EvalTask, Turn};
use super::trace::{BeliefRankEntry, HsmTurnTrace, RankedContextResult};

const BASELINE_EVAL_SYSTEM: &str = "You are a helpful AI assistant. Answer the user's questions thoroughly and accurately. Be concise.";

async fn finalize_turn_metrics(
    client: &LlmClient,
    model: &str,
    task_id: String,
    turn_idx: usize,
    turn: &Turn,
    response: String,
    latency_ms: u64,
    prompt_tokens: usize,
    completion_tokens: usize,
    error: Option<String>,
    injected_memory_context: &str,
) -> TurnMetrics {
    let keyword_score = score_keywords(&response, &turn.expected_keywords);
    let mut extras = judges::evaluate_turn_rubric(turn, &response, injected_memory_context, keyword_score);
    let mut pt = prompt_tokens;
    let mut ct = completion_tokens;
    let mut judge_calls = 0u32;
    if judges::llm_judge_enabled() && !judges::rubric_turn_pass(&extras) {
        if let Ok((pass, note, jpt, jct, jc)) =
            judges::llm_judge_turn(client, model, turn, &response).await
        {
            extras.llm_judge_pass = pass;
            extras.llm_judge_notes = note;
            extras.judge_prompt_tokens = jpt;
            extras.judge_completion_tokens = jct;
            extras.judge_llm_calls = jc;
            pt += jpt;
            ct += jct;
            judge_calls = jc;
        }
    }
    let rubric_composite = turn_rubric_composite(keyword_score, &extras);
    let rubric_pass = judges::rubric_turn_pass_with_llm(&extras);
    let http = 1u32 + judge_calls;
    TurnMetrics {
        task_id,
        turn_index: turn_idx,
        session: turn.session,
        requires_recall: turn.requires_recall,
        response,
        latency_ms,
        prompt_tokens: pt,
        completion_tokens: ct,
        keyword_score,
        llm_calls: http,
        error,
        deterministic_pass: extras.deterministic_pass,
        rubric_pass,
        rubric_composite,
        grounding_applicable: extras.grounding_applicable,
        grounding_score: extras.grounding_score,
        grounding_pass: extras.grounding_pass,
        tool_check_applicable: extras.tool_check_applicable,
        tool_pass: extras.tool_pass,
        llm_judge_pass: extras.llm_judge_pass,
        llm_judge_notes: extras.llm_judge_notes.clone(),
        wall_clock_ms: latency_ms,
        llm_http_requests: http,
    }
}

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
            system_prompt: BASELINE_EVAL_SYSTEM.to_string(),
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
                let latency_ms = turn_start.elapsed().as_millis() as u64;

                history.push(Message::user(&turn.user));
                let tm = finalize_turn_metrics(
                    &self.client,
                    &self.model,
                    task.id.clone(),
                    turn_idx,
                    turn,
                    response_text,
                    latency_ms,
                    prompt_tokens,
                    completion_tokens,
                    error,
                    "",
                )
                .await;
                history.push(Message::assistant(tm.response.clone()));
                metrics.turns.push(tm);
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

/// Tunable harness policy for meta-search (see [`HsmRunnerConfig`]).
pub trait HarnessPolicy: Clone + Send + Sync {
    fn runner_config(&self) -> &HsmRunnerConfig;
}

impl HarnessPolicy for HsmRunnerConfig {
    fn runner_config(&self) -> &HsmRunnerConfig {
        self
    }
}

/// HSM-II runner: uses persistent memory (beliefs), context ranking,
/// and reputation-based skill selection to augment each LLM call.
pub struct HsmRunner<P: HarnessPolicy = HsmRunnerConfig> {
    client: LlmClient,
    system_prompt: String,
    model: String,
    /// Persistent belief store (survives across sessions)
    beliefs: Vec<StoredBelief>,
    /// Skill bank with usage tracking (reputation)
    skills: Vec<TrackedSkill>,
    /// Cross-session conversation summaries
    session_summaries: HashMap<String, Vec<SessionSummary>>,
    policy: P,
    /// When true, each HSM turn appends to `traces` (for outer-loop / proposer feedback).
    collect_traces: bool,
    traces: Vec<HsmTurnTrace>,
}

/// Tunable HSM harness knobs used by meta-harness search.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HsmRunnerConfig {
    pub context_top_k: usize,
    pub context_score_threshold: f64,
    pub skill_success_threshold: f64,
    pub skill_reputation_alpha: f64,
    pub store_belief_min_score: f64,
    pub context_char_budget: usize,
    pub include_session_summaries: bool,
    pub query_overlap_weight: f64,
    pub domain_match_bonus: f64,
    pub same_task_bonus: f64,
    pub belief_keyword_overlap_weight: f64,
    pub llm_temperature: f64,
    pub llm_max_tokens: usize,
}

impl Default for HsmRunnerConfig {
    fn default() -> Self {
        Self {
            context_top_k: 5,
            context_score_threshold: 0.1,
            skill_success_threshold: 0.5,
            skill_reputation_alpha: 0.3,
            store_belief_min_score: 0.3,
            context_char_budget: 3000,
            include_session_summaries: true,
            query_overlap_weight: 0.15,
            domain_match_bonus: 0.3,
            same_task_bonus: 0.4,
            belief_keyword_overlap_weight: 0.2,
            llm_temperature: 0.3,
            llm_max_tokens: 1500,
        }
    }
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

impl HsmRunner<HsmRunnerConfig> {
    pub fn new(client: LlmClient) -> Self {
        Self::with_policy(client, HsmRunnerConfig::default())
    }

    /// Backwards-compatible alias for [`HsmRunner::with_policy`].
    pub fn with_config(client: LlmClient, config: HsmRunnerConfig) -> Self {
        HsmRunner::with_policy(client, config)
    }
}

impl<P: HarnessPolicy> HsmRunner<P> {
    #[inline]
    fn cfg(&self) -> &HsmRunnerConfig {
        self.policy.runner_config()
    }

    pub fn with_policy(client: LlmClient, policy: P) -> Self {
        let model = std::env::var("OLLAMA_MODEL")
            .or_else(|_| std::env::var("DEFAULT_LLM_MODEL"))
            .unwrap_or_else(|_| "qwen3:1.7b".to_string());
        Self {
            client,
            model,
            system_prompt: format!(
                "You are a helpful AI assistant with persistent memory. You remember previous conversations and use relevant context to give better answers. Be concise.\n\n{}",
                LIVING_PROMPT_SEED
            ),
            beliefs: Vec::new(),
            skills: Self::seed_skills(),
            session_summaries: HashMap::new(),
            policy,
            collect_traces: false,
            traces: Vec::new(),
        }
    }

    /// Enable per-turn HSM traces (retrieval ranks, skill, context preview). Clears any previous traces when set to true.
    pub fn set_collect_traces(&mut self, on: bool) {
        self.collect_traces = on;
        if on {
            self.traces.clear();
        }
    }

    /// Take accumulated traces and reset the buffer.
    pub fn take_traces(&mut self) -> Vec<HsmTurnTrace> {
        std::mem::take(&mut self.traces)
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
                let ctx = self.build_ranked_context(
                    &turn.user,
                    &task.domain,
                    turn.requires_recall,
                    &task.id,
                );

                // ── REPUTATION-BASED SKILL SELECTION ──
                let best_skill = self.select_skill(&task.domain);

                if self.collect_traces {
                    self.traces.push(HsmTurnTrace {
                        task_id: task.id.clone(),
                        turn_index: turn_idx,
                        session: turn.session,
                        requires_recall: turn.requires_recall,
                        selected_skill_id: best_skill.as_ref().map(|s| s.id.clone()),
                        selected_skill_domain: best_skill.as_ref().map(|s| s.domain.clone()),
                        belief_ranks: ctx.belief_ranks.clone(),
                        session_summaries_injected: ctx.session_summary_sessions.clone(),
                        injected_char_len: ctx.injected_text.len(),
                        injected_preview: ctx.injected_text.chars().take(800).collect::<String>(),
                    });
                }

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
                if !ctx.injected_text.is_empty() {
                    let context_block = format!(
                        "## Relevant context from previous sessions:\n{}",
                        ctx.injected_text
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
                let latency_ms = turn_start.elapsed().as_millis() as u64;

                let tm = finalize_turn_metrics(
                    &self.client,
                    &self.model,
                    task.id.clone(),
                    turn_idx,
                    turn,
                    response_text,
                    latency_ms,
                    prompt_tokens,
                    completion_tokens,
                    error,
                    &ctx.injected_text,
                )
                .await;

                // Update skill reputation
                if let Some(skill) = &best_skill {
                    self.update_skill_reputation(&skill.id, tm.keyword_score);
                }

                // Extract and store new beliefs from this response
                if tm.keyword_score > self.cfg().store_belief_min_score {
                    self.beliefs.push(StoredBelief {
                        content: format!(
                            "Q: {}\nA: {}",
                            truncate(&turn.user, 300),
                            truncate(&tm.response, 600)
                        ),
                        confidence: tm.keyword_score,
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

                history.push(Message::user(&turn.user));
                history.push(Message::assistant(tm.response.clone()));
                metrics.turns.push(tm);
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

    /// Rank beliefs and session summaries; build injected context block + trace metadata.
    fn build_ranked_context(
        &self,
        query: &str,
        domain: &str,
        requires_recall: bool,
        task_id: &str,
    ) -> RankedContextResult {
        if !requires_recall {
            return RankedContextResult::empty();
        }

        let query_lower = query.to_lowercase();
        let query_words: Vec<&str> = query_lower.split_whitespace().collect();

        let mut scored: Vec<(usize, f64)> = self
            .beliefs
            .iter()
            .enumerate()
            .map(|(i, belief)| {
                let mut score = 0.0;

                let belief_lower = belief.content.to_lowercase();
                let matching_words = query_words
                    .iter()
                    .filter(|w| w.len() > 3 && belief_lower.contains(**w))
                    .count();
                score += matching_words as f64 * self.cfg().query_overlap_weight;

                if belief.domain.as_deref() == Some(domain) {
                    score += self.cfg().domain_match_bonus;
                }

                if belief.source_task == task_id {
                    score += self.cfg().same_task_bonus;
                }

                let kw_overlap = belief
                    .keywords
                    .iter()
                    .filter(|kw| query_lower.contains(&kw.to_lowercase()))
                    .count();
                score += kw_overlap as f64 * self.cfg().belief_keyword_overlap_weight;

                score *= belief.confidence;

                (i, score)
            })
            .filter(|(_, s)| *s > self.cfg().context_score_threshold)
            .collect();

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let mut session_summary_sessions = Vec::new();
        let mut context_parts: Vec<String> = Vec::new();

        if self.cfg().include_session_summaries {
            if let Some(summaries) = self.session_summaries.get(task_id) {
                for s in summaries {
                    session_summary_sessions.push(s.session);
                    context_parts.push(format!(
                        "- [Session {}] {}",
                        s.session,
                        truncate(&s.summary, 500)
                    ));
                }
            }
        }

        let mut belief_ranks: Vec<BeliefRankEntry> = Vec::new();
        for (idx, score) in scored.iter().take(self.cfg().context_top_k) {
            let belief = &self.beliefs[*idx];
            belief_ranks.push(BeliefRankEntry {
                belief_index: *idx,
                score: *score,
                source_task: belief.source_task.clone(),
                preview: truncate(&belief.content, 400).to_string(),
            });
            context_parts.push(format!("- {}", truncate(&belief.content, 400)));
        }

        let joined = context_parts.join("\n");
        let injected_text = if joined.len() > self.cfg().context_char_budget {
            truncate(&joined, self.cfg().context_char_budget).to_string()
        } else {
            joined
        };

        RankedContextResult {
            injected_text,
            belief_ranks,
            session_summary_sessions: session_summary_sessions,
        }
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
        let thr = self.cfg().skill_success_threshold;
        let alpha = self.cfg().skill_reputation_alpha;
        if let Some(skill) = self.skills.iter_mut().find(|s| s.id == skill_id) {
            skill.usage_count += 1;
            if keyword_score >= thr {
                skill.success_count += 1;
            }
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
            temperature: self.cfg().llm_temperature,
            max_tokens: Some(self.cfg().llm_max_tokens),
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
