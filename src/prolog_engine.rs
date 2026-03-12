//! Minimal Prolog-like inference engine over the hypergraph.
//!
//! Provides structured reasoning as System 2 complement to the LLM (System 1).
//! Handles graph topology queries, belief consistency checking, skill applicability,
//! and ontology constraint enforcement — things LLMs are poor at.
//!
//! This is NOT a full ISO Prolog implementation. It supports:
//! - Ground facts derived from world state
//! - Horn clause rules with conjunctive bodies
//! - Depth-limited backtracking search
//! - Variable unification over vertex/edge properties
//!
//! Designed for parallel execution via reasoning braids.

use std::collections::HashMap;

use crate::consensus::{
    AssociationType, EmergentAssociation, IdentityBridgeRegularizer, SkillStatus,
};
use crate::hyper_stigmergy::{ExperienceOutcome, HyperStigmergicMorphogenesis};
use crate::meta_graph::MetaGraph;
use crate::skill::{ApplicabilityCondition, ConditionArg, SkillBank};

// ── Terms ────────────────────────────────────────────────────────────────

/// A Prolog term — the fundamental data unit
#[derive(Clone, Debug, PartialEq)]
pub enum Term {
    Atom(String),
    Int(i64),
    Float(f64),
    Var(String),
    /// Compound term: functor(args...)
    Compound(String, Vec<Term>),
    List(Vec<Term>),
}

impl Term {
    pub fn atom(s: &str) -> Self {
        Term::Atom(s.to_string())
    }
    pub fn var(s: &str) -> Self {
        Term::Var(s.to_string())
    }
    pub fn int(n: i64) -> Self {
        Term::Int(n)
    }
    pub fn float(f: f64) -> Self {
        Term::Float(f)
    }

    pub fn is_var(&self) -> bool {
        matches!(self, Term::Var(_))
    }

    pub fn as_atom(&self) -> Option<&str> {
        match self {
            Term::Atom(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Term::Float(f) => Some(*f),
            Term::Int(i) => Some(*i as f64),
            _ => None,
        }
    }
}

/// A fact or goal: predicate(arg1, arg2, ...)
#[derive(Clone, Debug)]
pub struct Atom {
    pub predicate: String,
    pub args: Vec<Term>,
}

impl Atom {
    pub fn new(pred: &str, args: Vec<Term>) -> Self {
        Atom {
            predicate: pred.to_string(),
            args,
        }
    }
}

/// A Horn clause rule: head :- body1, body2, ...
#[derive(Clone, Debug)]
pub struct Rule {
    pub head: Atom,
    pub body: Vec<Atom>,
}

/// Variable bindings (substitution environment)
type Bindings = HashMap<String, Term>;

// ── Query Results ────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct QueryResult {
    pub query: String,
    pub solutions: Vec<Bindings>,
    pub succeeded: bool,
}

impl QueryResult {
    pub fn failed(query: &str) -> Self {
        Self {
            query: query.to_string(),
            solutions: vec![],
            succeeded: false,
        }
    }

    pub fn first_binding(&self, var: &str) -> Option<&Term> {
        self.solutions.first().and_then(|b| b.get(var))
    }

    pub fn all_bindings(&self, var: &str) -> Vec<&Term> {
        self.solutions.iter().filter_map(|b| b.get(var)).collect()
    }
}

// ── Prolog Engine ────────────────────────────────────────────────────────

pub struct PrologEngine {
    facts: Vec<Atom>,
    rules: Vec<Rule>,
    max_depth: usize,
}

impl PrologEngine {
    pub fn new(max_depth: usize) -> Self {
        Self {
            facts: Vec::new(),
            rules: Vec::new(),
            max_depth,
        }
    }

    /// Build engine from current world state — extracts all facts and loads rules
    pub fn from_world(world: &HyperStigmergicMorphogenesis, skill_bank: &SkillBank) -> Self {
        let mut engine = Self::new(20);
        engine.load_world_facts(world);
        engine.load_skill_facts(skill_bank);
        engine.load_builtin_rules(world);
        engine
    }

