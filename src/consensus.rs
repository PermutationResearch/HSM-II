//! Agent consensus evaluation for emergent association utility scoring.
//!
//! Implements:
//! - EmergentAssociation detection (new subgraphs, belief resolutions, risk mitigations)
//! - Agent jury with role-weighted bidding (GRPO-style)
//! - Anti-majority consensus (ACPO) with pairwise cosine correlation penalty
//! - Bayesian utility scoring with suspension/promotion thresholds
//! - Topological jury layers: parallel dyads → Chronicler synthesis → ACPO
//! - CorrelationMonitor with rolling window re-specialization triggers
//! - Edge-level context filtering (ContextPolicy) for KV cache optimization
//!
//! Integration: Slots into evolve() every 10 ticks. After distillation, a mini-consensus
//! round runs via braids. Agents bid on association value; GRPO aggregates utility.
//!
//! Score formula: (AssociationCount * CoherenceDelta) / AgentDiversity
//! Thresholds: >0.7 utility → promote; <0.3 → suspend (not deprecate)

use std::collections::{HashMap, VecDeque};

use serde::{Deserialize, Serialize};

use crate::agent::{Agent, Role};
use crate::federation::types::{CrossSystemVote, RemoteAgentBid, SystemId};
use crate::skill::{Skill, SkillBank};

// ── Emergent Association Types ──────────────────────────────────────────

/// An emergent association detected post-skill application.
/// Represents new subgraph structures, belief resolutions, or risk mitigations
/// that arose from applying a skill.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EmergentAssociation {
    pub skill_id: String,
    pub association_type: AssociationType,
    pub vertices_involved: Vec<u64>,
    pub coherence_delta: f64,
    pub novelty_score: f64,
    pub detected_at_tick: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum AssociationType {
    /// New bridge between previously disconnected clusters
    BridgeFormation {
        from_cluster: usize,
        to_cluster: usize,
    },
    /// Belief conflict resolved via weighted synthesis
    BeliefResolution { belief_ids: Vec<usize> },
    /// Risk mitigated (e.g., hub pruned for scalability)
    RiskMitigation { risk_type: String },
    /// New emergent cluster detected
    ClusterEmergence { cluster_size: usize },
    /// Cross-domain skill transfer observed
    CrossDomainTransfer {
        source_domain: String,
        target_domain: String,
    },
    /// Identity bridge (self-referencing for reversal curse fix)
    IdentityBridge { concept: String },
    /// Cross-system consensus: multiple federated systems agreed on this edge
    CrossSystemConsensus {
        systems: Vec<SystemId>,
        agreement_score: f64,
    },
    /// Cross-system synthesis: dense cross-domain co-occurrence across systems
    CrossSystemSynthesis {
        systems: Vec<SystemId>,
        domains: Vec<String>,
    },
    /// Federated cluster: emergent cluster spanning multiple systems
    FederatedCluster {
        systems: Vec<SystemId>,
        cluster_size: usize,
    },
}

// ── Consensus Evaluation ────────────────────────────────────────────────

/// Result of a consensus evaluation round for a single skill
#[derive(Clone, Debug)]
pub struct ConsensusResult {
    pub skill_id: String,
    pub utility_score: f64,
    pub association_count: usize,
    pub agent_bids: Vec<AgentBid>,
    pub diversity_factor: f64,
    /// Pairwise bid correlation for this evaluation (fed to CorrelationMonitor)
    pub bid_correlation: f64,
    pub verdict: ConsensusVerdict,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ConsensusVerdict {
    /// Utility > 0.7 — promote skill (increase confidence, possibly to Advanced)
    Promote,
    /// 0.3 <= utility <= 0.7 — maintain current state
    Maintain,
    /// Utility < 0.3 — suspend skill (can be revived later, NOT deprecated)
    Suspend,
}

/// A single agent's bid on a skill's association value
#[derive(Clone, Debug)]
pub struct AgentBid {
    pub agent_id: u64,
    pub role: Role,
    pub bid_value: f64,
    pub rationale: BidRationale,
}

#[derive(Clone, Debug)]
pub enum BidRationale {
    /// Catalyst/Explorer sees new connections spawned
    NewConnections(usize),
    /// Architect sees structural improvement
    StructuralImprovement(f64),
    /// Critic sees remaining risks (skeptical assessment)
    RiskConcern(f64),
    /// Chronicler sees pattern consistency
    PatternConsistency(f64),
    /// Explorer sees diversity across association types
    DiversityProbe(f64),
}

// ── Skill Status (replaces binary deprecated/active) ────────────────────

/// Extended skill lifecycle status for consensus-based evaluation.
/// Replaces the old binary active/deprecated model.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum SkillStatus {
    /// Skill is active and being used
    Active,
    /// Skill promoted via consensus to advanced level
    Advanced,
    /// Skill suspended — low utility but can be revived if new associations appear
    Suspended {
        suspended_at_tick: u64,
        revival_attempts: u32,
    },
    /// Skill deprecated — consistently low utility across multiple evaluations
    Deprecated,
}

