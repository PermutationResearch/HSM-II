//! Context manager for situation-aware skill retrieval.
//!
//! Tracks current system state and scores skills by relevance to that state.

use crate::agent::{AgentId, Role};
use crate::skill::SkillLevel;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

/// Snapshot of current system context
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContextSnapshot {
    pub timestamp: u64,
    pub active_agents: Vec<AgentId>,
    pub dominant_roles: Vec<Role>,
    pub current_goals: Vec<String>,
    pub recent_skills_used: Vec<String>,
    pub system_load: f64,     // 0-1
    pub error_rate: f64,      // 0-1
    pub coherence_score: f64, // 0-1
}

impl Default for ContextSnapshot {
    fn default() -> Self {
        Self {
            timestamp: 0,
            active_agents: Vec::new(),
            dominant_roles: Vec::new(),
            current_goals: Vec::new(),
            recent_skills_used: Vec::new(),
            system_load: 0.5,
            error_rate: 0.0,
            coherence_score: 1.0,
        }
    }
}

/// Manages context snapshots and relevance scoring
pub struct ContextManager {
    current: ContextSnapshot,
    history: VecDeque<ContextSnapshot>,
    skill_context_history: HashMap<String, Vec<ContextUsageRecord>>,
    max_history: usize,
}

#[derive(Clone, Debug)]
struct ContextUsageRecord {
    context: ContextSnapshot,
    success: bool,
    _timestamp: u64,
}

impl ContextManager {
    pub fn new() -> Self {
        Self {
            current: ContextSnapshot::default(),
            history: VecDeque::with_capacity(100),
            skill_context_history: HashMap::new(),
            max_history: 100,
        }
    }

    /// Update current context
    pub fn update(&mut self, snapshot: ContextSnapshot) {
        // Archive current to history
        if self.current.timestamp > 0 {
            self.history.push_back(self.current.clone());
            if self.history.len() > self.max_history {
                self.history.pop_front();
            }
        }

        self.current = snapshot;
    }

    /// Record skill usage in a context
    pub fn record_usage(&mut self, skill_id: &str, success: bool, context: ContextSnapshot) {
        let record = ContextUsageRecord {
            context,
            success,
            _timestamp: current_timestamp(),
        };

        self.skill_context_history
            .entry(skill_id.to_string())
            .or_insert_with(Vec::new)
            .push(record);
    }

    /// Calculate relevance score for a skill in the current context
    pub fn relevance_score(&self, skill_id: &str, context: &ContextSnapshot) -> f64 {
        let mut score = 0.5; // Base score

        // Check skill usage history
        if let Some(history) = self.skill_context_history.get(skill_id) {
            // Score based on similar past contexts
            for record in history.iter().rev().take(10) {
                let context_similarity = self.context_similarity(context, &record.context);
                let success_bonus = if record.success { 0.2 } else { -0.1 };
                score += context_similarity * success_bonus;
            }
        }

        // Boost skills that match current goals
        if let Some(last_goal) = context.current_goals.last() {
            // Simple keyword matching (in production, use embeddings)
            if skill_id.to_lowercase().contains(&last_goal.to_lowercase()) {
                score += 0.15;
            }
        }

        // Adjust based on system state
        if context.error_rate > 0.1 {
            // Boost recovery/error-handling skills
            if skill_id.contains("recover") || skill_id.contains("fix") {
                score += 0.2;
            }
        }

        if context.system_load > 0.8 {
            // Boost efficiency skills under load
            if skill_id.contains("optimize") || skill_id.contains("cache") {
                score += 0.15;
            }
        }

        score.clamp(0.0, 1.0)
    }

    /// Get current context
    pub fn current(&self) -> &ContextSnapshot {
        &self.current
    }

    /// Get context history
    pub fn history(&self) -> &VecDeque<ContextSnapshot> {
        &self.history
    }

    /// Calculate similarity between two contexts (0-1)
    fn context_similarity(&self, a: &ContextSnapshot, b: &ContextSnapshot) -> f64 {
        let mut similarity = 0.0;

        // Role overlap
        let role_overlap = a
            .dominant_roles
            .iter()
            .filter(|r| b.dominant_roles.contains(r))
            .count() as f64;
        similarity += (role_overlap / 5.0).min(1.0) * 0.3;

        // Goal overlap
        let goal_overlap = a
            .current_goals
            .iter()
            .filter(|g| b.current_goals.contains(g))
            .count() as f64;
        similarity += (goal_overlap / 3.0).min(1.0) * 0.3;

        // System state similarity
        let load_diff = (a.system_load - b.system_load).abs();
        let error_diff = (a.error_rate - b.error_rate).abs();
        similarity += (1.0 - load_diff) * 0.2;
        similarity += (1.0 - error_diff) * 0.2;

        similarity
    }
}

impl Default for ContextManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Scores skills by relevance to context
pub struct RelevanceScorer;

impl RelevanceScorer {
    /// Score a skill's relevance to a goal
    pub fn goal_relevance(skill: &crate::skill::Skill, goal: &str) -> f64 {
        let goal_lower = goal.to_lowercase();
        let title_lower = skill.title.to_lowercase();
        let principle_lower = skill.principle.to_lowercase();

        let mut score = 0.0;

        // Title match
        if title_lower.contains(&goal_lower) {
            score += 0.4;
        }

        // Principle match
        if principle_lower.contains(&goal_lower) {
            score += 0.3;
        }

        // Applicability conditions match
        for condition in &skill.when_to_apply {
            if condition.predicate.to_lowercase().contains(&goal_lower) {
                score += 0.1;
            }
        }

        // Confidence bonus
        score += skill.confidence * 0.2;

        score.min(1.0)
    }

    /// Score skill level appropriateness for context
    pub fn level_appropriateness(skill: &crate::skill::Skill, expertise_level: f64) -> f64 {
        match skill.level {
            SkillLevel::General => 0.8, // Always appropriate
            SkillLevel::RoleSpecific(_) => 0.7,
            SkillLevel::TaskSpecific(_) => {
                if expertise_level > 0.7 {
                    0.9 // High expertise benefits from specific skills
                } else {
                    0.5 // Low expertise may struggle with specific skills
                }
            }
        }
    }
}

fn current_timestamp() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