    /// Extract ground facts from world state
    fn load_world_facts(&mut self, world: &HyperStigmergicMorphogenesis) {
        // Agent facts: agent(id, role, curiosity, harmony, growth)
        for agent in &world.agents {
            let role_str = format!("{:?}", agent.role).to_lowercase();
            self.facts.push(Atom::new(
                "agent",
                vec![
                    Term::Int(agent.id as i64),
                    Term::atom(&role_str),
                    Term::float(agent.drives.curiosity),
                    Term::float(agent.drives.harmony),
                    Term::float(agent.drives.growth),
                ],
            ));

            self.facts.push(Atom::new(
                "agent_role",
                vec![Term::Int(agent.id as i64), Term::atom(&role_str)],
            ));

            // JoulWork (JW) facts: jw(id, value)
            // JW = E × η × W (Council-revised formula)
            // E = learning_rate × (1 + curiosity)  [energy expenditure]
            // η = (growth × transcendence) / (avg_growth × avg_transcendence)  [normalized efficiency]
            // W = coherence_contribution × network_amplifier  [work output]
            self.facts.push(Atom::new(
                "jw",
                vec![Term::Int(agent.id as i64), Term::float(agent.jw)],
            ));

            // Classify agents by JW value
            if agent.jw > 0.7 {
                self.facts.push(Atom::new(
                    "high_jw_agent",
                    vec![Term::Int(agent.id as i64), Term::float(agent.jw)],
                ));
            } else if agent.jw < 0.3 {
                self.facts.push(Atom::new(
                    "low_jw_agent",
                    vec![Term::Int(agent.id as i64), Term::float(agent.jw)],
                ));
            }
        }

        // Global JW facts
        let global_jw = world.global_jw();
        self.facts
            .push(Atom::new("global_jw", vec![Term::float(global_jw)]));

        // JW-based agent classification
        let high_jw_count = world.agents.iter().filter(|a| a.jw > 0.6).count();
        let low_jw_count = world.agents.iter().filter(|a| a.jw < 0.3).count();
        self.facts.push(Atom::new(
            "high_jw_count",
            vec![Term::Int(high_jw_count as i64)],
        ));
        self.facts.push(Atom::new(
            "low_jw_count",
            vec![Term::Int(low_jw_count as i64)],
        ));

        // JW health indicators
        if global_jw > 0.6 {
            self.facts.push(Atom::new("healthy_jw_economy", vec![]));
        } else if global_jw < 0.3 {
            self.facts.push(Atom::new("jw_economy_at_risk", vec![]));
        }

        // Edge facts: edge(source, target, weight, emergent)
        for edge in &world.edges {
            for i in 0..edge.participants.len() {
                for j in (i + 1)..edge.participants.len() {
                    self.facts.push(Atom::new(
                        "edge",
                        vec![
                            Term::Int(edge.participants[i] as i64),
                            Term::Int(edge.participants[j] as i64),
                            Term::float(edge.weight),
                            Term::Atom(if edge.emergent {
                                "true".into()
                            } else {
                                "false".into()
                            }),
                        ],
                    ));
                }
            }
        }

        // Vertex facts: vertex(index, kind, name)
        for (i, meta) in world.vertex_meta.iter().enumerate() {
            let kind_str = format!("{:?}", meta.kind).to_lowercase();
            self.facts.push(Atom::new(
                "vertex",
                vec![
                    Term::Int(i as i64),
                    Term::atom(&kind_str),
                    Term::atom(&meta.name),
                ],
            ));
        }

        // Belief facts: belief(id, confidence, source)
        for belief in &world.beliefs {
            let source_str = format!("{:?}", belief.source).to_lowercase();
            self.facts.push(Atom::new(
                "belief",
                vec![
                    Term::Int(belief.id as i64),
                    Term::float(belief.confidence),
                    Term::atom(&source_str),
                ],
            ));
        }

        // Ontology facts: ontology(concept, parent)
        for (concept, entry) in &world.ontology {
            for parent in &entry.parent_concepts {
                self.facts.push(Atom::new(
                    "ontology",
                    vec![Term::atom(concept), Term::atom(parent)],
                ));
            }
        }

        // System state facts
        self.facts.push(Atom::new(
            "coherence",
            vec![Term::float(world.global_coherence())],
        ));

        self.facts
            .push(Atom::new("tick", vec![Term::Int(world.tick_count as i64)]));

        self.facts.push(Atom::new(
            "edge_count",
            vec![Term::Int(world.edges.len() as i64)],
        ));

        self.facts.push(Atom::new(
            "agent_count",
            vec![Term::Int(world.agents.len() as i64)],
        ));

        let edge_density = world.edges.len() as f64 / world.agents.len().max(1) as f64;
        self.facts
            .push(Atom::new("edge_density", vec![Term::float(edge_density)]));

        // Experience summary facts
        let positive_exp = world
            .experiences
            .iter()
            .filter(|e| matches!(e.outcome, ExperienceOutcome::Positive { .. }))
            .count();
        let negative_exp = world
            .experiences
            .iter()
            .filter(|e| matches!(e.outcome, ExperienceOutcome::Negative { .. }))
            .count();

        self.facts.push(Atom::new(
            "experience_count",
            vec![Term::atom("positive"), Term::Int(positive_exp as i64)],
        ));
        self.facts.push(Atom::new(
            "experience_count",
            vec![Term::atom("negative"), Term::Int(negative_exp as i64)],
        ));
    }

    /// Load skill facts for applicability queries
    fn load_skill_facts(&mut self, bank: &SkillBank) {
        for skill in &bank.general_skills {
            self.facts.push(Atom::new(
                "skill",
                vec![
                    Term::atom(&skill.id),
                    Term::atom("general"),
                    Term::float(skill.confidence),
                ],
            ));
        }
        for (role_key, skills) in &bank.role_skills {
            for skill in skills {
                self.facts.push(Atom::new(
                    "skill",
                    vec![
                        Term::atom(&skill.id),
                        Term::atom(&role_key.to_lowercase()),
                        Term::float(skill.confidence),
                    ],
                ));
            }
        }
        for (task_key, skills) in &bank.task_skills {
            for skill in skills {
                self.facts.push(Atom::new(
                    "skill",
                    vec![
                        Term::atom(&skill.id),
                        Term::atom(&task_key.to_lowercase()),
                        Term::float(skill.confidence),
                    ],
                ));
            }
        }
    }

