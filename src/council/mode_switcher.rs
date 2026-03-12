//! Mode switcher - automatically selects council mode based on decision context.
//!
//! The mode switcher analyzes proposals and available agents to determine
//! whether to use Debate, Orchestrate, or Simple mode.

use super::{CouncilMember, Proposal};
use crate::agent::Role;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Available council modes
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum CouncilMode {
    /// Full deliberation with structured argumentation
    Debate,
    /// Hierarchical command with sub-task delegation
    Orchestrate,
    /// Direct voting with minimal overhead
    Simple,
    /// LLM-powered deliberation with genuine agent reasoning
    /// (higher quality, increased latency)
    LLMDeliberation,
}

/// Configuration for mode selection thresholds
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModeConfig {
    /// Complexity threshold for Debate mode (0-1)
    pub debate_complexity_threshold: f64,
    /// Urgency threshold for Orchestrate mode (0-1)
    pub orchestrate_urgency_threshold: f64,
    /// Complexity threshold for LLM Deliberation (very high complexity)
    pub llm_deliberation_complexity_threshold: f64,
    /// Minimum agents needed for Debate mode
    pub min_agents_for_debate: usize,
    /// Maximum agents for Simple mode (larger = use Debate/Orchestrate)
    pub max_agents_for_simple: usize,
    /// Historical window for mode effectiveness tracking
    pub history_window_size: usize,
    /// Role diversity weight in mode selection
    pub diversity_weight: f64,
    /// Whether LLM deliberation is enabled (requires LLM endpoint)
    pub llm_deliberation_enabled: bool,
    /// Latency budget in ms - if exceeded, fall back to heuristic debate
    pub llm_latency_budget_ms: u64,
}

impl Default for ModeConfig {
    fn default() -> Self {
        Self {
            debate_complexity_threshold: 0.6,
            orchestrate_urgency_threshold: 0.7,
            llm_deliberation_complexity_threshold: 0.8,
            min_agents_for_debate: 4,
            max_agents_for_simple: 3,
            history_window_size: 100,
            diversity_weight: 0.3,
            llm_deliberation_enabled: true,
            llm_latency_budget_ms: 10000, // 10 seconds
        }
    }
}

/// Tracks mode switching decisions and their effectiveness
pub struct ModeSwitcher {
    config: ModeConfig,
    history: VecDeque<ModeSwitchEvent>,
    effectiveness_scores: [f64; 4], // Debate, Orchestrate, Simple, LLMDeliberation
}

/// Per-mode score breakdown used for runtime visualization and debugging.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModeScoreBreakdown {
    pub raw: f64,
    pub adjusted: f64,
}

/// Full mode-selection report with features and normalized confidence.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModeSelectionReport {
    pub selected_mode: CouncilMode,
    pub confidence: f64,
    pub complexity: f64,
    pub urgency: f64,
    pub agent_count: usize,
    pub role_diversity: f64,
    pub debate: ModeScoreBreakdown,
    pub orchestrate: ModeScoreBreakdown,
    pub simple: ModeScoreBreakdown,
    pub llm: ModeScoreBreakdown,
}

/// Record of a mode switch decision
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModeSwitchEvent {
    pub timestamp: u64,
    pub proposal_id: String,
    pub selected_mode: CouncilMode,
    pub complexity: f64,
    pub urgency: f64,
    pub agent_count: usize,
    pub role_diversity: f64,
    /// Outcome satisfaction score (0-1), populated after decision execution
    pub outcome_score: Option<f64>,
}

impl ModeSwitcher {
    pub fn new(config: ModeConfig) -> Self {
        Self {
            config,
            history: VecDeque::new(),
            effectiveness_scores: [0.75, 0.75, 0.75, 0.75], // Start with neutral scores
        }
    }

    /// Select the appropriate council mode for a proposal
    pub fn select_mode(
        &self,
        proposal: &Proposal,
        available_agents: &[CouncilMember],
    ) -> CouncilMode {
        self.select_mode_with_report(proposal, available_agents)
            .selected_mode
    }

