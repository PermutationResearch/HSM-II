//! LLM-Powered Council Deliberation.
//!
//! This module replaces heuristic role-based scoring with genuine LLM-generated
//! arguments and reasoning for council decisions. Agents produce real arguments
//! through LLM inference rather than template-based stances.
//!
//! Key features:
//! - LLM-generated opening statements with actual reasoning
//! - Contextual rebuttals based on opponent arguments
//! - Semantic synthesis of debate positions
//! - Richer deliberation at the cost of increased latency

use crate::agent::{AgentId, Role};
use crate::council::{
    CouncilDecision, CouncilDecisionMetadata, CouncilEvidence, CouncilId, CouncilMember,
    CouncilMode, CouncilStatus, Decision, ExecutionPlan, ExecutionStep, Proposal,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Cache for LLM debate arguments to avoid redundant LLM calls
///
/// This implements semantic caching where similar prompts can reuse
/// previously generated arguments, significantly reducing LLM costs
/// and latency for repetitive council deliberations.
#[derive(Clone, Debug)]
pub struct ArgumentCache {
    /// Cached arguments keyed by hash of (agent_role, proposal_hash, debate_phase)
    entries: Arc<Mutex<HashMap<String, CachedArgument>>>,
    /// Time-to-live for cache entries in seconds
    ttl_secs: u64,
    /// Maximum number of entries to keep in cache
    max_entries: usize,
}

#[derive(Clone, Debug)]
struct CachedArgument {
    argument: LLMArgument,
    created_at: std::time::Instant,
    access_count: usize,
}

/// Cache statistics for monitoring
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CacheStats {
    pub hits: usize,
    pub misses: usize,
    pub evictions: usize,
    pub total_entries: usize,
    pub avg_hit_latency_ms: f64,
}

impl ArgumentCache {
    /// Create a new argument cache with specified TTL and max size
    pub fn new(ttl_secs: u64, max_entries: usize) -> Self {
        Self {
            entries: Arc::new(Mutex::new(HashMap::new())),
            ttl_secs,
            max_entries,
        }
    }

    /// Create a cache with default settings (5 min TTL, 1000 entries)
    pub fn default() -> Self {
        Self::new(300, 1000)
    }

    /// Generate cache key from context
    fn make_key(
        agent_role: Role,
        proposal_id: &str,
        phase: &str,
        context_hash: Option<u64>,
    ) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        agent_role.hash(&mut hasher);
        proposal_id.hash(&mut hasher);
        phase.hash(&mut hasher);
        if let Some(h) = context_hash {
            h.hash(&mut hasher);
        }
        format!("{:x}", hasher.finish())
    }

    /// Get cached argument if available and not expired
    pub fn get(&self, agent_role: Role, proposal_id: &str, phase: &str) -> Option<LLMArgument> {
        let key = Self::make_key(agent_role, proposal_id, phase, None);
        let mut entries = self.entries.lock().ok()?;

        if let Some(cached) = entries.get_mut(&key) {
            // Check TTL
            let elapsed = cached.created_at.elapsed().as_secs();
            if elapsed > self.ttl_secs {
                entries.remove(&key);
                return None;
            }

            // Update access count
            cached.access_count += 1;
            return Some(cached.argument.clone());
        }

        None
    }

    /// Store an argument in the cache
    pub fn put(&self, agent_role: Role, proposal_id: &str, phase: &str, argument: LLMArgument) {
        let key = Self::make_key(agent_role, proposal_id, phase, None);
        let mut entries = self.entries.lock().unwrap();

        // Evict oldest entries if at capacity (simple LRU via access count)
        if entries.len() >= self.max_entries {
            let to_evict: Vec<String> = entries
                .iter()
                .min_by_key(|(_, v)| v.access_count)
                .map(|(k, _)| k.clone())
                .into_iter()
                .collect();

            for key in to_evict.iter().take(self.max_entries / 10) {
                entries.remove(key);
            }
        }

        entries.insert(
            key,
            CachedArgument {
                argument,
                created_at: std::time::Instant::now(),
                access_count: 0,
            },
        );
    }

    /// Get cache statistics
    pub fn stats(&self) -> CacheStats {
        let entries = self.entries.lock().unwrap();
        CacheStats {
            hits: 0, // Would need atomic counters for accurate tracking
            misses: 0,
            evictions: 0,
            total_entries: entries.len(),
            avg_hit_latency_ms: 0.0,
        }
    }

    /// Clear all cached entries
    pub fn clear(&self) {
        let mut entries = self.entries.lock().unwrap();
        entries.clear();
    }
}