    /// Load builtin inference rules
    fn load_builtin_rules(&mut self, world: &HyperStigmergicMorphogenesis) {
        // connected(A, B) :- edge(A, B, W, _), W > 0.5
        // (Implemented as a builtin query, not as a rule — see evaluate_builtin)

        // Build adjacency info for cluster detection
        let mut adj: HashMap<i64, Vec<i64>> = HashMap::new();
        for edge in &world.edges {
            for i in 0..edge.participants.len() {
                for j in (i + 1)..edge.participants.len() {
                    let a = edge.participants[i] as i64;
                    let b = edge.participants[j] as i64;
                    adj.entry(a).or_default().push(b);
                    adj.entry(b).or_default().push(a);
                }
            }
        }

        // Detect disconnected clusters
        let agent_ids: Vec<i64> = world.agents.iter().map(|a| a.id as i64).collect();
        let clusters = find_connected_components(&agent_ids, &adj);

        if clusters.len() > 1 {
            self.facts
                .push(Atom::new("disconnected_clusters_exist", vec![]));
            self.facts.push(Atom::new(
                "cluster_count",
                vec![Term::Int(clusters.len() as i64)],
            ));
            for (i, cluster) in clusters.iter().enumerate() {
                for &agent_id in cluster {
                    self.facts.push(Atom::new(
                        "cluster_member",
                        vec![Term::Int(i as i64), Term::Int(agent_id)],
                    ));
                }
            }
        }

        // Detect stale edges (age > 50 ticks)
        for (i, edge) in world.edges.iter().enumerate() {
            if edge.age > 50 {
                self.facts.push(Atom::new(
                    "stale_edge",
                    vec![Term::Int(i as i64), Term::Int(edge.age as i64)],
                ));
            }
        }

        // Detect weak edges (weight < 0.5)
        let has_weak = world.edges.iter().any(|e| e.weight < 0.5);
        if has_weak {
            self.facts.push(Atom::new("weak_edges_exist", vec![]));
        }

        // Detect belief contradictions (simplified: beliefs about same topic with opposite confidence)
        if world.beliefs.len() >= 2 {
            for i in 0..world.beliefs.len() {
                for j in (i + 1)..world.beliefs.len() {
                    let b1 = &world.beliefs[i];
                    let b2 = &world.beliefs[j];
                    // Simple heuristic: if one has high and one has low confidence
                    // and they share keywords, consider them potentially contradicting
                    if (b1.confidence - b2.confidence).abs() > 0.5 {
                        if !b1.contradicting_evidence.is_empty()
                            || !b2.contradicting_evidence.is_empty()
                        {
                            self.facts.push(Atom::new(
                                "belief_contradiction",
                                vec![Term::Int(b1.id as i64), Term::Int(b2.id as i64)],
                            ));
                        }
                    }
                }
            }
            let has_contradictions = self
                .facts
                .iter()
                .any(|f| f.predicate == "belief_contradiction");
            if has_contradictions {
                self.facts
                    .push(Atom::new("belief_contradictions_exist", vec![]));
            }
        }

        // Coherence plateau detection
        let recent_deltas: Vec<f64> = world
            .improvement_history
            .iter()
            .rev()
            .take(5)
            .map(|e| (e.coherence_after - e.coherence_before).abs())
            .collect();
        if recent_deltas.len() >= 3 && recent_deltas.iter().all(|&d| d < 0.001) {
            self.facts.push(Atom::new(
                "coherence_plateau",
                vec![Term::Int(recent_deltas.len() as i64)],
            ));
            self.facts.push(Atom::new("coherence_stable", vec![]));
        }

        // Multiple clusters detection
        if clusters.len() > 1 {
            self.facts.push(Atom::new("multiple_clusters", vec![]));
        }

        // Ontology availability
        if !world.ontology.is_empty() {
            self.facts.push(Atom::new("ontology_available", vec![]));
        }

        // Consecutive no-improvement detection
        let no_improve_streak = world
            .improvement_history
            .iter()
            .rev()
            .take_while(|e| e.coherence_after <= e.coherence_before)
            .count();
        if no_improve_streak >= 3 {
            self.facts.push(Atom::new(
                "consecutive_no_improvement",
                vec![Term::Int(no_improve_streak as i64)],
            ));
        }
    }

    // ── Query Execution ──────────────────────────────────────────────────

    /// Execute a single query and return all solutions
    pub fn query(&self, goal: &Atom) -> QueryResult {
        let query_str = format!(
            "{}({})",
            goal.predicate,
            goal.args
                .iter()
                .map(|a| format!("{:?}", a))
                .collect::<Vec<_>>()
                .join(", ")
        );

        let mut solutions = Vec::new();
        let bindings = Bindings::new();
        self.solve(goal, &bindings, 0, &mut solutions);

        QueryResult {
            query: query_str,
            succeeded: !solutions.is_empty(),
            solutions,
        }
    }