    /// Select mode and return score breakdown for observability/UI.
    pub fn select_mode_with_report(
        &self,
        proposal: &Proposal,
        available_agents: &[CouncilMember],
    ) -> ModeSelectionReport {
        let agent_count = available_agents.len();
        let diversity = self.calculate_diversity(available_agents);

        // Calculate mode suitability scores
        let debate_score = self.score_debate_suitability(proposal, agent_count, diversity);
        let orchestrate_score =
            self.score_orchestrate_suitability(proposal, agent_count, diversity);
        let simple_score = self.score_simple_suitability(proposal, agent_count, diversity);
        let llm_score = self.score_llm_suitability(proposal, agent_count, diversity);

        // Apply historical effectiveness adjustments
        let adjusted_scores = [
            debate_score * self.effectiveness_scores[0],
            orchestrate_score * self.effectiveness_scores[1],
            simple_score * self.effectiveness_scores[2],
            llm_score * self.effectiveness_scores[3],
        ];

        // Select mode with highest adjusted score
        let (max_idx, max_score) = adjusted_scores
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .map(|(i, s)| (i, *s))
            .unwrap_or((2, adjusted_scores[2])); // Default to Simple

        let selected_mode = match max_idx {
            0 => CouncilMode::Debate,
            1 => CouncilMode::Orchestrate,
            2 => CouncilMode::Simple,
            3 => CouncilMode::LLMDeliberation,
            _ => CouncilMode::Simple,
        };

        let score_sum = adjusted_scores.iter().sum::<f64>();
        let confidence = if score_sum > 0.0 {
            (max_score / score_sum).clamp(0.0, 1.0)
        } else {
            0.25
        };

        ModeSelectionReport {
            selected_mode,
            confidence,
            complexity: proposal.complexity,
            urgency: proposal.urgency,
            agent_count,
            role_diversity: diversity,
            debate: ModeScoreBreakdown {
                raw: debate_score,
                adjusted: adjusted_scores[0],
            },
            orchestrate: ModeScoreBreakdown {
                raw: orchestrate_score,
                adjusted: adjusted_scores[1],
            },
            simple: ModeScoreBreakdown {
                raw: simple_score,
                adjusted: adjusted_scores[2],
            },
            llm: ModeScoreBreakdown {
                raw: llm_score,
                adjusted: adjusted_scores[3],
            },
        }
    }

    /// Score LLM deliberation suitability
    fn score_llm_suitability(
        &self,
        proposal: &Proposal,
        agent_count: usize,
        diversity: f64,
    ) -> f64 {
        // LLM deliberation requires high complexity and low urgency
        // (because it's slow but produces richer reasoning)

        if !self.config.llm_deliberation_enabled {
            return 0.0;
        }

        let mut score = 0.0;

        // Very high complexity favors LLM deliberation
        if proposal.complexity >= self.config.llm_deliberation_complexity_threshold {
            score += proposal.complexity * 0.5;
        }

        // Need enough agents for meaningful deliberation
        if agent_count >= self.config.min_agents_for_debate {
            score += 0.2;
        }

        // Diversity is important for rich deliberation
        score += diversity * 0.2;

        // High urgency is a penalty (LLM is slow)
        score -= proposal.urgency * 0.4;

        score.max(0.0)
    }

    /// Record the outcome of a mode selection for learning
    pub fn record_outcome(&mut self, proposal_id: &str, success_score: f64) {
        for event in self.history.iter_mut() {
            if event.proposal_id == proposal_id {
                event.outcome_score = Some(success_score);
                self.update_effectiveness_scores();
                break;
            }
        }
    }

    fn score_debate_suitability(
        &self,
        proposal: &Proposal,
        agent_count: usize,
        diversity: f64,
    ) -> f64 {
        let mut score = 0.0;

        // Complexity factor - high complexity favors debate
        if proposal.complexity >= self.config.debate_complexity_threshold {
            score += proposal.complexity * 0.4;
        }

        // Agent count factor - enough agents for rich debate
        if agent_count >= self.config.min_agents_for_debate {
            score += 0.2;
        }

        // Diversity factor - diverse perspectives improve debate
        score += diversity * self.config.diversity_weight;

        // Urgency penalty - debate takes time
        score -= proposal.urgency * 0.2;

        score.max(0.0)
    }

    fn score_orchestrate_suitability(
        &self,
        proposal: &Proposal,
        agent_count: usize,
        diversity: f64,
    ) -> f64 {
        let mut score = 0.0;

        // Urgency factor - high urgency favors orchestration
        if proposal.urgency >= self.config.orchestrate_urgency_threshold {
            score += proposal.urgency * 0.4;
        }

        // Complexity factor - complex tasks benefit from structured delegation
        score += proposal.complexity * 0.2;

        // Agent count factor - need enough agents for task distribution
        if agent_count >= 3 {
            score += (agent_count as f64 / 10.0).min(0.2);
        }

        // Role diversity helps with task assignment
        score += diversity * 0.2;

        score.max(0.0)
    }

    fn score_simple_suitability(
        &self,
        proposal: &Proposal,
        agent_count: usize,
        _diversity: f64,
    ) -> f64 {
        let mut score = 0.0;

        // Low complexity favors simple mode
        score += (1.0 - proposal.complexity) * 0.4;

        // Low urgency favors simple mode (no time pressure)
        score += (1.0 - proposal.urgency) * 0.2;

        // Small group favors simple mode
        if agent_count <= self.config.max_agents_for_simple {
            score += 0.3;
        }

        // Routine proposals (keywords)
        let routine_keywords = ["routine", "standard", "minor", "update", "fix"];
        let desc_lower = proposal.description.to_lowercase();
        if routine_keywords.iter().any(|kw| desc_lower.contains(kw)) {
            score += 0.2;
        }

        score.max(0.0)
    }

