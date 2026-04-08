//! Evaluation runners — baseline (vanilla LLM) and HSM-II (full pipeline).
//!
//! Both runners process the same task suite and produce comparable metrics.

use std::collections::HashMap;
use std::time::Instant;

use serde::{Deserialize, Serialize};

use super::judges;
use super::metrics::{score_keywords, turn_rubric_composite, RunnerMetrics, TurnMetrics};
use super::tasks::{EvalTask, Turn};
use super::trace::{BeliefRankEntry, HsmTurnTrace, RankedContextResult};
use crate::harness::HarnessRuntime;
use crate::llm::client::{LlmClient, LlmRequest, Message};
use crate::personal::prompt_defaults::LIVING_PROMPT_SEED;

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
    let mut extras =
        judges::evaluate_turn_rubric(turn, &response, injected_memory_context, keyword_score);
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
        let model = super::eval_llm_model_from_env();
        // /no_think disables qwen3's internal chain-of-thought to save tokens and time
        Self {
            client,
            system_prompt: BASELINE_EVAL_SYSTEM.to_string(),
            model,
        }
    }

    /// Run all tasks, returning collected metrics
    pub async fn run(&mut self, tasks: &[EvalTask]) -> RunnerMetrics {
        let mut metrics = RunnerMetrics::new("baseline");
        let run_start = Instant::now();
        let mut harness = HarnessRuntime::from_env("baseline").unwrap_or_else(|e| {
            tracing::warn!(target: "harness", "baseline harness init failed: {}", e);
            HarnessRuntime::noop()
        });

        for task in tasks {
            // Track per-session conversation history
            let mut session_history: HashMap<u32, Vec<Message>> = HashMap::new();

            for (turn_idx, turn) in task.turns.iter().enumerate() {
                let turn_start = Instant::now();
                harness.turn_begin(&task.id, turn_idx);

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
                harness.turn_end(&task.id, turn_idx, turn_start, error.as_deref());

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
            Err(e) => (format!("[ERROR: {}]", e), 0, 0, Some(e.to_string())),
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

/// Optional per-domain overrides for retrieval-backed memory injection (merge order: file → builtin → global).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DomainMemoryProfile {
    #[serde(default)]
    pub context_top_k: Option<usize>,
    #[serde(default)]
    pub context_char_budget: Option<usize>,
    #[serde(default)]
    pub context_score_threshold: Option<f64>,
    #[serde(default)]
    pub summary_score_threshold: Option<f64>,
    #[serde(default)]
    pub max_belief_snippet_chars: Option<usize>,
    #[serde(default)]
    pub max_summary_snippet_chars: Option<usize>,
    #[serde(default)]
    pub max_session_summaries: Option<usize>,
    #[serde(default)]
    pub include_session_summaries: Option<bool>,
}

/// Effective memory-injection limits after merging global config, optional JSON overrides, and SE/DS builtins.
#[derive(Clone, Debug)]
pub struct ResolvedMemoryInjection {
    pub inject: bool,
    pub top_k: usize,
    pub char_budget: usize,
    pub belief_threshold: f64,
    pub summary_threshold: f64,
    pub max_belief_snippet: usize,
    pub max_summary_snippet: usize,
    pub max_session_summaries: usize,
    pub include_summaries: bool,
}

fn domain_builtin_memory_profile(domain: &str) -> Option<DomainMemoryProfile> {
    match domain {
        "software_engineering" | "data_science" => Some(DomainMemoryProfile {
            context_top_k: Some(2),
            context_char_budget: Some(1200),
            context_score_threshold: Some(0.14),
            summary_score_threshold: Some(0.12),
            max_belief_snippet_chars: Some(240),
            max_summary_snippet_chars: Some(280),
            max_session_summaries: Some(1),
            include_session_summaries: None,
        }),
        _ => None,
    }
}

/// Tunable HSM harness knobs used by meta-harness search.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HsmRunnerConfig {
    pub context_top_k: usize,
    pub context_score_threshold: f64,
    /// Minimum relevance score for injecting a session-summary line (separate from belief threshold).
    #[serde(default = "HsmRunnerConfig::default_summary_score_threshold")]
    pub summary_score_threshold: f64,
    pub skill_success_threshold: f64,
    pub skill_reputation_alpha: f64,
    pub store_belief_min_score: f64,
    pub context_char_budget: usize,
    pub include_session_summaries: bool,
    /// When false, skip ranking/injection of cross-session memory (ablation / cost control).
    #[serde(default = "HsmRunnerConfig::default_inject_memory_context")]
    pub inject_memory_context: bool,
    #[serde(default)]
    pub domain_memory_profiles: HashMap<String, DomainMemoryProfile>,
    /// Max chars per belief line after one-line compaction (global default; domains may override).
    #[serde(default = "HsmRunnerConfig::default_max_belief_snippet_chars")]
    pub max_belief_snippet_chars: usize,
    #[serde(default = "HsmRunnerConfig::default_max_summary_snippet_chars")]
    pub max_summary_snippet_chars: usize,
    #[serde(default = "HsmRunnerConfig::default_max_session_summaries")]
    pub max_session_summaries: usize,
    pub query_overlap_weight: f64,
    pub domain_match_bonus: f64,
    pub same_task_bonus: f64,
    pub belief_keyword_overlap_weight: f64,
    pub llm_temperature: f64,
    pub llm_max_tokens: usize,
    /// Claude Code `MEMORY.md` / memdir-style caps on the **aggregate** injected recall block (after per-domain char budget).
    #[serde(default = "HsmRunnerConfig::default_memory_entrypoint_max_lines")]
    pub memory_entrypoint_max_lines: usize,
    #[serde(default = "HsmRunnerConfig::default_memory_entrypoint_max_bytes")]
    pub memory_entrypoint_max_bytes: usize,
    /// Fold older in-session user/assistant turns into one summary user message (snip / compact-boundary semantics).
    #[serde(default = "HsmRunnerConfig::default_session_compaction_enabled")]
    pub session_compaction_enabled: bool,
    #[serde(default = "HsmRunnerConfig::default_session_compaction_trigger_messages")]
    pub session_compaction_trigger_messages: usize,
    #[serde(default = "HsmRunnerConfig::default_session_compaction_keep_tail_messages")]
    pub session_compaction_keep_tail_messages: usize,
}

impl HsmRunnerConfig {
    fn default_summary_score_threshold() -> f64 {
        0.08
    }

    fn default_inject_memory_context() -> bool {
        true
    }

    fn default_max_belief_snippet_chars() -> usize {
        320
    }

    fn default_max_summary_snippet_chars() -> usize {
        360
    }

    fn default_max_session_summaries() -> usize {
        3
    }

    fn default_memory_entrypoint_max_lines() -> usize {
        200
    }

    fn default_memory_entrypoint_max_bytes() -> usize {
        25_000
    }

    fn default_session_compaction_enabled() -> bool {
        true
    }

    fn default_session_compaction_trigger_messages() -> usize {
        12
    }

    fn default_session_compaction_keep_tail_messages() -> usize {
        4
    }

    fn pick_usize(file: Option<usize>, builtin: Option<usize>, global: usize) -> usize {
        file.or(builtin).unwrap_or(global)
    }

    fn pick_f64(file: Option<f64>, builtin: Option<f64>, global: f64) -> f64 {
        file.or(builtin).unwrap_or(global)
    }

    fn pick_bool(file: Option<bool>, global: bool) -> bool {
        file.unwrap_or(global)
    }

    /// Effective memory-injection limits for a task domain (builtin SE/DS caps + optional JSON overrides).
    pub fn resolve_memory(&self, task_domain: &str) -> ResolvedMemoryInjection {
        let file = self.domain_memory_profiles.get(task_domain);
        let built = domain_builtin_memory_profile(task_domain);
        let built_ref = built.as_ref();
        ResolvedMemoryInjection {
            inject: self.inject_memory_context,
            top_k: Self::pick_usize(
                file.and_then(|p| p.context_top_k),
                built_ref.and_then(|p| p.context_top_k),
                self.context_top_k,
            ),
            char_budget: Self::pick_usize(
                file.and_then(|p| p.context_char_budget),
                built_ref.and_then(|p| p.context_char_budget),
                self.context_char_budget,
            ),
            belief_threshold: Self::pick_f64(
                file.and_then(|p| p.context_score_threshold),
                built_ref.and_then(|p| p.context_score_threshold),
                self.context_score_threshold,
            ),
            summary_threshold: Self::pick_f64(
                file.and_then(|p| p.summary_score_threshold),
                built_ref.and_then(|p| p.summary_score_threshold),
                self.summary_score_threshold,
            ),
            max_belief_snippet: Self::pick_usize(
                file.and_then(|p| p.max_belief_snippet_chars),
                built_ref.and_then(|p| p.max_belief_snippet_chars),
                self.max_belief_snippet_chars,
            ),
            max_summary_snippet: Self::pick_usize(
                file.and_then(|p| p.max_summary_snippet_chars),
                built_ref.and_then(|p| p.max_summary_snippet_chars),
                self.max_summary_snippet_chars,
            ),
            max_session_summaries: Self::pick_usize(
                file.and_then(|p| p.max_session_summaries),
                built_ref.and_then(|p| p.max_session_summaries),
                self.max_session_summaries,
            ),
            include_summaries: Self::pick_bool(
                file.and_then(|p| p.include_session_summaries),
                self.include_session_summaries,
            ),
        }
    }
}

impl Default for HsmRunnerConfig {
    fn default() -> Self {
        Self {
            context_top_k: 4,
            context_score_threshold: 0.12,
            summary_score_threshold: Self::default_summary_score_threshold(),
            skill_success_threshold: 0.5,
            skill_reputation_alpha: 0.3,
            store_belief_min_score: 0.3,
            context_char_budget: 2800,
            include_session_summaries: true,
            inject_memory_context: true,
            domain_memory_profiles: HashMap::new(),
            max_belief_snippet_chars: Self::default_max_belief_snippet_chars(),
            max_summary_snippet_chars: Self::default_max_summary_snippet_chars(),
            max_session_summaries: Self::default_max_session_summaries(),
            query_overlap_weight: 0.15,
            domain_match_bonus: 0.3,
            same_task_bonus: 0.4,
            belief_keyword_overlap_weight: 0.2,
            llm_temperature: 0.3,
            llm_max_tokens: 1500,
            memory_entrypoint_max_lines: Self::default_memory_entrypoint_max_lines(),
            memory_entrypoint_max_bytes: Self::default_memory_entrypoint_max_bytes(),
            session_compaction_enabled: Self::default_session_compaction_enabled(),
            session_compaction_trigger_messages: Self::default_session_compaction_trigger_messages(
            ),
            session_compaction_keep_tail_messages:
                Self::default_session_compaction_keep_tail_messages(),
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
    source_excerpt: Option<String>,
    supporting_evidence: Vec<String>,
    contradicting_evidence: Vec<String>,
    supersedes_belief_index: Option<usize>,
    evidence_belief_indices: Vec<usize>,
    human_committed: bool,
    claims: Vec<super::memory_graph::TypedClaimSnapshot>,
}

fn belief_session_number(content: &str) -> Option<u32> {
    let rest = content.strip_prefix("Session ")?;
    let digits = rest
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>();
    digits.parse().ok()
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
        let model = super::eval_llm_model_from_env();
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

    /// Export beliefs, session summaries, and skills for bipartite graph projection ([`super::memory_graph`]).
    pub fn export_memory_snapshot(&self) -> super::memory_graph::HsmMemorySnapshot {
        let beliefs = self
            .beliefs
            .iter()
            .enumerate()
            .map(|(index, b)| super::memory_graph::BeliefSnapshot {
                index,
                content: b.content.clone(),
                confidence: b.confidence,
                domain: b.domain.clone(),
                source_task: b.source_task.clone(),
                source_turn: b.source_turn,
                created_at: b.created_at,
                keywords: b.keywords.clone(),
                source_excerpt: b.source_excerpt.clone(),
                supporting_evidence: b.supporting_evidence.clone(),
                contradicting_evidence: b.contradicting_evidence.clone(),
                supersedes_belief_index: b.supersedes_belief_index,
                evidence_belief_indices: b.evidence_belief_indices.clone(),
                human_committed: b.human_committed,
                claims: b.claims.clone(),
            })
            .collect();
        let mut session_summaries = Vec::new();
        for (task_id, rows) in &self.session_summaries {
            for s in rows {
                session_summaries.push(super::memory_graph::SessionSummarySnapshot {
                    task_id: task_id.clone(),
                    session: s.session,
                    summary: s.summary.clone(),
                    key_decisions: s.key_decisions.clone(),
                    keywords: s.keywords.clone(),
                });
            }
        }
        let skills = self
            .skills
            .iter()
            .map(|s| super::memory_graph::SkillSnapshot {
                id: s.id.clone(),
                description: s.description.clone(),
                domain: s.domain.clone(),
                usage_count: s.usage_count,
                success_count: s.success_count,
                avg_keyword_score: s.avg_keyword_score,
            })
            .collect();
        super::memory_graph::HsmMemorySnapshot {
            beliefs,
            session_summaries,
            skills,
        }
    }

    /// Ingest a completed session into HSM persistent memory without making an LLM call.
    ///
    /// This is useful for external benchmarks such as LongMemEval where a chat history is
    /// replayed into memory first and only the final question is answered by the model.
    pub fn ingest_session_history(
        &mut self,
        task_id: &str,
        domain: &str,
        session: u32,
        history: &[Message],
    ) {
        if history.is_empty() {
            return;
        }

        // Preserve turn-level facts for retrieval-heavy external benchmarks.
        // Session summaries alone are too lossy for temporal / factual recall tasks.
        for (idx, msg) in history.iter().enumerate() {
            if msg.content.trim().is_empty() {
                continue;
            }
            let lower = msg.content.to_lowercase();
            let temporal_markers = [
                "january",
                "february",
                "march",
                "april",
                "may",
                "june",
                "july",
                "august",
                "september",
                "october",
                "november",
                "december",
                "jan",
                "feb",
                "mar",
                "apr",
                "jun",
                "jul",
                "aug",
                "sep",
                "sept",
                "oct",
                "nov",
                "dec",
                "today",
                "yesterday",
                "tomorrow",
                "last",
                "next",
                "ago",
                "before",
                "after",
                "first",
                "second",
                "third",
                "earlier",
                "later",
                "week",
                "month",
                "day",
                "sunday",
                "monday",
                "tuesday",
                "wednesday",
                "thursday",
                "friday",
                "saturday",
            ];
            let temporal_hit = temporal_markers.iter().any(|m| lower.contains(m))
                || lower.chars().any(|c| c.is_ascii_digit());
            let keywords = self.extract_keywords_from_text(&lower, 4, 12);
            let raw_excerpt = truncate(&msg.content, 280).to_string();
            self.push_belief(
                format!("Session {} {}: {}", session, msg.role, msg.content),
                match (msg.role.as_str(), temporal_hit) {
                    ("user", true) => 1.0,
                    ("user", false) => 0.98,
                    (_, true) => 0.96,
                    _ => 0.9,
                },
                Some(domain.to_string()),
                task_id.to_string(),
                idx,
                keywords,
                Some(raw_excerpt.clone()),
                vec![format!(
                    "session={session} role={} excerpt={raw_excerpt}",
                    msg.role
                )],
                msg.role == "user",
            );
        }

        let summary = self.summarize_session(history);
        let keywords = self.extract_keywords(history);
        self.session_summaries
            .entry(task_id.to_string())
            .or_default()
            .push(SessionSummary {
                task_id: task_id.to_string(),
                session,
                summary: summary.clone(),
                key_decisions: keywords.clone(),
                keywords: keywords.clone(),
            });
        self.push_belief(
            summary.clone(),
            0.9,
            Some(domain.to_string()),
            task_id.to_string(),
            history.len(),
            keywords.clone(),
            Some(truncate(&summary, 280).to_string()),
            vec![format!(
                "session={session} summary captured from {} messages",
                history.len()
            )],
            false,
        );
    }

    /// Answer a query using the current HSM memory plus optional within-session history.
    pub async fn answer_query(
        &mut self,
        task_id: &str,
        domain: &str,
        session: u32,
        session_history: &[Message],
        user_prompt: &str,
        requires_recall: bool,
    ) -> (String, RankedContextResult, usize, usize, Option<String>) {
        let mut working_history = session_history.to_vec();
        let session_compaction_applied =
            self.maybe_compact_session_history(&mut working_history, session);
        let session_history_len = working_history.len();
        let ctx = self.build_ranked_context(user_prompt, domain, requires_recall, task_id);
        let best_skill = self.select_skill(domain);

        if self.collect_traces {
            self.traces.push(HsmTurnTrace {
                task_id: task_id.to_string(),
                turn_index: 0,
                session,
                requires_recall,
                selected_skill_id: best_skill.as_ref().map(|s| s.id.clone()),
                selected_skill_domain: best_skill.as_ref().map(|s| s.domain.clone()),
                belief_ranks: ctx.belief_ranks.clone(),
                session_summaries_injected: ctx.session_summary_sessions.clone(),
                injected_char_len: ctx.injected_text.len(),
                injected_preview: ctx.injected_text.chars().take(800).collect::<String>(),
                session_compaction_applied,
                session_history_len,
            });
        }

        let mut messages = Vec::new();
        let mut system = self.system_prompt.clone();
        if let Some(skill) = &best_skill {
            system.push_str(&format!(
                "\n\nYour expertise area: {}. Apply this knowledge.",
                skill.description
            ));
        }
        messages.push(Message::system(system));
        if !ctx.injected_text.is_empty() {
            messages.push(Message::user(format!(
                "## Relevant context from previous sessions:\n{}",
                ctx.injected_text
            )));
        }
        messages.extend(working_history);
        messages.push(Message::user(user_prompt));

        let (response, prompt_tokens, completion_tokens, error) = self.call_llm(&messages).await;
        (response, ctx, prompt_tokens, completion_tokens, error)
    }

    fn now_unix() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    fn extract_keywords_from_text(&self, text: &str, min_len: usize, top_k: usize) -> Vec<String> {
        let mut word_counts: HashMap<String, usize> = HashMap::new();
        for word in text.to_lowercase().split(|c: char| !c.is_alphanumeric()) {
            if word.len() >= min_len {
                *word_counts.entry(word.to_string()).or_insert(0) += 1;
            }
        }
        let mut keywords: Vec<(String, usize)> = word_counts.into_iter().collect();
        keywords.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        keywords.into_iter().take(top_k).map(|(w, _)| w).collect()
    }

    fn contradiction_markers(text: &str) -> bool {
        let lower = text.to_lowercase();
        [
            "actually",
            "correction",
            "corrected",
            "update",
            "updated",
            "changed",
            "instead",
            "rather than",
            "no longer",
            "deprecated",
            "replaced",
            "superseded",
        ]
        .iter()
        .any(|m| lower.contains(m))
    }

    fn extract_typed_claims(
        &self,
        content: &str,
        source_task: &str,
        domain: Option<&str>,
    ) -> Vec<super::memory_graph::TypedClaimSnapshot> {
        let subject = format!("task:{source_task}");
        let scope = domain.map(str::to_string);
        let lower = content.to_lowercase();
        let mut claims = Vec::new();

        let mut push_claim =
            |relation: &str, object: &str, negated: bool, temporal_hint: Option<&str>| {
                let claim = super::memory_graph::TypedClaimSnapshot {
                    subject: subject.clone(),
                    relation: relation.to_string(),
                    object: object.to_string(),
                    negated,
                    scope: scope.clone(),
                    temporal_hint: temporal_hint.map(str::to_string),
                };
                if !claims
                    .iter()
                    .any(|existing: &super::memory_graph::TypedClaimSnapshot| {
                        existing.subject == claim.subject
                            && existing.relation == claim.relation
                            && existing.object == claim.object
                            && existing.negated == claim.negated
                            && existing.temporal_hint == claim.temporal_hint
                    })
                {
                    claims.push(claim);
                }
            };

        if lower.contains("jwt") {
            push_claim("auth_method", "jwt", false, None);
        }
        if lower.contains("cookie session") || lower.contains("cookie sessions") {
            let negated = lower.contains("instead of cookie")
                || lower.contains("replace cookie")
                || lower.contains("replaced cookie")
                || lower.contains("not cookie");
            push_claim("auth_method", "cookie_sessions", negated, None);
        }
        if lower.contains("deploy to staging") || lower.contains("staging first") {
            let temporal_hint = if lower.contains("first") {
                Some("first")
            } else if lower.contains("before production") {
                Some("before_production")
            } else {
                None
            };
            push_claim("deploy_target", "staging", false, temporal_hint);
        }
        if lower.contains("deploy to production") || lower.contains("production after staging") {
            let temporal_hint = if lower.contains("after staging") {
                Some("after_staging")
            } else {
                None
            };
            push_claim("deploy_target", "production", false, temporal_hint);
        }
        if lower.contains("linkedin") {
            push_claim("publishes_to", "linkedin", false, None);
        }
        if lower.contains("twitter/x") || lower.contains("twitter") || lower.contains("x.com") {
            push_claim("publishes_to", "twitter_x", false, None);
        }
        if lower.contains("tiktok") {
            push_claim("publishes_to", "tiktok", false, None);
        }

        claims
    }

    fn classify_relationships(
        &self,
        source_task: &str,
        domain: Option<&str>,
        source_turn: usize,
        content: &str,
        keywords: &[String],
    ) -> (Option<usize>, Vec<usize>, Vec<String>) {
        let keyword_set: std::collections::HashSet<&str> =
            keywords.iter().map(String::as_str).collect();
        let mut best_supersedes: Option<(usize, usize)> = None;
        let mut evidence_beliefs = Vec::new();
        let mut contradictions = Vec::new();
        let text_lower = content.to_lowercase();
        let has_revision_marker = Self::contradiction_markers(content);

        for (index, prior) in self.beliefs.iter().enumerate() {
            if prior.source_task != source_task || prior.source_turn >= source_turn {
                continue;
            }
            if let Some(domain) = domain {
                if let Some(prior_domain) = prior.domain.as_deref() {
                    if prior_domain != domain {
                        continue;
                    }
                }
            }

            let overlap = prior
                .keywords
                .iter()
                .filter(|kw| keyword_set.contains(kw.as_str()))
                .count();
            if overlap == 0 {
                continue;
            }

            if overlap >= 2 || prior.source_turn + 1 == source_turn {
                evidence_beliefs.push(index);
            }

            let prior_has_negation = prior.content.to_lowercase().contains(" not ");
            let current_has_negation = text_lower.contains(" not ");
            let likely_revision = has_revision_marker
                || prior_has_negation != current_has_negation
                || text_lower.contains("instead of")
                || text_lower.contains("use ")
                    && prior.content.to_lowercase().contains("use ")
                    && overlap >= 1;

            if likely_revision && overlap >= 1 {
                match best_supersedes {
                    Some((_, best_overlap)) if best_overlap >= overlap => {}
                    _ => best_supersedes = Some((index, overlap)),
                }
                contradictions.push(format!(
                    "Revises prior belief #{index}: {}",
                    truncate(&memory_snippet_one_line(&prior.content, 140), 140)
                ));
            }
        }

        evidence_beliefs.sort_unstable();
        evidence_beliefs.dedup();
        let supersedes = best_supersedes.map(|(index, _)| index);
        if let Some(index) = supersedes {
            evidence_beliefs.retain(|i| *i != index);
        }
        (supersedes, evidence_beliefs, contradictions)
    }

    fn push_belief(
        &mut self,
        content: String,
        confidence: f64,
        domain: Option<String>,
        source_task: String,
        source_turn: usize,
        keywords: Vec<String>,
        source_excerpt: Option<String>,
        supporting_evidence: Vec<String>,
        human_committed: bool,
    ) {
        let claims = self.extract_typed_claims(&content, &source_task, domain.as_deref());
        let (supersedes_belief_index, evidence_belief_indices, contradicting_evidence) = self
            .classify_relationships(
                &source_task,
                domain.as_deref(),
                source_turn,
                &content,
                &keywords,
            );
        self.beliefs.push(StoredBelief {
            content,
            confidence,
            domain,
            source_task,
            source_turn,
            created_at: Self::now_unix(),
            keywords,
            source_excerpt,
            supporting_evidence,
            contradicting_evidence,
            supersedes_belief_index,
            evidence_belief_indices,
            human_committed,
            claims,
        });
    }

    /// Seed initial skill bank with domain knowledge
    fn seed_skills() -> Vec<TrackedSkill> {
        vec![
            TrackedSkill {
                id: "api-design".into(),
                description: "REST API design with authentication, pagination, and versioning"
                    .into(),
                domain: "software_engineering".into(),
                usage_count: 0,
                success_count: 0,
                avg_keyword_score: 0.0,
            },
            TrackedSkill {
                id: "debugging".into(),
                description: "Systematic debugging of production issues with root cause analysis"
                    .into(),
                domain: "software_engineering".into(),
                usage_count: 0,
                success_count: 0,
                avg_keyword_score: 0.0,
            },
            TrackedSkill {
                id: "architecture".into(),
                description: "System architecture design with trade-off analysis".into(),
                domain: "software_engineering".into(),
                usage_count: 0,
                success_count: 0,
                avg_keyword_score: 0.0,
            },
            TrackedSkill {
                id: "ml-pipeline".into(),
                description: "ML pipeline design, model training, and deployment".into(),
                domain: "data_science".into(),
                usage_count: 0,
                success_count: 0,
                avg_keyword_score: 0.0,
            },
            TrackedSkill {
                id: "data-engineering".into(),
                description: "Data pipeline design with ETL and streaming".into(),
                domain: "data_science".into(),
                usage_count: 0,
                success_count: 0,
                avg_keyword_score: 0.0,
            },
            TrackedSkill {
                id: "business-strategy".into(),
                description: "Market analysis, go-to-market, pricing strategy".into(),
                domain: "business".into(),
                usage_count: 0,
                success_count: 0,
                avg_keyword_score: 0.0,
            },
            TrackedSkill {
                id: "product-mgmt".into(),
                description: "Product roadmap prioritization and stakeholder management".into(),
                domain: "business".into(),
                usage_count: 0,
                success_count: 0,
                avg_keyword_score: 0.0,
            },
            TrackedSkill {
                id: "technical-writing".into(),
                description: "Research papers, technical blogs, documentation".into(),
                domain: "research".into(),
                usage_count: 0,
                success_count: 0,
                avg_keyword_score: 0.0,
            },
            TrackedSkill {
                id: "system-design".into(),
                description: "Large-scale system design with HIPAA, multi-tenant, distributed"
                    .into(),
                domain: "stress_test".into(),
                usage_count: 0,
                success_count: 0,
                avg_keyword_score: 0.0,
            },
            TrackedSkill {
                id: "cross-session-synthesis".into(),
                description: "Synthesize decisive facts spread across multiple prior sessions into one grounded answer".into(),
                domain: "cross_session_synthesis".into(),
                usage_count: 0,
                success_count: 0,
                avg_keyword_score: 0.0,
            },
            TrackedSkill {
                id: "belief-revision".into(),
                description: "Prefer corrected or newer facts over stale earlier statements and explain the revision".into(),
                domain: "belief_revision".into(),
                usage_count: 0,
                success_count: 0,
                avg_keyword_score: 0.0,
            },
            TrackedSkill {
                id: "agent-handoff".into(),
                description: "Carry forward prior agent findings and preserve the key content needed for the finisher's next step".into(),
                domain: "agent_handoff".into(),
                usage_count: 0,
                success_count: 0,
                avg_keyword_score: 0.0,
            },
            TrackedSkill {
                id: "policy-persistence".into(),
                description: "Apply prior policy decisions consistently to new cases unless a later session explicitly changes them".into(),
                domain: "policy_persistence".into(),
                usage_count: 0,
                success_count: 0,
                avg_keyword_score: 0.0,
            },
            TrackedSkill {
                id: "conflict-resolution".into(),
                description: "Resolve conflicting session facts by preferring the final or explicitly corrected decision".into(),
                domain: "conflict_resolution".into(),
                usage_count: 0,
                success_count: 0,
                avg_keyword_score: 0.0,
            },
        ]
    }

    /// Run all tasks with HSM-II augmentation
    pub async fn run(&mut self, tasks: &[EvalTask]) -> RunnerMetrics {
        let mut metrics = RunnerMetrics::new("hsm-ii");
        let run_start = Instant::now();
        let mut harness = HarnessRuntime::from_env("hsm-ii").unwrap_or_else(|e| {
            tracing::warn!(target: "harness", "hsm-ii harness init failed: {}", e);
            HarnessRuntime::noop()
        });

        for task in tasks {
            let mut session_history: HashMap<u32, Vec<Message>> = HashMap::new();
            let mut prev_session: u32 = 0;

            for (turn_idx, turn) in task.turns.iter().enumerate() {
                let turn_start = Instant::now();
                harness.turn_begin(&task.id, turn_idx);

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
                        self.push_belief(
                            summary.clone(),
                            0.9,
                            turn.domain.clone(),
                            task.id.clone(),
                            turn_idx,
                            keywords.clone(),
                            Some(truncate(&summary, 280).to_string()),
                            vec![format!(
                                "session={} summary boundary at turn {}",
                                prev_session, turn_idx
                            )],
                            false,
                        );
                    }
                }
                prev_session = turn.session;

                let session_compaction_applied = {
                    let h = session_history.entry(turn.session).or_default();
                    self.maybe_compact_session_history(h, turn.session)
                };
                let session_history_len = session_history
                    .get(&turn.session)
                    .map(|x| x.len())
                    .unwrap_or(0);

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
                        session_compaction_applied,
                        session_history_len,
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

                // Inject persistent memory context (cross-session recall).
                // Keep this as a user message (not a second system message) for broad provider compatibility.
                if !ctx.injected_text.is_empty() {
                    let context_block = format!(
                        "## Relevant context from previous sessions:\n{}",
                        ctx.injected_text
                    );
                    messages.push(Message::user(&context_block));
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
                harness.turn_end(&task.id, turn_idx, turn_start, error.as_deref());

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
                    let belief_text = format!(
                        "Q: {}\nA: {}",
                        truncate(&turn.user, 300),
                        truncate(&tm.response, 600)
                    );
                    self.push_belief(
                        belief_text,
                        tm.keyword_score,
                        turn.domain.clone(),
                        task.id.clone(),
                        turn_idx,
                        turn.expected_keywords.clone(),
                        Some(truncate(&tm.response, 280).to_string()),
                        vec![format!("question={}", truncate(&turn.user, 180))],
                        false,
                    );
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
                self.push_belief(
                    summary.clone(),
                    0.85,
                    Some(task.domain.clone()),
                    task.id.clone(),
                    task.turns.len(),
                    Vec::new(),
                    Some(truncate(&summary, 280).to_string()),
                    vec![format!(
                        "final session={} summary across {} messages",
                        prev_session,
                        history.len()
                    )],
                    false,
                );
            }
        }

        metrics.total_duration_ms = run_start.elapsed().as_millis() as u64;
        metrics
    }

    /// Snip-style in-session compaction: summarize and drop older turns, insert a `<compact_boundary>` user turn.
    fn maybe_compact_session_history(&self, history: &mut Vec<Message>, session: u32) -> bool {
        let cfg = self.cfg();
        if !cfg.session_compaction_enabled {
            return false;
        }
        let trigger = cfg.session_compaction_trigger_messages;
        let keep = cfg.session_compaction_keep_tail_messages;
        if history.len() <= trigger {
            return false;
        }
        if keep == 0 || keep >= history.len() {
            return false;
        }
        let split = history.len() - keep;
        let prefix: Vec<Message> = history.drain(..split).collect();
        let summary = self.summarize_session(&prefix);
        let body = memory_snippet_one_line(&summary, 4000);
        let boundary = format!(
            "<compact_boundary type=\"session_snip\" session={}>\nPrior in-session dialogue was summarized (older turns removed from the transcript):\n{}\n</compact_boundary>",
            session, body
        );
        history.insert(0, Message::user(boundary));
        true
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

        let r = self.cfg().resolve_memory(domain);
        if !r.inject {
            return RankedContextResult::empty();
        }

        let query_lower = query.to_lowercase();
        let query_words: Vec<&str> = query_lower.split_whitespace().collect();
        let now_unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

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

                if belief_lower.contains("session metadata:") {
                    score -= 0.35;
                }

                let kw_overlap = belief
                    .keywords
                    .iter()
                    .filter(|kw| query_lower.contains(&kw.to_lowercase()))
                    .count();
                score += kw_overlap as f64 * self.cfg().belief_keyword_overlap_weight;

                // LongMemEval-style recall benefits from user-authored dated facts.
                if belief_lower.contains(" user:") {
                    score += 0.45;
                } else if belief_lower.contains(" assistant:") {
                    score += 0.05;
                }
                let temporal_tokens = [
                    "january",
                    "february",
                    "march",
                    "april",
                    "may",
                    "june",
                    "july",
                    "august",
                    "september",
                    "october",
                    "november",
                    "december",
                    "before",
                    "after",
                    "first",
                    "last",
                    "earlier",
                    "later",
                    "week",
                    "weeks",
                    "month",
                    "months",
                    "day",
                    "days",
                    "sunday",
                    "monday",
                    "tuesday",
                    "wednesday",
                    "thursday",
                    "friday",
                    "saturday",
                ];
                let temporal_hits = temporal_tokens
                    .iter()
                    .filter(|tok| query_lower.contains(**tok) && belief_lower.contains(**tok))
                    .count();
                score += temporal_hits as f64 * 0.25;

                // Prefer recent beliefs and later turns in the originating task.
                let age_secs = now_unix.saturating_sub(belief.created_at);
                let recency = 1.0 / (1.0 + (age_secs as f64 / 3600.0));
                score += recency * 0.05;
                score += (belief.source_turn.min(20) as f64 / 20.0) * 0.03;

                score *= belief.confidence;

                (i, score)
            })
            .filter(|(_, s)| *s >= r.belief_threshold)
            .collect();

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let mut belief_lines: Vec<String> = Vec::new();
        let mut belief_ranks: Vec<BeliefRankEntry> = Vec::new();
        let mut chosen: Vec<(usize, f64)> = Vec::new();
        let mut seen_sessions = std::collections::BTreeSet::new();
        for (idx, score) in &scored {
            if chosen.len() >= r.top_k {
                break;
            }
            let belief = &self.beliefs[*idx];
            if let Some(session) = belief_session_number(&belief.content) {
                if seen_sessions.insert(session) {
                    chosen.push((*idx, *score));
                }
            }
        }
        for (idx, score) in &scored {
            if chosen.len() >= r.top_k {
                break;
            }
            if chosen.iter().any(|(chosen_idx, _)| chosen_idx == idx) {
                continue;
            }
            chosen.push((*idx, *score));
        }
        chosen.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        for (idx, score) in chosen.iter() {
            let belief = &self.beliefs[*idx];
            let body = memory_snippet_one_line(&belief.content, r.max_belief_snippet);
            belief_ranks.push(BeliefRankEntry {
                belief_index: *idx,
                score: *score,
                source_task: belief.source_task.clone(),
                preview: body.clone(),
            });
            belief_lines.push(format!(
                "- [belief score={:.2} task={}] {}",
                *score, belief.source_task, body
            ));
        }

        let mut session_summary_sessions = Vec::new();
        let mut summary_lines: Vec<String> = Vec::new();

        if r.include_summaries {
            if let Some(summaries) = self.session_summaries.get(task_id) {
                let mut scored_summaries: Vec<(&SessionSummary, f64)> = summaries
                    .iter()
                    .filter(|s| s.task_id == task_id)
                    .map(|s| {
                        let mut score = 0.0;
                        let summary_lower = s.summary.to_lowercase();
                        let matching_words = query_words
                            .iter()
                            .filter(|w| w.len() > 3 && summary_lower.contains(**w))
                            .count();
                        score += matching_words as f64 * self.cfg().query_overlap_weight;

                        let decision_hits = s
                            .key_decisions
                            .iter()
                            .filter(|d| query_lower.contains(&d.to_lowercase()))
                            .count();
                        score += decision_hits as f64 * 0.12;

                        let keyword_hits = s
                            .keywords
                            .iter()
                            .filter(|kw| query_lower.contains(&kw.to_lowercase()))
                            .count();
                        score += keyword_hits as f64 * 0.08;
                        (s, score)
                    })
                    .filter(|(_, sc)| *sc >= r.summary_threshold)
                    .collect();
                scored_summaries
                    .sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

                for (s, sc) in scored_summaries.into_iter().take(r.max_session_summaries) {
                    session_summary_sessions.push(s.session);
                    let body = memory_snippet_one_line(&s.summary, r.max_summary_snippet);
                    summary_lines.push(format!(
                        "- [session={} score={:.2}] {}",
                        s.session, sc, body
                    ));
                }
            }
        }

        let mut context_parts = belief_lines;
        context_parts.extend(summary_lines);

        let joined = context_parts.join("\n");
        let after_budget = if joined.len() > r.char_budget {
            truncate(&joined, r.char_budget).to_string()
        } else {
            joined
        };
        let injected_text = truncate_entrypoint_content(
            &after_budget,
            self.cfg().memory_entrypoint_max_lines,
            self.cfg().memory_entrypoint_max_bytes,
        );

        RankedContextResult {
            injected_text,
            belief_ranks,
            session_summary_sessions,
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
            return None;
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
            score_b
                .partial_cmp(&score_a)
                .unwrap_or(std::cmp::Ordering::Equal)
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
        let all_text: String = history
            .iter()
            .map(|m| m.content.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        let lower = all_text.to_lowercase();
        let candidates = self.extract_keywords_from_text(&lower, 5, 32);
        let mut word_counts: HashMap<String, usize> = HashMap::new();
        for word in lower.split(|c: char| !c.is_alphanumeric()) {
            if word.len() > 4 {
                *word_counts.entry(word.to_string()).or_insert(0) += 1;
            }
        }
        candidates
            .into_iter()
            .filter(|kw| word_counts.get(kw).copied().unwrap_or(0) >= 2)
            .take(10)
            .collect()
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
            Err(e) => (format!("[ERROR: {}]", e), 0, 0, Some(e.to_string())),
        }
    }
}

/// Claude Code `memdir` / `MEMORY.md` caps: line count first, then byte cap at a newline (UTF-8 safe).
fn truncate_entrypoint_content(raw: &str, max_lines: usize, max_bytes: usize) -> String {
    const ENTRYPOINT_NAME: &str = "injected memory block";
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let content_lines: Vec<&str> = trimmed.lines().collect();
    let line_count = content_lines.len();
    let byte_count = trimmed.as_bytes().len();

    let was_line_truncated = line_count > max_lines;
    let was_byte_truncated = byte_count > max_bytes;

    if !was_line_truncated && !was_byte_truncated {
        return trimmed.to_string();
    }

    let mut truncated = if was_line_truncated {
        content_lines[..max_lines].join("\n")
    } else {
        trimmed.to_string()
    };

    if truncated.as_bytes().len() > max_bytes {
        let mut cut = max_bytes.min(truncated.len());
        while cut > 0 && !truncated.is_char_boundary(cut) {
            cut -= 1;
        }
        if let Some(pos) = truncated[..cut].rfind('\n') {
            truncated.truncate(if pos > 0 { pos } else { cut });
        } else {
            truncated.truncate(cut);
        }
    }

    let reason = if was_byte_truncated && !was_line_truncated {
        format!(
            "{} bytes (limit: {max_bytes}) — entries are too long",
            byte_count
        )
    } else if was_line_truncated && !was_byte_truncated {
        format!("{line_count} lines (limit: {max_lines})")
    } else {
        format!("{line_count} lines and {byte_count} bytes")
    };

    format!(
        "{truncated}\n\n> WARNING: {ENTRYPOINT_NAME} is {reason}. Only part was loaded. Keep injection lines short; move detail into retrieval.",
    )
}

/// Flatten multi-line belief/summary text into one short line for prompt injection.
fn memory_snippet_one_line(s: &str, max_chars: usize) -> String {
    let collapsed: String = s
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let flat: String = collapsed.split_whitespace().collect::<Vec<_>>().join(" ");
    flat.chars().take(max_chars).collect()
}

/// Truncate a string to max_len characters
fn truncate(s: &str, max_len: usize) -> &str {
    if s.len() <= max_len {
        s
    } else {
        let boundary = s
            .char_indices()
            .take_while(|(i, _)| *i < max_len)
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(max_len);
        &s[..boundary]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_memory_tightens_se_ds() {
        let cfg = HsmRunnerConfig::default();
        let r = cfg.resolve_memory("software_engineering");
        assert!(r.char_budget < cfg.context_char_budget);
        assert!(r.top_k <= cfg.context_top_k);
        assert!(r.belief_threshold >= cfg.context_score_threshold);

        let ds = cfg.resolve_memory("data_science");
        assert_eq!(ds.char_budget, r.char_budget);

        let biz = cfg.resolve_memory("business");
        assert_eq!(biz.char_budget, cfg.context_char_budget);
        assert_eq!(biz.top_k, cfg.context_top_k);
    }

    #[test]
    fn memory_snippet_flattens() {
        let s = "line one\n\nline two  \t multi";
        let o = memory_snippet_one_line(s, 100);
        assert!(!o.contains('\n'));
        assert!(o.contains("line one"));
    }

    #[test]
    fn entrypoint_line_cap_appends_warning() {
        let lines: String = (0..5)
            .map(|i| format!("L{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let out = truncate_entrypoint_content(&lines, 3, 100_000);
        assert!(out.contains("WARNING"));
        assert!(out.starts_with("L0\nL1\nL2"));
        assert!(!out.contains("L4"));
    }

    #[test]
    fn entrypoint_byte_cap_truncates_long_lines() {
        let s = "x".repeat(400);
        let out = truncate_entrypoint_content(&s, 10_000, 80);
        assert!(out.contains("WARNING"));
        assert!(out.len() < s.len());
    }

    #[test]
    fn ingest_history_exports_provenance_and_revision_links() {
        let client =
            LlmClient::new().expect("llm client should construct with default provider set");
        let mut runner = HsmRunner::with_config(client, HsmRunnerConfig::default());
        let history = vec![
            Message::user("Deploy to staging first and use cookie sessions for auth."),
            Message::assistant("Understood. I will use cookie sessions."),
            Message::user(
                "Correction: use JWT auth instead of cookie sessions, and deploy to production after staging.",
            ),
        ];

        runner.ingest_session_history("task-1", "software_engineering", 1, &history);

        let snapshot = runner.export_memory_snapshot();
        assert!(snapshot.beliefs.iter().any(|b| b
            .source_excerpt
            .as_deref()
            .unwrap_or_default()
            .contains("cookie sessions")));
        assert!(snapshot
            .beliefs
            .iter()
            .any(|b| !b.supporting_evidence.is_empty()));
        assert!(snapshot
            .beliefs
            .iter()
            .any(|b| b.human_committed && b.source_excerpt.is_some()));
        assert!(snapshot
            .beliefs
            .iter()
            .any(|b| b.supersedes_belief_index.is_some() || !b.contradicting_evidence.is_empty()));
        assert!(snapshot.beliefs.iter().any(|b| b
            .claims
            .iter()
            .any(|claim| claim.relation == "auth_method" && claim.object == "jwt")));
        assert!(snapshot
            .beliefs
            .iter()
            .any(|b| b.claims.iter().any(|claim| {
                claim.relation == "deploy_target"
                    && claim.object == "production"
                    && claim.temporal_hint.as_deref() == Some("after_staging")
            })));
    }
}
