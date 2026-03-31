//! SkillRL-inspired skill system for hyper-stigmergic morphogenesis.
//!
//! Implements hierarchical skill library (SkillBank) with:
//! - Experience-based distillation (τ⁺/τ⁻ → skills)
//! - Adaptive retrieval (embedding similarity + Prolog applicability)
//! - Recursive evolution (co-evolve skills with agent policy)
//!
//! Reference: SkillRL (arxiv 2602.08234) — Evolving Agents via
//! Recursive Skill-Augmented Reinforcement Learning

use std::collections::HashMap;
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::agent::Role;
use crate::consensus::{BayesianConfidence, SkillStatus};
use crate::hyper_stigmergy::{Experience, ExperienceOutcome, ImprovementEvent};

// ── Skill Types ──────────────────────────────────────────────────────────

/// A reusable behavioral pattern distilled from experience trajectories.
/// Maps to SkillRL's skill representation: (title, principle, when_to_apply).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Skill {
    pub id: String,
    pub title: String,
    pub principle: String,
    /// Logical predicates for applicability — fed to Prolog engine
    pub when_to_apply: Vec<ApplicabilityCondition>,
    pub level: SkillLevel,
    pub source: SkillSource,
    pub confidence: f64,
    pub usage_count: u64,
    pub success_count: u64,
    pub failure_count: u64,
    pub embedding: Option<Vec<f32>>,
    pub created_at: u64,
    pub last_evolved: u64,
    /// Consensus-based lifecycle status (Active/Advanced/Suspended/Deprecated)
    #[serde(default)]
    pub status: SkillStatus,
    /// Bayesian confidence posterior (Beta distribution)
    #[serde(default)]
    pub bayesian: BayesianConfidence,
    /// Exponentially weighted credit score from causal feedback
    #[serde(default)]
    pub credit_ema: f64,
    /// Number of credit updates applied
    #[serde(default)]
    pub credit_count: u64,
    /// Last tick when credit was updated
    #[serde(default)]
    pub last_credit_tick: u64,

    // ── Delegation / Orchestrator Fields ──
    /// How this skill was sourced. Only HumanCurated and Promoted skills
    /// should appear in active delegation briefings. Auto-generated skills
    /// stay in Proposed until a human reviews them.
    #[serde(default)]
    pub curation: SkillCuration,

    /// Scoping constraints — determines WHICH delegations should receive
    /// this skill in their briefing. Prevents broadcast.
    #[serde(default)]
    pub scope: SkillScope,

    /// Delegation quality score — tracks how well this skill performs
    /// as an ORCHESTRATOR (hiring decisions, briefing quality).
    /// Separate from credit_ema which tracks execution quality.
    #[serde(default)]
    pub delegation_ema: f64,

    /// Number of times this skill has acted as an orchestrator (hired others)
    #[serde(default)]
    pub delegation_count: u64,

    /// Number of times this skill has been hired by an orchestrator
    #[serde(default)]
    pub hired_count: u64,
}

/// Applicability condition — a structured predicate that the Prolog engine evaluates.
/// More precise than pure embedding similarity for structured conditions.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApplicabilityCondition {
    pub predicate: String,
    pub args: Vec<ConditionArg>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ConditionArg {
    Float(f64),
    Int(i64),
    Str(String),
    Role(Role),
    Var(String),
}

/// SkillRL's two-level hierarchy: general (𝒮_g) + task/role-specific (𝒮_k)
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum SkillLevel {
    /// Universal strategies applicable across all contexts
    General,
    /// Per-role specialization (Architect/Catalyst/Chronicler)
    RoleSpecific(Role),
    /// Per task-type or mutation-type specialization
    TaskSpecific(String),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SkillSource {
    /// Distilled from a successful or failed experience trajectory
    Distilled {
        from_experience_ids: Vec<usize>,
        trajectory_type: TrajectoryType,
    },
    /// Evolved from a parent skill during recursive evolution
    Evolved {
        parent_skill_id: String,
        evolution_epoch: u64,
    },
    /// Bootstrap skills seeded at initialization
    Seeded,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum TrajectoryType {
    Success,
    Failure,
}

// ── Delegation Primitives ────────────────────────────────────────────────
//
// Skills are not tasks dispatched to contractors — they are problem subspaces
// delegated to autonomous orchestrators who can hire further orchestrators.
// Each delegation carries a signed proof chain and a curated skill briefing:
// the 2-3 procedural knowledge modules the hire ACTUALLY needs (not everything).
//
// Key insight: auto-generated skills converge toward generic approaches.
// Skills matter where the model's training distribution has GAPS in procedural
// knowledge. Human-curated, focused skills with 2-3 modules outperform
// comprehensive documentation dumps. The orchestrator's core competency
// is assembling the right DelegationPackage for each hire.

/// How a skill was sourced — distinguishes curated expertise from auto-generated proposals.
/// Only HumanCurated and Promoted skills should be injected into active delegation briefings.
/// Proposed skills await human review before they enter the active pool.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SkillCuration {
    /// Extracted from real domain expertise by a human curator.
    /// These are the high-value skills that fill training distribution gaps.
    HumanCurated {
        curator: String,
        domain: String,
        curated_at: u64,
    },
    /// Auto-generated (from distillation, LLM traces, etc.) — NOT yet reviewed.
    /// Lives in a proposal queue, never auto-injected into active briefings.
    Proposed {
        source_description: String,
        proposed_at: u64,
    },
    /// Was Proposed, then human-approved and promoted to active pool.
    Promoted {
        original_source: String,
        promoted_by: String,
        promoted_at: u64,
    },
    /// Legacy: pre-existing skills before curation system. Treated as Proposed.
    Legacy,
}

impl Default for SkillCuration {
    fn default() -> Self {
        SkillCuration::Legacy
    }
}

/// Scoping constraints for a skill — determines WHEN and WHERE this skill
/// should be included in a delegation briefing.
///
/// A subagent implementing one phase of a feature doesn't need the same index
/// as the main thread architecting and phasing the plan. Scope enforces this.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SkillScope {
    /// Problem domains this skill is relevant to.
    /// e.g., ["cryptography", "token_generation", "jwt"]
    pub domains: Vec<String>,

    /// Delegation depth range where this skill applies.
    /// (0, 0) = root orchestrator only
    /// (1, 3) = mid-level managers
    /// (0, 255) = any depth (use sparingly — most skills should be scoped)
    pub depth_range: (u8, u8),

    /// Contexts where this skill is actively HARMFUL and must be excluded.
    /// Negative scoping prevents broadcasting.
    pub exclude_contexts: Vec<String>,

    /// Maximum simultaneous delegations that should receive this skill.
    /// Forces surgical selection over broadcast. 0 = unlimited (discouraged).
    pub max_concurrent_assignments: u8,
}

impl Default for SkillScope {
    fn default() -> Self {
        SkillScope {
            domains: Vec::new(),
            depth_range: (0, 255),
            exclude_contexts: Vec::new(),
            max_concurrent_assignments: 3,
        }
    }
}

impl SkillScope {
    /// Check if this skill is relevant for a given delegation context.
    pub fn matches(&self, subproblem_domains: &[String], depth: u8) -> bool {
        // Depth check
        if depth < self.depth_range.0 || depth > self.depth_range.1 {
            return false;
        }
        // Exclusion check
        for excl in &self.exclude_contexts {
            for domain in subproblem_domains {
                if domain.contains(excl.as_str()) {
                    return false;
                }
            }
        }
        // Domain relevance — if skill has no domain tags, it's unscoped (legacy)
        if self.domains.is_empty() {
            return true;
        }
        // At least one domain must overlap
        for skill_domain in &self.domains {
            for sub_domain in subproblem_domains {
                if skill_domain == sub_domain
                    || sub_domain.contains(skill_domain.as_str())
                    || skill_domain.contains(sub_domain.as_str())
                {
                    return true;
                }
            }
        }
        false
    }
}

/// Signed attestation in a delegation chain.
/// Each delegation produces a signature that chains upward — the audit trail
/// captures not just WHAT happened, but WHO decided to delegate WHAT to WHOM.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProofSignature {
    /// The skill that made this delegation decision
    pub signer_skill_id: String,
    /// What claim this signature attests to
    pub claim: String,
    /// Message IDs that support this delegation decision
    pub evidence_msg_ids: Vec<String>,
    /// Parent signature (if this is a sub-delegation)
    /// Forms the chain: root_sig → child_sig → grandchild_sig
    pub parent_signature_id: Option<String>,
    /// Unique ID for this signature
    pub signature_id: String,
    /// When this signature was created
    pub timestamp: u64,
}