    /// Execute a named query — convenience for common patterns
    pub fn named_query(&self, name: &str) -> QueryResult {
        match name {
            "belief_conflicts" => self.query(&Atom::new(
                "belief_contradiction",
                vec![Term::var("B1"), Term::var("B2")],
            )),
            "stale_edges" => self.query(&Atom::new(
                "stale_edge",
                vec![Term::var("EdgeIdx"), Term::var("Age")],
            )),
            "disconnected_clusters" => self.query(&Atom::new(
                "cluster_member",
                vec![Term::var("ClusterId"), Term::var("AgentId")],
            )),
            "coherence_plateau" => {
                self.query(&Atom::new("coherence_plateau", vec![Term::var("Length")]))
            }
            "weak_edges" => self.query(&Atom::new("weak_edges_exist", vec![])),
            "belief_contradictions" => {
                self.query(&Atom::new("belief_contradictions_exist", vec![]))
            }
            "emergent_associations" => self.query(&Atom::new(
                "emergent_association",
                vec![
                    Term::var("SkillId"),
                    Term::var("Type"),
                    Term::var("CoherenceDelta"),
                    Term::var("NoveltyScore"),
                ],
            )),
            "identity_bridges" => self.query(&Atom::new(
                "identity_bridge",
                vec![
                    Term::var("SkillId"),
                    Term::var("Title"),
                    Term::var("Principle"),
                ],
            )),
            // Federation queries
            "cross_system_bridges" => self.query(&Atom::new(
                "cross_system_bridge",
                vec![Term::var("V1"), Term::var("V2")],
            )),
            "trust_violations" => self.query(&Atom::new(
                "trust_violation",
                vec![Term::var("System"), Term::var("Score")],
            )),
            "federation_opportunities" => self.query(&Atom::new(
                "federation_opportunity",
                vec![Term::var("Vertex")],
            )),
            "remote_edges" => self.query(&Atom::new(
                "remote_edge",
                vec![
                    Term::var("EdgeId"),
                    Term::var("System"),
                    Term::var("EdgeType"),
                    Term::var("Weight"),
                    Term::var("Scope"),
                ],
            )),
            "shared_edges" => self.query(&Atom::new(
                "shared_edge",
                vec![
                    Term::var("EdgeId"),
                    Term::var("Layer"),
                    Term::var("SystemCount"),
                ],
            )),
            // JoulWork (JW) queries
            "high_jw_agents" => self.query(&Atom::new(
                "high_jw_agent",
                vec![Term::var("AgentId"), Term::var("JW")],
            )),
            "low_jw_agents" => self.query(&Atom::new(
                "low_jw_agent",
                vec![Term::var("AgentId"), Term::var("JW")],
            )),
            "jw_economy_health" => self.query(&Atom::new("global_jw", vec![Term::var("JW")])),
            "productive_agents" => {
                // Agents with JW > 0.5
                self.query(&Atom::new(
                    "jw",
                    vec![Term::var("AgentId"), Term::var("JW")],
                ))
            }
            _ => QueryResult::failed(name),
        }
    }

    /// Check if a skill's applicability conditions are met.
    /// This is the Prolog-side of the hybrid retrieval strategy.
    pub fn check_skill_applicable(&self, conditions: &[ApplicabilityCondition]) -> bool {
        if conditions.is_empty() {
            return true;
        }

        for cond in conditions {
            let goal = self.condition_to_atom(cond);
            let result = self.query(&goal);
            if !result.succeeded {
                return false;
            }
        }
        true
    }

    /// Get all applicable skill IDs given current world state
    pub fn find_applicable_skills(&self, bank: &SkillBank) -> Vec<String> {
        let mut applicable = Vec::new();

        // Check general skills
        for skill in &bank.general_skills {
            if self.check_skill_applicable(&skill.when_to_apply) {
                applicable.push(skill.id.clone());
            }
        }
        // Check role skills
        for skills in bank.role_skills.values() {
            for skill in skills {
                if self.check_skill_applicable(&skill.when_to_apply) {
                    applicable.push(skill.id.clone());
                }
            }
        }
        // Check task skills
        for skills in bank.task_skills.values() {
            for skill in skills {
                if self.check_skill_applicable(&skill.when_to_apply) {
                    applicable.push(skill.id.clone());
                }
            }
        }

        applicable
    }

    /// Load identity bridge facts from the skill bank for reversal curse mitigation.
    /// Creates bidirectional unification paths: identity_bridge(SkillId, Title, Principle).
    pub fn load_identity_bridges(&mut self, skill_bank: &SkillBank) {
        for (pred, args) in IdentityBridgeRegularizer::as_prolog_facts(skill_bank) {
            self.facts.push(Atom::new(
                &pred,
                args.into_iter().map(|a| Term::atom(&a)).collect(),
            ));
        }
    }

    /// Load federation facts from the MetaGraph into the engine.
    /// Creates remote_edge, remote_vertex, trust_score, shared_edge predicates
    /// and federation inference rules.
    pub fn load_remote_facts(&mut self, meta_graph: &MetaGraph) {
        // remote_edge(EdgeId, System, EdgeType, Weight, Scope)
        for edge in &meta_graph.shared_edges {
            let scope_str = if edge.contributing_systems.len() > 1 {
                "shared"
            } else {
                "local"
            };
            self.facts.push(Atom::new(
                "remote_edge",
                vec![
                    Term::atom(&edge.id),
                    Term::atom(&edge.provenance.origin_system),
                    Term::atom(&edge.edge_type),
                    Term::float(edge.weight),
                    Term::atom(scope_str),
                ],
            ));

            // shared_edge(EdgeId, Layer, ContributingSystemCount)
            let layer_str = format!("{:?}", edge.layer).to_lowercase();
            self.facts.push(Atom::new(
                "shared_edge",
                vec![
                    Term::atom(&edge.id),
                    Term::atom(&layer_str),
                    Term::Int(edge.contributing_systems.len() as i64),
                ],
            ));
        }

        // remote_vertex(Name, System, Kind)
        for (name, meta) in &meta_graph.shared_vertex_meta {
            self.facts.push(Atom::new(
                "remote_vertex",
                vec![
                    Term::atom(name),
                    Term::atom(&meta.origin_system),
                    Term::atom("shared"),
                ],
            ));
            for (sys_id, alias) in &meta.aliases {
                self.facts.push(Atom::new(
                    "remote_vertex",
                    vec![Term::atom(alias), Term::atom(sys_id), Term::atom("alias")],
                ));
            }
        }

        // trust_score(FromSystem, ToSystem, Score)
        for ((from, to), edge) in &meta_graph.trust_graph.edges {
            self.facts.push(Atom::new(
                "trust_score",
                vec![Term::atom(from), Term::atom(to), Term::float(edge.score)],
            ));
        }

        // known_system(SystemId)
        for sys_id in meta_graph.known_systems.keys() {
            self.facts
                .push(Atom::new("known_system", vec![Term::atom(sys_id)]));
        }

        // Federation summary facts
        self.facts.push(Atom::new(
            "shared_edge_count",
            vec![Term::Int(meta_graph.shared_edges.len() as i64)],
        ));
        self.facts.push(Atom::new(
            "known_system_count",
            vec![Term::Int(meta_graph.known_systems.len() as i64)],
        ));

        // Load federation rules
        self.load_federation_rules(meta_graph);
    }

