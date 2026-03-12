use serde::{Deserialize, Serialize};

use crate::consensus::{AssociationType, EmergentAssociation};

pub type AgentId = u64;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Agent {
    pub id: AgentId,
    pub drives: Drives,
    pub learning_rate: f64,
    pub description: String,
    pub role: Role,
    pub bid_bias: f64,
    /// Thermodynamic Wage Metric: JW = E × η × W
    /// E  = energy expenditure  (growth × learning_rate)
    /// η  = efficiency factor   (harmony × coherence_stability)
    /// W  = work output         (curiosity × transcendence × network_amplifier)
    #[serde(default)]
    pub jw: f64,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum Role {
    Architect,
    Catalyst,
    Chronicler,
    /// Evidence rigor, risk assessment — skeptical evaluator
    Critic,
    /// Diversity probe, novelty seeking — encourages exploration
    Explorer,
    /// Code specialist — uses read/bash/edit/write/grep/find/ls tools
    Coder,
}

impl Role {
    /// Total number of distinct roles (for diversity calculations)
    pub const COUNT: usize = 6;
}

impl Default for Role {
    fn default() -> Self {
        Role::Architect
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Drives {
    pub curiosity: f64,
    pub harmony: f64,
    pub growth: f64,
    pub transcendence: f64,
}

impl Agent {
    pub fn new(id: AgentId, drives: Drives, learning_rate: f64) -> Self {
        Self {
            id,
            drives,
            learning_rate,
            description: String::new(),
            role: Role::Architect,
            bid_bias: 1.0,
            jw: 0.0,
        }
    }

    /// Calculate Thermodynamic Wage Metric: JW = E × η × W
    ///
    /// E = energy expenditure = growth × learning_rate × task_complexity
    /// η = efficiency factor = harmony × coherence_alignment  
    /// W = work output = curiosity × transcendence × network_amplification
    ///
    /// Returns JW score in range [0, 1] suitable for:
    /// - Agent performance evaluation
    /// - Micropayment calculation in agentic economies
    /// - Replicator selection in DKS (Dynamic Kinetic Stability)
    pub fn calculate_jw(&self, coherence: f64, network_degree: usize) -> f64 {
        // Energy expenditure: how much effort the agent puts in
        let e = self.drives.growth * self.learning_rate;

        // Efficiency factor: how well agent works with system coherence
        let coherence_alignment = 1.0 - (self.drives.harmony - coherence).abs();
        let eta = self.drives.harmony * coherence_alignment.max(0.1);

        // Work output: creative production scaled by network connectivity
        let network_amplification = 1.0 + (network_degree as f64 / 10.0).min(2.0);
        let w = self.drives.curiosity * self.drives.transcendence * network_amplification;

        // JW = E × η × W (normalized to [0, 1])
        let raw_jw = e * eta * w;
        raw_jw.min(1.0).max(0.0)
    }

    pub fn calculate_bid(&self, context: &str, temperature: f64) -> f64 {
        let base_score = match self.role {
            Role::Architect => {
                if context.contains("structure") || context.contains("coherence") {
                    1.5
                } else {
                    0.8
                }
            }
            Role::Catalyst => {
                if context.contains("innovation") || context.contains("stagnation") {
                    1.5
                } else {
                    0.9
                }
            }
            Role::Chronicler => {
                if context.contains("document") || context.contains("history") {
                    1.5
                } else {
                    0.7
                }
            }
            Role::Critic => {
                // Critics activate on risk/conflict/evidence keywords
                if context.contains("risk")
                    || context.contains("conflict")
                    || context.contains("violation")
                {
                    1.4
                } else {
                    0.7
                }
            }
            Role::Explorer => {
                // Explorers activate on exploration/novelty keywords
                if context.contains("explore")
                    || context.contains("novel")
                    || context.contains("discover")
                {
                    1.5
                } else {
                    1.0
                }
            }
            Role::Coder => {
                // Coders activate on code/implementation/debugging keywords
                if context.contains("code")
                    || context.contains("implement")
                    || context.contains("debug")
                    || context.contains("refactor")
                    || context.contains("test")
                    || context.contains("build")
                {
                    1.6
                } else {
                    0.7
                }
            }
        };

        let noise = rand::random::<f64>() * temperature;
        base_score * self.bid_bias + noise
    }

    /// Bid on a set of emergent associations produced by a skill.
    /// Each role evaluates associations differently:
    /// - Architect: values structural bridges, cluster emergence
    /// - Catalyst: values novelty, cross-domain transfer
    /// - Chronicler: values consistency, belief resolution
    /// - Critic: skeptical — penalizes low coherence, values risk mitigation evidence
    /// - Explorer: values vertex coverage, cross-domain diversity, novelty
    pub fn bid_association(&self, associations: &[EmergentAssociation]) -> f64 {
        if associations.is_empty() {
            return 0.5 * self.bid_bias; // neutral bid for no associations
        }

        let score = match self.role {
            Role::Architect => {
                // Architects reward structural improvements
                let structural: f64 = associations
                    .iter()
                    .map(|a| match &a.association_type {
                        AssociationType::BridgeFormation { .. } => 0.9,
                        AssociationType::ClusterEmergence { cluster_size } => {
                            (*cluster_size as f64 / 10.0).min(1.0)
                        }
                        AssociationType::RiskMitigation { .. } => 0.7,
                        _ => 0.3,
                    })
                    .sum::<f64>()
                    / associations.len() as f64;

                let coherence_boost: f64 = associations
                    .iter()
                    .map(|a| a.coherence_delta.max(0.0))
                    .sum::<f64>()
                    / associations.len() as f64;

                structural * 0.6 + coherence_boost * 10.0 * 0.4
            }
            Role::Catalyst => {
                // Catalysts reward novelty and exploration
                let novelty_avg: f64 = associations.iter().map(|a| a.novelty_score).sum::<f64>()
                    / associations.len() as f64;

                let cross_domain_bonus = associations
                    .iter()
                    .filter(|a| {
                        matches!(
                            a.association_type,
                            AssociationType::CrossDomainTransfer { .. }
                        )
                    })
                    .count() as f64
                    * 0.2;

                let vertex_coverage = associations
                    .iter()
                    .flat_map(|a| a.vertices_involved.iter())
                    .collect::<std::collections::HashSet<_>>()
                    .len() as f64
                    / 20.0;

                (novelty_avg * 0.5 + cross_domain_bonus + vertex_coverage.min(1.0) * 0.3).min(1.0)
            }
            Role::Chronicler => {
                // Chroniclers reward pattern consistency and belief resolution
                let belief_resolutions = associations
                    .iter()
                    .filter(|a| {
                        matches!(a.association_type, AssociationType::BeliefResolution { .. })
                    })
                    .count() as f64;

                let identity_bridges = associations
                    .iter()
                    .filter(|a| {
                        matches!(a.association_type, AssociationType::IdentityBridge { .. })
                    })
                    .count() as f64;

                let consistency =
                    (belief_resolutions * 0.3 + identity_bridges * 0.1) / associations.len() as f64;
                let avg_coherence = associations.iter().map(|a| a.coherence_delta).sum::<f64>()
                    / associations.len() as f64;

                (consistency + avg_coherence.max(0.0) * 5.0).min(1.0) * 0.5 + 0.3
                // conservative baseline
            }
            Role::Critic => {
                // Critics are skeptical — they penalize low coherence and reward risk evidence
                let risk_mitigation_count = associations
                    .iter()
                    .filter(|a| {
                        matches!(a.association_type, AssociationType::RiskMitigation { .. })
                    })
                    .count() as f64;

                let avg_coherence_delta =
                    associations.iter().map(|a| a.coherence_delta).sum::<f64>()
                        / associations.len() as f64;

                // Critics penalize negative coherence (something went wrong)
                let coherence_penalty = if avg_coherence_delta < 0.0 {
                    avg_coherence_delta.abs() * 3.0 // amplify negative signal
                } else {
                    0.0
                };

                // Reward risk mitigations, penalize lack of evidence
                let evidence_score =
                    (risk_mitigation_count * 0.3 / associations.len() as f64).min(0.8);
                let base = 0.4 + evidence_score; // start skeptical
                (base - coherence_penalty).clamp(0.1, 0.9) // never fully convinced
            }
            Role::Explorer => {
                // Explorers reward cross-domain diversity and broad vertex coverage
                let cross_domain_count = associations
                    .iter()
                    .filter(|a| {
                        matches!(
                            a.association_type,
                            AssociationType::CrossDomainTransfer { .. }
                        )
                    })
                    .count() as f64;

                let novelty_avg: f64 = associations.iter().map(|a| a.novelty_score).sum::<f64>()
                    / associations.len() as f64;

                let unique_vertices = associations
                    .iter()
                    .flat_map(|a| a.vertices_involved.iter())
                    .collect::<std::collections::HashSet<_>>()
                    .len() as f64;
                let coverage = (unique_vertices / 30.0).min(1.0); // broader range than Catalyst

                // Unique association type diversity
                let type_diversity = {
                    let mut types = std::collections::HashSet::new();
                    for a in associations {
                        types.insert(std::mem::discriminant(&a.association_type));
                    }
                    types.len() as f64 / 6.0 // 6 possible AssociationType variants
                };

                (novelty_avg * 0.3
                    + cross_domain_count * 0.15
                    + coverage * 0.25
                    + type_diversity * 0.3)
                    .min(1.0)
            }
            Role::Coder => {
                // Coders value precise, actionable associations that lead to working implementations
                // They prefer clarity and minimal complexity
                let clarity_score = associations
                    .iter()
                    .map(|a| if a.coherence_delta > 0.0 { 0.8 } else { 0.3 })
                    .sum::<f64>()
                    / associations.len() as f64;

                // Prefer focused associations over scattered ones
                let focus_bonus = if associations.len() <= 3 { 0.2 } else { 0.0 };

                // Value structural improvements (better architecture)
                let structural_value = associations
                    .iter()
                    .map(|a| match &a.association_type {
                        AssociationType::BridgeFormation { .. } => 0.9,
                        AssociationType::ClusterEmergence { .. } => 0.7,
                        _ => 0.4,
                    })
                    .sum::<f64>()
                    / associations.len() as f64;

                (clarity_score * 0.4 + structural_value * 0.4 + focus_bonus).min(1.0)
            }
        };

        (score * self.bid_bias).clamp(0.0, 1.0)
    }

    /// GRPO-style bid_bias update: Group Relative Policy Optimization
    /// Given a group of reward outcomes, compute advantage = (R_i - mean) / std
    /// and update bid_bias proportionally.
    pub fn grpo_update_bid(&mut self, rewards: &[f64], own_reward: f64, lr: f64) {
        if rewards.is_empty() {
            return;
        }

        let mean = rewards.iter().sum::<f64>() / rewards.len() as f64;
        let variance =
            rewards.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / rewards.len() as f64;
        let std = variance.sqrt().max(1e-8);

        let advantage = (own_reward - mean) / std;

        // Clipped update: avoid extreme swings
        let clipped_advantage = advantage.clamp(-2.0, 2.0);
        self.bid_bias += lr * clipped_advantage;

        // Keep bid_bias in reasonable range
        self.bid_bias = self.bid_bias.clamp(0.1, 5.0);
    }
}