/// The package an orchestrator assembles when hiring a sub-orchestrator.
/// This IS the orchestrator's core competency — the quality of this package
/// directly determines the orchestrator's credit_ema.
///
/// Key constraint: skill_briefing should contain 2-3 modules, NOT the full bank.
/// "A subagent tasked with implementing one phase of a feature doesn't need
/// the same index as the main thread tasked with architecting and phasing the plan."
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DelegationPackage {
    /// What portion of the problem this hire OWNS.
    /// Not a task description — a problem subspace with full autonomy.
    pub subproblem: String,

    /// Problem domain tags — used to scope skill selection.
    pub subproblem_domains: Vec<String>,

    /// The 2-3 curated skill modules this specific hire needs.
    /// NOT the full bank. The orchestrator's JUDGMENT about what fills
    /// this hire's training distribution gaps for this specific subproblem.
    pub skill_briefing: Vec<String>, // Skill IDs — max 5, ideal 2-3

    /// Authority signature from the hiring orchestrator.
    pub signature: ProofSignature,

    /// Resource/autonomy budget allocated to this hire.
    /// Constrains how deep the hire can further delegate.
    pub budget: f64,

    /// Depth of this delegation in the tree (root = 0).
    pub depth: u8,
}

/// Record of an orchestrator hiring a sub-orchestrator.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SkillHire {
    /// Unique identifier for this hire
    pub hire_id: String,
    /// The orchestrator that made the hiring decision
    pub parent_skill_id: String,
    /// The skill hired as sub-orchestrator
    pub child_skill_id: String,
    /// The delegation package given to the hire
    pub package: DelegationPackage,
    /// Current status of this hire
    pub status: HireStatus,
    /// Outcome quality (set after completion) — feeds credit propagation
    pub outcome_score: Option<f64>,
    /// When this hire was created
    pub created_at: u64,
    /// When this hire completed (if it did)
    pub completed_at: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum HireStatus {
    /// Hire is active, sub-orchestrator is working
    Active,
    /// Sub-orchestrator completed its subproblem
    Completed,
    /// Hire was revoked (orchestrator decided to re-delegate)
    Revoked,
    /// Sub-orchestrator failed its subproblem
    Failed,
}

/// The full delegation tree for a plan step.
/// Enables recursive credit propagation: leaf outcomes flow up through
/// the hire chain, crediting/debiting each orchestrator's judgment quality.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HireTree {
    /// Root skill that owns the top-level problem
    pub root_skill_id: String,
    /// All hires in this tree (flat for easy iteration; tree structure via parent_skill_id)
    pub hires: Vec<SkillHire>,
    /// Plan step index this tree serves
    pub plan_step_index: usize,
}

impl HireTree {
    pub fn new(root_skill_id: String, plan_step_index: usize) -> Self {
        HireTree {
            root_skill_id,
            hires: Vec::new(),
            plan_step_index,
        }
    }

    /// Get all direct hires by a given orchestrator
    pub fn children_of(&self, parent_id: &str) -> Vec<&SkillHire> {
        self.hires
            .iter()
            .filter(|h| h.parent_skill_id == parent_id)
            .collect()
    }

    /// Get the hire chain from a leaf back to root (for credit propagation)
    pub fn chain_to_root(&self, leaf_skill_id: &str) -> Vec<&SkillHire> {
        let mut chain = Vec::new();
        let mut current = leaf_skill_id;
        // Walk up the tree
        while let Some(hire) = self.hires.iter().find(|h| h.child_skill_id == current) {
            chain.push(hire);
            current = &hire.parent_skill_id;
            if current == self.root_skill_id {
                break;
            }
        }
        chain
    }

    /// Maximum depth of the hire tree
    pub fn max_depth(&self) -> u8 {
        self.hires
            .iter()
            .map(|h| h.package.depth)
            .max()
            .unwrap_or(0)
    }

    /// All leaf hires (no children — these are the executors)
    pub fn leaves(&self) -> Vec<&SkillHire> {
        self.hires
            .iter()
            .filter(|h| {
                !self
                    .hires
                    .iter()
                    .any(|other| other.parent_skill_id == h.child_skill_id)
            })
            .collect()
    }

    /// Propagate outcome credit recursively through the hire chain.
    ///
    /// Credit rule: each level gets a blend of:
    /// - Direct outcome quality (did the subtree produce good results?)
    /// - Delegation quality (did this orchestrator's briefing decisions help?)
    ///
    /// The SIGNATURE chain ensures accountability: you can trace exactly which
    /// orchestrator made which delegation decision, and how that decision
    /// contributed to the final outcome.
    pub fn propagate_credit(
        &self,
        bank: &mut SkillBank,
        base_delta: f64,
        tick: u64,
    ) -> CreditPropagationResult {
        let mut result = CreditPropagationResult::default();

        // Phase 1: Credit leaf executors with direct outcome
        for leaf in self.leaves() {
            let leaf_delta = base_delta * leaf_credit_weight(leaf);
            let ids = vec![leaf.child_skill_id.clone()];
            let report = bank.apply_skill_credit(&ids, leaf_delta, tick);
            result.leaf_updates += report.updated;
            result.total_credit_distributed += leaf_delta.abs();
        }

        // Phase 2: Credit managers based on their subtree outcomes
        // Walk from deepest level upward to root
        let max_d = self.max_depth();
        for depth in (0..max_d).rev() {
            let managers_at_depth: Vec<&SkillHire> = self
                .hires
                .iter()
                .filter(|h| h.package.depth == depth)
                .collect();

            for manager_hire in managers_at_depth {
                let children = self.children_of(&manager_hire.child_skill_id);
                if children.is_empty() {
                    continue; // leaf, already handled
                }

                // Manager credit = weighted avg of children outcomes × delegation quality factor
                let child_outcomes: Vec<f64> =
                    children.iter().filter_map(|c| c.outcome_score).collect();

                if child_outcomes.is_empty() {
                    continue;
                }

                let avg_child_outcome =
                    child_outcomes.iter().sum::<f64>() / child_outcomes.len() as f64;

                // Delegation quality bonus/penalty:
                // Good briefing (few, relevant skills) + good outcome = high credit
                // Bad briefing (too many skills, wrong skills) + bad outcome = high penalty
                let briefing_size = manager_hire.package.skill_briefing.len() as f64;
                let briefing_efficiency = if briefing_size <= 3.0 {
                    1.0 + (3.0 - briefing_size) * 0.1 // Bonus for focused briefings
                } else {
                    1.0 - (briefing_size - 3.0) * 0.15 // Penalty for over-briefing
                };

                let manager_delta = base_delta * avg_child_outcome * briefing_efficiency * 0.8;
                let ids = vec![manager_hire.child_skill_id.clone()];
                let report = bank.apply_skill_credit(&ids, manager_delta, tick);
                result.manager_updates += report.updated;
                result.total_credit_distributed += manager_delta.abs();
            }
        }

        // Phase 3: Credit root orchestrator
        let all_outcomes: Vec<f64> = self.hires.iter().filter_map(|h| h.outcome_score).collect();
        if !all_outcomes.is_empty() {
            let tree_outcome = all_outcomes.iter().sum::<f64>() / all_outcomes.len() as f64;
            let root_delta = base_delta * tree_outcome * 0.6; // Root gets less direct credit
            let ids = vec![self.root_skill_id.clone()];
            let report = bank.apply_skill_credit(&ids, root_delta, tick);
            result.root_updated = report.updated > 0;
            result.total_credit_distributed += root_delta.abs();
        }

        result
    }
}

/// Weight for leaf credit — based on subproblem scope
fn leaf_credit_weight(hire: &SkillHire) -> f64 {
    match hire.status {
        HireStatus::Completed => 1.0,
        HireStatus::Failed => -0.5,
        HireStatus::Active => 0.0,   // Not yet resolved
        HireStatus::Revoked => -0.2, // Manager decided to re-delegate
    }
}

#[derive(Clone, Debug, Default)]
pub struct CreditPropagationResult {
    pub leaf_updates: usize,
    pub manager_updates: usize,
    pub root_updated: bool,
    pub total_credit_distributed: f64,
}

// ── SkillBank ────────────────────────────────────────────────────────────