    /// Load federation-specific inference rules.
    fn load_federation_rules(&mut self, meta_graph: &MetaGraph) {
        // Precompute cross_system_bridge facts:
        // cross_system_bridge(V1, V2) when a shared edge connects V1 and V2 from different systems
        for edge in &meta_graph.shared_edges {
            if edge.contributing_systems.len() >= 2 && edge.vertices.len() >= 2 {
                self.facts.push(Atom::new(
                    "cross_system_bridge",
                    vec![Term::atom(&edge.vertices[0]), Term::atom(&edge.vertices[1])],
                ));
            }
        }

        // trust_violation(System) — systems below trust threshold
        for ((from, to), edge) in &meta_graph.trust_graph.edges {
            if edge.score < meta_graph.trust_graph.default_trust {
                self.facts.push(Atom::new(
                    "trust_violation",
                    vec![Term::atom(to), Term::float(edge.score)],
                ));
                let _ = from; // used in the key
            }
        }

        // federation_opportunity: local vertices that have remote counterparts
        // but no shared edges yet
        let shared_vertex_names: std::collections::HashSet<&str> = meta_graph
            .shared_vertex_meta
            .keys()
            .map(|s| s.as_str())
            .collect();
        // Mark vertices that exist in the shared meta but have few edges
        for name in &shared_vertex_names {
            let has_shared_edge = meta_graph
                .shared_edges
                .iter()
                .any(|e| e.vertices.iter().any(|v| v == *name));
            if !has_shared_edge {
                self.facts
                    .push(Atom::new("federation_opportunity", vec![Term::atom(name)]));
            }
        }
    }

    /// Load emergent association facts into the engine for querying.
    /// Called after association detection so braids can query them.
    pub fn load_association_facts(&mut self, associations: &[EmergentAssociation]) {
        for assoc in associations {
            // emergent_association(SkillId, Type, CoherenceDelta, NoveltyScore)
            let type_str = match &assoc.association_type {
                AssociationType::BridgeFormation { .. } => "bridge_formation",
                AssociationType::BeliefResolution { .. } => "belief_resolution",
                AssociationType::RiskMitigation { .. } => "risk_mitigation",
                AssociationType::ClusterEmergence { .. } => "cluster_emergence",
                AssociationType::CrossDomainTransfer { .. } => "cross_domain_transfer",
                AssociationType::IdentityBridge { .. } => "identity_bridge",
                AssociationType::CrossSystemConsensus { .. } => "cross_system_consensus",
                AssociationType::CrossSystemSynthesis { .. } => "cross_system_synthesis",
                AssociationType::FederatedCluster { .. } => "federated_cluster",
            };
            self.facts.push(Atom::new(
                "emergent_association",
                vec![
                    Term::atom(&assoc.skill_id),
                    Term::atom(type_str),
                    Term::float(assoc.coherence_delta),
                    Term::float(assoc.novelty_score),
                ],
            ));

            // Per-vertex involvement: association_vertex(SkillId, VertexId)
            for &v in &assoc.vertices_involved {
                self.facts.push(Atom::new(
                    "association_vertex",
                    vec![Term::atom(&assoc.skill_id), Term::Int(v as i64)],
                ));
            }
        }
    }