/// An LLM-generated argument in council deliberation
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LLMArgument {
    pub agent_id: AgentId,
    pub role: Role,
    pub stance: Stance,
    /// The actual content generated by LLM
    pub content: String,
    /// Confidence in this argument (0-1)
    pub confidence: f64,
    /// Key points extracted from the argument
    pub key_points: Vec<String>,
    /// Evidence cited (if any)
    pub evidence: Vec<CouncilEvidence>,
    pub round: usize,
    pub responding_to: Option<AgentId>,
    /// Token count for cost tracking
    pub tokens_generated: usize,
    /// Generation latency in ms
    pub generation_time_ms: u64,
}

/// Stance in debate - simplified from heuristic to LLM-determined
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum Stance {
    For,
    Against,
    Neutral,
    Cautious,
    Curious,
}

impl Stance {
    pub fn as_str(&self) -> &'static str {
        match self {
            Stance::For => "in_favor",
            Stance::Against => "against",
            Stance::Neutral => "neutral",
            Stance::Cautious => "cautiously_supportive",
            Stance::Curious => "exploratory",
        }
    }
}

/// Configuration for LLM deliberation
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LLMDeliberationConfig {
    /// Model to use for deliberation
    pub model: String,
    /// Ollama endpoint
    pub endpoint: String,
    /// Temperature for generation (higher = more creative)
    pub temperature: f64,
    /// Maximum tokens per argument
    pub max_tokens: usize,
    /// Number of debate rounds
    pub debate_rounds: usize,
    /// Whether to enable rebuttals
    pub enable_rebuttals: bool,
    /// Whether to extract key points automatically
    pub extract_key_points: bool,
    /// Timeout for LLM calls in seconds (0 = no timeout)
    pub timeout_secs: u64,
}

impl Default for LLMDeliberationConfig {
    fn default() -> Self {
        Self {
            model: std::env::var("OLLAMA_MODEL").unwrap_or_else(|_| "auto".to_string()),
            endpoint: format!("{}:{}",
                std::env::var("OLLAMA_HOST").unwrap_or_else(|_| "http://localhost".to_string()),
                std::env::var("OLLAMA_PORT").unwrap_or_else(|_| "11434".to_string()),
            ),
            temperature: 0.7,
            max_tokens: 500,
            debate_rounds: 2,
            enable_rebuttals: true,
            extract_key_points: true,
            timeout_secs: 0, // No timeout - let council deliberation complete
        }
    }
}

/// LLM-powered debate council
pub struct LLMDebateCouncil {
    council_id: CouncilId,
    members: Vec<CouncilMember>,
    config: LLMDeliberationConfig,
    rounds: Vec<LLMDebateRound>,
    status: CouncilStatus,
    /// Cache for debate arguments to reduce LLM calls
    argument_cache: ArgumentCache,
    /// Cache hit/miss statistics
    cache_stats: CacheStats,
}