/// Hierarchical skill library: 𝒮 = 𝒮_g ∪ ⋃_k 𝒮_k
///
/// Implements SkillRL's three mechanisms:
/// 1. Experience-based distillation → `distill_from_experiences()`
/// 2. Adaptive retrieval → `retrieve()`
/// 3. Recursive evolution → `evolve()`
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SkillBank {
    pub general_skills: Vec<Skill>,
    pub role_skills: HashMap<String, Vec<Skill>>,
    pub task_skills: HashMap<String, Vec<Skill>>,
    pub evolution_epoch: u64,
    pub total_distillations: u64,
    /// SkillRL's δ threshold for retrieval similarity
    pub retrieval_threshold: f64,
    /// SkillRL's K for top-K retrieval
    pub retrieval_top_k: usize,
    /// Evolution trigger: evolve when success rate drops below this
    pub evolution_trigger_threshold: f64,
    /// Next skill ID counter
    next_id: u64,
    #[serde(default)]
    pub credit_history: Vec<SkillCreditRecord>,
    /// Active delegation/hire trees — one per plan step being orchestrated.
    /// These are the live organizational structures of recursive delegation.
    #[serde(default)]
    pub hire_trees: Vec<HireTree>,
    /// Completed hire trees — archived for audit and credit analysis.
    #[serde(default)]
    pub hire_history: Vec<HireTree>,
}

#[derive(Clone, Debug, Default)]
pub struct SkillCreditReport {
    pub updated: usize,
    pub suspended: usize,
    pub revived: usize,
    pub mean_credit: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SkillCreditRecord {
    pub tick: u64,
    pub skill_id: String,
    pub credit_delta: f64,
    pub credit_ema: f64,
    pub confidence: f64,
    pub status: SkillStatus,
}

impl Default for SkillBank {
    fn default() -> Self {
        Self {
            general_skills: Vec::new(),
            role_skills: HashMap::new(),
            task_skills: HashMap::new(),
            evolution_epoch: 0,
            total_distillations: 0,
            retrieval_threshold: 0.4, // SkillRL default δ
            retrieval_top_k: 6,       // SkillRL default K
            evolution_trigger_threshold: 0.4,
            next_id: 0,
            credit_history: Vec::new(),
            hire_trees: Vec::new(),
            hire_history: Vec::new(),
        }
    }
}

impl SkillBank {
    /// Create a new SkillBank seeded with bootstrap general skills.
    /// These correspond to SkillRL's initial 𝒮_g before any distillation.
    pub fn new_with_seeds() -> Self {
        let now = now_secs();
        let mut bank = Self::default();

        // Seed general skills — universal strategies for hypergraph morphogenesis
        let seeds = vec![
            ("Coherence Preservation",
             "Before any mutation, verify that expected coherence delta is positive or near-zero",
             vec![cond("coherence_above", &[ConditionArg::Float(0.0)])]),
            ("Exploration-Exploitation Balance",
             "When coherence plateaus (delta < 0.001 for 5+ ticks), increase exploration temperature",
             vec![cond("coherence_plateau", &[ConditionArg::Int(5)])]),
            ("Edge Density Regulation",
             "Maintain edge-to-agent ratio between 1.5 and 5.0 to avoid fragmentation or noise",
             vec![cond("edge_density_range", &[ConditionArg::Float(1.5), ConditionArg::Float(5.0)])]),
            ("Novelty Gradient Following",
             "Pursue mutations with novelty > 0.5 when coherence is stable or rising",
             vec![cond("coherence_stable", &[]), cond("novelty_above", &[ConditionArg::Float(0.5)])]),
            ("Loop Escape Trigger",
             "Switch strategy after 3+ consecutive no-improvement ticks",
             vec![cond("consecutive_no_improvement", &[ConditionArg::Int(3)])]),
            ("Destructive Action Gate",
             "Abliteration and large-scale pruning require coherence > 0.5 and human approval",
             vec![cond("coherence_above", &[ConditionArg::Float(0.5)]), cond("action_is_destructive", &[])]),
            ("Belief Contradiction Resolution",
             "When contradicting beliefs exist, prioritize the one with more supporting evidence",
             vec![cond("belief_contradictions_exist", &[])]),
            ("Cluster Connectivity Maintenance",
             "Ensure no agent cluster becomes fully disconnected from the main graph",
             vec![cond("disconnected_clusters_exist", &[])]),
        ];

        for (title, principle, conditions) in seeds {
            bank.general_skills.push(Skill {
                id: format!("gen_{:03}", bank.next_id),
                title: title.to_string(),
                principle: principle.to_string(),
                when_to_apply: conditions,
                level: SkillLevel::General,
                source: SkillSource::Seeded,
                confidence: 0.7,
                usage_count: 0,
                success_count: 0,
                failure_count: 0,
                embedding: None,
                created_at: now,
                last_evolved: now,
                status: SkillStatus::Active,
                bayesian: BayesianConfidence::new(2.0, 1.0), // slight positive prior for seeds
                credit_ema: 0.0,
                credit_count: 0,
                last_credit_tick: 0,
                curation: SkillCuration::Legacy, // Seeds are generic — await human curation
                scope: SkillScope::default(),
                delegation_ema: 0.0,
                delegation_count: 0,
                hired_count: 0,
            });
            bank.next_id += 1;
        }

        // Seed role-specific skills
        let role_seeds: Vec<(Role, &str, &str, Vec<ApplicabilityCondition>)> = vec![
            (Role::Architect, "Structural Integrity First",
             "Prioritize topology mutations that increase spectral gap of the adjacency matrix",
             vec![cond("role_is", &[ConditionArg::Role(Role::Architect)])]),
            (Role::Architect, "Ontology-Guided Linking",
             "When linking agents, prefer connections that align with ontology concept hierarchy",
             vec![cond("role_is", &[ConditionArg::Role(Role::Architect)]), cond("ontology_available", &[])]),
            (Role::Catalyst, "Controlled Disruption",
             "Introduce novelty by rewiring 10-20% of weakest edges rather than random mutations",
             vec![cond("role_is", &[ConditionArg::Role(Role::Catalyst)]), cond("weak_edges_exist", &[])]),
            (Role::Catalyst, "Cross-Cluster Bridging",
             "Create edges between distant clusters to increase information flow",
             vec![cond("role_is", &[ConditionArg::Role(Role::Catalyst)]), cond("multiple_clusters", &[])]),
            (Role::Chronicler, "Experience Compression",
             "Summarize every 10 experiences into a single high-confidence belief",
             vec![cond("role_is", &[ConditionArg::Role(Role::Chronicler)]),
                  cond("experience_count_above", &[ConditionArg::Int(10)])]),
            (Role::Chronicler, "Belief Confidence Calibration",
             "Decay beliefs that haven't been confirmed by recent experiences",
             vec![cond("role_is", &[ConditionArg::Role(Role::Chronicler)])]),
        ];

        for (role, title, principle, conditions) in role_seeds {
            let role_key = format!("{:?}", role);
            bank.role_skills.entry(role_key).or_default().push(Skill {
                id: format!("role_{:03}", bank.next_id),
                title: title.to_string(),
                principle: principle.to_string(),
                when_to_apply: conditions,
                level: SkillLevel::RoleSpecific(role),
                source: SkillSource::Seeded,
                confidence: 0.6,
                usage_count: 0,
                success_count: 0,
                failure_count: 0,
                embedding: None,
                created_at: now,
                last_evolved: now,
                status: SkillStatus::Active,
                bayesian: BayesianConfidence::new(1.5, 1.0),
                credit_ema: 0.0,
                credit_count: 0,
                last_credit_tick: 0,
                curation: SkillCuration::Legacy,
                scope: SkillScope::default(),
                delegation_ema: 0.0,
                delegation_count: 0,
                hired_count: 0,
            });
            bank.next_id += 1;
        }

        bank
    }

    /// Total skill count across all levels
    pub fn total_skills(&self) -> usize {
        self.general_skills.len()
            + self.role_skills.values().map(|v| v.len()).sum::<usize>()
            + self.task_skills.values().map(|v| v.len()).sum::<usize>()
    }

    // ── Retrieval (SkillRL §4.2) ─────────────────────────────────────────