    /// Detect emergent associations from the current world state.
    /// Analyzes graph topology, beliefs, and edges to find new structures
    /// that arose from skill applications.
    pub fn detect_emergent_associations(
        &self,
        world: &HyperStigmergicMorphogenesis,
        skill_bank: &SkillBank,
    ) -> Vec<EmergentAssociation> {
        let mut associations = Vec::new();
        let tick = world.tick_count;

        // 1. Detect bridge formations: edges connecting previously disconnected clusters
        let cluster_members = self.query(&Atom::new(
            "cluster_member",
            vec![Term::var("ClusterId"), Term::var("AgentId")],
        ));
        if cluster_members.succeeded {
            // Group by cluster
            let mut clusters: HashMap<String, Vec<i64>> = HashMap::new();
            for sol in &cluster_members.solutions {
                if let (Some(cid), Some(aid)) = (sol.get("ClusterId"), sol.get("AgentId")) {
                    let cid_str = format!("{:?}", cid);
                    if let Term::Int(id) = aid {
                        clusters.entry(cid_str).or_default().push(*id);
                    }
                }
            }

            // Check for cross-cluster edges (potential bridges from recent skills)
            let cluster_list: Vec<(String, Vec<i64>)> = clusters.into_iter().collect();
            for i in 0..cluster_list.len() {
                for j in (i + 1)..cluster_list.len() {
                    // Check if any recent edge connects these clusters
                    for edge in &world.edges {
                        if edge.age < 5 {
                            // recent edge
                            let in_i = edge
                                .participants
                                .iter()
                                .any(|p| cluster_list[i].1.contains(&(*p as i64)));
                            let in_j = edge
                                .participants
                                .iter()
                                .any(|p| cluster_list[j].1.contains(&(*p as i64)));
                            if in_i && in_j {
                                // Find which skill might have caused this
                                let skill_id = self.recent_skill_for_edge(world, skill_bank);
                                associations.push(EmergentAssociation {
                                    skill_id,
                                    association_type: AssociationType::BridgeFormation {
                                        from_cluster: i,
                                        to_cluster: j,
                                    },
                                    vertices_involved: edge.participants.clone(),
                                    coherence_delta: edge.weight * 0.1,
                                    novelty_score: if edge.emergent { 0.8 } else { 0.4 },
                                    detected_at_tick: tick,
                                });
                            }
                        }
                    }
                }
            }
        }

        // 2. Detect belief resolutions
        let contradictions = self.query(&Atom::new(
            "belief_contradiction",
            vec![Term::var("B1"), Term::var("B2")],
        ));
        if contradictions.succeeded && contradictions.solutions.len() < 3 {
            // Fewer contradictions than before = potential resolution
            for sol in &contradictions.solutions {
                if let (Some(Term::Int(b1)), Some(Term::Int(b2))) = (sol.get("B1"), sol.get("B2")) {
                    let skill_id = self.recent_skill_for_belief(world, skill_bank);
                    associations.push(EmergentAssociation {
                        skill_id,
                        association_type: AssociationType::BeliefResolution {
                            belief_ids: vec![*b1 as usize, *b2 as usize],
                        },
                        vertices_involved: vec![],
                        coherence_delta: 0.05,
                        novelty_score: 0.6,
                        detected_at_tick: tick,
                    });
                }
            }
        }

        // 3. Detect risk mitigations (stale edges being cleaned up)
        let stale = self.query(&Atom::new(
            "stale_edge",
            vec![Term::var("Idx"), Term::var("Age")],
        ));
        if !stale.succeeded || stale.solutions.is_empty() {
            // No stale edges = risk mitigation occurred
            let skill_id = self.recent_skill_for_edge(world, skill_bank);
            if world.tick_count > 5 {
                associations.push(EmergentAssociation {
                    skill_id,
                    association_type: AssociationType::RiskMitigation {
                        risk_type: "stale_edge_cleanup".to_string(),
                    },
                    vertices_involved: vec![],
                    coherence_delta: 0.02,
                    novelty_score: 0.3,
                    detected_at_tick: tick,
                });
            }
        }

        // 4. Add identity bridges from regularizer
        let identity_bridges = IdentityBridgeRegularizer::generate_bridges(skill_bank);
        associations.extend(identity_bridges);

        associations
    }

    /// Heuristic: find the most recently used skill likely responsible for edge changes
    fn recent_skill_for_edge(
        &self,
        _world: &HyperStigmergicMorphogenesis,
        skill_bank: &SkillBank,
    ) -> String {
        // Return the most recently used active skill
        skill_bank
            .all_skills()
            .into_iter()
            .filter(|s| matches!(s.status, SkillStatus::Active | SkillStatus::Advanced))
            .max_by_key(|s| s.usage_count)
            .map(|s| s.id.clone())
            .unwrap_or_else(|| "unknown".to_string())
    }

    /// Heuristic: find the most recently used skill likely responsible for belief changes
    fn recent_skill_for_belief(
        &self,
        _world: &HyperStigmergicMorphogenesis,
        skill_bank: &SkillBank,
    ) -> String {
        skill_bank
            .all_skills()
            .into_iter()
            .filter(|s| s.principle.contains("belief") || s.principle.contains("conflict"))
            .filter(|s| matches!(s.status, SkillStatus::Active | SkillStatus::Advanced))
            .max_by_key(|s| s.usage_count)
            .map(|s| s.id.clone())
            .unwrap_or_else(|| "unknown".to_string())
    }

    /// Convert an ApplicabilityCondition to a Prolog Atom for querying
    fn condition_to_atom(&self, cond: &ApplicabilityCondition) -> Atom {
        let args: Vec<Term> = cond
            .args
            .iter()
            .map(|arg| match arg {
                ConditionArg::Float(f) => Term::Float(*f),
                ConditionArg::Int(i) => Term::Int(*i),
                ConditionArg::Str(s) => Term::Atom(s.clone()),
                ConditionArg::Role(r) => Term::Atom(format!("{:?}", r).to_lowercase()),
                ConditionArg::Var(v) => Term::Var(v.clone()),
            })
            .collect();

        Atom::new(&cond.predicate, args)
    }

    // ── Unification & Resolution ─────────────────────────────────────────