impl Default for SkillStatus {
    fn default() -> Self {
        SkillStatus::Active
    }
}

// ── Bayesian Confidence ────────────────────────────────────────────────

/// Bayesian confidence tracker with Beta distribution priors.
/// Replaces raw success_rate < 0.3 threshold with posterior estimates.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BayesianConfidence {
    /// Alpha parameter (successes + prior)
    pub alpha: f64,
    /// Beta parameter (failures + prior)
    pub beta: f64,
}

impl Default for BayesianConfidence {
    fn default() -> Self {
        // Weak prior: equivalent to seeing 1 success and 1 failure
        Self {
            alpha: 1.0,
            beta: 1.0,
        }
    }
}

impl BayesianConfidence {
    pub fn new(prior_alpha: f64, prior_beta: f64) -> Self {
        Self {
            alpha: prior_alpha,
            beta: prior_beta,
        }
    }

    /// Update posterior with observation
    pub fn update(&mut self, success: bool) {
        if success {
            self.alpha += 1.0;
        } else {
            self.beta += 1.0;
        }
    }

    /// Posterior mean: E[θ] = α / (α + β)
    pub fn mean(&self) -> f64 {
        self.alpha / (self.alpha + self.beta)
    }

    /// Posterior variance: Var[θ] = αβ / ((α+β)²(α+β+1))
    pub fn variance(&self) -> f64 {
        let total = self.alpha + self.beta;
        (self.alpha * self.beta) / (total * total * (total + 1.0))
    }

    /// 95% credible interval lower bound (conservative estimate)
    /// Uses normal approximation for large α+β, else falls back to mean - 2*std
    pub fn lower_bound_95(&self) -> f64 {
        let mean = self.mean();
        let std = self.variance().sqrt();
        (mean - 1.96 * std).max(0.0)
    }

    /// Total observations
    pub fn total_observations(&self) -> f64 {
        self.alpha + self.beta - 2.0 // subtract priors
    }

    /// Whether we have enough data for a confident assessment
    pub fn is_confident(&self) -> bool {
        self.total_observations() >= 5.0
    }
}

// ── Consensus Engine ───────────────────────────────────────────────────

/// The consensus evaluation engine.
/// Runs post-distillation to assess skill utility via agent jury.
pub struct ConsensusEngine {
    /// Threshold above which skills are promoted
    pub promote_threshold: f64,
    /// Threshold below which skills are suspended
    pub suspend_threshold: f64,
    /// Anti-correlation penalty factor (ACPO)
    pub anti_correlation_penalty: f64,
    /// Minimum diversity factor to avoid groupthink
    pub min_diversity: f64,
    /// Lambda for pairwise cosine correlation diversity penalty
    pub correlation_lambda: f64,
    /// Jury pipeline for topological layer execution
    pub jury_pipeline: JuryPipeline,
}

impl Default for ConsensusEngine {
    fn default() -> Self {
        Self {
            promote_threshold: 0.7,
            suspend_threshold: 0.3,
            anti_correlation_penalty: 0.15,
            min_diversity: 0.3,
            correlation_lambda: 0.6,
            jury_pipeline: JuryPipeline::default(),
        }
    }
}

impl ConsensusEngine {
    /// Evaluate a skill's utility via agent consensus on its emergent associations.
    ///
    /// Process:
    /// 1. Detect emergent associations from this skill
    /// 2. Agent jury bids on association value (via topological layers)
    /// 3. Anti-majority aggregation (ACPO) with correlation penalty
    /// 4. Compute utility = avg(bids) * diversity_factor * (1 - correlation * λ)
    /// 5. Return verdict (Promote/Maintain/Suspend)
    pub fn evaluate_skill(
        &self,
        skill: &Skill,
        associations: &[EmergentAssociation],
        agents: &[Agent],
        coherence_delta: f64,
    ) -> ConsensusResult {
        // Filter associations relevant to this skill
        let relevant: Vec<&EmergentAssociation> = associations
            .iter()
            .filter(|a| a.skill_id == skill.id)
            .collect();

        let association_count = relevant.len();

        // Execute via topological jury layers
        let bids = self.execute_jury_pipeline(skill, &relevant, agents, coherence_delta);

        // Anti-correlation aggregation (ACPO) with enhanced correlation penalty
        let (utility_score, diversity_factor, correlation) = self.acpo_aggregate(&bids);

        // Determine verdict
        let verdict = if utility_score > self.promote_threshold {
            ConsensusVerdict::Promote
        } else if utility_score < self.suspend_threshold {
            ConsensusVerdict::Suspend
        } else {
            ConsensusVerdict::Maintain
        };

        ConsensusResult {
            skill_id: skill.id.clone(),
            utility_score,
            association_count,
            agent_bids: bids,
            diversity_factor,
            bid_correlation: correlation,
            verdict,
        }
    }