    fn calculate_diversity(&self, agents: &[CouncilMember]) -> f64 {
        let total = agents.len() as f64;
        if total <= 1.0 {
            return 0.0;
        }

        // Count role occurrences
        let mut role_counts = [0; 6]; // One for each Role variant
        for agent in agents {
            let idx = match agent.role {
                Role::Architect => 0,
                Role::Catalyst => 1,
                Role::Chronicler => 2,
                Role::Critic => 3,
                Role::Explorer => 4,
                Role::Coder => 5,
            };
            role_counts[idx] += 1;
        }

        // Calculate Gini-Simpson diversity index
        let mut diversity = 1.0;
        for count in role_counts.iter() {
            let p = *count as f64 / total;
            diversity -= p * p;
        }

        diversity
    }

    fn update_effectiveness_scores(&mut self) {
        // Calculate rolling average effectiveness per mode
        let mut mode_scores: [Vec<f64>; 4] = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];

        for event in self
            .history
            .iter()
            .rev()
            .take(self.config.history_window_size)
        {
            if let Some(score) = event.outcome_score {
                let idx = match event.selected_mode {
                    CouncilMode::Debate => 0,
                    CouncilMode::Orchestrate => 1,
                    CouncilMode::Simple => 2,
                    CouncilMode::LLMDeliberation => 3,
                };
                mode_scores[idx].push(score);
            }
        }

        for i in 0..4 {
            if !mode_scores[i].is_empty() {
                let avg: f64 = mode_scores[i].iter().sum::<f64>() / mode_scores[i].len() as f64;
                // Smooth update with exponential moving average
                self.effectiveness_scores[i] = 0.7 * self.effectiveness_scores[i] + 0.3 * avg;
            }
        }
    }

    /// Get current effectiveness scores for each mode
    pub fn effectiveness_scores(&self) -> &[f64; 4] {
        &self.effectiveness_scores
    }

    /// Get recent mode selection history
    pub fn recent_history(&self, n: usize) -> Vec<&ModeSwitchEvent> {
        self.history.iter().rev().take(n).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_member(id: u64, role: Role) -> CouncilMember {
        CouncilMember {
            agent_id: id,
            role,
            expertise_score: 0.7,
            participation_weight: 1.0,
        }
    }

    #[test]
    fn test_simple_mode_for_low_complexity() {
        let config = ModeConfig::default();
        let switcher = ModeSwitcher::new(config);

        let agents = vec![
            create_test_member(1, Role::Architect),
            create_test_member(2, Role::Catalyst),
        ];

        let proposal = Proposal {
            id: "test1".to_string(),
            title: "Simple update".to_string(),
            description: "Routine maintenance task".to_string(),
            proposer: 1,
            proposed_at: 0,
            complexity: 0.2,
            urgency: 0.3,
            required_roles: vec![],
            task_key: None,
            stigmergic_context: None,
        };

        let mode = switcher.select_mode(&proposal, &agents);
        assert_eq!(mode, CouncilMode::Simple);
    }

    #[test]
    fn test_debate_mode_for_high_complexity() {
        let config = ModeConfig::default();
        let switcher = ModeSwitcher::new(config);

        let agents = vec![
            create_test_member(1, Role::Architect),
            create_test_member(2, Role::Catalyst),
            create_test_member(3, Role::Critic),
            create_test_member(4, Role::Chronicler),
        ];

        let proposal = Proposal {
            id: "test2".to_string(),
            title: "Complex refactoring".to_string(),
            description: "Multi-system integration with recursive synthesis".to_string(),
            proposer: 1,
            proposed_at: 0,
            complexity: 0.8,
            urgency: 0.4,
            required_roles: vec![],
            task_key: None,
            stigmergic_context: None,
        };

        let mode = switcher.select_mode(&proposal, &agents);
        assert_eq!(mode, CouncilMode::Debate);
    }

    #[test]
    fn test_orchestrate_mode_for_high_urgency() {
        let config = ModeConfig::default();
        let switcher = ModeSwitcher::new(config);

        let agents = vec![
            create_test_member(1, Role::Architect),
            create_test_member(2, Role::Catalyst),
            create_test_member(3, Role::Critic),
        ];

        let proposal = Proposal {
            id: "test3".to_string(),
            title: "Urgent fix".to_string(),
            description: "Critical bug fix needed immediately".to_string(),
            proposer: 1,
            proposed_at: 0,
            complexity: 0.5,
            urgency: 0.9,
            required_roles: vec![],
            task_key: None,
            stigmergic_context: None,
        };

        let mode = switcher.select_mode(&proposal, &agents);
        assert_eq!(mode, CouncilMode::Orchestrate);
    }
}