    /// Attempt to unify two terms under given bindings
    fn unify(&self, t1: &Term, t2: &Term, bindings: &Bindings) -> Option<Bindings> {
        let t1 = self.resolve(t1, bindings);
        let t2 = self.resolve(t2, bindings);

        match (&t1, &t2) {
            (Term::Var(v), _) => {
                let mut new_bindings = bindings.clone();
                new_bindings.insert(v.clone(), t2);
                Some(new_bindings)
            }
            (_, Term::Var(v)) => {
                let mut new_bindings = bindings.clone();
                new_bindings.insert(v.clone(), t1);
                Some(new_bindings)
            }
            (Term::Atom(a), Term::Atom(b)) if a == b => Some(bindings.clone()),
            (Term::Int(a), Term::Int(b)) if a == b => Some(bindings.clone()),
            (Term::Float(a), Term::Float(b)) if (a - b).abs() < 1e-9 => Some(bindings.clone()),
            // Int/Float cross-unification
            (Term::Int(a), Term::Float(b)) if (*a as f64 - b).abs() < 1e-9 => {
                Some(bindings.clone())
            }
            (Term::Float(a), Term::Int(b)) if (a - *b as f64).abs() < 1e-9 => {
                Some(bindings.clone())
            }
            (Term::Compound(f1, args1), Term::Compound(f2, args2))
                if f1 == f2 && args1.len() == args2.len() =>
            {
                let mut current = bindings.clone();
                for (a1, a2) in args1.iter().zip(args2.iter()) {
                    match self.unify(a1, a2, &current) {
                        Some(new) => current = new,
                        None => return None,
                    }
                }
                Some(current)
            }
            _ => None,
        }
    }

    /// Resolve a term by following variable bindings
    fn resolve(&self, term: &Term, bindings: &Bindings) -> Term {
        match term {
            Term::Var(v) => {
                if let Some(bound) = bindings.get(v) {
                    self.resolve(bound, bindings)
                } else {
                    term.clone()
                }
            }
            _ => term.clone(),
        }
    }

    /// Unify two atoms (predicate + args)
    fn unify_atoms(&self, a1: &Atom, a2: &Atom, bindings: &Bindings) -> Option<Bindings> {
        if a1.predicate != a2.predicate || a1.args.len() != a2.args.len() {
            return None;
        }
        let mut current = bindings.clone();
        for (t1, t2) in a1.args.iter().zip(a2.args.iter()) {
            match self.unify(t1, t2, &current) {
                Some(new) => current = new,
                None => return None,
            }
        }
        Some(current)
    }

    /// SLD resolution: solve a goal against facts and rules
    fn solve(&self, goal: &Atom, bindings: &Bindings, depth: usize, solutions: &mut Vec<Bindings>) {
        if depth >= self.max_depth {
            return;
        }

        // Try builtin predicates first
        if let Some(result) = self.evaluate_builtin(goal, bindings) {
            if result {
                solutions.push(bindings.clone());
            }
            return;
        }

        // Try unifying with facts
        for fact in &self.facts {
            if let Some(new_bindings) = self.unify_atoms(goal, fact, bindings) {
                solutions.push(new_bindings);
            }
        }

        // Try rules
        for rule in &self.rules {
            // Freshen rule variables to avoid capture
            let fresh_rule = self.freshen_rule(rule, depth);
            if let Some(new_bindings) = self.unify_atoms(goal, &fresh_rule.head, bindings) {
                // Solve all body goals conjunctively
                self.solve_conjunction(&fresh_rule.body, &new_bindings, depth + 1, solutions);
            }
        }
    }

    /// Solve a conjunction of goals
    fn solve_conjunction(
        &self,
        goals: &[Atom],
        bindings: &Bindings,
        depth: usize,
        solutions: &mut Vec<Bindings>,
    ) {
        if goals.is_empty() {
            solutions.push(bindings.clone());
            return;
        }

        let mut first_solutions = Vec::new();
        self.solve(&goals[0], bindings, depth, &mut first_solutions);

        for sol in first_solutions {
            self.solve_conjunction(&goals[1..], &sol, depth, solutions);
        }
    }

