//! Token-budget-aware context packer.
//!
//! Honcho's context API returns messages + conclusions + summaries packed to
//! fit exactly N tokens. This module implements the same concept for HSM-II:
//! given a token budget, fill it greedily with the most valuable context —
//! peer representation first, then recall results, then recent messages.
//!
//! Token counting uses a simple heuristic (4 chars ≈ 1 token) which avoids
//! pulling in a full tokenizer library while being accurate enough for budgeting.

use serde::{Deserialize, Serialize};

use crate::honcho::user_representation::UserRepresentation;
use crate::memory::RecallResult;

// ── Token estimation ──────────────────────────────────────────────────────────

/// Estimate token count from byte length (4 chars ≈ 1 token).
pub fn estimate_tokens(text: &str) -> usize {
    (text.len() + 3) / 4
}

// ── ContextBudget ─────────────────────────────────────────────────────────────

/// Controls how the available token budget is allocated across context categories.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContextBudget {
    /// Total token ceiling.
    pub total: usize,
    /// Max tokens for the peer's UserRepresentation block.
    pub peer_representation: usize,
    /// Max tokens for EntitySummary recall results.
    pub entity_summaries: usize,
    /// Max tokens for recent conversation messages.
    pub recent_messages: usize,
    /// Max tokens for one-line conclusions / extracted facts.
    pub conclusions: usize,
}

impl ContextBudget {
    /// Sensible defaults for a 4 096-token window.
    pub fn default_4k() -> Self {
        Self {
            total: 3_500,
            peer_representation: 600,
            entity_summaries: 800,
            recent_messages: 1_500,
            conclusions: 600,
        }
    }

    /// Sensible defaults for an 8 192-token window.
    pub fn default_8k() -> Self {
        Self {
            total: 7_500,
            peer_representation: 1_000,
            entity_summaries: 2_000,
            recent_messages: 3_000,
            conclusions: 1_500,
        }
    }

    pub fn from_total(total: usize) -> Self {
        // Proportional split: 15 / 25 / 40 / 20
        Self {
            total,
            peer_representation: total * 15 / 100,
            entity_summaries: total * 25 / 100,
            recent_messages: total * 40 / 100,
            conclusions: total * 20 / 100,
        }
    }
}

// ── PackedMessage ─────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PackedMessage {
    pub role: String,
    pub content: String,
    pub token_estimate: usize,
}

impl PackedMessage {
    pub fn new(role: impl Into<String>, content: impl Into<String>) -> Self {
        let content = content.into();
        let token_estimate = estimate_tokens(&content);
        Self {
            role: role.into(),
            content,
            token_estimate,
        }
    }
}

// ── PackedContext ─────────────────────────────────────────────────────────────

/// The assembled context bundle returned by `PackedContextBuilder::build()`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PackedContext {
    /// Peer representation block (may be empty for new peers).
    pub peer_representation: Option<String>,
    /// Relevant EntitySummary recall results.
    pub entity_summaries: Vec<String>,
    /// Recent conversation messages, newest-last, fitting within budget.
    pub messages: Vec<PackedMessage>,
    /// Extracted conclusion bullets (one-liners from memory extracts).
    pub conclusions: Vec<String>,
    /// Estimated tokens consumed.
    pub token_count: usize,
    /// The budget this context was packed against.
    pub budget: usize,
}

impl PackedContext {
    /// Render the full context as a markdown string for injection into a system prompt.
    pub fn render(&self) -> String {
        let mut out = String::new();

        if let Some(repr) = &self.peer_representation {
            out.push_str(repr);
            out.push_str("\n---\n\n");
        }

        if !self.entity_summaries.is_empty() {
            out.push_str("## Entity Summaries\n");
            for s in &self.entity_summaries {
                out.push_str(s);
                out.push('\n');
            }
            out.push('\n');
        }

        if !self.conclusions.is_empty() {
            out.push_str("## Key conclusions from prior sessions\n");
            for c in &self.conclusions {
                out.push_str("- ");
                out.push_str(c);
                out.push('\n');
            }
            out.push('\n');
        }

        out
    }
}