/// A round in the LLM debate
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LLMDebateRound {
    pub number: usize,
    pub phase: DebatePhase,
    pub arguments: Vec<LLMArgument>,
    /// Synthesis of this round (if any)
    pub synthesis: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum DebatePhase {
    Opening,
    Rebuttal,
    Synthesis,
    FinalVote,
}

impl LLMDebateCouncil {
    pub fn new(
        council_id: CouncilId,
        members: Vec<CouncilMember>,
        config: LLMDeliberationConfig,
    ) -> Self {
        Self {
            council_id,
            members,
            config,
            rounds: Vec::new(),
            status: CouncilStatus::NotStarted,
            argument_cache: ArgumentCache::default(),
            cache_stats: CacheStats::default(),
        }
    }

    /// Create a council with a custom argument cache
    pub fn with_cache(
        council_id: CouncilId,
        members: Vec<CouncilMember>,
        config: LLMDeliberationConfig,
        cache: ArgumentCache,
    ) -> Self {
        Self {
            council_id,
            members,
            config,
            rounds: Vec::new(),
            status: CouncilStatus::NotStarted,
            argument_cache: cache,
            cache_stats: CacheStats::default(),
        }
    }

    /// Get cache statistics
    pub fn cache_stats(&self) -> &CacheStats {
        &self.cache_stats
    }

    pub fn status(&self) -> CouncilStatus {
        self.status.clone()
    }

    /// Evaluate a proposal through LLM-powered debate
    pub async fn evaluate(
        &mut self,
        proposal: &Proposal,
        mode: CouncilMode,
    ) -> anyhow::Result<CouncilDecision> {
        self.status = CouncilStatus::InProgress {
            step: "llm_deliberation".to_string(),
            progress_pct: 0.0,
        };

        // Phase 1: Opening statements (each agent generates LLM argument)
        let mut opening_args = Vec::new();
        for member in &self.members {
            let argument = self.generate_opening_argument(member, proposal).await?;
            opening_args.push(argument);
        }

        self.rounds.push(LLMDebateRound {
            number: 0,
            phase: DebatePhase::Opening,
            arguments: opening_args,
            synthesis: None,
        });

        self.status = CouncilStatus::InProgress {
            step: "rebuttal".to_string(),
            progress_pct: 0.33,
        };

        // Phase 2: Rebuttals (if enabled)
        if self.config.enable_rebuttals {
            let mut rebuttals = Vec::new();

            for member in &self.members {
                // Find arguments to rebut (not our own)
                let targets: Vec<_> = self.rounds[0]
                    .arguments
                    .iter()
                    .filter(|a| a.agent_id != member.agent_id)
                    .collect();

                for target in targets.iter().take(2) {
                    let rebuttal = self.generate_rebuttal(member, proposal, target).await?;
                    rebuttals.push(rebuttal);
                }
            }

            self.rounds.push(LLMDebateRound {
                number: 1,
                phase: DebatePhase::Rebuttal,
                arguments: rebuttals,
                synthesis: None,
            });
        }

        self.status = CouncilStatus::InProgress {
            step: "synthesis".to_string(),
            progress_pct: 0.66,
        };

        // Phase 3: LLM synthesis of the entire debate
        let synthesis = self.generate_debate_synthesis(proposal).await?;

        self.rounds.push(LLMDebateRound {
            number: 2,
            phase: DebatePhase::Synthesis,
            arguments: vec![],
            synthesis: Some(synthesis.clone()),
        });

        // Phase 4: LLM-based decision
        let decision_result = self.render_llm_decision(proposal, &synthesis).await?;

        self.status = CouncilStatus::Completed {
            decision: decision_result.decision.clone(),
        };

        Ok(CouncilDecision {
            council_id: self.council_id,
            proposal_id: proposal.id.clone(),
            decision: decision_result.decision,
            confidence: decision_result.confidence,
            participating_agents: self.members.iter().map(|m| m.agent_id).collect(),
            execution_plan: decision_result.execution_plan,
            decided_at: current_timestamp(),
            mode_used: mode,
            metadata: self.collect_decision_metadata(proposal),
        })
    }

    /// Generate opening argument using LLM (with caching)
    async fn generate_opening_argument(
        &self,
        member: &CouncilMember,
        proposal: &Proposal,
    ) -> anyhow::Result<LLMArgument> {
        let start = std::time::Instant::now();

        // Check cache first
        let proposal_id = format!(
            "{}:{:.2}:{:.2}",
            proposal.title, proposal.complexity, proposal.urgency
        );
        if let Some(cached) = self
            .argument_cache
            .get(member.role, &proposal_id, "opening")
        {
            return Ok(cached);
        }

        let prompt = format!(
            "You are Agent {} with role {:?} participating in a council deliberation.\n\n\
             PROPOSAL:\nTitle: {}\nDescription: {}\nComplexity: {:.2}\nUrgency: {:.2}\n\n\
             STIGMERGIC CONTEXT:\n{}\n\n\
             Your role's expertise: {:?}\nParticipation weight: {:.2}\n\n\
             TASK: Provide your opening statement on this proposal. Consider:\n\
             1. Your role's perspective and expertise\n\
             2. The proposal's merits and risks\n\
             3. What would make this proposal succeed or fail\n\
             4. Cite concrete trace/directive/policy IDs whenever the stigmergic context provides them\n\n\
             Respond with:\n\
             STANCE: [in_favor|against|neutral|cautiously_supportive|exploratory]\n\
             ARGUMENT: [Your detailed reasoning, 2-4 sentences]\n\
             KEY_POINTS: [Bullet points of main considerations]\n\
             EVIDENCE_IDS: [trace-1, directive:task-key, policy:policy-1]",
            member.agent_id,
            member.role,
            proposal.title,
            proposal.description,
            proposal.complexity,
            proposal.urgency,
            self.format_stigmergic_context(proposal),
            member.role,
            member.participation_weight
        );

        let response = self.call_llm(&prompt).await?;

        // Parse response
        let (stance, content, key_points) = self.parse_argument_response(&response);

        let generation_time = start.elapsed().as_millis() as u64;
        let evidence = self.resolve_response_evidence(proposal, &response, 2);

        let argument = LLMArgument {
            agent_id: member.agent_id,
            role: member.role,
            stance,
            content,
            confidence: 0.8, // Could be extracted from LLM certainty indicators
            key_points,
            evidence,
            round: 0,
            responding_to: None,
            tokens_generated: response.split_whitespace().count(),
            generation_time_ms: generation_time,
        };

        // Cache the result
        self.argument_cache
            .put(member.role, &proposal_id, "opening", argument.clone());

        Ok(argument)
    }

    /// Generate rebuttal using LLM (with caching)
    async fn generate_rebuttal(
        &self,
        member: &CouncilMember,
        proposal: &Proposal,
        target: &LLMArgument,
    ) -> anyhow::Result<LLMArgument> {
        let start = std::time::Instant::now();

        // Check cache first (rebuttals depend on target argument)
        let proposal_id = format!(
            "{}:{:.2}:{:.2}",
            proposal.title, proposal.complexity, proposal.urgency
        );
        let target_hash = format!(
            "{:?}:{}",
            target.stance,
            &target.content[..target.content.len().min(50)]
        );
        if let Some(cached) = self.argument_cache.get(
            member.role,
            &format!("{}:{}", proposal_id, target_hash),
            "rebuttal",
        ) {
            return Ok(cached);
        }

        let prompt = format!(
            "You are Agent {} with role {:?} in a council deliberation.\n\n\
             PROPOSAL: {}\n\
             STIGMERGIC CONTEXT:\n{}\n\n\
             You are responding to Agent {}'s argument:\n\
             Stance: {:?}\n\
             Content: {}\n\
             Evidence IDs already cited: {}\n\n\
             TASK: Provide a rebuttal or constructive critique. Consider:\n\
             1. Weaknesses in their reasoning\n\
             2. Alternative perspectives\n\
             3. Additional risks or benefits they missed\n\
             4. Reuse or challenge concrete trace/directive/policy IDs when possible\n\n\
             Respond with:\n\
             ARGUMENT: [Your concise rebuttal, 2-3 sentences]\n\
             EVIDENCE_IDS: [trace-1, directive:task-key]\n\n\
             Be respectful but critical.",
            member.agent_id,
            member.role,
            proposal.title,
            self.format_stigmergic_context(proposal),
            target.agent_id,
            target.stance,
            target.content,
            self.format_evidence_ids(&target.evidence)
        );

        let response = self.call_llm(&prompt).await?;

        // Rebuttals are typically against the target's position
        let stance = match target.stance {
            Stance::For => Stance::Cautious,
            Stance::Against => Stance::Curious,
            _ => Stance::Against,
        };

        let generation_time = start.elapsed().as_millis() as u64;
        let evidence = self.resolve_response_evidence(proposal, &response, 2);

        let argument = LLMArgument {
            agent_id: member.agent_id,
            role: member.role,
            stance,
            content: self.extract_argument_text(&response),
            confidence: 0.75,
            key_points: vec![],
            evidence,
            round: 1,
            responding_to: Some(target.agent_id),
            tokens_generated: response.split_whitespace().count(),
            generation_time_ms: generation_time,
        };

        // Cache the result
        self.argument_cache.put(
            member.role,
            &format!("{}:{}", proposal_id, target_hash),
            "rebuttal",
            argument.clone(),
        );

        Ok(argument)
    }

    /// Generate synthesis of the entire debate using LLM
    async fn generate_debate_synthesis(&self, proposal: &Proposal) -> anyhow::Result<String> {
        // Collect all arguments
        let mut all_arguments = String::new();
        for round in &self.rounds {
            all_arguments.push_str(&format!(
                "\n### Round {} ({:?})\n",
                round.number, round.phase
            ));
            for arg in &round.arguments {
                all_arguments.push_str(&format!(
                    "Agent {} ({:?}): {:?} - {} | evidence_ids=[{}]\n",
                    arg.agent_id,
                    arg.role,
                    arg.stance,
                    arg.content,
                    self.format_evidence_ids(&arg.evidence)
                ));
            }
        }

        let prompt = format!(
            "You are the council synthesizer. Review the following debate and provide a summary.\n\n\
             PROPOSAL: {}\nDescription: {}\n\n\
             STIGMERGIC CONTEXT:\n{}\n\n\
             DEBATE ARGUMENTS:\n{}\n\n\
             TASK: Synthesize the key points of agreement, disagreement, and unresolved questions. \
             Provide a balanced summary that captures the essence of the deliberation and name the most relevant trace/directive/policy IDs when they matter. \
             Keep your synthesis to 3-5 sentences.",
            proposal.title,
            proposal.description,
            self.format_stigmergic_context(proposal),
            all_arguments
        );

        self.call_llm(&prompt).await
    }

    /// Render final decision using LLM analysis
    async fn render_llm_decision(
        &self,
        proposal: &Proposal,
        synthesis: &str,
    ) -> anyhow::Result<DecisionResult> {
        let prompt = format!(
            "You are the council decision maker. Based on the following deliberation synthesis, \
             make a final decision.\n\n\
             PROPOSAL: {}\nDescription: {}\n\n\
             STIGMERGIC CONTEXT:\n{}\n\n\
             DELIBERATION SYNTHESIS:\n{}\n\n\
             TASK: Determine the council's decision. Consider:\n\
             - The strength of arguments for and against\n\
             - Potential risks and benefits\n\
             - Whether the cited traces/directives/policy shifts indicate a reliable execution path\n\
             - Whether more information is needed\n\n\
             Respond EXACTLY in this format:\n\
             DECISION: [approve|reject|defer]\n\
             CONFIDENCE: [0.0-1.0]\n\
             REASONING: [One sentence explaining the decision]",
            proposal.title,
            proposal.description,
            self.format_stigmergic_context(proposal),
            synthesis
        );

        let response = self.call_llm(&prompt).await?;

        // Parse decision
        let (decision, confidence, _reasoning) = self.parse_decision_response(&response);

        let execution_plan = if matches!(decision, Decision::Approve) {
            Some(self.create_execution_plan(proposal))
        } else {
            None
        };

        Ok(DecisionResult {
            decision,
            confidence,
            execution_plan,
        })
    }

    /// Call the LLM via Ollama
    async fn call_llm(&self, prompt: &str) -> anyhow::Result<String> {
        use reqwest::Client;
        use serde_json::json;

        // Auto-detect model if set to "auto"
        let model = if self.config.model == "auto" {
            let tags_url = format!("{}/api/tags", self.config.endpoint);
            match reqwest::get(&tags_url).await {
                Ok(resp) => {
                    if let Ok(json) = resp.json::<serde_json::Value>().await {
                        json.get("models")
                            .and_then(|m| m.as_array())
                            .and_then(|arr| arr.first())
                            .and_then(|m| m.get("name"))
                            .and_then(|n| n.as_str())
                            .unwrap_or("llama3.2")
                            .to_string()
                    } else {
                        "llama3.2".to_string()
                    }
                }
                Err(_) => "llama3.2".to_string(),
            }
        } else {
            self.config.model.clone()
        };

        let mut builder = Client::builder();
        if self.config.timeout_secs > 0 {
            builder = builder.timeout(std::time::Duration::from_secs(self.config.timeout_secs));
        }
        let client = builder.build()?;

        let url = format!("{}/api/generate", self.config.endpoint);

        let request_body = json!({
            "model": model,
            "prompt": prompt,
            "stream": false,
            "options": {
                "temperature": self.config.temperature,
                "num_predict": self.config.max_tokens,
            }
        });

        let response = client.post(&url).json(&request_body).send().await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            anyhow::bail!("LLM API error: {}", error_text);
        }

        let response_json: serde_json::Value = response.json().await?;

        let generated_text = response_json
            .get("response")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        Ok(generated_text)
    }

    /// Parse LLM argument response
    fn parse_argument_response(&self, response: &str) -> (Stance, String, Vec<String>) {
        let mut stance = Stance::Neutral;
        let mut content = String::new();
        let mut key_points = Vec::new();

        for line in response.lines() {
            let line = line.trim();
            if line.starts_with("STANCE:") {
                let stance_str = line.trim_start_matches("STANCE:").trim().to_lowercase();
                stance = match stance_str.as_str() {
                    s if s.contains("favor") => Stance::For,
                    s if s.contains("against") => Stance::Against,
                    s if s.contains("cautious") => Stance::Cautious,
                    s if s.contains("explor") => Stance::Curious,
                    _ => Stance::Neutral,
                };
            } else if line.starts_with("ARGUMENT:") {
                content = line.trim_start_matches("ARGUMENT:").trim().to_string();
            } else if line.starts_with("EVIDENCE_IDS:") {
                continue;
            } else if line.starts_with("KEY_POINTS:") || line.starts_with("-") {
                let point = if line.starts_with("KEY_POINTS:") {
                    line.trim_start_matches("KEY_POINTS:").trim().to_string()
                } else {
                    line.trim_start_matches("-").trim().to_string()
                };
                if !point.is_empty() {
                    key_points.push(point);
                }
            } else if !line.is_empty() && content.is_empty() {
                // Fallback: use first non-empty line as content
                content = line.to_string();
            }
        }

        if content.is_empty() {
            content = response.to_string();
        }

        (stance, content, key_points)
    }

    /// Parse LLM decision response
    fn parse_decision_response(&self, response: &str) -> (Decision, f64, String) {
        let mut decision = Decision::Defer {
            reason: "Unable to parse LLM response".to_string(),
        };
        let mut confidence = 0.5;
        let mut reasoning = String::new();

        for line in response.lines() {
            let line = line.trim();
            if line.starts_with("DECISION:") {
                let decision_str = line.trim_start_matches("DECISION:").trim().to_lowercase();
                decision = match decision_str.as_str() {
                    d if d.contains("approve") => Decision::Approve,
                    d if d.contains("reject") => Decision::Reject,
                    _ => Decision::Defer {
                        reason: "Council could not reach consensus".to_string(),
                    },
                };
            } else if line.starts_with("CONFIDENCE:") {
                let conf_str = line.trim_start_matches("CONFIDENCE:").trim();
                confidence = conf_str.parse::<f64>().unwrap_or(0.5).clamp(0.0, 1.0);
            } else if line.starts_with("REASONING:") {
                reasoning = line.trim_start_matches("REASONING:").trim().to_string();
            }
        }

        if reasoning.is_empty() {
            reasoning = "Decision based on council deliberation".to_string();
        }

        (decision, confidence, reasoning)
    }

    fn create_execution_plan(&self, proposal: &Proposal) -> ExecutionPlan {
        let steps = vec![
            ExecutionStep {
                sequence: 1,
                description: format!("Initialize: {}", proposal.title),
                assigned_agent: Some(proposal.proposer),
                dependencies: vec![],
            },
            ExecutionStep {
                sequence: 2,
                description: "Validate implementation requirements".to_string(),
                assigned_agent: None,
                dependencies: vec![1],
            },
            ExecutionStep {
                sequence: 3,
                description: "Document changes".to_string(),
                assigned_agent: None,
                dependencies: vec![2],
            },
        ];

        ExecutionPlan {
            steps,
            estimated_duration_ms: 60000,
            rollback_strategy: Some("Revert to previous state".to_string()),
        }
    }

    fn format_stigmergic_context(&self, proposal: &Proposal) -> String {
        let Some(context) = &proposal.stigmergic_context else {
            return "none".to_string();
        };

        let mut lines = vec![format!(
            "- Directive confidence {:.2}; rationale: {}",
            context.confidence, context.rationale
        )];
        if let Some(agent_id) = context.preferred_agent {
            lines.push(format!("- Preferred agent: {agent_id}"));
        }
        if let Some(tool) = &context.preferred_tool {
            lines.push(format!("- Preferred tool: {:?}", tool));
        }
        if context.require_council_review {
            lines.push("- Policy requires council review".to_string());
        }
        if !context.graph_snapshot_bullets.is_empty() {
            lines.push("- Graph snapshot bullets:".to_string());
            lines.extend(
                context
                    .graph_snapshot_bullets
                    .iter()
                    .map(|bullet| format!("  * {bullet}")),
            );
        }
        let all_evidence = context.all_evidence();
        if !all_evidence.is_empty() {
            lines.push("- Evidence catalog:".to_string());
            lines.extend(
                all_evidence
                    .iter()
                    .map(|item| format!("  * {} => {}", item.id, item.summary)),
            );
        }
        if !context.graph_queries.is_empty() {
            lines.push("- Live graph queries:".to_string());
            lines.extend(context.graph_queries.iter().map(|query| {
                let evidence_ids = self.format_evidence_ids(&query.evidence);
                format!(
                    "  * {} | {} | results={}",
                    query.purpose,
                    query.query,
                    if evidence_ids.is_empty() {
                        "none".to_string()
                    } else {
                        evidence_ids
                    }
                )
            }));
        }
        lines.join("\n")
    }

    fn resolve_response_evidence(
        &self,
        proposal: &Proposal,
        response: &str,
        fallback_limit: usize,
    ) -> Vec<CouncilEvidence> {
        let Some(context) = &proposal.stigmergic_context else {
            return Vec::new();
        };

        let available = context.all_evidence();
        let requested_ids = self.extract_evidence_ids(response);
        let mut evidence = available
            .iter()
            .filter(|item| requested_ids.iter().any(|id| id == &item.id))
            .cloned()
            .collect::<Vec<_>>();

        if evidence.is_empty() {
            evidence = available.into_iter().take(fallback_limit).collect();
        }
        evidence
    }

    fn extract_evidence_ids(&self, response: &str) -> Vec<String> {
        let mut ids = Vec::new();
        for raw in response
            .split(|c: char| c.is_whitespace() || matches!(c, '[' | ']' | ',' | ';' | '(' | ')'))
        {
            let token = raw.trim_matches(|c: char| c == '.' || c == '"' || c == '\'');
            if token.starts_with("trace-")
                || token.starts_with("directive:")
                || token.starts_with("policy:")
                || token.starts_with("query:")
            {
                ids.push(token.to_string());
            }
        }
        ids.sort();
        ids.dedup();
        ids
    }

    fn format_evidence_ids(&self, evidence: &[CouncilEvidence]) -> String {
        evidence
            .iter()
            .map(|item| item.id.clone())
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn extract_argument_text(&self, response: &str) -> String {
        for line in response.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("ARGUMENT:") {
                return trimmed.trim_start_matches("ARGUMENT:").trim().to_string();
            }
        }
        response.trim().to_string()
    }

    fn collect_decision_metadata(&self, proposal: &Proposal) -> CouncilDecisionMetadata {
        let mut metadata = proposal
            .stigmergic_context
            .as_ref()
            .map(|ctx| ctx.audit_metadata())
            .unwrap_or_default();
        for round in &self.rounds {
            for argument in &round.arguments {
                for evidence in &argument.evidence {
                    metadata.record_evidence(evidence);
                }
            }
        }
        metadata.dedupe();
        metadata
    }
}