    /// Execute jury pipeline with topological layers.
    ///
    /// Layer 0: Parallel dyads — (Architect+Critic) | (Catalyst+Explorer)
    /// Layer 1: Chronicler synthesizes layer-0 outputs
    /// Layer 2: ACPO aggregation (handled by caller)
    fn execute_jury_pipeline(
        &self,
        skill: &Skill,
        associations: &[&EmergentAssociation],
        agents: &[Agent],
        coherence_delta: f64,
    ) -> Vec<AgentBid> {
        let mut all_bids = Vec::new();

        // Layer 0: Execute dyad pairs
        // Each dyad consists of two complementary roles evaluating together
        for layer in &self.jury_pipeline.layers {
            for (role_a, role_b) in &layer.dyads {
                // Find agents with matching roles (or closest match)
                let agent_a = agents
                    .iter()
                    .find(|a| a.role == *role_a)
                    .or_else(|| agents.first());
                let agent_b = agents
                    .iter()
                    .find(|a| a.role == *role_b)
                    .or_else(|| agents.last());

                if let Some(a) = agent_a {
                    all_bids.push(self.agent_bid(a, skill, associations, coherence_delta));
                }
                if let Some(b) = agent_b {
                    all_bids.push(self.agent_bid(b, skill, associations, coherence_delta));
                }
            }
        }

        // Layer 1: Chronicler synthesis — add synthesizer's bid
        let synthesizer = agents
            .iter()
            .find(|a| a.role == self.jury_pipeline.synthesizer)
            .or_else(|| agents.iter().find(|a| a.role == Role::Chronicler));

        if let Some(synth) = synthesizer {
            // Synthesizer bids with awareness of layer-0 average
            let layer0_avg = if all_bids.is_empty() {
                0.5
            } else {
                all_bids.iter().map(|b| b.bid_value).sum::<f64>() / all_bids.len() as f64
            };

            // Chronicler's synthesis bid is anchored to layer-0 average but adjusted by consistency
            let consistency = if skill.usage_count > 0 {
                skill.success_count as f64 / skill.usage_count as f64
            } else {
                0.5
            };
            let synth_bid = (layer0_avg * 0.6 + consistency * 0.4) * synth.bid_bias;

            all_bids.push(AgentBid {
                agent_id: synth.id,
                role: synth.role,
                bid_value: synth_bid.clamp(0.0, 1.0),
                rationale: BidRationale::PatternConsistency(consistency),
            });
        }

        // Also include bids from any agents not captured by the pipeline roles
        for agent in agents {
            let already_bid = all_bids.iter().any(|b| b.agent_id == agent.id);
            if !already_bid {
                all_bids.push(self.agent_bid(agent, skill, associations, coherence_delta));
            }
        }

        all_bids
    }