// ── PackedContextBuilder ──────────────────────────────────────────────────────

/// Builder that fills a `PackedContext` within a fixed token budget.
pub struct PackedContextBuilder {
    budget: ContextBudget,
    peer_repr: Option<UserRepresentation>,
    entity_summaries: Vec<RecallResult>,
    messages: Vec<(String, String)>, // (role, content)
    conclusions: Vec<String>,
}

impl PackedContextBuilder {
    pub fn new(budget: ContextBudget) -> Self {
        Self {
            budget,
            peer_repr: None,
            entity_summaries: Vec::new(),
            messages: Vec::new(),
            conclusions: Vec::new(),
        }
    }

    pub fn with_peer_representation(mut self, repr: UserRepresentation) -> Self {
        self.peer_repr = Some(repr);
        self
    }

    pub fn with_entity_summaries(mut self, results: Vec<RecallResult>) -> Self {
        self.entity_summaries = results;
        self
    }

    /// Add messages oldest-first; the builder will include as many as the budget allows.
    pub fn with_messages(mut self, messages: Vec<(String, String)>) -> Self {
        self.messages = messages;
        self
    }

    pub fn with_conclusions(mut self, conclusions: Vec<String>) -> Self {
        self.conclusions = conclusions;
        self
    }

    /// Greedily pack context into the token budget.
    pub fn build(self) -> PackedContext {
        let mut token_count = 0usize;
        let mut ctx = PackedContext {
            peer_representation: None,
            entity_summaries: Vec::new(),
            messages: Vec::new(),
            conclusions: Vec::new(),
            token_count: 0,
            budget: self.budget.total,
        };

        // 1. Peer representation (highest priority)
        if let Some(repr) = self.peer_repr {
            let block = repr.render_context(self.budget.peer_representation * 4); // bytes
            let tokens = estimate_tokens(&block);
            if tokens <= self.budget.peer_representation && token_count + tokens <= self.budget.total {
                ctx.peer_representation = Some(block);
                token_count += tokens;
            }
        }

        // 2. Entity summaries (sorted by recall score descending)
        let mut es = self.entity_summaries;
        es.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        let mut es_tokens = 0usize;
        for recall in es {
            let text = recall.entry.abstract_l0
                .as_deref()
                .unwrap_or(&recall.entry.content)
                .to_string();
            let t = estimate_tokens(&text);
            if es_tokens + t <= self.budget.entity_summaries
                && token_count + t <= self.budget.total
            {
                ctx.entity_summaries.push(text);
                es_tokens += t;
                token_count += t;
            }
        }

        // 3. Conclusions (newest first, trimmed to budget)
        let mut conc_tokens = 0usize;
        for c in self.conclusions.into_iter().rev() {
            let t = estimate_tokens(&c);
            if conc_tokens + t <= self.budget.conclusions
                && token_count + t <= self.budget.total
            {
                ctx.conclusions.push(c);
                conc_tokens += t;
                token_count += t;
            }
        }
        ctx.conclusions.reverse(); // restore chronological order

        // 4. Messages: fit as many recent messages as possible (newest last)
        let mut msg_tokens = 0usize;
        let msg_budget = self.budget.recent_messages.min(self.budget.total - token_count);
        // Walk from newest to oldest, then reverse for final output
        let mut packed_msgs: Vec<PackedMessage> = Vec::new();
        for (role, content) in self.messages.into_iter().rev() {
            let pm = PackedMessage::new(role, content);
            if msg_tokens + pm.token_estimate <= msg_budget {
                msg_tokens += pm.token_estimate;
                packed_msgs.push(pm);
            } else {
                break;
            }
        }
        packed_msgs.reverse();
        token_count += msg_tokens;
        ctx.messages = packed_msgs;

        ctx.token_count = token_count;
        ctx
    }
}