struct DecisionResult {
    decision: Decision,
    confidence: f64,
    execution_plan: Option<ExecutionPlan>,
}

fn current_timestamp() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Debate statistics for analysis
#[derive(Clone, Debug)]
pub struct DebateStats {
    pub total_arguments: usize,
    pub avg_confidence: f64,
    pub avg_generation_time_ms: u64,
    pub total_tokens: usize,
    pub stance_distribution: std::collections::HashMap<String, usize>,
}

impl LLMDebateCouncil {
    /// Get statistics about the debate
    pub fn get_stats(&self) -> DebateStats {
        let mut total_args = 0;
        let mut total_confidence = 0.0;
        let mut total_time = 0u64;
        let mut total_tokens = 0usize;
        let mut stance_dist: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();

        for round in &self.rounds {
            for arg in &round.arguments {
                total_args += 1;
                total_confidence += arg.confidence;
                total_time += arg.generation_time_ms;
                total_tokens += arg.tokens_generated;
                *stance_dist
                    .entry(arg.stance.as_str().to_string())
                    .or_insert(0) += 1;
            }
        }

        DebateStats {
            total_arguments: total_args,
            avg_confidence: if total_args > 0 {
                total_confidence / total_args as f64
            } else {
                0.0
            },
            avg_generation_time_ms: if total_args > 0 {
                total_time / total_args as u64
            } else {
                0
            },
            total_tokens,
            stance_distribution: stance_dist,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::council::{
        CouncilEvidence, CouncilEvidenceKind, CouncilGraphQuery, StigmergicCouncilContext,
    };

    #[test]
    fn test_parse_argument_response() {
        let council = LLMDebateCouncil::new(
            uuid::Uuid::new_v4(),
            vec![],
            LLMDeliberationConfig::default(),
        );

        let response = r#"
STANCE: in_favor
ARGUMENT: This proposal enhances our capabilities significantly.
KEY_POINTS: - Improves efficiency
- Reduces risk
"#;

        let (stance, content, key_points) = council.parse_argument_response(response);
        assert_eq!(stance, Stance::For);
        assert!(content.contains("enhances"));
        assert_eq!(key_points.len(), 2);
    }

    #[test]
    fn test_parse_decision_response() {
        let council = LLMDebateCouncil::new(
            uuid::Uuid::new_v4(),
            vec![],
            LLMDeliberationConfig::default(),
        );

        let response = r#"
DECISION: approve
CONFIDENCE: 0.85
REASONING: Strong arguments for with minimal risks.
"#;

        let (decision, confidence, reasoning) = council.parse_decision_response(response);
        assert!(matches!(decision, Decision::Approve));
        assert!((confidence - 0.85).abs() < 0.01);
        assert!(reasoning.contains("Strong arguments"));
    }

    #[test]
    fn test_extracts_stigmergic_evidence_ids_from_response() {
        let council = LLMDebateCouncil::new(
            uuid::Uuid::new_v4(),
            vec![],
            LLMDeliberationConfig::default(),
        );
        let proposal = Proposal::new("p1", "Compile Code", "Compile code safely", 1)
            .with_stigmergic_context(StigmergicCouncilContext {
                preferred_agent: Some(1),
                preferred_tool: None,
                confidence: 0.9,
                require_council_review: false,
                rationale: "recent traces prefer agent 1".into(),
                evidence: vec![
                    CouncilEvidence {
                        id: "trace-42".into(),
                        kind: CouncilEvidenceKind::Trace,
                        summary: "trace-42 says agent 1 compiled successfully".into(),
                    },
                    CouncilEvidence {
                        id: "directive:compile_code".into(),
                        kind: CouncilEvidenceKind::Directive,
                        summary: "directive prefers agent 1".into(),
                    },
                ],
                graph_snapshot_bullets: vec!["agent 1 is currently trusted".into()],
                graph_queries: vec![CouncilGraphQuery {
                    purpose: "recent task traces".into(),
                    query: "MATCH (t:StigmergicTrace) RETURN t LIMIT 1".into(),
                    evidence: vec![],
                }],
            });

        let response = "ARGUMENT: We should follow trace-42 and the directive.\nEVIDENCE_IDS: [trace-42, directive:compile_code]";
        let evidence = council.resolve_response_evidence(&proposal, response, 2);
        assert_eq!(evidence.len(), 2);
        assert!(evidence.iter().any(|item| item.id == "trace-42"));
        assert!(evidence
            .iter()
            .any(|item| item.id == "directive:compile_code"));
    }
}