    /// Generate an agent's bid on a skill's association value.
    /// Each role brings a different perspective.
    fn agent_bid(
        &self,
        agent: &Agent,
        skill: &Skill,
        associations: &[&EmergentAssociation],
        coherence_delta: f64,
    ) -> AgentBid {
        let (bid_value, rationale) = match agent.role {
            Role::Architect => {
                // Architects value structural improvements
                let structural_score = associations
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
                    / associations.len().max(1) as f64;

                let combined = structural_score * 0.6 + coherence_delta.max(0.0) * 0.4;
                (
                    combined * agent.bid_bias,
                    BidRationale::StructuralImprovement(structural_score),
                )
            }
            Role::Catalyst => {
                // Catalysts value novelty and new connections
                let new_connections: usize =
                    associations.iter().map(|a| a.vertices_involved.len()).sum();
                let novelty_avg = associations.iter().map(|a| a.novelty_score).sum::<f64>()
                    / associations.len().max(1) as f64;

                let combined = (new_connections as f64 / 20.0).min(1.0) * 0.5 + novelty_avg * 0.5;
                (
                    combined * agent.bid_bias,
                    BidRationale::NewConnections(new_connections),
                )
            }
            Role::Chronicler => {
                // Chroniclers value pattern consistency across uses
                let consistency = if skill.usage_count > 0 {
                    skill.success_count as f64 / skill.usage_count as f64
                } else {
                    0.5 // prior for unused skills
                };
                let cross_domain = associations.iter().any(|a| {
                    matches!(
                        a.association_type,
                        AssociationType::CrossDomainTransfer { .. }
                    )
                });
                let bonus = if cross_domain { 0.2 } else { 0.0 };

                (
                    (consistency + bonus).min(1.0) * agent.bid_bias,
                    BidRationale::PatternConsistency(consistency),
                )
            }
            Role::Critic => {
                // Critics are skeptical — penalize low coherence, reward risk evidence
                let risk_count = associations
                    .iter()
                    .filter(|a| {
                        matches!(a.association_type, AssociationType::RiskMitigation { .. })
                    })
                    .count() as f64;

                let avg_coherence = associations.iter().map(|a| a.coherence_delta).sum::<f64>()
                    / associations.len().max(1) as f64;

                let coherence_penalty = if avg_coherence < 0.0 {
                    avg_coherence.abs() * 3.0
                } else {
                    0.0
                };

                let evidence = (risk_count * 0.3 / associations.len().max(1) as f64).min(0.8);
                let base = 0.4 + evidence;
                let value = (base - coherence_penalty).clamp(0.1, 0.9) * agent.bid_bias;
                (value, BidRationale::RiskConcern(coherence_penalty))
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

                let novelty_avg = associations.iter().map(|a| a.novelty_score).sum::<f64>()
                    / associations.len().max(1) as f64;

                let unique_vertices = associations
                    .iter()
                    .flat_map(|a| a.vertices_involved.iter())
                    .collect::<std::collections::HashSet<_>>()
                    .len() as f64;
                let coverage = (unique_vertices / 30.0).min(1.0);

                let type_diversity = {
                    let mut types = std::collections::HashSet::new();
                    for a in associations.iter() {
                        types.insert(std::mem::discriminant(&a.association_type));
                    }
                    types.len() as f64 / 6.0
                };

                let combined = (novelty_avg * 0.3
                    + cross_domain_count * 0.15
                    + coverage * 0.25
                    + type_diversity * 0.3)
                    .min(1.0);
                (
                    combined * agent.bid_bias,
                    BidRationale::DiversityProbe(type_diversity),
                )
            }
            Role::Coder => {
                // Coders value precision and actionable improvements
                let clarity_score = associations
                    .iter()
                    .map(|a| if a.coherence_delta > 0.0 { 1.0 } else { 0.0 })
                    .sum::<f64>()
                    / associations.len().max(1) as f64;

                let structural_value = associations
                    .iter()
                    .map(|a| match &a.association_type {
                        AssociationType::BridgeFormation { .. } => 0.9,
                        AssociationType::ClusterEmergence { .. } => 0.7,
                        _ => 0.4,
                    })
                    .sum::<f64>()
                    / associations.len().max(1) as f64;

                let combined = clarity_score * 0.5 + structural_value * 0.5;
                (
                    combined * agent.bid_bias,
                    BidRationale::StructuralImprovement(structural_value),
                )
            }
        };

        AgentBid {
            agent_id: agent.id,
            role: agent.role,
            bid_value: bid_value.clamp(0.0, 1.0),
            rationale,
        }
    }

    /// Anti-Consensus Policy Optimization (ACPO) aggregation.
    /// Enhanced with pairwise cosine correlation penalty.
    ///
    /// Returns (utility_score, diversity_factor, bid_correlation)
    fn acpo_aggregate(&self, bids: &[AgentBid]) -> (f64, f64, f64) {
        if bids.is_empty() {
            return (0.5, 0.0, 0.0);
        }

        let values: Vec<f64> = bids.iter().map(|b| b.bid_value).collect();
        let mean = values.iter().sum::<f64>() / values.len() as f64;

        // Compute bid variance as a diversity proxy
        let variance = if values.len() > 1 {
            values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (values.len() - 1) as f64
        } else {
            0.0
        };
        let std_dev = variance.sqrt();

        // Role diversity: how many distinct roles are represented?
        let mut role_set = std::collections::HashSet::new();
        for bid in bids {
            role_set.insert(bid.role);
        }
        let role_diversity = role_set.len() as f64 / Role::COUNT as f64;

        // Diversity factor: combines bid variance + role diversity
        let diversity_factor = (std_dev * 0.5 + role_diversity * 0.5).max(self.min_diversity);

        // Pairwise bid correlation (cosine similarity proxy)
        // High correlation = all bids clustered near mean = low spread
        let correlation = Self::bid_correlation(&values);

        // Correlation-based diversity penalty (the core ACPO enhancement)
        let correlation_penalty = correlation * self.correlation_lambda;

        // Legacy anti-correlation penalty for very low variance
        let groupthink_penalty = if std_dev < 0.1 && bids.len() > 2 {
            self.anti_correlation_penalty
        } else {
            0.0
        };

        // Final utility: mean * diversity * (1 - correlation_penalty) - groupthink_penalty
        let utility = (mean * diversity_factor * (1.0 - correlation_penalty) - groupthink_penalty)
            .clamp(0.0, 1.0);

        (utility, diversity_factor, correlation)
    }

    /// Compute pairwise bid correlation.
    /// Returns 0.0 (uncorrelated/diverse) to 1.0 (perfectly correlated/groupthink).
    ///
    /// Uses bid value spread as correlation proxy: if all bids are near the mean,
    /// correlation is high; if bids are spread out, correlation is low.
    fn bid_correlation(values: &[f64]) -> f64 {
        if values.len() < 2 {
            return 0.0;
        }

        let _mean = values.iter().sum::<f64>() / values.len() as f64;

        // Pairwise absolute difference (lower = more correlated)
        let mut total_diff = 0.0;
        let mut pairs = 0;
        for i in 0..values.len() {
            for j in (i + 1)..values.len() {
                total_diff += (values[i] - values[j]).abs();
                pairs += 1;
            }
        }

        if pairs == 0 {
            return 0.0;
        }

        let avg_diff = total_diff / pairs as f64;
        // Invert: low difference = high correlation
        // Normalize: max reasonable difference is ~1.0
        (1.0 - avg_diff.min(1.0)).clamp(0.0, 1.0)
    }