    /// Evaluate builtin predicates (comparison, arithmetic, etc.)
    fn evaluate_builtin(&self, goal: &Atom, bindings: &Bindings) -> Option<bool> {
        match goal.predicate.as_str() {
            // coherence_above(Threshold) — check if global coherence > threshold
            "coherence_above" if goal.args.len() == 1 => {
                let threshold = self.resolve(&goal.args[0], bindings).as_f64()?;
                let coherence = self
                    .facts
                    .iter()
                    .find(|f| f.predicate == "coherence")
                    .and_then(|f| f.args.first()?.as_f64())?;
                Some(coherence > threshold)
            }

            // novelty_above(Threshold)
            "novelty_above" if goal.args.len() == 1 => {
                // This is a dynamic check — novelty is computed per-action, not stored
                // Always true for now; actual novelty checking happens at action time
                Some(true)
            }

            // edge_density_range(Min, Max)
            "edge_density_range" if goal.args.len() == 2 => {
                let min = self.resolve(&goal.args[0], bindings).as_f64()?;
                let max = self.resolve(&goal.args[1], bindings).as_f64()?;
                let density = self
                    .facts
                    .iter()
                    .find(|f| f.predicate == "edge_density")
                    .and_then(|f| f.args.first()?.as_f64())?;
                Some(density >= min && density <= max)
            }

            // role_is(Role) — check against current context role
            "role_is" if goal.args.len() == 1 => {
                // This needs the current agent role in context
                // For skill applicability, we just check if ANY agent has this role
                let target_role = self.resolve(&goal.args[0], bindings).as_atom()?.to_string();
                let has_role = self.facts.iter().any(|f| {
                    f.predicate == "agent_role"
                        && f.args.get(1).and_then(|a| a.as_atom()) == Some(&target_role)
                });
                Some(has_role)
            }

            // experience_count_above(N) — check if total experiences > N
            "experience_count_above" if goal.args.len() == 1 => {
                let threshold = match self.resolve(&goal.args[0], bindings) {
                    Term::Int(n) => n,
                    _ => return None,
                };
                let total: i64 = self
                    .facts
                    .iter()
                    .filter(|f| f.predicate == "experience_count")
                    .filter_map(|f| {
                        f.args
                            .get(1)
                            .and_then(|a| if let Term::Int(n) = a { Some(*n) } else { None })
                    })
                    .sum();
                Some(total > threshold)
            }

            // action_is_destructive — marker for destructive action gating
            "action_is_destructive" if goal.args.is_empty() => {
                // This is context-dependent — checked at action proposal time
                Some(false) // Default: not destructive
            }

            // context_contains(Pattern) — check if current context contains substring
            "context_contains" if goal.args.len() == 1 => {
                // This is a dynamic check against the current intent/context
                // Always true in Prolog evaluation — actual filtering at retrieval time
                Some(true)
            }

            // mutation_type_is(Type) — check current mutation type
            "mutation_type_is" if goal.args.len() == 1 => {
                Some(true) // Dynamic, checked at action time
            }

            // context_not_contains(Pattern) — negative applicability condition
            // Used by hard negative mining to prevent skill application in wrong contexts
            "context_not_contains" if goal.args.len() == 1 => {
                // Dynamic — evaluated at retrieval time against actual context
                Some(true)
            }

            // llm_condition(Condition) — parsed from LLM output, always soft-true
            "llm_condition" => Some(true),

            // ── Federation builtins ──

            // trust_above(System, Threshold) — check if trust to a system exceeds threshold
            "trust_above" if goal.args.len() == 2 => {
                let _system = self.resolve(&goal.args[0], bindings);
                let threshold = self.resolve(&goal.args[1], bindings).as_f64()?;
                // Check if any trust_score fact exceeds threshold
                let has_trust = self.facts.iter().any(|f| {
                    f.predicate == "trust_score"
                        && f.args
                            .get(2)
                            .and_then(|a| a.as_f64())
                            .map(|s| s >= threshold)
                            .unwrap_or(false)
                });
                Some(has_trust)
            }

            // is_remote_edge(EdgeId) — check if an edge originated from a remote system
            "is_remote_edge" if goal.args.len() == 1 => {
                let edge_id = self.resolve(&goal.args[0], bindings);
                let is_remote = self
                    .facts
                    .iter()
                    .any(|f| f.predicate == "remote_edge" && f.args.first() == Some(&edge_id));
                Some(is_remote)
            }

            // has_federation_opportunity — check if any federation opportunities exist
            "has_federation_opportunity" if goal.args.is_empty() => {
                let has_opp = self
                    .facts
                    .iter()
                    .any(|f| f.predicate == "federation_opportunity");
                Some(has_opp)
            }

            _ => None, // Not a builtin — fall through to fact/rule matching
        }
    }

    /// Create fresh variable names to avoid capture in rule application
    fn freshen_rule(&self, rule: &Rule, depth: usize) -> Rule {
        let suffix = format!("_{}", depth);
        Rule {
            head: self.freshen_atom(&rule.head, &suffix),
            body: rule
                .body
                .iter()
                .map(|a| self.freshen_atom(a, &suffix))
                .collect(),
        }
    }

    fn freshen_atom(&self, atom: &Atom, suffix: &str) -> Atom {
        Atom {
            predicate: atom.predicate.clone(),
            args: atom
                .args
                .iter()
                .map(|t| self.freshen_term(t, suffix))
                .collect(),
        }
    }

    fn freshen_term(&self, term: &Term, suffix: &str) -> Term {
        match term {
            Term::Var(v) => Term::Var(format!("{}{}", v, suffix)),
            Term::Compound(f, args) => Term::Compound(
                f.clone(),
                args.iter().map(|a| self.freshen_term(a, suffix)).collect(),
            ),
            Term::List(items) => {
                Term::List(items.iter().map(|i| self.freshen_term(i, suffix)).collect())
            }
            other => other.clone(),
        }
    }

    // ── Convenience ──────────────────────────────────────────────────────

    pub fn fact_count(&self) -> usize {
        self.facts.len()
    }
    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }

    /// Add a custom rule
    pub fn add_rule(&mut self, rule: Rule) {
        self.rules.push(rule);
    }

    /// Add a custom fact
    pub fn add_fact(&mut self, fact: Atom) {
        self.facts.push(fact);
    }
}

// ── Graph Algorithms ─────────────────────────────────────────────────────

/// Find connected components in an adjacency list
fn find_connected_components(nodes: &[i64], adj: &HashMap<i64, Vec<i64>>) -> Vec<Vec<i64>> {
    let mut visited: HashMap<i64, bool> = nodes.iter().map(|&n| (n, false)).collect();
    let mut components = Vec::new();

    for &node in nodes {
        if !visited[&node] {
            let mut component = Vec::new();
            let mut stack = vec![node];
            while let Some(current) = stack.pop() {
                if visited.get(&current) == Some(&true) {
                    continue;
                }
                visited.insert(current, true);
                component.push(current);
                if let Some(neighbors) = adj.get(&current) {
                    for &neighbor in neighbors {
                        if visited.get(&neighbor) == Some(&false) {
                            stack.push(neighbor);
                        }
                    }
                }
            }
            if !component.is_empty() {
                components.push(component);
            }
        }
    }

    components
}