    /// Adaptive retrieval: 𝒮_ret = 𝒮_g ∪ TopK({s ∈ 𝒮_k : sim(e_d, e_s) > δ}, K)
    ///
    /// Hybrid strategy:
    /// 1. Always include all general skills
    /// 2. Filter by Prolog applicability (logical conditions)
    /// 3. Rank by embedding similarity
    /// 4. Return top-K
    pub fn retrieve(
        &self,
        role: &Role,
        context_embedding: Option<&[f32]>,
        applicable_skill_ids: &[String], // From Prolog engine
    ) -> RetrievedSkills {
        // Filter out suspended and deprecated skills from retrieval
        let mut general: Vec<Skill> = self
            .general_skills
            .iter()
            .filter(|s| matches!(s.status, SkillStatus::Active | SkillStatus::Advanced))
            .cloned()
            .collect();
        let mut specific = Vec::new();

        // Gather role-specific candidates
        let role_key = format!("{:?}", role);
        if let Some(role_skills) = self.role_skills.get(&role_key) {
            specific.extend(role_skills.iter().cloned());
        }

        // Gather task-specific candidates
        for task_skills in self.task_skills.values() {
            specific.extend(task_skills.iter().cloned());
        }

        // Filter by Prolog applicability (if provided)
        if !applicable_skill_ids.is_empty() {
            specific.retain(|s| applicable_skill_ids.contains(&s.id));
        }

        // Rank by embedding similarity (if embeddings available)
        if let Some(ctx_emb) = context_embedding {
            specific.sort_by(|a, b| {
                let sim_a = a
                    .embedding
                    .as_ref()
                    .map(|e| cosine_similarity(ctx_emb, e))
                    .unwrap_or(0.0);
                let sim_b = b
                    .embedding
                    .as_ref()
                    .map(|e| cosine_similarity(ctx_emb, e))
                    .unwrap_or(0.0);
                sim_b
                    .partial_cmp(&sim_a)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });

            // Filter by threshold
            specific.retain(|s| {
                s.embedding
                    .as_ref()
                    .map(|e| cosine_similarity(ctx_emb, e) > self.retrieval_threshold)
                    .unwrap_or(true) // Keep skills without embeddings
            });
        }

        // Top-K selection
        specific.truncate(self.retrieval_top_k);

        // Sort general by confidence (highest first)
        general.sort_by(|a, b| {
            b.confidence
                .partial_cmp(&a.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        RetrievedSkills { general, specific }
    }

    /// Format retrieved skills into a prompt section for LLM context augmentation.
    /// This is the skill→prompt injection point (SkillRL's policy conditioning).
    pub fn format_for_prompt(retrieved: &RetrievedSkills) -> String {
        let mut prompt = String::new();

        prompt.push_str("### Active Skills (General Strategies)\n");
        for skill in &retrieved.general {
            prompt.push_str(&format!(
                "- [{}] {}: {} (confidence: {:.0}%)\n",
                skill.id,
                skill.title,
                skill.principle,
                skill.confidence * 100.0
            ));
        }

        if !retrieved.specific.is_empty() {
            prompt.push_str("\n### Active Skills (Context-Specific)\n");
            for skill in &retrieved.specific {
                prompt.push_str(&format!(
                    "- [{}] {}: {} (confidence: {:.0}%, used: {}x)\n",
                    skill.id,
                    skill.title,
                    skill.principle,
                    skill.confidence * 100.0,
                    skill.usage_count
                ));
            }
        }

        prompt
    }

    pub fn ensure_plan_skill(&mut self, title: &str, principle: &str) -> Skill {
        if let Some(skill) = self.general_skills.iter_mut().find(|s| s.title == title) {
            skill.principle = principle.to_string();
            skill.last_evolved = now_secs();
            skill.credit_ema = (skill.credit_ema * 0.7) + 0.3;
            skill.confidence = (skill.confidence + 0.03).min(0.95);
            return skill.clone();
        }
        let id = format!("plan_{}", self.next_id);
        self.next_id += 1;
        let now = now_secs();
        let skill = Skill {
            id: id.clone(),
            title: title.to_string(),
            principle: principle.to_string(),
            when_to_apply: Vec::new(),
            level: SkillLevel::General,
            source: SkillSource::Distilled {
                from_experience_ids: Vec::new(),
                trajectory_type: TrajectoryType::Success,
            },
            confidence: 0.7,
            usage_count: 0,
            success_count: 0,
            failure_count: 0,
            embedding: None,
            created_at: now,
            last_evolved: now,
            status: SkillStatus::Active,
            bayesian: BayesianConfidence::new(1.5, 1.0),
            credit_ema: 0.0,
            credit_count: 0,
            last_credit_tick: now,
            curation: SkillCuration::Proposed {
                source_description: "plan_step_extraction".into(),
                proposed_at: now,
            },
            scope: SkillScope::default(),
            delegation_ema: 0.0,
            delegation_count: 0,
            hired_count: 0,
        };
        self.general_skills.push(skill.clone());
        skill
    }

    /// Ingest a merged Trace2Skill document as a **Proposed** general skill (or update by title).
    pub fn ingest_trace2skill_proposal(
        &mut self,
        title: &str,
        principle: &str,
        trajectory_ids: &[String],
    ) -> Skill {
        let now = now_secs();
        let source_description = format!(
            "trace2skill:{}",
            trajectory_ids
                .iter()
                .take(12)
                .cloned()
                .collect::<Vec<_>>()
                .join(",")
        );
        if let Some(skill) = self
            .general_skills
            .iter_mut()
            .find(|s| s.title == title)
        {
            skill.principle = format!(
                "{}\n\n--- trace2skill ---\n{}",
                skill.principle, principle
            );
            skill.last_evolved = now;
            skill.curation = SkillCuration::Proposed {
                source_description,
                proposed_at: now,
            };
            skill.source = SkillSource::Distilled {
                from_experience_ids: Vec::new(),
                trajectory_type: TrajectoryType::Success,
            };
            return skill.clone();
        }
        let id = format!("t2s_{}", self.next_id);
        self.next_id += 1;
        let skill = Skill {
            id: id.clone(),
            title: title.to_string(),
            principle: principle.to_string(),
            when_to_apply: Vec::new(),
            level: SkillLevel::General,
            source: SkillSource::Distilled {
                from_experience_ids: Vec::new(),
                trajectory_type: TrajectoryType::Success,
            },
            confidence: 0.65,
            usage_count: 0,
            success_count: 0,
            failure_count: 0,
            embedding: None,
            created_at: now,
            last_evolved: now,
            status: SkillStatus::Active,
            bayesian: BayesianConfidence::new(1.5, 1.0),
            credit_ema: 0.0,
            credit_count: 0,
            last_credit_tick: now,
            curation: SkillCuration::Proposed {
                source_description,
                proposed_at: now,
            },
            scope: SkillScope::default(),
            delegation_ema: 0.0,
            delegation_count: 0,
            hired_count: 0,
        };
        self.general_skills.push(skill.clone());
        skill
    }

    // ── Distillation (SkillRL §4.1) ──────────────────────────────────────

    /// Distill skills from experience trajectories.
    /// s⁺ = ℳ_T(τ⁺, d) for successes, s⁻ = ℳ_T(τ⁻, d) for failures.
    ///
    /// This is the heuristic fallback — the Ollama-powered version is in
    /// `distill_via_llm()` which calls the teacher model.
    pub fn distill_from_experiences(
        &mut self,
        experiences: &[Experience],
        improvement_events: &[ImprovementEvent],
    ) -> DistillationResult {
        let now = now_secs();
        let mut new_skills = Vec::new();

        // Separate success/failure trajectories
        let successes: Vec<&Experience> = experiences
            .iter()
            .filter(|e| matches!(e.outcome, ExperienceOutcome::Positive { .. }))
            .collect();
        let failures: Vec<&Experience> = experiences
            .iter()
            .filter(|e| matches!(e.outcome, ExperienceOutcome::Negative { .. }))
            .collect();

        // Distill from successful trajectories — extract generalizable patterns
        if successes.len() >= 3 {
            // Find common contexts across successes
            let common_patterns = find_common_patterns(
                &successes
                    .iter()
                    .map(|e| e.context.as_str())
                    .collect::<Vec<_>>(),
            );
            for pattern in common_patterns {
                let skill = Skill {
                    id: format!("dist_{:03}", self.next_id),
                    title: format!("Success Pattern: {}", truncate(&pattern, 40)),
                    principle: format!(
                        "When context includes '{}', follow the approach that led to positive outcomes in {} experiences",
                        truncate(&pattern, 60),
                        successes.len()
                    ),
                    when_to_apply: vec![cond("context_contains", &[ConditionArg::Str(pattern.clone())])],
                    level: SkillLevel::General,
                    source: SkillSource::Distilled {
                        from_experience_ids: successes.iter().map(|e| e.id).collect(),
                        trajectory_type: TrajectoryType::Success,
                    },
                    confidence: 0.5 + 0.1 * successes.len().min(5) as f64,
                    usage_count: 0,
                    success_count: 0,
                    failure_count: 0,
                    embedding: None,
                    created_at: now,
                    last_evolved: now,
                    status: SkillStatus::Active,
                    bayesian: BayesianConfidence::default(),
                    credit_ema: 0.0,
                    credit_count: 0,
                    last_credit_tick: 0,
                    curation: SkillCuration::Proposed {
                        source_description: "distilled_from_success_trajectories".into(),
                        proposed_at: now,
                    },
                    scope: SkillScope::default(),
                    delegation_ema: 0.0,
                    delegation_count: 0,
                    hired_count: 0,
                };
                self.next_id += 1;
                new_skills.push(skill);
            }
        }

        // Distill from failure trajectories — extract avoidance patterns
        if failures.len() >= 2 {
            let failure_patterns = find_common_patterns(
                &failures
                    .iter()
                    .map(|e| e.context.as_str())
                    .collect::<Vec<_>>(),
            );
            for pattern in failure_patterns {
                let avg_delta: f64 = failures
                    .iter()
                    .filter_map(|e| match &e.outcome {
                        ExperienceOutcome::Negative { coherence_delta } => Some(*coherence_delta),
                        _ => None,
                    })
                    .sum::<f64>()
                    / failures.len().max(1) as f64;

                let skill = Skill {
                    id: format!("dist_{:03}", self.next_id),
                    title: format!("Failure Avoidance: {}", truncate(&pattern, 40)),
                    principle: format!(
                        "AVOID actions in context '{}' — {} failures observed with avg coherence delta {:.4}",
                        truncate(&pattern, 50),
                        failures.len(),
                        avg_delta
                    ),
                    when_to_apply: vec![cond("context_contains", &[ConditionArg::Str(pattern)])],
                    level: SkillLevel::General,
                    source: SkillSource::Distilled {
                        from_experience_ids: failures.iter().map(|e| e.id).collect(),
                        trajectory_type: TrajectoryType::Failure,
                    },
                    confidence: 0.6,
                    usage_count: 0,
                    success_count: 0,
                    failure_count: 0,
                    embedding: None,
                    created_at: now,
                    last_evolved: now,
                    status: SkillStatus::Active,
                    bayesian: BayesianConfidence::default(),
                    credit_ema: 0.0,
                    credit_count: 0,
                    last_credit_tick: 0,
                    curation: SkillCuration::Proposed {
                        source_description: "distilled_from_failure_trajectories".into(),
                        proposed_at: now,
                    },
                    scope: SkillScope::default(),
                    delegation_ema: 0.0,
                    delegation_count: 0,
                    hired_count: 0,
                };
                self.next_id += 1;
                new_skills.push(skill);
            }
        }

        // Distill from improvement events — mutation-type specific skills
        let mut by_mutation: HashMap<String, Vec<&ImprovementEvent>> = HashMap::new();
        for event in improvement_events {
            let key = format!("{:?}", event.mutation_type);
            by_mutation.entry(key).or_default().push(event);
        }

        for (mutation_type, events) in &by_mutation {
            let successes: Vec<&&ImprovementEvent> = events
                .iter()
                .filter(|e| e.applied && e.coherence_after > e.coherence_before)
                .collect();
            let failures: Vec<&&ImprovementEvent> = events
                .iter()
                .filter(|e| e.applied && e.coherence_after <= e.coherence_before)
                .collect();

            if successes.len() + failures.len() >= 3 {
                let success_rate =
                    successes.len() as f64 / (successes.len() + failures.len()) as f64;
                let avg_success_delta: f64 = successes
                    .iter()
                    .map(|e| e.coherence_after - e.coherence_before)
                    .sum::<f64>()
                    / successes.len().max(1) as f64;

                let skill = Skill {
                    id: format!("task_{:03}", self.next_id),
                    title: format!("{} Strategy", mutation_type),
                    principle: format!(
                        "{} has {:.0}% success rate with avg delta {:.4}. {}",
                        mutation_type,
                        success_rate * 100.0,
                        avg_success_delta,
                        if success_rate > 0.6 {
                            "Prefer this mutation type."
                        } else {
                            "Use cautiously, high failure rate."
                        }
                    ),
                    when_to_apply: vec![cond(
                        "mutation_type_is",
                        &[ConditionArg::Str(mutation_type.clone())],
                    )],
                    level: SkillLevel::TaskSpecific(mutation_type.clone()),
                    source: SkillSource::Distilled {
                        from_experience_ids: vec![],
                        trajectory_type: if success_rate > 0.5 {
                            TrajectoryType::Success
                        } else {
                            TrajectoryType::Failure
                        },
                    },
                    confidence: success_rate,
                    usage_count: 0,
                    success_count: 0,
                    failure_count: 0,
                    embedding: None,
                    created_at: now,
                    last_evolved: now,
                    status: SkillStatus::Active,
                    bayesian: BayesianConfidence::default(),
                    credit_ema: 0.0,
                    credit_count: 0,
                    last_credit_tick: 0,
                    curation: SkillCuration::Proposed {
                        source_description: format!("distilled_from_{}_events", mutation_type),
                        proposed_at: now,
                    },
                    scope: SkillScope {
                        domains: vec![mutation_type.clone()],
                        ..SkillScope::default()
                    },
                    delegation_ema: 0.0,
                    delegation_count: 0,
                    hired_count: 0,
                };
                self.next_id += 1;
                new_skills.push(skill);
            }
        }

        // Add to bank
        let count = new_skills.len();
        for skill in new_skills.iter().cloned() {
            match &skill.level {
                SkillLevel::General => self.general_skills.push(skill),
                SkillLevel::RoleSpecific(role) => {
                    let key = format!("{:?}", role);
                    self.role_skills.entry(key).or_default().push(skill);
                }
                SkillLevel::TaskSpecific(task) => {
                    self.task_skills
                        .entry(task.clone())
                        .or_default()
                        .push(skill);
                }
            }
        }

        self.total_distillations += 1;

        DistillationResult {
            new_skills: count,
            from_successes: successes.len(),
            from_failures: failures.len(),
        }
    }

    /// Build the Ollama prompt for LLM-powered distillation (teacher model ℳ_T).
    /// Returns the prompt string to send to Ollama.
    pub fn build_distillation_prompt(
        &self,
        experiences: &[Experience],
        improvement_events: &[ImprovementEvent],
    ) -> String {
        let mut prompt = String::new();
        prompt.push_str(
            "You are analyzing a hyper-stigmergic morphogenesis system's execution traces.\n",
        );
        prompt.push_str("Extract reusable SKILLS that capture generalizable strategies.\n\n");

        prompt.push_str("## Successful Experiences\n");
        for exp in experiences
            .iter()
            .filter(|e| matches!(e.outcome, ExperienceOutcome::Positive { .. }))
        {
            let delta = match &exp.outcome {
                ExperienceOutcome::Positive { coherence_delta } => *coherence_delta,
                _ => 0.0,
            };
            prompt.push_str(&format!(
                "- [+{:.4}] {} | context: {}\n",
                delta, exp.description, exp.context
            ));
        }

        prompt.push_str("\n## Failed Experiences\n");
        for exp in experiences
            .iter()
            .filter(|e| matches!(e.outcome, ExperienceOutcome::Negative { .. }))
        {
            let delta = match &exp.outcome {
                ExperienceOutcome::Negative { coherence_delta } => *coherence_delta,
                _ => 0.0,
            };
            prompt.push_str(&format!(
                "- [{:.4}] {} | context: {}\n",
                delta, exp.description, exp.context
            ));
        }

        prompt.push_str("\n## Improvement Events\n");
        for event in improvement_events.iter().rev().take(10) {
            let delta = event.coherence_after - event.coherence_before;
            prompt.push_str(&format!(
                "- {:?}: delta={:+.4}, novelty={:.2}, applied={}\n",
                event.mutation_type, delta, event.novelty_score, event.applied
            ));
        }

        prompt.push_str("\n## Existing Skills (avoid duplicates)\n");
        for skill in &self.general_skills {
            prompt.push_str(&format!("- {}: {}\n", skill.title, skill.principle));
        }

        prompt.push_str(concat!(
            "\nFor each new skill, output in this format:\n",
            "SKILL: <title>\n",
            "PRINCIPLE: <1-2 sentence actionable strategy>\n",
            "WHEN: <logical condition for applicability>\n",
            "LEVEL: general|architect|catalyst|chronicler|<task_type>\n\n",
            "Extract 2-5 skills. Focus on patterns that generalize.\n"
        ));

        prompt
    }

    /// Parse LLM distillation output into skills
    pub fn parse_distilled_skills(&mut self, llm_output: &str) -> Vec<Skill> {
        let now = now_secs();
        let mut skills = Vec::new();
        let mut current_title = String::new();
        let mut current_principle = String::new();
        let mut current_when = String::new();
        let mut current_level = String::new();

        for line in llm_output.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("SKILL:") {
                // Flush previous
                if !current_title.is_empty() && !current_principle.is_empty() {
                    skills.push(self.build_parsed_skill(
                        &current_title,
                        &current_principle,
                        &current_when,
                        &current_level,
                        now,
                    ));
                }
                current_title = trimmed.trim_start_matches("SKILL:").trim().to_string();
                current_principle.clear();
                current_when.clear();
                current_level.clear();
            } else if trimmed.starts_with("PRINCIPLE:") {
                current_principle = trimmed.trim_start_matches("PRINCIPLE:").trim().to_string();
            } else if trimmed.starts_with("WHEN:") {
                current_when = trimmed.trim_start_matches("WHEN:").trim().to_string();
            } else if trimmed.starts_with("LEVEL:") {
                current_level = trimmed.trim_start_matches("LEVEL:").trim().to_string();
            }
        }
        // Flush last
        if !current_title.is_empty() && !current_principle.is_empty() {
            skills.push(self.build_parsed_skill(
                &current_title,
                &current_principle,
                &current_when,
                &current_level,
                now,
            ));
        }

        // Add to bank
        for skill in &skills {
            match &skill.level {
                SkillLevel::General => self.general_skills.push(skill.clone()),
                SkillLevel::RoleSpecific(role) => {
                    let key = format!("{:?}", role);
                    self.role_skills.entry(key).or_default().push(skill.clone());
                }
                SkillLevel::TaskSpecific(task) => {
                    self.task_skills
                        .entry(task.clone())
                        .or_default()
                        .push(skill.clone());
                }
            }
        }

        skills
    }

    fn build_parsed_skill(
        &mut self,
        title: &str,
        principle: &str,
        when: &str,
        level: &str,
        now: u64,
    ) -> Skill {
        let skill_level = match level.to_lowercase().as_str() {
            "general" => SkillLevel::General,
            "architect" => SkillLevel::RoleSpecific(Role::Architect),
            "catalyst" => SkillLevel::RoleSpecific(Role::Catalyst),
            "chronicler" => SkillLevel::RoleSpecific(Role::Chronicler),
            other => SkillLevel::TaskSpecific(other.to_string()),
        };

        let conditions = if when.is_empty() {
            vec![]
        } else {
            vec![ApplicabilityCondition {
                predicate: "llm_condition".to_string(),
                args: vec![ConditionArg::Str(when.to_string())],
            }]
        };

        let id = format!("llm_{:03}", self.next_id);
        self.next_id += 1;

        Skill {
            id,
            title: title.to_string(),
            principle: principle.to_string(),
            when_to_apply: conditions,
            level: skill_level,
            source: SkillSource::Distilled {
                from_experience_ids: vec![],
                trajectory_type: TrajectoryType::Success,
            },
            confidence: 0.5,
            usage_count: 0,
            success_count: 0,
            failure_count: 0,
            embedding: None,
            created_at: now,
            last_evolved: now,
            status: SkillStatus::Active,
            bayesian: BayesianConfidence::default(),
            credit_ema: 0.0,
            credit_count: 0,
            last_credit_tick: 0,
            curation: SkillCuration::Proposed {
                source_description: "llm_distillation".into(),
                proposed_at: now,
            },
            scope: SkillScope::default(),
            delegation_ema: 0.0,
            delegation_count: 0,
            hired_count: 0,
        }
    }

    // ── Evolution (SkillRL §4.3) ─────────────────────────────────────────

    /// Recursive skill evolution with Bayesian confidence + consensus integration.
    ///
    /// Replaces raw success_rate < 0.3 with:
    /// 1. Bayesian posterior lower bound (conservative credible interval)
    /// 2. Suspension instead of immediate deprecation
    /// 3. Hard negative mining: tighten applicability from failure patterns
    /// 4. Source-tagged bias: preserve high-confidence Seeded skills longer
    ///
    /// If `consensus_results` is provided, uses consensus verdicts for promotion/suspension.
    /// Otherwise falls back to Bayesian-only evaluation.
    pub fn evolve(&mut self, failed_experiences: &[Experience]) -> EvolutionResult {
        let now = now_secs();
        let mut refined = 0;
        let mut deprecated = 0;
        let mut suspended = 0;

        // Phase 1: Bayesian evaluation of all skills
        let all_skills = self.all_skills_mut();
        for skill in all_skills {
            // Skip already deprecated skills
            if skill.status == SkillStatus::Deprecated {
                continue;
            }

            // Only evaluate skills with enough observations
            if !skill.bayesian.is_confident() {
                continue;
            }

            let posterior_mean = skill.bayesian.mean();
            let lower_bound = skill.bayesian.lower_bound_95();

            // Source-tagged bias: Seeded skills get a gentler threshold
            let suspend_threshold = match &skill.source {
                SkillSource::Seeded => 0.2, // more lenient
                SkillSource::Evolved { .. } => 0.25,
                SkillSource::Distilled { .. } => 0.3,
            };

            if lower_bound < suspend_threshold {
                // Suspend (not deprecate) — can be revived by consensus
                match &skill.status {
                    SkillStatus::Suspended {
                        revival_attempts, ..
                    } => {
                        if *revival_attempts >= 3 {
                            skill.status = SkillStatus::Deprecated;
                            skill.confidence *= 0.3;
                            deprecated += 1;
                        } else {
                            skill.status = SkillStatus::Suspended {
                                suspended_at_tick: now,
                                revival_attempts: revival_attempts + 1,
                            };
                            skill.confidence *= 0.8;
                            suspended += 1;
                        }
                    }
                    _ => {
                        skill.status = SkillStatus::Suspended {
                            suspended_at_tick: now,
                            revival_attempts: 0,
                        };
                        skill.confidence *= 0.85;
                        suspended += 1;
                    }
                }
                skill.last_evolved = now;
            } else if posterior_mean > 0.7 {
                // Strengthen: promote to Advanced
                skill.confidence = (skill.confidence * 1.1).min(0.99);
                if skill.status != SkillStatus::Advanced {
                    skill.status = SkillStatus::Advanced;
                }
                skill.last_evolved = now;
                refined += 1;
            }

            // Hard negative mining: tighten applicability from failures
            if posterior_mean < 0.4 && skill.failure_count > 3 {
                // Find common context patterns in failures for this skill
                let failure_contexts: Vec<&str> = failed_experiences
                    .iter()
                    .filter(|e| matches!(e.outcome, ExperienceOutcome::Negative { .. }))
                    .map(|e| e.context.as_str())
                    .take(5)
                    .collect();

                if !failure_contexts.is_empty() {
                    // Add a negative applicability condition
                    let common = failure_contexts[0];
                    if common.len() > 5 {
                        let negative_cond = ApplicabilityCondition {
                            predicate: "context_not_contains".to_string(),
                            args: vec![ConditionArg::Str(truncate(common, 30).to_string())],
                        };
                        if !skill
                            .when_to_apply
                            .iter()
                            .any(|c| c.predicate == "context_not_contains")
                        {
                            skill.when_to_apply.push(negative_cond);
                        }
                    }
                }
            }
        }

        // Phase 2: Remove only Deprecated skills (not Suspended)
        self.general_skills
            .retain(|s| s.status != SkillStatus::Deprecated);
        for skills in self.role_skills.values_mut() {
            skills.retain(|s| s.status != SkillStatus::Deprecated);
        }
        for skills in self.task_skills.values_mut() {
            skills.retain(|s| s.status != SkillStatus::Deprecated);
        }

        self.evolution_epoch += 1;

        EvolutionResult {
            epoch: self.evolution_epoch,
            skills_refined: refined,
            skills_deprecated: deprecated,
            skills_suspended: suspended,
            total_skills: self.total_skills(),
        }
    }

    /// Revive a suspended skill if new evidence suggests it's useful.
    /// Called when consensus detects emergent associations for a suspended skill.
    pub fn revive_skill(&mut self, skill_id: &str) -> bool {
        if let Some(skill) = self.get_skill_mut(skill_id) {
            if matches!(skill.status, SkillStatus::Suspended { .. }) {
                skill.status = SkillStatus::Active;
                skill.confidence = (skill.confidence + 0.1).min(0.7);
                skill.bayesian.alpha += 1.0; // positive evidence injection
                return true;
            }
        }
        false
    }

    // ── GRPO-style Skill Updates ─────────────────────────────────────────

    /// Update skill confidence based on usage outcome.
    /// Maps to SkillRL's co-evolution: skills that contribute to positive
    /// outcomes gain confidence, negative outcomes decrease it.
    pub fn update_skill_outcomes(&mut self, used_skill_ids: &[String], outcome_positive: bool) {
        let all_skills = self.all_skills_mut();
        for skill in all_skills {
            if used_skill_ids.contains(&skill.id) {
                skill.usage_count += 1;
                // Update Bayesian posterior
                skill.bayesian.update(outcome_positive);
                if outcome_positive {
                    skill.success_count += 1;
                    skill.confidence = (skill.confidence + 0.02).min(0.99);
                } else {
                    skill.failure_count += 1;
                    skill.confidence = (skill.confidence - 0.03).max(0.05);
                }
            }
        }
    }

    /// Apply causal credit deltas to used skills and adjust gating.
    /// This is the "credit-to-control" loop: skills with persistent negative credit are suspended,
    /// and consistently positive skills are strengthened or revived.
    pub fn apply_skill_credit(
        &mut self,
        used_skill_ids: &[String],
        credit_delta: f64,
        tick: u64,
    ) -> SkillCreditReport {
        if used_skill_ids.is_empty() {
            return SkillCreditReport::default();
        }

        let per_skill = credit_delta / used_skill_ids.len() as f64;
        let mut updated = 0;
        let mut suspended = 0;
        let mut revived = 0;
        let mut credit_sum = 0.0;

        let mut records: Vec<SkillCreditRecord> = Vec::new();
        let all_skills = self.all_skills_mut();
        for skill in all_skills {
            if !used_skill_ids.contains(&skill.id) {
                continue;
            }
            updated += 1;
            skill.credit_count += 1;
            skill.last_credit_tick = tick;

            // EMA update
            let alpha = 0.1;
            skill.credit_ema = (1.0 - alpha) * skill.credit_ema + alpha * per_skill;
            credit_sum += skill.credit_ema;

            // Confidence adjustment
            let conf_delta = (per_skill * 0.1).clamp(-0.05, 0.05);
            skill.confidence = (skill.confidence + conf_delta).clamp(0.01, 0.99);

            // Gating rules
            if skill.credit_count > 10 && skill.credit_ema < -0.02 {
                if !matches!(
                    skill.status,
                    SkillStatus::Suspended { .. } | SkillStatus::Deprecated
                ) {
                    skill.status = SkillStatus::Suspended {
                        suspended_at_tick: tick,
                        revival_attempts: 0,
                    };
                    suspended += 1;
                }
            }

            if skill.credit_ema > 0.05 {
                if matches!(skill.status, SkillStatus::Suspended { .. }) {
                    skill.status = SkillStatus::Active;
                    skill.confidence = (skill.confidence + 0.05).min(0.95);
                    revived += 1;
                }
            }

            records.push(SkillCreditRecord {
                tick,
                skill_id: skill.id.clone(),
                credit_delta: per_skill,
                credit_ema: skill.credit_ema,
                confidence: skill.confidence,
                status: skill.status.clone(),
            });
        }
        self.credit_history.extend(records);

        SkillCreditReport {
            updated,
            suspended,
            revived,
            mean_credit: if updated > 0 {
                credit_sum / updated as f64
            } else {
                0.0
            },
        }
    }

    pub fn drain_credit_history(&mut self) -> Vec<SkillCreditRecord> {
        let mut drained = Vec::new();
        std::mem::swap(&mut drained, &mut self.credit_history);
        drained
    }

    pub fn export_credit_history_json(
        path: &std::path::Path,
        records: &[SkillCreditRecord],
    ) -> Result<(), std::io::Error> {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        for record in records {
            let line = serde_json::to_string(record).unwrap_or_default();
            writeln!(file, "{}", line)?;
        }
        Ok(())
    }

    pub fn export_credit_history_csv(
        path: &std::path::Path,
        records: &[SkillCreditRecord],
    ) -> Result<(), std::io::Error> {
        let new_file = !path.exists();
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        if new_file {
            writeln!(
                file,
                "tick,skill_id,credit_delta,credit_ema,confidence,status"
            )?;
        }
        for record in records {
            writeln!(
                file,
                "{},{},{:.6},{:.6},{:.6},{:?}",
                record.tick,
                record.skill_id,
                record.credit_delta,
                record.credit_ema,
                record.confidence,
                record.status
            )?;
        }
        Ok(())
    }

    /// Get references to all skills across all levels
    pub fn all_skills(&self) -> Vec<&Skill> {
        let mut all = Vec::new();
        for s in &self.general_skills {
            all.push(s);
        }
        for skills in self.role_skills.values() {
            for s in skills {
                all.push(s);
            }
        }
        for skills in self.task_skills.values() {
            for s in skills {
                all.push(s);
            }
        }
        all
    }

    /// Get mutable references to all skills across all levels
    fn all_skills_mut(&mut self) -> Vec<&mut Skill> {
        let mut all = Vec::new();
        for s in &mut self.general_skills {
            all.push(s);
        }
        for skills in self.role_skills.values_mut() {
            for s in skills {
                all.push(s);
            }
        }
        for skills in self.task_skills.values_mut() {
            for s in skills {
                all.push(s);
            }
        }
        all
    }

    /// Get all skill IDs (for Prolog fact generation)
    pub fn all_skill_ids(&self) -> Vec<String> {
        let mut ids = Vec::new();
        for s in &self.general_skills {
            ids.push(s.id.clone());
        }
        for skills in self.role_skills.values() {
            for s in skills {
                ids.push(s.id.clone());
            }
        }
        for skills in self.task_skills.values() {
            for s in skills {
                ids.push(s.id.clone());
            }
        }
        ids
    }

    /// Get a skill by ID
    pub fn get_skill(&self, id: &str) -> Option<&Skill> {
        self.general_skills
            .iter()
            .find(|s| s.id == id)
            .or_else(|| self.role_skills.values().flatten().find(|s| s.id == id))
            .or_else(|| self.task_skills.values().flatten().find(|s| s.id == id))
    }

    /// Get a mutable skill by ID (for consensus verdict application)
    pub fn get_skill_mut(&mut self, id: &str) -> Option<&mut Skill> {
        // Check general skills first
        if let Some(s) = self.general_skills.iter_mut().find(|s| s.id == id) {
            return Some(s);
        }
        // Check role skills
        for skills in self.role_skills.values_mut() {
            if let Some(s) = skills.iter_mut().find(|s| s.id == id) {
                return Some(s);
            }
        }
        // Check task skills
        for skills in self.task_skills.values_mut() {
            if let Some(s) = skills.iter_mut().find(|s| s.id == id) {
                return Some(s);
            }
        }
        None
    }

    /// Get all non-deprecated, non-suspended skill IDs (for retrieval filtering)
    pub fn active_skill_ids(&self) -> Vec<String> {
        self.all_skills()
            .iter()
            .filter(|s| matches!(s.status, SkillStatus::Active | SkillStatus::Advanced))
            .map(|s| s.id.clone())
            .collect()
    }

    // ── Delegation-Aware Retrieval ───────────────────────────────────────

    /// Retrieve skills scoped for a specific delegation.
    ///
    /// Unlike `retrieve()` which broadcasts all general + top-K, this returns
    /// ONLY the 2-3 curated modules relevant to a specific subproblem at a
    /// specific depth. This IS the orchestrator's core competency.
    ///
    /// Only returns skills that are:
    /// - HumanCurated or Promoted (not Proposed or Legacy)
    /// - Active or Advanced status
    /// - Scope-matched to the subproblem domains and depth
    /// - Within max_concurrent_assignments limit
    pub fn retrieve_for_delegation(
        &self,
        subproblem_domains: &[String],
        depth: u8,
        max_skills: usize,
        context_embedding: Option<&[f32]>,
    ) -> Vec<&Skill> {
        let max_skills = max_skills.min(5); // Hard cap — never more than 5

        let mut candidates: Vec<(&Skill, f64)> = self
            .all_skills()
            .into_iter()
            .filter(|s| {
                // Status gate
                matches!(s.status, SkillStatus::Active | SkillStatus::Advanced)
            })
            .filter(|s| {
                // Curation gate — only human-curated or promoted skills
                // in active briefings. Legacy/Proposed are excluded.
                matches!(
                    s.curation,
                    SkillCuration::HumanCurated { .. } | SkillCuration::Promoted { .. }
                )
            })
            .filter(|s| {
                // Scope gate
                s.scope.matches(subproblem_domains, depth)
            })
            .map(|s| {
                // Score: combine scope relevance + embedding similarity + credit_ema
                let mut score = 0.0;

                // Domain overlap strength
                let domain_overlap = s
                    .scope
                    .domains
                    .iter()
                    .filter(|d| {
                        subproblem_domains
                            .iter()
                            .any(|sd| sd.contains(d.as_str()) || d.contains(sd.as_str()))
                    })
                    .count() as f64;
                score += domain_overlap * 0.4;

                // Embedding similarity (if available)
                if let (Some(ctx_emb), Some(skill_emb)) = (context_embedding, s.embedding.as_ref())
                {
                    score += cosine_similarity(ctx_emb, skill_emb) * 0.3;
                }

                // Credit signal — skills with positive delegation history score higher
                score += (s.credit_ema + s.delegation_ema).clamp(-0.3, 0.3);

                // Confidence boost
                score += s.confidence * 0.2;

                (s, score)
            })
            .collect();

        // Sort by score descending
        candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Return top-N
        candidates
            .into_iter()
            .take(max_skills)
            .map(|(s, _)| s)
            .collect()
    }

    /// Retrieve skills using LEGACY broadcast mode (general + top-K).
    /// This is the OLD approach — kept for backward compatibility but
    /// delegation-aware code should use `retrieve_for_delegation()` instead.
    /// Includes ALL curation statuses (Legacy, Proposed, etc.) for backward compat.
    pub fn retrieve_all_legacy(
        &self,
        role: &Role,
        context_embedding: Option<&[f32]>,
        applicable_skill_ids: &[String],
    ) -> RetrievedSkills {
        // Delegate to existing retrieve() — same behavior as before
        self.retrieve(role, context_embedding, applicable_skill_ids)
    }

    /// Check if a skill is eligible for inclusion in delegation briefings.
    /// Only human-curated or promoted skills qualify.
    pub fn is_briefing_eligible(&self, skill_id: &str) -> bool {
        self.get_skill(skill_id)
            .map(|s| {
                matches!(
                    s.curation,
                    SkillCuration::HumanCurated { .. } | SkillCuration::Promoted { .. }
                )
            })
            .unwrap_or(false)
    }

    /// Promote a Proposed skill to active pool after human review.
    pub fn promote_skill(&mut self, skill_id: &str, promoted_by: &str) -> bool {
        if let Some(skill) = self.get_skill_mut(skill_id) {
            let source_desc = match &skill.curation {
                SkillCuration::Proposed {
                    source_description, ..
                } => source_description.clone(),
                SkillCuration::Legacy => "legacy_skill".to_string(),
                _ => return false, // Already promoted or curated
            };
            skill.curation = SkillCuration::Promoted {
                original_source: source_desc,
                promoted_by: promoted_by.to_string(),
                promoted_at: now_secs(),
            };
            true
        } else {
            false
        }
    }

    /// Add a human-curated skill directly to the bank.
    /// This is the preferred path for high-value procedural knowledge.
    pub fn add_curated_skill(
        &mut self,
        title: &str,
        principle: &str,
        curator: &str,
        domain: &str,
        scope: SkillScope,
        level: SkillLevel,
    ) -> Skill {
        let now = now_secs();
        let id = format!("curated_{:03}", self.next_id);
        self.next_id += 1;

        let skill = Skill {
            id: id.clone(),
            title: title.to_string(),
            principle: principle.to_string(),
            when_to_apply: Vec::new(),
            level: level.clone(),
            source: SkillSource::Seeded, // technically human-sourced
            confidence: 0.8,             // Higher initial confidence for curated skills
            usage_count: 0,
            success_count: 0,
            failure_count: 0,
            embedding: None,
            created_at: now,
            last_evolved: now,
            status: SkillStatus::Active,
            bayesian: BayesianConfidence::new(3.0, 1.0), // Strong positive prior
            credit_ema: 0.0,
            credit_count: 0,
            last_credit_tick: 0,
            curation: SkillCuration::HumanCurated {
                curator: curator.to_string(),
                domain: domain.to_string(),
                curated_at: now,
            },
            scope,
            delegation_ema: 0.0,
            delegation_count: 0,
            hired_count: 0,
        };

        match &level {
            SkillLevel::General => self.general_skills.push(skill.clone()),
            SkillLevel::RoleSpecific(role) => {
                let key = format!("{:?}", role);
                self.role_skills.entry(key).or_default().push(skill.clone());
            }
            SkillLevel::TaskSpecific(task) => {
                self.task_skills
                    .entry(task.clone())
                    .or_default()
                    .push(skill.clone());
            }
        }

        skill
    }

    /// Count skills by curation status — for diagnostics
    pub fn curation_summary(&self) -> HashMap<String, usize> {
        let mut counts = HashMap::new();
        for skill in self.all_skills() {
            let key = match &skill.curation {
                SkillCuration::HumanCurated { .. } => "human_curated",
                SkillCuration::Proposed { .. } => "proposed",
                SkillCuration::Promoted { .. } => "promoted",
                SkillCuration::Legacy => "legacy",
            };
            *counts.entry(key.to_string()).or_default() += 1;
        }
        counts
    }
}

// ── Result Types ─────────────────────────────────────────────────────────

/// Skills retrieved for a given context — always includes general + filtered specific
#[derive(Clone, Debug)]
pub struct RetrievedSkills {
    pub general: Vec<Skill>,
    pub specific: Vec<Skill>,
}

impl RetrievedSkills {
    pub fn all_ids(&self) -> Vec<String> {
        self.general
            .iter()
            .chain(self.specific.iter())
            .map(|s| s.id.clone())
            .collect()
    }

    pub fn total(&self) -> usize {
        self.general.len() + self.specific.len()
    }
}

#[derive(Clone, Debug)]
pub struct DistillationResult {
    pub new_skills: usize,
    pub from_successes: usize,
    pub from_failures: usize,
}

#[derive(Clone, Debug)]
pub struct EvolutionResult {
    pub epoch: u64,
    pub skills_refined: usize,
    pub skills_deprecated: usize,
    pub skills_suspended: usize,
    pub total_skills: usize,
}

// ── Helpers ──────────────────────────────────────────────────────────────

fn cond(predicate: &str, args: &[ConditionArg]) -> ApplicabilityCondition {
    ApplicabilityCondition {
        predicate: predicate.to_string(),
        args: args.to_vec(),
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f64 = a
        .iter()
        .zip(b.iter())
        .map(|(x, y)| (*x as f64) * (*y as f64))
        .sum();
    let mag_a: f64 = a.iter().map(|x| (*x as f64).powi(2)).sum::<f64>().sqrt();
    let mag_b: f64 = b.iter().map(|x| (*x as f64).powi(2)).sum::<f64>().sqrt();
    if mag_a == 0.0 || mag_b == 0.0 {
        return 0.0;
    }
    dot / (mag_a * mag_b)
}

fn truncate(s: &str, max_len: usize) -> &str {
    if s.len() <= max_len {
        s
    } else {
        &s[..s
            .char_indices()
            .take(max_len)
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(0)]
    }
}

/// Find common substrings/keywords across multiple context strings
fn find_common_patterns(contexts: &[&str]) -> Vec<String> {
    if contexts.is_empty() {
        return vec![];
    }

    // Extract words from all contexts, find those appearing in >50%
    let mut word_counts: HashMap<String, usize> = HashMap::new();
    for ctx in contexts {
        let words: std::collections::HashSet<String> = ctx
            .split_whitespace()
            .map(|w| {
                w.to_lowercase()
                    .trim_matches(|c: char| !c.is_alphanumeric())
                    .to_string()
            })
            .filter(|w| w.len() > 3)
            .collect();
        for word in words {
            *word_counts.entry(word).or_default() += 1;
        }
    }

    let threshold = (contexts.len() as f64 * 0.5).ceil() as usize;
    let mut common: Vec<String> = word_counts
        .into_iter()
        .filter(|(_, count)| *count >= threshold)
        .map(|(word, _)| word)
        .collect();
    common.sort();

    // Return up to 3 common patterns
    if common.is_empty() {
        // Fallback: return first context's summary
        vec![contexts[0].chars().take(50).collect()]
    } else {
        common.truncate(3);
        common
    }
}