    /// Run consensus evaluation on all skills in the bank.
    /// Called during evolve() every N ticks.
    pub fn evaluate_all_skills(
        &self,
        skill_bank: &SkillBank,
        associations: &[EmergentAssociation],
        agents: &[Agent],
        coherence_delta: f64,
    ) -> Vec<ConsensusResult> {
        let all_skills = skill_bank.all_skills();
        all_skills
            .iter()
            .map(|skill| self.evaluate_skill(skill, associations, agents, coherence_delta))
            .collect()
    }

    /// Evaluate a skill using cross-system consensus votes.
    /// Remote votes are weighted by the trust score of their originating system.
    pub fn evaluate_cross_system_skill(
        &self,
        skill: &Skill,
        local_associations: &[EmergentAssociation],
        local_agents: &[Agent],
        remote_votes: &[CrossSystemVote],
        trust_scores: &[(SystemId, f64)],
        coherence_delta: f64,
    ) -> ConsensusResult {
        // Start with local evaluation
        let mut result =
            self.evaluate_skill(skill, local_associations, local_agents, coherence_delta);

        if remote_votes.is_empty() {
            return result;
        }

        // Compute trust-weighted remote consensus
        let trust_map: std::collections::HashMap<&SystemId, f64> =
            trust_scores.iter().map(|(s, t)| (s, *t)).collect();

        let mut weighted_sum = 0.0;
        let mut weight_total = 0.0;

        for vote in remote_votes {
            if vote.skill_id != skill.id {
                continue;
            }

            let trust = trust_map.get(&vote.voter_system).copied().unwrap_or(0.3);
            let vote_value = match vote.verdict {
                ConsensusVerdict::Promote => 0.85,
                ConsensusVerdict::Maintain => 0.5,
                ConsensusVerdict::Suspend => 0.15,
            };

            weighted_sum += vote_value * trust * vote.confidence;
            weight_total += trust * vote.confidence;
        }

        if weight_total > 0.0 {
            let remote_utility = weighted_sum / weight_total;
            // Blend local (60%) and remote (40%) utility
            result.utility_score = result.utility_score * 0.6 + remote_utility * 0.4;

            // Re-evaluate verdict with blended score
            result.verdict = if result.utility_score > self.promote_threshold {
                ConsensusVerdict::Promote
            } else if result.utility_score < self.suspend_threshold {
                ConsensusVerdict::Suspend
            } else {
                ConsensusVerdict::Maintain
            };
        }

        result
    }

    /// Aggregate local + remote agent bids for cross-system ACPO.
    /// Remote bids are trust-weighted with a cross-system diversity bonus.
    pub fn cross_system_acpo_aggregate(
        &self,
        local_bids: &[AgentBid],
        remote_bids: &[RemoteAgentBid],
        trust_scores: &[(SystemId, f64)],
    ) -> (f64, f64, f64) {
        if local_bids.is_empty() && remote_bids.is_empty() {
            return (0.5, 0.0, 0.0);
        }

        let trust_map: std::collections::HashMap<&SystemId, f64> =
            trust_scores.iter().map(|(s, t)| (s, *t)).collect();

        // Collect all bid values (local at full weight, remote trust-discounted)
        let mut all_values: Vec<f64> = local_bids.iter().map(|b| b.bid_value).collect();

        for rb in remote_bids {
            let trust = trust_map.get(&rb.system_id).copied().unwrap_or(0.3);
            all_values.push(rb.bid_value * trust);
        }

        if all_values.is_empty() {
            return (0.5, 0.0, 0.0);
        }

        let mean = all_values.iter().sum::<f64>() / all_values.len() as f64;

        // Compute diversity including cross-system spread
        let variance = if all_values.len() > 1 {
            all_values.iter().map(|v| (v - mean).powi(2)).sum::<f64>()
                / (all_values.len() - 1) as f64
        } else {
            0.0
        };
        let std_dev = variance.sqrt();

        // Cross-system diversity bonus: more unique systems = higher diversity
        let unique_systems: std::collections::HashSet<&SystemId> =
            remote_bids.iter().map(|b| &b.system_id).collect();
        let system_diversity_bonus = (unique_systems.len() as f64 * 0.05).min(0.2);

        let diversity_factor =
            (std_dev * 0.4 + system_diversity_bonus + 0.3).max(self.min_diversity);

        let correlation = Self::bid_correlation(&all_values);
        let correlation_penalty = correlation * self.correlation_lambda;

        let utility = (mean * diversity_factor * (1.0 - correlation_penalty)).clamp(0.0, 1.0);

        (utility, diversity_factor, correlation)
    }

