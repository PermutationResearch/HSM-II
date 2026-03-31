//! Rich per-turn traces for HSM eval (retrieval ranks, skill pick, context preview).

use serde::{Deserialize, Serialize};

/// One belief row after ranking (before character budget trim).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BeliefRankEntry {
    pub belief_index: usize,
    pub score: f64,
    pub source_task: String,
    pub preview: String,
}

/// Serializable summary of what HSM injected and selected for one turn.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HsmTurnTrace {
    pub task_id: String,
    pub turn_index: usize,
    pub session: u32,
    pub requires_recall: bool,
    pub selected_skill_id: Option<String>,
    pub selected_skill_domain: Option<String>,
    pub belief_ranks: Vec<BeliefRankEntry>,
    pub session_summaries_injected: Vec<u32>,
    pub injected_char_len: usize,
    pub injected_preview: String,
    /// True when in-session history was folded via snip-style compaction this turn.
    #[serde(default)]
    pub session_compaction_applied: bool,
    /// Messages in the active session buffer after compaction (before the current user turn is appended for the call).
    #[serde(default)]
    pub session_history_len: usize,
}

/// Result of context ranking: text injected into the prompt plus trace metadata.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RankedContextResult {
    pub injected_text: String,
    pub belief_ranks: Vec<BeliefRankEntry>,
    pub session_summary_sessions: Vec<u32>,
}

impl RankedContextResult {
    pub fn empty() -> Self {
        Self {
            injected_text: String::new(),
            belief_ranks: Vec::new(),
            session_summary_sessions: Vec::new(),
        }
    }
}