    /// Apply consensus verdicts to the skill bank.
    /// - Promote: boost confidence, mark as Advanced
    /// - Suspend: reduce confidence but keep in bank for potential revival
    /// - Maintain: no change
    pub fn apply_verdicts(
        skill_bank: &mut SkillBank,
        results: &[ConsensusResult],
        current_tick: u64,
    ) -> ConsensusApplyResult {
        let mut promoted = 0;
        let mut suspended = 0;
        let mut maintained = 0;

        for result in results {
            match result.verdict {
                ConsensusVerdict::Promote => {
                    if let Some(skill) = skill_bank.get_skill_mut(&result.skill_id) {
                        skill.confidence = (skill.confidence + 0.1).min(0.99);
                        skill.status = SkillStatus::Advanced;
                        promoted += 1;
                    }
                }
                ConsensusVerdict::Suspend => {
                    if let Some(skill) = skill_bank.get_skill_mut(&result.skill_id) {
                        // Don't immediately deprecate — suspend for potential revival
                        match &skill.status {
                            SkillStatus::Suspended {
                                revival_attempts, ..
                            } => {
                                if *revival_attempts >= 3 {
                                    // After 3 failed revival attempts, actually deprecate
                                    skill.status = SkillStatus::Deprecated;
                                    skill.confidence *= 0.3;
                                } else {
                                    skill.status = SkillStatus::Suspended {
                                        suspended_at_tick: current_tick,
                                        revival_attempts: revival_attempts + 1,
                                    };
                                    skill.confidence *= 0.7;
                                }
                            }
                            _ => {
                                skill.status = SkillStatus::Suspended {
                                    suspended_at_tick: current_tick,
                                    revival_attempts: 0,
                                };
                                skill.confidence *= 0.8;
                            }
                        }
                        suspended += 1;
                    }
                }
                ConsensusVerdict::Maintain => {
                    maintained += 1;
                }
            }
        }

        ConsensusApplyResult {
            promoted,
            suspended,
            maintained,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ConsensusApplyResult {
    pub promoted: usize,
    pub suspended: usize,
    pub maintained: usize,
}

// ── Jury Pipeline (Topological Layers) ──────────────────────────────────

/// A single layer of the jury pipeline — contains dyad pairs that execute concurrently.
#[derive(Clone, Debug)]
pub struct JuryLayer {
    pub dyads: Vec<(Role, Role)>,
}

/// Topological jury pipeline: parallel dyads → synthesizer.
///
/// Layer 0 (parallel):  (Architect + Critic)  |  (Catalyst + Explorer)
///                      → structural review       → novelty assessment
///
/// Layer 1 (sequential): Chronicler synthesizes both layer-0 outputs
///
/// Layer 2: ACPO aggregates with diversity penalty
#[derive(Clone, Debug)]
pub struct JuryPipeline {
    pub layers: Vec<JuryLayer>,
    pub synthesizer: Role,
}

impl Default for JuryPipeline {
    fn default() -> Self {
        Self {
            layers: vec![JuryLayer {
                dyads: vec![
                    (Role::Architect, Role::Critic),
                    (Role::Catalyst, Role::Explorer),
                ],
            }],
            synthesizer: Role::Chronicler,
        }
    }
}

/// Result of a dyad evaluation (used internally by JuryPipeline)
#[derive(Clone, Debug)]
pub struct DyadResult {
    pub role_a: Role,
    pub role_b: Role,
    pub bid_a: f64,
    pub bid_b: f64,
    pub combined_assessment: f64,
}

// ── Context Policy (Edge-Level Filtering) ───────────────────────────────

/// Context filtering policy for KV cache optimization.
/// Controls how context flows between jury layers.
///
/// - KeepAll: Full history flows through all layers
/// - SummarizePrefix: Compress shared context to N tokens (future: token budget)
/// - ClearAfterRound: Discard agent-specific KV post-consensus, keep synthesis only
#[derive(Clone, Debug)]
pub enum ContextPolicy {
    /// Full context preserved across layers
    KeepAll,
    /// Compress shared context (braid findings + association list)
    SummarizePrefix,
    /// Discard per-agent context after each consensus round
    ClearAfterRound,
}

impl Default for ContextPolicy {
    fn default() -> Self {
        ContextPolicy::ClearAfterRound
    }
}

/// Jury context prepared for a consensus round.
/// Shared prefix is computed once and reused across Layer-0 agents (CrossKV optimization).
#[derive(Clone, Debug)]
pub struct JuryContext {
    /// Shared context prefix: braid findings, association summary
    pub shared_prefix: String,
    /// Per-role context suffix: role-specific skills and prompts
    pub per_role_suffix: HashMap<Role, String>,
    /// Context filtering policy
    pub policy: ContextPolicy,
}

impl JuryContext {
    /// Build jury context from braid synthesis and skill bank.
    pub fn from_synthesis(
        braid_prompt: &str,
        skill_bank: &SkillBank,
        associations: &[EmergentAssociation],
    ) -> Self {
        let shared_prefix = format!(
            "{}\n[Associations: {} detected across {} skills]",
            braid_prompt,
            associations.len(),
            associations
                .iter()
                .map(|a| a.skill_id.as_str())
                .collect::<std::collections::HashSet<_>>()
                .len()
        );

        let mut per_role_suffix = HashMap::new();

        // Architect+Critic focus: structural associations
        let structural_summary: Vec<_> = associations
            .iter()
            .filter(|a| {
                matches!(
                    a.association_type,
                    AssociationType::BridgeFormation { .. }
                        | AssociationType::ClusterEmergence { .. }
                        | AssociationType::RiskMitigation { .. }
                )
            })
            .map(|a| format!("{:?}", a.association_type))
            .take(5)
            .collect();
        let structural_ctx = format!("[Structural focus: {}]", structural_summary.join(", "));
        per_role_suffix.insert(Role::Architect, structural_ctx.clone());
        per_role_suffix.insert(Role::Critic, structural_ctx);

        // Catalyst+Explorer focus: novelty associations
        let novelty_summary: Vec<_> = associations
            .iter()
            .filter(|a| {
                matches!(
                    a.association_type,
                    AssociationType::CrossDomainTransfer { .. }
                        | AssociationType::IdentityBridge { .. }
                )
            })
            .map(|a| format!("{:?}", a.association_type))
            .take(5)
            .collect();
        let novelty_ctx = format!("[Novelty focus: {}]", novelty_summary.join(", "));
        per_role_suffix.insert(Role::Catalyst, novelty_ctx.clone());
        per_role_suffix.insert(Role::Explorer, novelty_ctx);

        // Chronicler: synthesis overview
        let active_skills = skill_bank.active_skill_ids().len();
        per_role_suffix.insert(
            Role::Chronicler,
            format!(
                "[Synthesis: {} active skills, {} associations]",
                active_skills,
                associations.len()
            ),
        );

        Self {
            shared_prefix,
            per_role_suffix,
            policy: ContextPolicy::default(),
        }
    }
}

// ── Correlation Monitor ─────────────────────────────────────────────────

/// Monitors rolling average pairwise bid correlation across consensus rounds.
/// Triggers re-specialization when correlation exceeds threshold (agents converging
/// = loss of cognitive diversity).
///
/// Normal ticks ──→ correlation > 0.80 ──→ RespecAction::Trigger
///                                            │
///                                    Re-specialize agents:
///                                    - Halve GRPO advantage for correlated agents
///                                    - Boost skill diversity delta
///                                    - Nudge role selection
///                                            │
///              correlation < 0.60 ──→ RespecAction::Resume
pub struct CorrelationMonitor {
    /// Rolling window of correlation values
    pub history: VecDeque<f64>,
    /// Window size for rolling average
    pub window_size: usize,
    /// Correlation threshold to trigger re-specialization
    pub threshold: f64,
    /// Recovery threshold to resume normal operation
    pub recovery_threshold: f64,
    /// Whether currently in re-specialization mode
    pub in_respec: bool,
}

impl Default for CorrelationMonitor {
    fn default() -> Self {
        Self {
            history: VecDeque::new(),
            window_size: 20,
            threshold: 0.80,
            recovery_threshold: 0.60,
            in_respec: false,
        }
    }
}

/// Re-specialization action triggered by CorrelationMonitor
#[derive(Clone, Debug, PartialEq)]
pub enum RespecAction {
    /// Correlation too high — trigger re-specialization of agents
    Trigger,
    /// Correlation recovered — resume normal operation
    Resume,
}

impl CorrelationMonitor {
    pub fn new(window_size: usize, threshold: f64, recovery: f64) -> Self {
        Self {
            history: VecDeque::new(),
            window_size,
            threshold,
            recovery_threshold: recovery,
            in_respec: false,
        }
    }

    /// Update with a new correlation observation from a consensus round.
    /// Returns a RespecAction if a state transition occurs.
    pub fn update(&mut self, correlation: f64) -> Option<RespecAction> {
        self.history.push_back(correlation);
        if self.history.len() > self.window_size {
            self.history.pop_front();
        }

        if self.history.is_empty() {
            return None;
        }

        let avg = self.history.iter().sum::<f64>() / self.history.len() as f64;

        if avg > self.threshold && !self.in_respec {
            self.in_respec = true;
            Some(RespecAction::Trigger)
        } else if avg < self.recovery_threshold && self.in_respec {
            self.in_respec = false;
            Some(RespecAction::Resume)
        } else {
            None
        }
    }

    /// Current rolling average correlation
    pub fn rolling_average(&self) -> f64 {
        if self.history.is_empty() {
            return 0.0;
        }
        self.history.iter().sum::<f64>() / self.history.len() as f64
    }

    /// Apply re-specialization actions to agents.
    /// - Halve GRPO advantage for agents with correlated bids
    /// - Inject noise into bid_bias to break correlation
    pub fn apply_respec(agents: &mut [Agent]) {
        for agent in agents.iter_mut() {
            // Inject diversity noise into bid_bias
            let noise = (rand::random::<f64>() - 0.5) * 0.4;
            agent.bid_bias = (agent.bid_bias + noise).clamp(0.3, 3.0);
        }
    }
}

// ── Identity Bridge Regularization ─────────────────────────────────────

/// Identity Bridge regularization for reversal curse mitigation.
/// During distillation, inject self-referencing "A → A" pairs so the
/// model learns symmetry implicitly.
///
/// Reference: Feb 2026 arXiv shows 1B models reach ~40% reversal success
/// with minimal identity bridge data tweaks.
pub struct IdentityBridgeRegularizer;

impl IdentityBridgeRegularizer {
    /// Generate identity bridge associations from current skill bank.
    /// For each skill, create a self-referencing fact that ensures
    /// bidirectional concept linking.
    pub fn generate_bridges(skill_bank: &SkillBank) -> Vec<EmergentAssociation> {
        let mut bridges = Vec::new();

        for skill in skill_bank.all_skills() {
            // Create identity bridge: skill's title ↔ skill's principle
            // This ensures the system can reason both forward (title → principle)
            // and reverse (principle → title)
            bridges.push(EmergentAssociation {
                skill_id: skill.id.clone(),
                association_type: AssociationType::IdentityBridge {
                    concept: skill.title.clone(),
                },
                vertices_involved: vec![],
                coherence_delta: 0.0, // neutral — regularization, not improvement
                novelty_score: 0.0,
                detected_at_tick: 0,
            });
        }

        bridges
    }

    /// Inject identity bridge Prolog facts into the engine.
    /// These create bidirectional unification paths:
    /// identity(skill_id, title, principle) — can be queried in either direction.
    pub fn as_prolog_facts(skill_bank: &SkillBank) -> Vec<(String, Vec<String>)> {
        skill_bank
            .all_skills()
            .iter()
            .map(|skill| {
                (
                    "identity_bridge".to_string(),
                    vec![
                        skill.id.clone(),
                        skill.title.clone(),
                        skill.principle.clone(),
                    ],
                )
            })
            .collect()
    }
}

// ── Guardian/Veto Critic ───────────────────────────────────────────────

/// Guardian critic that can veto skill applications or evolution decisions.
/// Pre-declares success criteria per workflow and blocks policy violations.
///
/// This addresses verification blind spots (Tech Severity 5/5) by routing
/// verification to distinct critic logic rather than relying on LLM self-checks.
pub struct GuardianCritic {
    /// Minimum coherence for skill application to proceed
    pub min_coherence: f64,
    /// Maximum allowed coherence drop per tick
    pub max_coherence_drop: f64,
    /// Maximum allowed unresolved belief conflicts
    pub max_unresolved_conflicts: usize,
    /// Veto threshold: if critic score < this, veto the action
    pub veto_threshold: f64,
}

impl Default for GuardianCritic {
    fn default() -> Self {
        Self {
            min_coherence: 0.2,
            max_coherence_drop: 0.15,
            max_unresolved_conflicts: 5,
            veto_threshold: 0.3,
        }
    }
}

/// Result of a guardian veto check
#[derive(Clone, Debug)]
pub struct VetoCheck {
    pub approved: bool,
    pub score: f64,
    pub violations: Vec<String>,
}

impl GuardianCritic {
    /// Check if a tick's proposed actions should be vetoed.
    /// Returns approval with score and any violations found.
    pub fn check(
        &self,
        coherence: f64,
        coherence_delta: f64,
        unresolved_conflicts: usize,
        skill_confidence: f64,
    ) -> VetoCheck {
        let mut violations = Vec::new();
        let mut score = 1.0;

        // Check coherence floor
        if coherence < self.min_coherence {
            violations.push(format!(
                "Coherence {:.3} below minimum {:.3}",
                coherence, self.min_coherence
            ));
            score *= 0.5;
        }

        // Check coherence drop
        if coherence_delta < -self.max_coherence_drop {
            violations.push(format!(
                "Coherence drop {:.4} exceeds max {:.4}",
                coherence_delta, self.max_coherence_drop
            ));
            score *= 0.3;
        }

        // Check unresolved conflicts
        if unresolved_conflicts > self.max_unresolved_conflicts {
            violations.push(format!(
                "{} unresolved conflicts exceed max {}",
                unresolved_conflicts, self.max_unresolved_conflicts
            ));
            score *= 0.7;
        }

        // Check skill confidence
        if skill_confidence < 0.2 {
            violations.push(format!(
                "Skill confidence {:.2} too low for application",
                skill_confidence
            ));
            score *= 0.8;
        }

        VetoCheck {
            approved: score >= self.veto_threshold,
            score,
            violations,
        }
    }
}
