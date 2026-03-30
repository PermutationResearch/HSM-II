use std::collections::{HashMap, HashSet};
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use nalgebra::DMatrix;
use rand::Rng;
use rand::SeedableRng;
use rand_distr::{Distribution, Normal};
use serde::{Deserialize, Serialize};

use crate::action::Action;
use crate::agent::{Agent, AgentId, Drives, Role};
use crate::council::{
    CouncilEvidence, CouncilEvidenceKind, CouncilGraphQuery, Proposal, StigmergicCouncilContext,
};
use crate::embedded_graph_store::{
    EmbeddedGraphStore, EMBEDDED_GRAPH_STORE_FILE, LEGACY_EMBEDDING_INDEX_FILE,
    LEGACY_WORLD_STATE_FILE,
};
use crate::embedding_index::InMemoryEmbeddingIndex;
use crate::federation::types::{EdgeScope, FederationConfig, KnowledgeLayer, Provenance, SystemId};
use crate::graph_runtime::{GraphActionResult, GraphRuntime};
use crate::hypergraph::Hypergraph;
use crate::property_graph::{PropertyGraphSnapshot, PropertyValue};
use crate::query_engine::{CypherEngine, QueryResultSet};
use crate::rlm::{BidConfig, Context, RlmAction};
use crate::social_memory::{DataSensitivity, DelegationCandidate, PromiseStatus, SocialMemory};
use crate::stigmergic_policy::{
    PolicyShift, RoutingDirective, StigmergicMemory, StigmergicTrace, TraceKind,
};

const EMBEDDING_DIM: usize = 768;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum VertexKind {
    Agent,
    Tool,
    Memory,
    Task,
    Property,
    Ontology,
    Belief,
    Experience,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VertexMeta {
    pub kind: VertexKind,
    pub name: String,
    pub created_at: u64,
    pub modified_at: u64,
    pub drift_count: u64,
    pub embedding: Option<Vec<f32>>,
    /// Federation: which system originally created this vertex.
    #[serde(default)]
    pub origin_system: Option<SystemId>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HyperEdge {
    pub participants: Vec<AgentId>,
    pub weight: f64,
    pub emergent: bool,
    pub age: u64,
    pub tags: HashMap<String, String>,
    pub created_at: u64,
    pub embedding: Option<Vec<f32>>,
    /// Which agent created this edge (None for emergent/system-created edges).
    /// Used for collective contribution tracking: when another agent's edge
    /// shares participants with this one, the creator gets trace-reuse credit.
    #[serde(default)]
    pub creator: Option<AgentId>,
    /// Federation: visibility scope of this edge.
    #[serde(default)]
    pub scope: Option<EdgeScope>,
    /// Federation: provenance chain tracking origin and hops.
    #[serde(default)]
    pub provenance: Option<Provenance>,
    /// Federation: trust tags from the originating system.
    #[serde(default)]
    pub trust_tags: Option<Vec<String>>,
    /// Federation: which system created this edge.
    #[serde(default)]
    pub origin_system: Option<SystemId>,
    /// Federation: knowledge abstraction layer.
    #[serde(default)]
    pub knowledge_layer: Option<KnowledgeLayer>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OntologyEntry {
    pub concept: String,
    pub instances: Vec<String>,
    pub confidence: f32,
    pub parent_concepts: Vec<String>,
    pub created_epoch: u64,
    pub last_modified: u64,
    pub embedding: Option<Vec<f32>>,
}

/// A Belief vertex — an opinion the system holds with confidence that evolves
/// (Hindsight pattern: evolving beliefs with confidence scores)
///
/// OpenViking-inspired L0/L1/L2 tiered context:
///   L0 (abstract_l0): ~50 token summary for quick relevance filtering
///   L1 (overview_l1): ~500 token overview for navigation/reranking
///   L2 (content):      full belief text for detailed processing
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Belief {
    pub id: usize,
    pub content: String,
    /// L0 abstract: single-sentence summary (~50 tokens) for rapid filtering
    #[serde(default)]
    pub abstract_l0: Option<String>,
    /// L1 overview: structured summary (~500 tokens) for navigation
    #[serde(default)]
    pub overview_l1: Option<String>,
    pub confidence: f64,
    pub source: BeliefSource,
    pub supporting_evidence: Vec<String>,
    pub contradicting_evidence: Vec<String>,
    pub created_at: u64,
    pub updated_at: u64,
    pub update_count: u32,
    /// Owning namespace / org slice for write-policy (`None` = global default).
    #[serde(default)]
    pub owner_namespace: Option<String>,
    /// Prior belief this one replaces (no silent merge).
    #[serde(default)]
    pub supersedes_belief_id: Option<usize>,
    /// Linked belief IDs cited as evidence (provenance).
    #[serde(default)]
    pub evidence_belief_ids: Vec<usize>,
    /// Human attestation bypasses automated explainability cap for high confidence.
    #[serde(default)]
    pub human_committed: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AddBeliefExtras {
    #[serde(default)]
    pub owner_namespace: Option<String>,
    #[serde(default)]
    pub supersedes_belief_id: Option<usize>,
    #[serde(default)]
    pub evidence_belief_ids: Vec<usize>,
    #[serde(default)]
    pub human_committed: bool,
    /// Seed supporting strings (provenance / held-out evaluation hooks).
    #[serde(default)]
    pub supporting_evidence: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum BeliefSource {
    Observation,  // Derived from direct system observation
    Reflection,   // Generated by reflect() via Ollama
    Inference,    // Inferred from other beliefs
    UserProvided, // From chat interactions
    Prediction,   // MiroFish-inspired: derived from scenario simulation
}

/// An Experience — a timestamped record of what happened and the outcome
/// (Hindsight pattern: experiences separate from world facts)
///
/// OpenViking-inspired L0/L1/L2 tiered context:
///   L0 (abstract_l0): ~50 token summary for quick relevance filtering
///   L1 (overview_l1): ~500 token overview for navigation/reranking
///   L2 (description):  full experience text for detailed processing
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Experience {
    pub id: usize,
    pub description: String,
    pub context: String,
    /// L0 abstract: single-sentence summary (~50 tokens) for rapid filtering
    #[serde(default)]
    pub abstract_l0: Option<String>,
    /// L1 overview: structured summary (~500 tokens) for navigation
    #[serde(default)]
    pub overview_l1: Option<String>,
    pub outcome: ExperienceOutcome,
    pub timestamp: u64,
    pub tick: u64,
    pub embedding: Option<Vec<f32>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ExperienceOutcome {
    Positive { coherence_delta: f64 },
    Negative { coherence_delta: f64 },
    Neutral,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum CompositeFactKind {
    Fact,
    Event,
    Promise,
    Delegation,
    Narrative,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TemporalSemantics {
    pub discovered_at: u64,
    pub created_at: u64,
    pub updated_at: u64,
    #[serde(default)]
    pub valid_from: Option<u64>,
    #[serde(default)]
    pub valid_until: Option<u64>,
    #[serde(default)]
    pub occurred_at: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct FactTemplate {
    pub id: String,
    pub label: String,
    pub narrative: String,
    pub slot_names: Vec<String>,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct FactSlotBinding {
    pub role: String,
    pub value: String,
    #[serde(default)]
    pub entity_ref: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct CompositeFact {
    pub id: String,
    pub kind: CompositeFactKind,
    pub label: String,
    pub details: String,
    #[serde(default)]
    pub template_id: Option<String>,
    pub slots: Vec<FactSlotBinding>,
    pub temporal: TemporalSemantics,
    pub confidence: f64,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub external_ref: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum RecursiveRelationKind {
    Contains,
    Clarifies,
    Extends,
    Supports,
    Contradicts,
    Causes,
    DependsOn,
    Fulfills,
    Violates,
    DerivedFrom,
}

impl RecursiveRelationKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Contains => "CONTAINS",
            Self::Clarifies => "CLARIFIES",
            Self::Extends => "EXTENDS",
            Self::Supports => "SUPPORTS",
            Self::Contradicts => "CONTRADICTS",
            Self::Causes => "CAUSES",
            Self::DependsOn => "DEPENDS_ON",
            Self::Fulfills => "FULFILLS",
            Self::Violates => "VIOLATES",
            Self::DerivedFrom => "DERIVED_FROM",
        }
    }

    pub fn from_str(value: &str) -> Self {
        match value {
            "CONTAINS" => Self::Contains,
            "CLARIFIES" => Self::Clarifies,
            "EXTENDS" => Self::Extends,
            "SUPPORTS" => Self::Supports,
            "CONTRADICTS" => Self::Contradicts,
            "CAUSES" => Self::Causes,
            "DEPENDS_ON" => Self::DependsOn,
            "FULFILLS" => Self::Fulfills,
            "VIOLATES" => Self::Violates,
            _ => Self::DerivedFrom,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct RecursiveFactRelation {
    pub id: String,
    pub from_fact_id: String,
    pub to_fact_id: String,
    pub kind: RecursiveRelationKind,
    pub confidence: f64,
    pub rationale: String,
    pub created_at: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum DelegationStatus {
    Proposed,
    Accepted,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct DelegationFrame {
    pub id: String,
    pub task_key: String,
    pub requester: Option<AgentId>,
    pub delegated_to: AgentId,
    pub rationale: String,
    pub confidence: f64,
    #[serde(default)]
    pub promise_id: Option<String>,
    pub status: DelegationStatus,
    pub created_at: u64,
    pub updated_at: u64,
    #[serde(default)]
    pub outcome_fact_id: Option<String>,
}

/// Pareto candidate for multi-objective optimization (GEPA pattern)
#[derive(Clone, Debug)]
pub struct ParetoCandidate {
    pub role: crate::agent::Role,
    pub coherence_score: f64,
    pub novelty_score: f64,
    pub cost_score: f64,
    pub dominated: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AdaptiveConfig {
    pub target_edge_density: f64,
    pub min_decay_rate: f64,
    pub max_decay_rate: f64,
    pub density_window: usize,
    pub history: Vec<f64>,
}

impl Default for AdaptiveConfig {
    fn default() -> Self {
        Self {
            target_edge_density: 2.5,
            min_decay_rate: 0.005,
            max_decay_rate: 0.05,
            density_window: 10,
            history: Vec::with_capacity(10),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DriftConfig {
    pub interval: u64,
    pub alpha: f32,
    pub noise_scale: f32,
    pub semantic_trigger: f32,
}

impl Default for DriftConfig {
    fn default() -> Self {
        Self {
            interval: 50,
            alpha: 0.15,
            noise_scale: 0.02,
            semantic_trigger: 0.85,
        }
    }
}

/// Persistence wrapper for complete system state
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SystemState {
    pub morphogenesis: HyperStigmergicMorphogenesis,
    pub rlm_state: Option<crate::rlm::RLMState>,
    pub saved_at: u64,
    pub version: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HyperStigmergicMorphogenesis {
    pub agents: Vec<Agent>,
    #[serde(default)]
    pub agent_action_stats: HashMap<AgentId, AgentActionStats>,
    pub adjacency: HashMap<AgentId, Vec<usize>>,
    pub edges: Vec<HyperEdge>,
    pub decay_rate: f64,
    pub tick_count: u64,
    pub prev_coherence: f64,
    pub hypergraph: Hypergraph,
    pub vertex_state: DMatrix<f32>,
    pub vertex_meta: Vec<VertexMeta>,
    pub num_agents_vertices: usize,
    pub num_tools_vertices: usize,
    pub num_memory_vertices: usize,
    pub num_task_vertices: usize,
    pub ontology: HashMap<String, OntologyEntry>,
    pub dynamic_concepts: Vec<String>,
    pub adaptive_config: AdaptiveConfig,
    pub drift_config: DriftConfig,
    pub adaptive_threshold: bool,
    pub property_vertices: HashMap<String, usize>,
    pub stigmergic_clusters: HashMap<Vec<AgentId>, (f64, u64)>,

    // Embedding infrastructure (skipped in serialization, rebuilt on load)
    #[serde(skip)]
    pub deep_memory: HashMap<usize, DMatrix<f32>>,
    #[serde(skip)]
    pub embedding_cache: HashMap<String, DMatrix<f32>>,
    #[serde(skip)]
    pub embedding_index: InMemoryEmbeddingIndex,

    /// Supervision-loop ledger: each entry is one bounded propose → review → record pass
    /// (`execute_self_improvement_cycle`); later steps are conditioned on this history.
    pub improvement_history: Vec<ImprovementEvent>,
    pub current_intent: Option<String>,
    /// Avoid-hints from the last reflection cycle; used to penalize matching mutation types
    #[serde(default)]
    pub avoid_hints: Vec<String>,

    // Hindsight-style memory networks
    pub beliefs: Vec<Belief>,
    pub experiences: Vec<Experience>,
    #[serde(default)]
    pub social_memory: SocialMemory,
    #[serde(default)]
    pub stigmergic_memory: StigmergicMemory,
    #[serde(default)]
    pub fact_templates: Vec<FactTemplate>,
    #[serde(default)]
    pub composite_facts: Vec<CompositeFact>,
    #[serde(default)]
    pub recursive_fact_relations: Vec<RecursiveFactRelation>,
    #[serde(default)]
    pub delegation_frames: Vec<DelegationFrame>,
    #[serde(default)]
    pub next_fact_template_id: u64,
    #[serde(default)]
    pub next_composite_fact_id: u64,
    #[serde(default)]
    pub next_fact_relation_id: u64,
    #[serde(default)]
    pub next_delegation_frame_id: u64,
    pub reflection_count: u64,
    pub last_reflection_tick: u64,
    /// Last tick when beliefs were re-evaluated against recent experiences
    #[serde(default)]
    pub last_belief_reeval_tick: u64,

    // SkillRL: hierarchical skill bank for recursive skill-augmented RL
    #[serde(default = "crate::skill::SkillBank::new_with_seeds")]
    pub skill_bank: crate::skill::SkillBank,

    /// Federation: configuration for distributed hypergraph federation.
    #[serde(default)]
    pub federation_config: Option<FederationConfig>,

    /// Append-only decision audit (cap applied on push). Skipped in bincode world snapshots for backward compatibility; export via JSON/`SystemState` when needed.
    #[serde(skip)]
    pub decision_log: Vec<DecisionRecord>,
    /// Lamport-style generation for contested writes (beliefs / merges). In-memory until a versioned migration exists for raw bincode worlds.
    #[serde(skip)]
    pub world_state_generation: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ImprovementEvent {
    pub timestamp: u64,
    pub intent: String,
    pub mutation_type: MutationType,
    pub coherence_before: f64,
    pub coherence_after: f64,
    pub novelty_score: f32,
    pub applied: bool,
    /// Simulated coherence term for picked candidate (explicit, not folded into novelty_score only).
    #[serde(default)]
    pub score_coherence_term: f32,
    #[serde(default)]
    pub score_novelty_term: f32,
    /// Cross-candidate disagreement signal used in ranking.
    #[serde(default)]
    pub score_dissent_term: f32,
    #[serde(default)]
    pub composite_objective: f32,
    #[serde(default)]
    pub exploration_pick: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DecisionRecord {
    pub timestamp: u64,
    pub kind: String,
    pub tick_count: u64,
    pub coherence_snapshot: f64,
    pub alternatives: usize,
    pub picked_exploration: bool,
    pub score_coherence_term: f32,
    pub score_novelty_term: f32,
    pub score_dissent_term: f32,
    pub composite_objective: f32,
    pub intent_summary: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum MutationType {
    TopologyAdjustment,
    OntologyExpansion,
    ParameterTuning,
    EdgeRewiring,
    VertexSplitting,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct AgentActionStats {
    pub edges_created: u64,
    pub tasks_created: u64,
    pub memories_added: u64,
    pub risk_mitigations: u64,
    pub successful_actions: u64,
    pub citations: u64,
    #[serde(default)]
    pub last_edges: u64,
    #[serde(default)]
    pub last_tasks: u64,
    #[serde(default)]
    pub last_memories: u64,
    #[serde(default)]
    pub last_risks: u64,
    #[serde(default)]
    pub last_citations: u64,
    #[serde(default)]
    pub last_degree: usize,

    // ── Collective contribution tracking ──
    // Traces reused: edges this agent created that other agents later connected to.
    // Measured by shared-participant overlap when new edges are created.
    #[serde(default)]
    pub traces_reused_by_others: u64,
    // Surviving edges: how many edges this agent originated that survived decay
    // (weight > 0.1 after aging). Durable contribution to collective knowledge.
    #[serde(default)]
    pub surviving_edges: u64,
    // Cascade citations: citation credit that propagated through positive outcomes.
    // When a council output citing this agent leads to a Positive experience,
    // the citing agent gets cascade credit.
    #[serde(default)]
    pub cascade_citations: u64,
    // Downstream successes: successful actions by agents connected to this agent's
    // edges. Measures enabling contribution — "I created infrastructure others built on."
    #[serde(default)]
    pub downstream_successes: u64,
    // Delta trackers for collective signals (same pattern as individual stats)
    #[serde(default)]
    pub last_traces_reused: u64,
    #[serde(default)]
    pub last_surviving_edges: u64,
    #[serde(default)]
    pub last_cascade_citations: u64,
    #[serde(default)]
    pub last_downstream_successes: u64,
}

// === CONSTRUCTION ===
impl HyperStigmergicMorphogenesis {
    fn vertex_counts(size_agents: usize) -> (usize, usize, usize, usize) {
        (size_agents, size_agents, size_agents, size_agents)
    }

    pub fn new(size: usize) -> Self {
        let mut rng = rand::thread_rng();
        let mut agents = Vec::with_capacity(size);

        for i in 0..size {
            let drives = Drives {
                curiosity: rng.gen_range(0.3..1.0),
                harmony: rng.gen_range(0.3..1.0),
                growth: rng.gen_range(0.5..1.2),
                transcendence: rng.gen_range(0.1..0.5),
            };
            let role = match i % Role::COUNT {
                0 => Role::Architect,
                1 => Role::Catalyst,
                2 => Role::Chronicler,
                3 => Role::Critic,
                4 => Role::Explorer,
                _ => Role::Coder,
            };
            let mut agent = Agent::new(i as u64, drives, 0.05);
            agent.role = role;
            agents.push(agent);
        }

        let (n_agents_v, n_tools_v, n_mem_v, n_tasks_v) = Self::vertex_counts(size);
        let num_vertices = n_agents_v + n_tools_v + n_mem_v + n_tasks_v;

        let mut vertex_meta = Vec::with_capacity(num_vertices);
        let now = Self::current_timestamp();

        for i in 0..n_agents_v {
            vertex_meta.push(VertexMeta {
                kind: VertexKind::Agent,
                name: format!("agent_{}", i),
                created_at: now,
                modified_at: now,
                drift_count: 0,
                embedding: None,
                origin_system: None,
            });
        }
        for i in 0..n_tools_v {
            vertex_meta.push(VertexMeta {
                kind: VertexKind::Tool,
                name: format!("tool_{}", i),
                created_at: now,
                modified_at: now,
                drift_count: 0,
                embedding: None,
                origin_system: None,
            });
        }
        for i in 0..n_mem_v {
            vertex_meta.push(VertexMeta {
                kind: VertexKind::Memory,
                name: format!("memory_{}", i),
                created_at: now,
                modified_at: now,
                drift_count: 0,
                embedding: None,
                origin_system: None,
            });
        }
        for i in 0..n_tasks_v {
            vertex_meta.push(VertexMeta {
                kind: VertexKind::Task,
                name: format!("task_{}", i),
                created_at: now,
                modified_at: now,
                drift_count: 0,
                embedding: None,
                origin_system: None,
            });
        }

        let mut property_vertices = HashMap::new();
        let properties = ["Curiosity", "Harmony", "Growth", "Transcendence"];
        for (i, prop) in properties.iter().enumerate() {
            let prop_vertex = num_vertices + i;
            vertex_meta.push(VertexMeta {
                kind: VertexKind::Property,
                name: prop.to_string(),
                created_at: now,
                modified_at: now,
                drift_count: 0,
                embedding: None,
                origin_system: None,
            });
            property_vertices.insert(prop.to_string(), prop_vertex);
        }

        let total_vertices = num_vertices + properties.len();
        let mut ontology = HashMap::new();

        let root_concepts = vec![
            ("Root", vec!["Entity", "Process", "Property", "Relation"]),
            ("Entity", vec!["Agent", "Tool", "Memory", "Task"]),
            (
                "Tool",
                vec![
                    "CollaborationModule",
                    "MessageBus",
                    "TaskRouter",
                    "SharedMemory",
                    "HandoffProtocol",
                ],
            ),
            (
                "Process",
                vec![
                    "Stigmergy",
                    "Morphogenesis",
                    "Emergence",
                    "Abliteration",
                    "HypergraphConvolution",
                ],
            ),
            (
                "Property",
                vec!["Curiosity", "Harmony", "Growth", "Transcendence"],
            ),
            (
                "Stigmergy",
                vec![
                    "TraceMediatedCoordination",
                    "IndirectInteraction",
                    "EnvironmentalFeedback",
                ],
            ),
            (
                "Morphogenesis",
                vec!["SelfOrganization", "PatternFormation", "LatentSurgery"],
            ),
        ];

        for (concept, instances) in root_concepts {
            ontology.insert(
                concept.to_string(),
                OntologyEntry {
                    concept: concept.to_string(),
                    instances: instances.iter().map(|s| s.to_string()).collect(),
                    confidence: 1.0,
                    parent_concepts: if concept == "Root" {
                        vec![]
                    } else {
                        vec!["Root".to_string()]
                    },
                    created_epoch: 0,
                    last_modified: now,
                    embedding: None,
                },
            );
        }

        Self {
            agents,
            agent_action_stats: HashMap::new(),
            adjacency: HashMap::new(),
            edges: Vec::new(),
            decay_rate: 0.02,
            tick_count: 0,
            prev_coherence: 0.7,
            hypergraph: Hypergraph {
                num_vertices: total_vertices,
                hyperedges: vec![],
                edge_weights: vec![],
            },
            vertex_state: DMatrix::<f32>::zeros(total_vertices, EMBEDDING_DIM),
            vertex_meta,
            num_agents_vertices: n_agents_v,
            num_tools_vertices: n_tools_v,
            num_memory_vertices: n_mem_v,
            num_task_vertices: n_tasks_v,
            ontology,
            dynamic_concepts: Vec::new(),
            adaptive_config: AdaptiveConfig::default(),
            drift_config: DriftConfig::default(),
            adaptive_threshold: true,
            property_vertices,
            stigmergic_clusters: HashMap::new(),
            deep_memory: HashMap::new(),
            embedding_cache: HashMap::new(),
            embedding_index: InMemoryEmbeddingIndex::new(EMBEDDING_DIM),
            improvement_history: Vec::new(),
            current_intent: None,
            avoid_hints: Vec::new(),
            beliefs: Vec::new(),
            experiences: Vec::new(),
            social_memory: SocialMemory::default(),
            stigmergic_memory: StigmergicMemory::default(),
            fact_templates: Vec::new(),
            composite_facts: Vec::new(),
            recursive_fact_relations: Vec::new(),
            delegation_frames: Vec::new(),
            next_fact_template_id: 0,
            next_composite_fact_id: 0,
            next_fact_relation_id: 0,
            next_delegation_frame_id: 0,
            reflection_count: 0,
            last_reflection_tick: 0,
            last_belief_reeval_tick: 0,
            skill_bank: crate::skill::SkillBank::new_with_seeds(),
            federation_config: None,
            decision_log: Vec::new(),
            world_state_generation: 0,
        }
    }

    pub fn current_timestamp() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }
}

// === CORE GRAPH OPS ===
impl HyperStigmergicMorphogenesis {
    pub fn from_property_graph_snapshot(snapshot: &PropertyGraphSnapshot) -> Self {
        let mut world = Self::new(0);
        world.agents.clear();
        world.agent_action_stats.clear();
        world.adjacency.clear();
        world.edges.clear();
        world.hypergraph.hyperedges.clear();
        world.hypergraph.edge_weights.clear();
        world.vertex_meta.clear();
        world.ontology.clear();
        world.property_vertices.clear();
        world.beliefs.clear();
        world.experiences.clear();
        world.social_memory = SocialMemory::default();
        world.stigmergic_memory = StigmergicMemory::default();
        world.fact_templates.clear();
        world.composite_facts.clear();
        world.recursive_fact_relations.clear();
        world.delegation_frames.clear();

        for node in &snapshot.nodes {
            // Load actual agents (has "Agent" label but NOT "Vertex" label)
            // Vertex_meta entries with kind=Agent have both labels, so we skip them here
            if node.labels.iter().any(|l| l == "Agent") && !node.labels.iter().any(|l| l == "Vertex") {
                let agent_id = get_int(&node.properties, "agent_id").unwrap_or(0) as u64;
                let mut agent = Agent::new(
                    agent_id,
                    Drives {
                        curiosity: get_float(&node.properties, "curiosity").unwrap_or(0.5),
                        harmony: get_float(&node.properties, "harmony").unwrap_or(0.5),
                        growth: get_float(&node.properties, "growth").unwrap_or(0.5),
                        transcendence: get_float(&node.properties, "transcendence").unwrap_or(0.5),
                    },
                    0.05,
                );
                agent.description = get_string(&node.properties, "description").unwrap_or_default();
                agent.role = parse_role(
                    get_string(&node.properties, "role")
                        .unwrap_or_else(|| "Architect".into())
                        .as_str(),
                );
                agent.jw = get_float(&node.properties, "jw").unwrap_or(0.0);
                world.agents.push(agent);
            } else if node.labels.iter().any(|l| l == "Vertex") {
                let idx = get_int(&node.properties, "vertex_index").unwrap_or(0) as usize;
                let kind = node
                    .labels
                    .iter()
                    .find_map(|label| parse_vertex_kind(label))
                    .unwrap_or(VertexKind::Memory);
                let meta = VertexMeta {
                    kind,
                    name: get_string(&node.properties, "name").unwrap_or_else(|| node.id.clone()),
                    created_at: get_int(&node.properties, "created_at").unwrap_or(0) as u64,
                    modified_at: get_int(&node.properties, "modified_at").unwrap_or(0) as u64,
                    drift_count: get_int(&node.properties, "drift_count").unwrap_or(0) as u64,
                    embedding: None,
                    origin_system: None,
                };
                ensure_vertex_slot(&mut world.vertex_meta, idx, meta.clone());
                if kind == VertexKind::Property {
                    world.property_vertices.insert(meta.name.clone(), idx);
                }
            } else if node.labels.iter().any(|l| l == "Ontology") {
                let concept =
                    get_string(&node.properties, "concept").unwrap_or_else(|| node.id.clone());
                world.ontology.insert(
                    concept.clone(),
                    OntologyEntry {
                        concept,
                        instances: get_string_list(&node.properties, "instances"),
                        confidence: get_float(&node.properties, "confidence").unwrap_or(0.5) as f32,
                        parent_concepts: vec!["Root".into()],
                        created_epoch: 0,
                        last_modified: 0,
                        embedding: None,
                    },
                );
            } else if node.labels.iter().any(|l| l == "Belief") {
                let content = get_string(&node.properties, "content").unwrap_or_default();
                let (l0, l1) = crate::memory::derive_hierarchy(&content);
                let owner_namespace = get_string(&node.properties, "owner_namespace");
                let supersedes_belief_id = get_int(&node.properties, "supersedes_belief_id")
                    .map(|i| i as usize);
                let evidence_belief_ids: Vec<usize> = get_string(
                    &node.properties,
                    "evidence_belief_ids",
                )
                .map(|s| {
                    s.split(',')
                        .filter_map(|t| t.trim().parse().ok())
                        .collect()
                })
                .unwrap_or_default();
                let human_committed = get_int(&node.properties, "human_committed")
                    .map(|i| i != 0)
                    .unwrap_or(false);
                world.beliefs.push(Belief {
                    id: world.beliefs.len(),
                    content,
                    abstract_l0: Some(l0),
                    overview_l1: Some(l1),
                    confidence: get_float(&node.properties, "confidence").unwrap_or(0.5),
                    source: BeliefSource::Observation,
                    supporting_evidence: Vec::new(),
                    contradicting_evidence: Vec::new(),
                    created_at: 0,
                    updated_at: 0,
                    update_count: 0,
                    owner_namespace,
                    supersedes_belief_id,
                    evidence_belief_ids,
                    human_committed,
                });
            } else if node.labels.iter().any(|l| l == "Experience") {
                let desc = get_string(&node.properties, "description").unwrap_or_default();
                let (l0, l1) = crate::memory::derive_hierarchy(&desc);
                world.experiences.push(Experience {
                    id: world.experiences.len(),
                    description: desc,
                    context: get_string(&node.properties, "context").unwrap_or_default(),
                    abstract_l0: Some(l0),
                    overview_l1: Some(l1),
                    outcome: ExperienceOutcome::Neutral,
                    timestamp: 0,
                    tick: get_int(&node.properties, "tick").unwrap_or(0) as u64,
                    embedding: None,
                });
            } else if node.labels.iter().any(|l| l == "StigmergicTrace") {
                world
                    .stigmergic_memory
                    .traces
                    .push(crate::stigmergic_policy::StigmergicTrace {
                        id: get_string(&node.properties, "trace_id")
                            .unwrap_or_else(|| node.id.clone()),
                        agent_id: get_int(&node.properties, "agent_id").unwrap_or(0) as u64,
                        model_id: get_string(&node.properties, "model_id").unwrap_or_default(),
                        task_key: get_string(&node.properties, "task_key"),
                        kind: TraceKind::from_str(
                            get_string(&node.properties, "kind")
                                .unwrap_or_else(|| "Observation".into())
                                .as_str(),
                        ),
                        summary: get_string(&node.properties, "summary").unwrap_or_default(),
                        success: get_bool(&node.properties, "success"),
                        outcome_score: get_float(&node.properties, "outcome_score"),
                        sensitivity: parse_data_sensitivity(
                            get_string(&node.properties, "sensitivity")
                                .unwrap_or_else(|| "Internal".into())
                                .as_str(),
                        ),
                        planned_tool: get_string(&node.properties, "planned_tool"),
                        recorded_at: get_int(&node.properties, "recorded_at").unwrap_or(0) as u64,
                        tick: get_int(&node.properties, "tick").unwrap_or(0) as u64,
                        metadata: parse_tags(get_string_list(&node.properties, "metadata")),
                    });
            } else if node.labels.iter().any(|l| l == "RoutingDirective") {
                let task_key =
                    get_string(&node.properties, "task_key").unwrap_or_else(|| node.id.clone());
                world.stigmergic_memory.directives.insert(
                    task_key.clone(),
                    crate::stigmergic_policy::RoutingDirective {
                        task_key,
                        preferred_agent: get_int(&node.properties, "preferred_agent")
                            .map(|v| v as u64),
                        preferred_tool: get_string(&node.properties, "preferred_tool")
                            .unwrap_or_else(|| "cypher".into()),
                        minimum_sensitivity: parse_data_sensitivity(
                            get_string(&node.properties, "minimum_sensitivity")
                                .unwrap_or_else(|| "Internal".into())
                                .as_str(),
                        ),
                        confidence: get_float(&node.properties, "confidence").unwrap_or(0.5),
                        rationale: get_string(&node.properties, "rationale").unwrap_or_default(),
                        updated_at: get_int(&node.properties, "updated_at").unwrap_or(0) as u64,
                    },
                );
            } else if node.labels.iter().any(|l| l == "PolicyShift") {
                world
                    .stigmergic_memory
                    .policy_shifts
                    .push(crate::stigmergic_policy::PolicyShift {
                        id: get_string(&node.properties, "policy_id")
                            .unwrap_or_else(|| node.id.clone()),
                        category: get_string(&node.properties, "category").unwrap_or_default(),
                        target_agent: get_int(&node.properties, "target_agent").map(|v| v as u64),
                        target_task: get_string(&node.properties, "target_task"),
                        value: get_string(&node.properties, "value").unwrap_or_default(),
                        confidence: get_float(&node.properties, "confidence").unwrap_or(0.5),
                        rationale: get_string(&node.properties, "rationale").unwrap_or_default(),
                        updated_at: get_int(&node.properties, "updated_at").unwrap_or(0) as u64,
                    });
            } else if node.labels.iter().any(|l| l == "FactTemplate") {
                world.fact_templates.push(FactTemplate {
                    id: get_string(&node.properties, "template_id")
                        .unwrap_or_else(|| node.id.clone()),
                    label: get_string(&node.properties, "label").unwrap_or_default(),
                    narrative: get_string(&node.properties, "narrative").unwrap_or_default(),
                    slot_names: get_string_list(&node.properties, "slot_names"),
                    created_at: get_int(&node.properties, "created_at").unwrap_or(0) as u64,
                    updated_at: get_int(&node.properties, "updated_at").unwrap_or(0) as u64,
                });
            } else if node.labels.iter().any(|l| l == "CompositeFact") {
                world.composite_facts.push(CompositeFact {
                    id: get_string(&node.properties, "fact_id").unwrap_or_else(|| node.id.clone()),
                    kind: parse_composite_fact_kind(
                        get_string(&node.properties, "fact_kind")
                            .unwrap_or_else(|| "Fact".into())
                            .as_str(),
                    ),
                    label: get_string(&node.properties, "label").unwrap_or_default(),
                    details: get_string(&node.properties, "details").unwrap_or_default(),
                    template_id: get_string(&node.properties, "template_id"),
                    slots: parse_slot_bindings(get_string_list(&node.properties, "slots")),
                    temporal: TemporalSemantics {
                        discovered_at: get_int(&node.properties, "discovered_at").unwrap_or(0)
                            as u64,
                        created_at: get_int(&node.properties, "created_at").unwrap_or(0) as u64,
                        updated_at: get_int(&node.properties, "updated_at").unwrap_or(0) as u64,
                        valid_from: get_int(&node.properties, "valid_from").map(|v| v as u64),
                        valid_until: get_int(&node.properties, "valid_until").map(|v| v as u64),
                        occurred_at: get_int(&node.properties, "occurred_at").map(|v| v as u64),
                    },
                    confidence: get_float(&node.properties, "confidence").unwrap_or(0.5),
                    tags: get_string_list(&node.properties, "tags"),
                    external_ref: get_string(&node.properties, "external_ref"),
                });
            } else if node.labels.iter().any(|l| l == "DelegationFrame") {
                world.delegation_frames.push(DelegationFrame {
                    id: get_string(&node.properties, "delegation_id")
                        .unwrap_or_else(|| node.id.clone()),
                    task_key: get_string(&node.properties, "task_key").unwrap_or_default(),
                    requester: get_int(&node.properties, "requester").map(|v| v as u64),
                    delegated_to: get_int(&node.properties, "delegated_to").unwrap_or(0) as u64,
                    rationale: get_string(&node.properties, "rationale").unwrap_or_default(),
                    confidence: get_float(&node.properties, "confidence").unwrap_or(0.5),
                    promise_id: get_string(&node.properties, "promise_id"),
                    status: parse_delegation_status(
                        get_string(&node.properties, "status")
                            .unwrap_or_else(|| "Proposed".into())
                            .as_str(),
                    ),
                    created_at: get_int(&node.properties, "created_at").unwrap_or(0) as u64,
                    updated_at: get_int(&node.properties, "updated_at").unwrap_or(0) as u64,
                    outcome_fact_id: get_string(&node.properties, "outcome_fact_id"),
                });
            }
        }

        for rel in &snapshot.relationships {
            if rel.rel_type == "HYPEREDGE_LINK" {
                let a = rel
                    .start_node
                    .strip_prefix("agent:")
                    .and_then(|s| s.parse::<u64>().ok());
                let b = rel
                    .end_node
                    .strip_prefix("agent:")
                    .and_then(|s| s.parse::<u64>().ok());
                if let (Some(a), Some(b)) = (a, b) {
                    world.edges.push(HyperEdge {
                        participants: vec![a, b],
                        weight: get_float(&rel.properties, "weight").unwrap_or(1.0),
                        emergent: get_bool(&rel.properties, "emergent").unwrap_or(false),
                        age: get_int(&rel.properties, "age").unwrap_or(0) as u64,
                        tags: parse_tags(get_string_list(&rel.properties, "tags")),
                        created_at: 0,
                        embedding: None,
                        creator: None,
                        scope: None,
                        provenance: None,
                        trust_tags: None,
                        origin_system: None,
                        knowledge_layer: None,
                    });
                }
            } else if rel.start_node.starts_with("fact:")
                && rel.end_node.starts_with("fact:")
                && matches!(
                    rel.rel_type.as_str(),
                    "CONTAINS"
                        | "CLARIFIES"
                        | "EXTENDS"
                        | "SUPPORTS"
                        | "CONTRADICTS"
                        | "CAUSES"
                        | "DEPENDS_ON"
                        | "FULFILLS"
                        | "VIOLATES"
                        | "DERIVED_FROM"
                )
            {
                world.recursive_fact_relations.push(RecursiveFactRelation {
                    id: get_string(&rel.properties, "relation_id")
                        .unwrap_or_else(|| rel.id.clone()),
                    from_fact_id: rel.start_node.trim_start_matches("fact:").to_string(),
                    to_fact_id: rel.end_node.trim_start_matches("fact:").to_string(),
                    kind: RecursiveRelationKind::from_str(&rel.rel_type),
                    confidence: get_float(&rel.properties, "confidence").unwrap_or(0.5),
                    rationale: get_string(&rel.properties, "rationale").unwrap_or_default(),
                    created_at: get_int(&rel.properties, "created_at").unwrap_or(0) as u64,
                });
            }
        }

        world.num_agents_vertices = world
            .vertex_meta
            .iter()
            .filter(|v| v.kind == VertexKind::Agent)
            .count();
        world.num_tools_vertices = world
            .vertex_meta
            .iter()
            .filter(|v| v.kind == VertexKind::Tool)
            .count();
        world.num_memory_vertices = world
            .vertex_meta
            .iter()
            .filter(|v| v.kind == VertexKind::Memory)
            .count();
        world.num_task_vertices = world
            .vertex_meta
            .iter()
            .filter(|v| v.kind == VertexKind::Task)
            .count();
        world.hypergraph.num_vertices = world.vertex_meta.len();
        world.hypergraph.hyperedges = world
            .edges
            .iter()
            .map(|e| e.participants.iter().map(|id| *id as usize).collect())
            .collect();
        world.hypergraph.edge_weights = world.edges.iter().map(|e| e.weight as f32).collect();
        world.vertex_state = DMatrix::<f32>::zeros(world.vertex_meta.len(), EMBEDDING_DIM);
        world.embedding_index = InMemoryEmbeddingIndex::new(EMBEDDING_DIM);
        world.stigmergic_memory.next_trace_id = world.stigmergic_memory.traces.len() as u64;
        world.stigmergic_memory.next_policy_id = world.stigmergic_memory.policy_shifts.len() as u64;
        world.next_fact_template_id = world.fact_templates.len() as u64;
        world.next_composite_fact_id = world.composite_facts.len() as u64;
        world.next_fact_relation_id = world.recursive_fact_relations.len() as u64;
        world.next_delegation_frame_id = world.delegation_frames.len() as u64;
        world.rebuild_adjacency();
        world
    }

    pub fn property_graph_snapshot(&self) -> PropertyGraphSnapshot {
        self.to_property_graph_snapshot()
    }

    pub fn run_cypher_like_query(&self, query: &str) -> QueryResultSet {
        let snapshot = self.property_graph_snapshot();
        CypherEngine::execute(&snapshot, query)
    }

    pub fn plan_and_execute_graph_action(&self, input: &str) -> GraphActionResult {
        GraphRuntime::execute(self, input)
    }

    pub fn record_agent_trace(
        &mut self,
        agent_id: AgentId,
        model_id: &str,
        task_key: Option<&str>,
        kind: TraceKind,
        summary: &str,
        success: Option<bool>,
        outcome_score: Option<f64>,
        sensitivity: DataSensitivity,
    ) -> String {
        self.stigmergic_memory.record_trace(
            agent_id,
            model_id,
            task_key,
            kind,
            summary,
            success,
            outcome_score,
            sensitivity,
            None,
            Self::current_timestamp(),
            self.tick_count,
            HashMap::new(),
        )
    }

    pub fn stigmergic_directive_for(
        &self,
        task_key: &str,
    ) -> Option<&crate::stigmergic_policy::RoutingDirective> {
        self.stigmergic_memory.directive_for(task_key)
    }

    pub fn recommended_tool_for_task(
        &self,
        task_key: &str,
        input: &str,
    ) -> crate::graph_runtime::GraphActionPlan {
        GraphRuntime::plan_with_preference(
            input,
            self.stigmergic_memory.preferred_tool_for(task_key),
        )
    }

    pub fn plan_and_execute_stigmergic_graph_action(
        &mut self,
        agent_id: AgentId,
        model_id: &str,
        task_key: &str,
        input: &str,
        sensitivity: DataSensitivity,
    ) -> GraphActionResult {
        let plan = self.recommended_tool_for_task(task_key, input);
        let directive_confidence = self
            .stigmergic_directive_for(task_key)
            .map(|d| d.confidence);
        self.stigmergic_memory.record_trace(
            agent_id,
            model_id,
            Some(task_key),
            TraceKind::QueryPlanned,
            format!("planned graph action: {}", plan.rationale),
            None,
            directive_confidence,
            sensitivity.clone(),
            Some(plan.tool.clone()),
            Self::current_timestamp(),
            self.tick_count,
            HashMap::new(),
        );
        let result = GraphRuntime::execute_plan(self, &plan);
        self.stigmergic_memory.record_trace(
            agent_id,
            model_id,
            Some(task_key),
            TraceKind::QueryExecuted,
            result.summary.clone(),
            Some(true),
            directive_confidence,
            sensitivity,
            Some(result.tool.clone()),
            Self::current_timestamp(),
            self.tick_count,
            HashMap::new(),
        );
        result
    }

    pub fn apply_stigmergic_cycle(&mut self) {
        self.stigmergic_memory
            .apply_cycle(&mut self.agents, &self.social_memory, self.tick_count);
    }

    pub fn enrich_council_proposal(&self, proposal: &mut Proposal) {
        let task_key = proposal
            .task_key
            .clone()
            .unwrap_or_else(|| proposal.title.to_lowercase());
        let directive = self.stigmergic_directive_for(&task_key);
        let require_council_review = self.stigmergic_memory.policy_shifts.iter().any(|shift| {
            shift.category == "council_review"
                && shift.target_task.as_deref() == Some(task_key.as_str())
                && shift.confidence >= 0.6
        });
        let evidence = self.collect_council_evidence(&task_key);
        let graph_snapshot_bullets = self.council_trace_graph_bullets(&task_key);
        let graph_queries = self.council_trace_graph_queries(&task_key);
        if let Some(directive) = directive {
            let preferred_tool =
                crate::graph_runtime::GraphRuntime::parse_tool_name(&directive.preferred_tool);
            proposal.task_key = Some(task_key);
            proposal.stigmergic_context = Some(StigmergicCouncilContext {
                preferred_agent: directive.preferred_agent,
                preferred_tool,
                confidence: directive.confidence,
                require_council_review,
                rationale: directive.rationale.clone(),
                evidence,
                graph_snapshot_bullets,
                graph_queries,
            });
        } else if require_council_review
            || !evidence.is_empty()
            || !graph_snapshot_bullets.is_empty()
            || !graph_queries.is_empty()
        {
            proposal.task_key = Some(task_key);
            proposal.stigmergic_context = Some(StigmergicCouncilContext {
                preferred_agent: None,
                preferred_tool: None,
                confidence: 0.5,
                require_council_review,
                rationale: if require_council_review {
                    "stigmergic policy requires council review".into()
                } else {
                    "stigmergic trace graph supplied council evidence".into()
                },
                evidence,
                graph_snapshot_bullets,
                graph_queries,
            });
        }
    }

    fn collect_council_evidence(&self, task_key: &str) -> Vec<CouncilEvidence> {
        let mut evidence = Vec::new();

        if let Some(directive) = self.stigmergic_directive_for(task_key) {
            evidence.push(self.directive_evidence(directive));
        }

        let mut traces = self
            .stigmergic_memory
            .traces
            .iter()
            .filter(|trace| trace.task_key.as_deref() == Some(task_key))
            .collect::<Vec<_>>();
        traces.sort_by(|a, b| b.recorded_at.cmp(&a.recorded_at).then(b.tick.cmp(&a.tick)));
        for trace in traces.into_iter().take(4) {
            evidence.push(self.trace_evidence(trace));
        }

        let mut shifts = self
            .stigmergic_memory
            .policy_shifts
            .iter()
            .filter(|shift| {
                shift.target_task.as_deref() == Some(task_key)
                    || (shift.category == "share_restriction"
                        && shift.target_agent.is_some_and(|agent_id| {
                            self.stigmergic_memory.is_agent_restricted(agent_id)
                        }))
            })
            .collect::<Vec<_>>();
        shifts.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        for shift in shifts.into_iter().take(2) {
            evidence.push(self.policy_shift_evidence(shift));
        }

        evidence
    }

    pub fn council_trace_graph_bullets(&self, task_key: &str) -> Vec<String> {
        let mut bullets = Vec::new();

        let mut trust_rank = self
            .agents
            .iter()
            .filter_map(|agent| {
                self.social_memory
                    .reputations
                    .get(&agent.id)
                    .map(|rep| (agent.id, rep.reliability_score(), rep.avg_quality()))
            })
            .collect::<Vec<_>>();
        trust_rank.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        if !trust_rank.is_empty() {
            let trusted = trust_rank
                .into_iter()
                .take(2)
                .map(|(agent_id, reliability, quality)| {
                    format!(
                        "agent {agent_id} is currently trusted (reliability {:.2}, avg quality {:.2})",
                        reliability, quality
                    )
                })
                .collect::<Vec<_>>()
                .join("; ");
            bullets.push(trusted);
        }

        let mut promises = self
            .social_memory
            .promises
            .values()
            .filter(|promise| promise.task_key == task_key)
            .collect::<Vec<_>>();
        promises.sort_by(|a, b| {
            b.resolved_at
                .unwrap_or(b.promised_at)
                .cmp(&a.resolved_at.unwrap_or(a.promised_at))
        });
        if let Some(promise) = promises.first() {
            bullets.push(format!(
                "latest promise {} for '{}' is {:?} with quality {:?}",
                promise.id, promise.task_key, promise.status, promise.quality_score
            ));
        }

        let mut shifts = self
            .stigmergic_memory
            .policy_shifts
            .iter()
            .filter(|shift| {
                shift.target_task.as_deref() == Some(task_key) || shift.target_task.is_none()
            })
            .collect::<Vec<_>>();
        shifts.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        if let Some(shift) = shifts.first() {
            bullets.push(format!(
                "policy shift {} says {} ({})",
                shift.id, shift.value, shift.rationale
            ));
        }

        let restricted = self
            .stigmergic_memory
            .policy_shifts
            .iter()
            .filter(|shift| shift.category == "share_restriction")
            .filter_map(|shift| shift.target_agent)
            .collect::<Vec<_>>();
        if !restricted.is_empty() {
            bullets.push(format!(
                "restricted sharing currently applies to agents {:?}",
                restricted
            ));
        }

        bullets
    }

    pub fn council_trace_graph_queries(&self, task_key: &str) -> Vec<CouncilGraphQuery> {
        let escaped_task = escape_cypher_value(task_key);
        let snapshot = self.property_graph_snapshot();
        let query_specs = vec![
            (
                "recent task traces",
                format!(
                    "MATCH (t:StigmergicTrace) WHERE t.task_key = '{}' RETURN t LIMIT 3",
                    escaped_task
                ),
            ),
            (
                "routing directive",
                format!(
                    "MATCH (d:RoutingDirective) WHERE d.task_key = '{}' RETURN d LIMIT 1",
                    escaped_task
                ),
            ),
            (
                "task policy shifts",
                format!(
                    "MATCH (p:PolicyShift) WHERE p.target_task = '{}' RETURN p LIMIT 2",
                    escaped_task
                ),
            ),
        ];

        query_specs
            .into_iter()
            .filter_map(|(purpose, query)| {
                let result = CypherEngine::execute(&snapshot, &query);
                let mut evidence = result
                    .nodes
                    .iter()
                    .filter_map(|node| self.graph_node_to_council_evidence(node))
                    .collect::<Vec<_>>();
                if evidence.is_empty() {
                    return None;
                }
                let query_id = format!("query:{}", purpose.replace(' ', "_"));
                evidence.push(CouncilEvidence {
                    id: query_id,
                    kind: CouncilEvidenceKind::GraphQuery,
                    summary: format!("{purpose} returned {} records", evidence.len()),
                });
                Some(CouncilGraphQuery {
                    purpose: purpose.to_string(),
                    query,
                    evidence,
                })
            })
            .collect()
    }

    fn trace_evidence(&self, trace: &StigmergicTrace) -> CouncilEvidence {
        let status = match trace.success {
            Some(true) => "success",
            Some(false) => "failure",
            None => "pending",
        };
        let task = trace.task_key.as_deref().unwrap_or("unscoped task");
        let score = trace
            .outcome_score
            .map(|value| format!(", score {:.2}", value))
            .unwrap_or_default();
        CouncilEvidence {
            id: trace.id.clone(),
            kind: CouncilEvidenceKind::Trace,
            summary: format!(
                "{task} {} by agent {} via {} at tick {}{} ({}){}",
                trace.kind.as_str(),
                trace.agent_id,
                trace
                    .planned_tool
                    .clone()
                    .unwrap_or_else(|| "unknown-tool".to_string()),
                trace.tick,
                score,
                status,
                if trace.summary.is_empty() {
                    String::new()
                } else {
                    format!(": {}", trace.summary)
                }
            ),
        }
    }

    fn directive_evidence(&self, directive: &RoutingDirective) -> CouncilEvidence {
        CouncilEvidence {
            id: format!(
                "directive:{}",
                normalize_council_id_fragment(&directive.task_key)
            ),
            kind: CouncilEvidenceKind::Directive,
            summary: format!(
                "task '{}' routes toward agent {:?} with tool {} at confidence {:.2}: {}",
                directive.task_key,
                directive.preferred_agent,
                directive.preferred_tool,
                directive.confidence,
                directive.rationale
            ),
        }
    }

    fn policy_shift_evidence(&self, shift: &PolicyShift) -> CouncilEvidence {
        CouncilEvidence {
            id: format!("policy:{}", shift.id),
            kind: CouncilEvidenceKind::PolicyShift,
            summary: format!(
                "{} => {} (confidence {:.2}, rationale: {})",
                shift.category, shift.value, shift.confidence, shift.rationale
            ),
        }
    }

    fn graph_node_to_council_evidence(
        &self,
        node: &crate::property_graph::GraphNodeRecord,
    ) -> Option<CouncilEvidence> {
        if node.labels.iter().any(|label| label == "StigmergicTrace") {
            let trace = StigmergicTrace {
                id: get_string(&node.properties, "trace_id")?,
                agent_id: get_int(&node.properties, "agent_id")? as u64,
                model_id: get_string(&node.properties, "model_id").unwrap_or_default(),
                task_key: get_string(&node.properties, "task_key"),
                kind: TraceKind::from_str(
                    get_string(&node.properties, "kind")
                        .unwrap_or_else(|| "Observation".to_string())
                        .as_str(),
                ),
                summary: get_string(&node.properties, "summary").unwrap_or_default(),
                success: get_bool(&node.properties, "success"),
                outcome_score: get_float(&node.properties, "outcome_score"),
                sensitivity: parse_data_sensitivity(
                    get_string(&node.properties, "sensitivity")
                        .unwrap_or_else(|| "Internal".to_string())
                        .as_str(),
                ),
                planned_tool: get_string(&node.properties, "planned_tool"),
                recorded_at: get_int(&node.properties, "recorded_at").unwrap_or_default() as u64,
                tick: get_int(&node.properties, "tick").unwrap_or_default() as u64,
                metadata: parse_tags(get_string_list(&node.properties, "metadata")),
            };
            return Some(self.trace_evidence(&trace));
        }
        if node.labels.iter().any(|label| label == "RoutingDirective") {
            let directive = RoutingDirective {
                task_key: get_string(&node.properties, "task_key")?,
                preferred_agent: get_int(&node.properties, "preferred_agent")
                    .map(|value| value as u64),
                preferred_tool: get_string(&node.properties, "preferred_tool").unwrap_or_default(),
                minimum_sensitivity: parse_data_sensitivity(
                    get_string(&node.properties, "minimum_sensitivity")
                        .unwrap_or_else(|| "Internal".to_string())
                        .as_str(),
                ),
                confidence: get_float(&node.properties, "confidence").unwrap_or(0.5),
                rationale: get_string(&node.properties, "rationale").unwrap_or_default(),
                updated_at: get_int(&node.properties, "updated_at").unwrap_or_default() as u64,
            };
            return Some(self.directive_evidence(&directive));
        }
        if node.labels.iter().any(|label| label == "PolicyShift") {
            let shift = PolicyShift {
                id: get_string(&node.properties, "policy_id")?,
                category: get_string(&node.properties, "category").unwrap_or_default(),
                target_agent: get_int(&node.properties, "target_agent").map(|value| value as u64),
                target_task: get_string(&node.properties, "target_task"),
                value: get_string(&node.properties, "value").unwrap_or_default(),
                confidence: get_float(&node.properties, "confidence").unwrap_or(0.5),
                rationale: get_string(&node.properties, "rationale").unwrap_or_default(),
                updated_at: get_int(&node.properties, "updated_at").unwrap_or_default() as u64,
            };
            return Some(self.policy_shift_evidence(&shift));
        }
        None
    }

    pub fn emergent_edge_count(&self) -> usize {
        self.edges.iter().filter(|e| e.emergent).count()
    }

    pub fn global_coherence(&self) -> f64 {
        if self.edges.is_empty() {
            return 0.0;
        }
        self.edges.iter().map(|e| e.weight).sum::<f64>() / self.agents.len().max(1) as f64
    }

    pub fn rebuild_adjacency(&mut self) {
        self.adjacency.clear();
        for (i, e) in self.edges.iter().enumerate() {
            for &p in &e.participants {
                self.adjacency.entry(p).or_default().push(i);
            }
        }
    }

    pub fn add_ontology_entry(
        &mut self,
        concept: &str,
        instance: String,
        confidence: f32,
        parents: Vec<String>,
    ) {
        let now = Self::current_timestamp();
        if let Some(entry) = self.ontology.get_mut(concept) {
            if !entry.instances.contains(&instance) {
                entry.instances.push(instance);
                entry.confidence = entry.confidence * 0.95 + confidence * 0.05;
                entry.last_modified = now;
            }
        } else {
            self.ontology.insert(
                concept.to_string(),
                OntologyEntry {
                    concept: concept.to_string(),
                    instances: vec![instance],
                    confidence,
                    parent_concepts: parents,
                    created_epoch: self.tick_count,
                    last_modified: now,
                    embedding: None,
                },
            );
            self.dynamic_concepts.push(concept.to_string());
        }
    }

    pub fn apply_action(&mut self, action: &Action) {
        let agent_id = self.infer_agent_id(action);
        if let Some(id) = agent_id {
            self.record_action(id, action);
        }
        self.apply_action_inner(action, agent_id);
    }

    pub fn apply_action_with_agent(&mut self, action: &Action, agent_id: Option<AgentId>) {
        if let Some(id) = agent_id {
            self.record_action(id, action);
        }
        self.apply_action_inner(action, agent_id);
    }

    fn apply_action_inner(&mut self, action: &Action, creator: Option<AgentId>) {
        match action {
            Action::LinkAgents { vertices, weight } => {
                let mut participants: Vec<u64> =
                    vertices.iter().copied().map(|v| v as u64).collect();
                if participants.len() >= 2 {
                    // Merge-like normalization: binary merge is the canonical syntactic operation.
                    if participants.len() == 2 {
                        participants.sort_unstable();
                    }
                    let mut tags = HashMap::new();
                    tags.insert(
                        "merge".to_string(),
                        if participants.len() == 2 {
                            "true".to_string()
                        } else {
                            "false".to_string()
                        },
                    );
                    tags.insert("merge_arity".to_string(), participants.len().to_string());
                    self.edges.push(HyperEdge {
                        participants,
                        weight: *weight as f64,
                        emergent: false,
                        age: 0,
                        tags,
                        created_at: self.tick_count,
                        embedding: None,
                        creator,
                        scope: None,
                        provenance: None,
                        trust_tags: None,
                        origin_system: None,
                        knowledge_layer: None,
                    });
                    self.rebuild_adjacency();
                }
            }
            _ => {}
        }
    }

    fn record_action(&mut self, agent_id: AgentId, action: &Action) {
        // ── Collective contribution: trace reuse detection ──
        // Before recording, check if this new edge overlaps with existing edges
        // created by *other* agents. If so, those creators get trace-reuse credit.
        if let Action::LinkAgents { vertices, .. } = action {
            let new_participants: HashSet<u64> =
                vertices.iter().copied().map(|v| v as u64).collect();
            // Collect creators who deserve reuse credit (avoid borrowing self mutably twice)
            let reused_creators: Vec<AgentId> = self
                .edges
                .iter()
                .filter_map(|edge| {
                    let creator = edge.creator?;
                    if creator == agent_id {
                        return None; // Don't credit self-reuse
                    }
                    let edge_set: HashSet<u64> = edge.participants.iter().copied().collect();
                    if !new_participants.is_disjoint(&edge_set) {
                        Some(creator)
                    } else {
                        None
                    }
                })
                .collect();
            for creator in reused_creators {
                self.agent_action_stats
                    .entry(creator)
                    .or_default()
                    .traces_reused_by_others += 1;
            }
        }

        let stats = self.agent_action_stats.entry(agent_id).or_default();
        stats.successful_actions += 1;
        match action {
            Action::LinkAgents { .. } => stats.edges_created += 1,
            Action::AddTask { .. } => stats.tasks_created += 1,
            Action::AddMemory { .. } => stats.memories_added += 1,
            Action::AbliterateSubspace { .. } => stats.risk_mitigations += 1,
            _ => {}
        }
    }

    pub fn record_citation(&mut self, agent_id: AgentId, count: u64) {
        let stats = self.agent_action_stats.entry(agent_id).or_default();
        stats.citations = stats.citations.saturating_add(count);
    }

    /// Record a cascade citation: the council output that cited these agents
    /// led to a positive outcome. This is stronger signal than raw citation —
    /// it means the cited agent's contribution *actually helped*.
    pub fn record_cascade_citations(&mut self, cited_agents: &[(AgentId, u32)], positive: bool) {
        if !positive {
            return;
        }
        for &(agent_id, count) in cited_agents {
            self.agent_action_stats
                .entry(agent_id)
                .or_default()
                .cascade_citations = self
                .agent_action_stats
                .entry(agent_id)
                .or_default()
                .cascade_citations
                .saturating_add(count as u64);
        }
    }

    /// Record downstream success: when an agent succeeds at an action,
    /// all agents connected to it via edges get downstream-success credit.
    /// This captures "I built infrastructure that enabled others."
    pub fn record_downstream_success(&mut self, successful_agent_id: AgentId) {
        // Find all agents connected to the successful agent via edges
        let connected: Vec<AgentId> = self
            .edges
            .iter()
            .filter(|edge| edge.participants.contains(&successful_agent_id))
            .flat_map(|edge| {
                edge.participants
                    .iter()
                    .copied()
                    .filter(|&p| p != successful_agent_id)
            })
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();

        for connected_id in connected {
            self.agent_action_stats
                .entry(connected_id)
                .or_default()
                .downstream_successes += 1;
        }
    }

    /// Compute surviving edges per agent. Called during compute_jw.
    /// An edge "survives" if weight > 0.1 after aging. This measures
    /// durable contribution to collective knowledge.
    fn compute_surviving_edges(&mut self) {
        // Reset all counts
        for stats in self.agent_action_stats.values_mut() {
            stats.surviving_edges = 0;
        }
        // Count surviving edges per creator
        for edge in &self.edges {
            if edge.weight > 0.1 {
                if let Some(creator) = edge.creator {
                    self.agent_action_stats
                        .entry(creator)
                        .or_default()
                        .surviving_edges += 1;
                }
            }
        }
    }

    fn infer_agent_id(&self, action: &Action) -> Option<AgentId> {
        match action {
            Action::DescribeAgent { agent_id, .. } => Some(*agent_id),
            Action::LinkAgents { vertices, .. } => vertices.first().map(|v| *v as u64),
            Action::LinkTaskAgents { agents, .. } => agents.first().copied(),
            Action::AddAgent { id, .. } => Some(*id as u64),
            _ => None,
        }
    }

    /// Interval (in ticks) between belief re-evaluations (Integration 4)
    const BELIEF_REEVAL_INTERVAL: u64 = 50;

    pub fn tick(&mut self) {
        self.tick_count += 1;
        self.decay_edges();
        self.reinforce_edges();
        self.interact_agents();
        self.compute_jw();
        self.apply_stigmergic_cycle();
        self.tune_threshold();

        // Integration 4: Periodically trigger belief re-evaluation
        if self.tick_count - self.last_belief_reeval_tick >= Self::BELIEF_REEVAL_INTERVAL {
            self.last_belief_reeval_tick = self.tick_count;
            // Note: Actual async re-evaluation is triggered from main loop
            // We just mark that it's time for re-evaluation
        }
    }

    /// Check if beliefs should be re-evaluated this tick (Integration 4)
    pub fn should_reevaluate_beliefs(&self) -> bool {
        self.tick_count == self.last_belief_reeval_tick
            && self.tick_count > 0
            && !self.beliefs.is_empty()
    }

    /// Thermodynamic Wage Metric (JW) — computed per agent each tick.
    ///
    /// JW = I × 0.30 + C × 0.40 + H × 0.20 + S × 0.10
    ///
    /// **Rewards collective contribution over individual performance.**
    ///
    /// I (Individual Action) = 0.30 weight
    ///   — edges created, tasks created, memories, risk mitigations, citations
    ///   — Same signals as before, but now only 30% of total (was 60%)
    ///
    /// C (Collective Contribution) = 0.40 weight — **the primary signal**
    ///   — traces_reused_by_others: other agents built on this agent's edges
    ///   — surviving_edges: edges this agent created that survived decay (durable knowledge)
    ///   — cascade_citations: citations that led to positive outcomes downstream
    ///   — downstream_successes: connected agents succeeded after this agent's work
    ///   These measure: "Did this agent's work get used by and help other agents?"
    ///
    /// H (Coherence) = 0.20 weight
    ///   — system coherence improvement this tick
    ///
    /// S (Stability) = 0.10 weight
    ///   — low coherence volatility
    ///
    /// Result stored on each Agent as `agent.jw` for RooDB persistence.
    /// Global JW = mean of all agents' JW values.
    pub fn compute_jw(&mut self) {
        // First, update surviving-edge counts
        self.compute_surviving_edges();

        let coherence_now = self.global_coherence();
        let coherence_delta = (coherence_now - self.prev_coherence).abs();
        let coherence_improvement = (coherence_now - self.prev_coherence).max(0.0);
        self.prev_coherence = coherence_now;

        // Pre-compute per-agent scores to avoid borrow conflicts.
        // Each entry: (agent_index, individual_score, collective_score)
        let mut scores: Vec<(usize, f64, f64)> = Vec::with_capacity(self.agents.len());

        for (idx, agent) in self.agents.iter().enumerate() {
            let stats = self.agent_action_stats.entry(agent.id).or_default();
            let degree = self.adjacency.get(&agent.id).map(|v| v.len()).unwrap_or(0);
            let degree_delta = degree.saturating_sub(stats.last_degree);
            stats.last_degree = degree;

            // ── Individual action delta ──
            let edges_delta = stats.edges_created.saturating_sub(stats.last_edges);
            let tasks_delta = stats.tasks_created.saturating_sub(stats.last_tasks);
            let memories_delta = stats.memories_added.saturating_sub(stats.last_memories);
            let risks_delta = stats.risk_mitigations.saturating_sub(stats.last_risks);
            let citations_delta = stats.citations.saturating_sub(stats.last_citations);

            stats.last_edges = stats.edges_created;
            stats.last_tasks = stats.tasks_created;
            stats.last_memories = stats.memories_added;
            stats.last_risks = stats.risk_mitigations;
            stats.last_citations = stats.citations;

            let action_raw = edges_delta as f64 * 0.6
                + tasks_delta as f64 * 0.6
                + memories_delta as f64 * 0.2
                + risks_delta as f64 * 0.8
                + citations_delta as f64 * 0.5
                + degree_delta as f64 * 0.4;
            let individual_score = (action_raw / 5.0).min(1.0);

            // ── Collective contribution delta ──
            let reuse_delta = stats
                .traces_reused_by_others
                .saturating_sub(stats.last_traces_reused);
            let survival_count = stats.surviving_edges; // absolute, not delta
            let cascade_delta = stats
                .cascade_citations
                .saturating_sub(stats.last_cascade_citations);
            let downstream_delta = stats
                .downstream_successes
                .saturating_sub(stats.last_downstream_successes);

            stats.last_traces_reused = stats.traces_reused_by_others;
            stats.last_cascade_citations = stats.cascade_citations;
            stats.last_downstream_successes = stats.downstream_successes;
            // surviving_edges is recomputed each tick, no delta needed

            // Weighted collective signals:
            //   trace reuse (0.35) — strongest: someone literally built on your work
            //   surviving edges (0.25) — durable knowledge contribution
            //   cascade citations (0.25) — your citation led to real positive outcome
            //   downstream success (0.15) — connected agents succeeded
            let collective_raw = reuse_delta as f64 * 0.35
                + (survival_count as f64 / 3.0).min(1.0) * 0.25
                + cascade_delta as f64 * 0.25
                + downstream_delta as f64 * 0.15;
            let collective_score = collective_raw.min(1.0);

            scores.push((idx, individual_score, collective_score));
        }

        // Apply JW with collective-first weighting
        let coherence_score = (coherence_improvement * 2.0).min(1.0);
        let stability_score = (1.0 - coherence_delta).clamp(0.0, 1.0);

        for (idx, individual, collective) in scores {
            self.agents[idx].jw = (0.30 * individual
                + 0.40 * collective
                + 0.20 * coherence_score
                + 0.10 * stability_score)
                .clamp(0.0, 1.0);
        }
    }

    /// Mean JW across all agents — global agentic economy health indicator.
    pub fn global_jw(&self) -> f64 {
        if self.agents.is_empty() {
            return 0.0;
        }
        self.agents.iter().map(|a| a.jw).sum::<f64>() / self.agents.len() as f64
    }

    fn decay_edges(&mut self) {
        for edge in &mut self.edges {
            edge.weight *= 1.0 - self.decay_rate;
            edge.age += 1;
        }
        self.edges.retain(|e| e.weight > 0.01);
    }

    /// Stigmergic reinforcement: edges between agents with aligned drives gain weight each tick.
    /// This counteracts pure decay and allows coherence to rise when the system is healthy.
    fn reinforce_edges(&mut self) {
        // Build agent id → vec index map for O(1) lookup regardless of id values
        let id_to_idx: std::collections::HashMap<u64, usize> = self
            .agents
            .iter()
            .enumerate()
            .map(|(idx, a)| (a.id, idx))
            .collect();

        for edge in &mut self.edges {
            if edge.participants.len() < 2 {
                continue;
            }
            // Compute mean pairwise drive alignment across all participant pairs
            let mut total_alignment = 0.0f64;
            let mut pair_count = 0usize;
            for i in 0..edge.participants.len() {
                for j in (i + 1)..edge.participants.len() {
                    if let (Some(&ai), Some(&aj)) = (
                        id_to_idx.get(&edge.participants[i]),
                        id_to_idx.get(&edge.participants[j]),
                    ) {
                        let a = &self.agents[ai];
                        let b = &self.agents[aj];
                        // Alignment = 1 - normalised drive distance (curiosity + harmony + growth)
                        let dist = ((a.drives.curiosity - b.drives.curiosity).powi(2)
                            + (a.drives.harmony - b.drives.harmony).powi(2)
                            + (a.drives.growth - b.drives.growth).powi(2))
                        .sqrt()
                            / 1.732; // max possible distance = sqrt(3)
                        total_alignment += 1.0 - dist;
                        pair_count += 1;
                    }
                }
            }
            if pair_count == 0 {
                continue;
            }
            let alignment = total_alignment / pair_count as f64; // 0..1
                                                                 // Reinforce proportional to alignment, scaled so a perfectly aligned edge
                                                                 // exactly offsets the default 2% decay at alignment=1.0.
                                                                 // At alignment=0.5 the edge is neutral (decay ≈ growth).
                                                                 // Below 0.5 the edge still slowly decays — low-alignment edges die naturally.
            let reinforce = self.decay_rate * 2.0 * alignment;
            edge.weight = (edge.weight + reinforce).min(2.0); // cap at 2.0 to avoid runaway
        }
    }

    /// Genuine agent interaction: agents connected by strong edges (weight > 0.8) nudge
    /// each other's drives toward their shared mean — stigmergic drive convergence.
    ///
    /// This is the indirect interaction mechanism of hyper-stigmergy: agents don't send
    /// messages; instead the shared environment (edge weights) mediates their influence.
    /// Strong edges = strong coupling = drives converge. Weak edges = independence.
    ///
    /// Rate: 1% of the gap per tick (slow, stable convergence).
    fn interact_agents(&mut self) {
        const INTERACTION_THRESHOLD: f64 = 0.8;
        const CONVERGENCE_RATE: f64 = 0.01;

        // Build id → index map
        let id_to_idx: std::collections::HashMap<u64, usize> = self
            .agents
            .iter()
            .enumerate()
            .map(|(idx, a)| (a.id, idx))
            .collect();

        // Accumulate drive deltas — apply after all edges are processed to avoid
        // order-dependent mutation within a single tick.
        let mut curiosity_delta = vec![0.0f64; self.agents.len()];
        let mut harmony_delta = vec![0.0f64; self.agents.len()];
        let mut growth_delta = vec![0.0f64; self.agents.len()];
        let mut transcend_delta = vec![0.0f64; self.agents.len()];
        let mut interaction_count = vec![0usize; self.agents.len()];

        for edge in &self.edges {
            if edge.weight < INTERACTION_THRESHOLD {
                continue;
            }
            if edge.participants.len() < 2 {
                continue;
            }

            // Collect the indices of agents on this edge
            let indices: Vec<usize> = edge
                .participants
                .iter()
                .filter_map(|id| id_to_idx.get(id).copied())
                .collect();
            if indices.len() < 2 {
                continue;
            }

            // Compute mean drives across participants
            let n = indices.len() as f64;
            let mean_c = indices
                .iter()
                .map(|&i| self.agents[i].drives.curiosity)
                .sum::<f64>()
                / n;
            let mean_h = indices
                .iter()
                .map(|&i| self.agents[i].drives.harmony)
                .sum::<f64>()
                / n;
            let mean_g = indices
                .iter()
                .map(|&i| self.agents[i].drives.growth)
                .sum::<f64>()
                / n;
            let mean_t = indices
                .iter()
                .map(|&i| self.agents[i].drives.transcendence)
                .sum::<f64>()
                / n;

            // Edge strength scales the convergence (stronger edge = faster pull)
            let strength = (edge.weight - INTERACTION_THRESHOLD).min(1.0);

            for &idx in &indices {
                let a = &self.agents[idx];
                curiosity_delta[idx] += (mean_c - a.drives.curiosity) * CONVERGENCE_RATE * strength;
                harmony_delta[idx] += (mean_h - a.drives.harmony) * CONVERGENCE_RATE * strength;
                growth_delta[idx] += (mean_g - a.drives.growth) * CONVERGENCE_RATE * strength;
                transcend_delta[idx] +=
                    (mean_t - a.drives.transcendence) * CONVERGENCE_RATE * strength;
                interaction_count[idx] += 1;
            }
        }

        // Apply accumulated deltas
        for (idx, agent) in self.agents.iter_mut().enumerate() {
            if interaction_count[idx] == 0 {
                continue;
            }
            agent.drives.curiosity =
                (agent.drives.curiosity + curiosity_delta[idx]).clamp(0.01, 3.0);
            agent.drives.harmony = (agent.drives.harmony + harmony_delta[idx]).clamp(0.01, 3.0);
            agent.drives.growth = (agent.drives.growth + growth_delta[idx]).clamp(0.01, 3.0);
            agent.drives.transcendence =
                (agent.drives.transcendence + transcend_delta[idx]).clamp(0.01, 3.0);
        }
    }

    fn tune_threshold(&mut self) {
        if !self.adaptive_threshold {
            return;
        }
        let num_agents = self.agents.len().max(1) as f64;
        let num_edges = self.edges.len() as f64;
        let density = num_edges / num_agents;
        self.adaptive_config.history.push(density);
        if self.adaptive_config.history.len() > self.adaptive_config.density_window {
            self.adaptive_config.history.remove(0);
        }
    }

    pub fn get_vertex_properties(&self, v_idx: usize) -> Vec<String> {
        self.adjacency
            .get(&(v_idx as u64))
            .map(|edges| {
                edges
                    .iter()
                    .filter_map(|&eidx| self.edges.get(eidx))
                    .filter(|e| e.tags.get("type") == Some(&"has_property".to_string()))
                    .filter_map(|e| e.tags.get("property").cloned())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn calculate_embedding_coverage(&self) -> f32 {
        if self.vertex_meta.is_empty() {
            return 0.0;
        }
        let embedded = self
            .vertex_meta
            .iter()
            .filter(|v| v.embedding.is_some())
            .count();
        embedded as f32 / self.vertex_meta.len() as f32
    }

    pub fn calculate_clustering_coefficient(&self) -> f64 {
        let mut total_coeff = 0.0;
        let n = self.agents.len();

        for agent in &self.agents {
            if let Some(neighbors) = self.adjacency.get(&agent.id) {
                let k = neighbors.len();
                if k < 2 {
                    continue;
                }

                let neighbor_ids: Vec<u64> = neighbors
                    .iter()
                    .filter_map(|&eidx| self.edges.get(eidx))
                    .flat_map(|e| e.participants.iter().copied())
                    .filter(|&id| id != agent.id)
                    .collect();

                let mut triangles = 0;
                for i in 0..neighbor_ids.len() {
                    for j in (i + 1)..neighbor_ids.len() {
                        if self.are_connected(neighbor_ids[i], neighbor_ids[j]) {
                            triangles += 1;
                        }
                    }
                }

                let possible = k * (k - 1) / 2;
                if possible > 0 {
                    total_coeff += triangles as f64 / possible as f64;
                }
            }
        }

        if n > 0 {
            total_coeff / n as f64
        } else {
            0.0
        }
    }

    fn are_connected(&self, a: u64, b: u64) -> bool {
        self.adjacency
            .get(&a)
            .map(|edges| {
                edges.iter().any(|&eidx| {
                    self.edges
                        .get(eidx)
                        .map(|e| e.participants.contains(&b))
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false)
    }

    pub fn bias_drives(&mut self, bias: &HashMap<String, f32>) {
        for agent in &mut self.agents {
            if let Some(&factor) = bias.get("curiosity") {
                agent.drives.curiosity *= factor as f64;
            }
            if let Some(&factor) = bias.get("harmony") {
                agent.drives.harmony *= factor as f64;
            }
            if let Some(&factor) = bias.get("growth") {
                agent.drives.growth *= factor as f64;
            }
            if let Some(&factor) = bias.get("transcendence") {
                agent.drives.transcendence *= factor as f64;
            }

            agent.drives.curiosity = agent.drives.curiosity.clamp(0.0, 3.0);
            agent.drives.harmony = agent.drives.harmony.clamp(0.0, 3.0);
            agent.drives.growth = agent.drives.growth.clamp(0.0, 3.0);
            agent.drives.transcendence = agent.drives.transcendence.clamp(0.0, 3.0);
        }
    }
}

fn ensure_vertex_slot(vertices: &mut Vec<VertexMeta>, idx: usize, meta: VertexMeta) {
    if vertices.len() <= idx {
        vertices.resize(
            idx + 1,
            VertexMeta {
                kind: VertexKind::Memory,
                name: String::new(),
                created_at: 0,
                modified_at: 0,
                drift_count: 0,
                embedding: None,
                origin_system: None,
            },
        );
    }
    vertices[idx] = meta;
}

fn parse_role(raw: &str) -> Role {
    match raw {
        "Architect" => Role::Architect,
        "Catalyst" => Role::Catalyst,
        "Chronicler" => Role::Chronicler,
        "Critic" => Role::Critic,
        "Explorer" => Role::Explorer,
        "Coder" => Role::Coder,
        _ => Role::Architect,
    }
}

fn parse_vertex_kind(raw: &str) -> Option<VertexKind> {
    match raw {
        "Agent" => Some(VertexKind::Agent),
        "Tool" => Some(VertexKind::Tool),
        "Memory" => Some(VertexKind::Memory),
        "Task" => Some(VertexKind::Task),
        "Property" => Some(VertexKind::Property),
        "Ontology" => Some(VertexKind::Ontology),
        "Belief" => Some(VertexKind::Belief),
        "Experience" => Some(VertexKind::Experience),
        _ => None,
    }
}

fn parse_data_sensitivity(raw: &str) -> DataSensitivity {
    match raw {
        "Public" => DataSensitivity::Public,
        "Internal" => DataSensitivity::Internal,
        "Confidential" => DataSensitivity::Confidential,
        "Secret" => DataSensitivity::Secret,
        _ => DataSensitivity::Internal,
    }
}

fn parse_composite_fact_kind(raw: &str) -> CompositeFactKind {
    match raw {
        "Fact" => CompositeFactKind::Fact,
        "Event" => CompositeFactKind::Event,
        "Promise" => CompositeFactKind::Promise,
        "Delegation" => CompositeFactKind::Delegation,
        "Narrative" => CompositeFactKind::Narrative,
        _ => CompositeFactKind::Fact,
    }
}

fn parse_delegation_status(raw: &str) -> DelegationStatus {
    match raw {
        "Proposed" => DelegationStatus::Proposed,
        "Accepted" => DelegationStatus::Accepted,
        "Completed" => DelegationStatus::Completed,
        "Failed" => DelegationStatus::Failed,
        "Cancelled" => DelegationStatus::Cancelled,
        _ => DelegationStatus::Proposed,
    }
}

fn parse_slot_bindings(raw: Vec<String>) -> Vec<FactSlotBinding> {
    raw.into_iter()
        .map(|value| {
            let mut parts = value.splitn(3, '|');
            let role = parts.next().unwrap_or_default().to_string();
            let slot_value = parts.next().unwrap_or_default().to_string();
            let entity_ref = parts
                .next()
                .and_then(|entity| (!entity.is_empty()).then(|| entity.to_string()));
            FactSlotBinding {
                role,
                value: slot_value,
                entity_ref,
            }
        })
        .collect()
}

fn escape_cypher_value(value: &str) -> String {
    value.replace('\'', " ")
}

fn normalize_council_id_fragment(value: &str) -> String {
    value
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect::<String>()
}

fn get_string(props: &HashMap<String, PropertyValue>, key: &str) -> Option<String> {
    match props.get(key)? {
        PropertyValue::String(v) => Some(v.clone()),
        _ => None,
    }
}

fn get_int(props: &HashMap<String, PropertyValue>, key: &str) -> Option<i64> {
    match props.get(key)? {
        PropertyValue::Integer(v) => Some(*v),
        _ => None,
    }
}

fn get_float(props: &HashMap<String, PropertyValue>, key: &str) -> Option<f64> {
    match props.get(key)? {
        PropertyValue::Float(v) => Some(*v),
        PropertyValue::Integer(v) => Some(*v as f64),
        _ => None,
    }
}

fn get_bool(props: &HashMap<String, PropertyValue>, key: &str) -> Option<bool> {
    match props.get(key)? {
        PropertyValue::Boolean(v) => Some(*v),
        _ => None,
    }
}

fn get_string_list(props: &HashMap<String, PropertyValue>, key: &str) -> Vec<String> {
    match props.get(key) {
        Some(PropertyValue::StringList(v)) => v.clone(),
        Some(PropertyValue::String(v)) => vec![v.clone()],
        _ => Vec::new(),
    }
}

fn parse_tags(items: Vec<String>) -> HashMap<String, String> {
    items
        .into_iter()
        .filter_map(|item| {
            item.split_once('=')
                .map(|(k, v)| (k.to_string(), v.to_string()))
        })
        .collect()
}

// === RLM BRIDGE ===
impl HyperStigmergicMorphogenesis {
    /// Semantic grep - find agents by embedding similarity
    pub fn colgrep_search(&self, query_embedding: Vec<f32>, threshold: f32) -> Vec<(AgentId, f32)> {
        self.agents
            .iter()
            .filter_map(|agent| {
                self.deep_memory
                    .get(&(agent.id as usize))
                    .map(|agent_emb| {
                        let row_vec: Vec<f32> = agent_emb.row(0).iter().cloned().collect();
                        let similarity = cosine_similarity(&query_embedding, &row_vec);
                        (agent.id, similarity)
                    })
                    .filter(|(_, sim)| *sim >= threshold)
            })
            .collect()
    }

    /// Get environment state for RLM queries
    pub fn get_environment_state(&self) -> EnvironmentState {
        EnvironmentState {
            tick_count: self.tick_count,
            agent_count: self.agents.len(),
            edge_count: self.edges.len(),
            coherence: self.global_coherence(),
            property_keys: self.property_vertices.keys().cloned().collect(),
            embedding_coverage: self.calculate_embedding_coverage(),
            recent_improvements: self.improvement_history.len(),
        }
    }

    /// Apply action from RLM with bid-based role selection
    pub fn apply_rlm_action(
        &mut self,
        action: &RlmAction,
        bid_config: &BidConfig,
    ) -> Result<Context, String> {
        match action {
            RlmAction::ExecuteAction(act) => {
                let selected_role = self.select_role_via_bidding(bid_config);
                println!("  Role selected via bidding: {:?}", selected_role);
                self.apply_action(act);
                Ok(Context::ActionExecuted(format!(
                    "{:?} by {:?}",
                    act, selected_role
                )))
            }
            RlmAction::QueryEnvironment(q) => {
                let state = self.get_environment_state();
                Ok(Context::Text(format!("Query '{}' | State: {:?}", q, state)))
            }
            RlmAction::PredictCoherence { context } => {
                let prediction = self.predict_coherence(context);
                Ok(Context::Prediction(prediction))
            }
            RlmAction::ComputeNovelty { proposal } => {
                let novelty = self.compute_novelty(proposal);
                Ok(Context::NoveltyScore(novelty))
            }
            RlmAction::SelfImprove { intent } => {
                let result = self.execute_self_improvement_cycle(intent);
                Ok(Context::ImprovementResult(result))
            }
            _ => Err("Action not handled by environment".to_string()),
        }
    }
}

// === FEATURE B: EMBEDDING-BACKED REASONING ===
impl HyperStigmergicMorphogenesis {
    /// B. Real embedding-backed predict_coherence using cosine similarity patterns
    pub fn predict_coherence(&self, context: &str) -> f32 {
        let context_emb = self.get_or_create_embedding(context);
        let similar_contexts = self.embedding_index.search(&context_emb, 5);

        if similar_contexts.is_empty() {
            return self.structural_coherence_prediction();
        }

        let mut total_weight = 0.0;
        let mut weighted_coherence = 0.0;

        for (idx, sim) in &similar_contexts {
            if let Some(event) = self.improvement_history.get(*idx) {
                let weight = (*sim).exp();
                weighted_coherence += event.coherence_after as f32 * weight;
                total_weight += weight;
            }
        }

        if total_weight > 0.0 {
            (weighted_coherence / total_weight).clamp(0.0, 1.0)
        } else {
            self.structural_coherence_prediction()
        }
    }

    /// B. Compute novelty as 1 - max_similarity (coverage proxy)
    pub fn compute_novelty(&self, proposal: &str) -> f32 {
        let proposal_emb = self.get_or_create_embedding(proposal);

        let mut max_sim = 0.0f32;
        for (_idx, meta) in self.vertex_meta.iter().enumerate() {
            if let Some(existing_emb) = &meta.embedding {
                let sim = cosine_similarity(&proposal_emb, existing_emb);
                max_sim = max_sim.max(sim);
            }
        }

        for (idx, _meta) in self.vertex_meta.iter().enumerate() {
            if let Some(deep_emb) = self.deep_memory.get(&idx) {
                let row_vec: Vec<f32> = deep_emb.row(0).iter().cloned().collect();
                let sim = cosine_similarity(&proposal_emb, &row_vec);
                max_sim = max_sim.max(sim);
            }
        }

        for entry in self.ontology.values() {
            if let Some(ont_emb) = &entry.embedding {
                let sim = cosine_similarity(&proposal_emb, ont_emb);
                max_sim = max_sim.max(sim);
            }
        }

        (1.0 - max_sim).clamp(0.0, 1.0)
    }

    fn structural_coherence_prediction(&self) -> f32 {
        let density = if self.agents.len() > 1 {
            self.edges.len() as f32
                / (self.agents.len() * (self.agents.len() - 1) / 2).max(1) as f32
        } else {
            0.0
        };
        let recency = 1.0 - (self.tick_count as f32 / (self.tick_count + 1000) as f32);
        (density * 0.5 + recency * 0.5).clamp(0.0, 1.0)
    }

    /// Get or create a deterministic embedding from text.
    /// In production, call Ollama nomic-embed-text; here we use hash-based.
    pub fn get_or_create_embedding(&self, text: &str) -> Vec<f32> {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        text.hash(&mut hasher);
        let seed = hasher.finish();

        let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
        let normal = Normal::new(0.0f32, 1.0f32).unwrap();
        let mut vec: Vec<f32> = (0..EMBEDDING_DIM)
            .map(|_| normal.sample(&mut rng))
            .collect();

        let norm = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            vec.iter_mut().for_each(|x| *x /= norm);
        }
        vec
    }
}

// === FEATURE C: SOFTMAX BIDDING ===
impl HyperStigmergicMorphogenesis {
    /// C. Softmax sampling + role bias for bidding
    pub fn select_role_via_bidding(&self, config: &BidConfig) -> Role {
        use rand::distributions::WeightedIndex;
        use rand::thread_rng;

        let roles = vec![
            Role::Architect,
            Role::Catalyst,
            Role::Chronicler,
            Role::Critic,
            Role::Explorer,
            Role::Coder,
        ];
        let mut bids: Vec<f64> = Vec::new();

        for role in &roles {
            let base_score = match role {
                Role::Architect => {
                    let coherence = self.global_coherence();
                    (1.5 - coherence).max(0.1) * config.architect_bias
                }
                Role::Catalyst => {
                    let stagnation = self.detect_stagnation_score();
                    stagnation * config.catalyst_bias
                }
                Role::Chronicler => {
                    let history_factor = (self.improvement_history.len() as f64 / 100.0).min(1.0);
                    history_factor * config.chronicler_bias
                }
                Role::Critic => {
                    // Critic activates more when there are unresolved belief conflicts
                    let conflict_factor = self
                        .beliefs
                        .iter()
                        .filter(|b| !b.contradicting_evidence.is_empty())
                        .count() as f64
                        / (self.beliefs.len().max(1) as f64);
                    (0.5 + conflict_factor).min(1.5)
                }
                Role::Explorer => {
                    // Explorer activates when system is stable (needs novelty injection)
                    let coherence = self.global_coherence();
                    coherence * 1.2 // higher coherence = more exploration needed
                }
                Role::Coder => {
                    // Coder activates when there are tasks that might involve code/tool usage
                    let tool_vertex_count = self
                        .vertex_meta
                        .iter()
                        .filter(|v| v.kind == VertexKind::Tool)
                        .count() as f64;
                    let tool_factor = (tool_vertex_count / 10.0).min(1.0);
                    0.6 + tool_factor * 0.4 // base interest + tool availability
                }
            };

            let skill_bias = self.skill_role_bias(role);
            let noise: f64 = rand::random::<f64>() * config.exploration_temperature;
            bids.push((base_score * skill_bias) + noise);
        }

        // Softmax
        let max_bid = bids.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let exp_bids: Vec<f64> = bids.iter().map(|b| (b - max_bid).exp()).collect();
        let sum_exp: f64 = exp_bids.iter().sum();
        let probs: Vec<f64> = exp_bids.iter().map(|e| e / sum_exp).collect();

        let dist = WeightedIndex::new(&probs).unwrap();
        let mut rng = thread_rng();
        roles[Distribution::sample(&dist, &mut rng)]
    }

    fn skill_role_bias(&self, role: &Role) -> f64 {
        let mut general_bonus = 0.0;
        if !self.skill_bank.general_skills.is_empty() {
            let sum: f64 = self
                .skill_bank
                .general_skills
                .iter()
                .map(|s| s.confidence)
                .sum();
            general_bonus =
                (sum / self.skill_bank.general_skills.len() as f64).clamp(0.0, 1.0) * 0.15;
        }

        let role_key = format!("{:?}", role);
        let mut role_bonus = 0.0;
        if let Some(skills) = self.skill_bank.role_skills.get(&role_key) {
            if !skills.is_empty() {
                let sum: f64 = skills.iter().map(|s| s.confidence).sum();
                role_bonus = (sum / skills.len() as f64).clamp(0.0, 1.0) * 0.25;
            }
        }

        1.0 + general_bonus + role_bonus
    }

    fn detect_stagnation_score(&self) -> f64 {
        if self.adaptive_config.history.len() < 3 {
            return 0.5;
        }
        let recent = &self.adaptive_config.history[self.adaptive_config.history.len() - 3..];
        let mean = recent.iter().sum::<f64>() / recent.len() as f64;
        let variance = recent.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / recent.len() as f64;
        (1.0 / (1.0 + variance * 10.0)).min(1.0)
    }
}

// === FEATURE D: RECURSIVE SUPERVISION LOOP (API: self-improvement cycle) ===
impl HyperStigmergicMorphogenesis {
    fn push_decision_record(&mut self, rec: DecisionRecord) {
        const MAX: usize = 10_000;
        self.decision_log.push(rec);
        if self.decision_log.len() > MAX {
            let drop_n = self.decision_log.len() - MAX;
            self.decision_log.drain(..drop_n);
        }
    }

    /// One bounded supervision iteration: **propose** candidate mutations, **review** them with
    /// simulation (and intent scoring), **record** the outcome in `improvement_history` and the
    /// embedding index so the next step is **conditioned** on prior passes — not open-ended
    /// self-modification.
    ///
    /// Integration 3: Mutation intent is scored before generating mutations.
    /// Vague intents are rewritten using keyword heuristics.
    ///
    /// Guardrails: multi-objective ranking (`GuardrailWeights`), ε exploration, optional freeze
    /// (`HSM_FREEZE_MUTATIONS`), and `decision_log` audit rows.
    pub fn execute_self_improvement_cycle(&mut self, intent: &str) -> ImprovementResult {
        let coherence_before = self.global_coherence();
        let timestamp = Self::current_timestamp();

        let freeze = matches!(
            std::env::var("HSM_FREEZE_MUTATIONS").as_deref(),
            Ok("1" | "true" | "yes")
        );
        if freeze {
            return ImprovementResult {
                success: false,
                coherence_delta: 0.0,
                mutation_applied: None,
                event: ImprovementEvent {
                    timestamp,
                    intent: intent.to_string(),
                    mutation_type: MutationType::ParameterTuning,
                    coherence_before,
                    coherence_after: coherence_before,
                    novelty_score: 0.0,
                    applied: false,
                    score_coherence_term: 0.0,
                    score_novelty_term: 0.0,
                    score_dissent_term: 0.0,
                    composite_objective: 0.0,
                    exploration_pick: false,
                },
            };
        }

        let (intent_score, rewritten_intent) = Self::evaluate_intent_fast(intent);
        let final_intent = rewritten_intent.as_deref().unwrap_or(intent);

        if intent_score < 0.6 {
            eprintln!(
                "[optimize_anything] Low-quality mutation intent (score={:.2}): {}",
                intent_score, intent
            );
            if let Some(ref rewritten) = rewritten_intent {
                eprintln!("[optimize_anything] Rewritten to: {}", rewritten);
            }
        }

        let candidates = self.generate_mutations(final_intent);
        if candidates.is_empty() {
            let event = ImprovementEvent {
                timestamp,
                intent: intent.to_string(),
                mutation_type: MutationType::ParameterTuning,
                coherence_before,
                coherence_after: coherence_before,
                novelty_score: 0.0,
                applied: false,
                score_coherence_term: 0.0,
                score_novelty_term: 0.0,
                score_dissent_term: 0.0,
                composite_objective: 0.0,
                exploration_pick: false,
            };
            self.improvement_history.push(event.clone());
            let intent_emb = self.get_or_create_embedding(intent);
            self.embedding_index
                .insert(self.improvement_history.len() - 1, intent_emb);
            return ImprovementResult {
                success: false,
                coherence_delta: 0.0,
                mutation_applied: None,
                event,
            };
        }

        let weights = crate::world_guardrails::GuardrailWeights::from_env();
        let mut scored: Vec<(MutationType, f32, f32)> = Vec::with_capacity(candidates.len());
        for mutation in candidates {
            let sim = self.simulate_mutation(&mutation) as f32;
            let nov = self.compute_novelty(&format!("{:?}", mutation));
            scored.push((mutation, sim, nov));
        }
        let sims: Vec<f32> = scored.iter().map(|(_, s, _)| *s).collect();
        let diss = crate::world_guardrails::dissent_from_simulations(&sims);

        let mut best_i = 0usize;
        let mut best_obj = f32::NEG_INFINITY;
        for (i, (_, sim, nov)) in scored.iter().enumerate() {
            let obj = weights.composite_rank(*sim, *nov, diss);
            if obj > best_obj {
                best_obj = obj;
                best_i = i;
            }
        }

        let use_random = rand::thread_rng()
            .gen_bool(crate::world_guardrails::exploration_epsilon());
        let pick_i = if use_random {
            rand::thread_rng().gen_range(0..scored.len())
        } else {
            best_i
        };

        let (picked_mut, sim_c, nov_c) = &scored[pick_i];
        let composite = weights.composite_rank(*sim_c, *nov_c, diss);
        let final_mutation_type = picked_mut.clone();

        self.apply_mutation(picked_mut);
        let coherence_after = self.global_coherence();
        let applied = true;

        let event = ImprovementEvent {
            timestamp,
            intent: intent.to_string(),
            mutation_type: final_mutation_type.clone(),
            coherence_before,
            coherence_after,
            novelty_score: *nov_c,
            applied,
            score_coherence_term: *sim_c,
            score_novelty_term: *nov_c,
            score_dissent_term: diss,
            composite_objective: composite,
            exploration_pick: use_random,
        };

        self.push_decision_record(DecisionRecord {
            timestamp,
            kind: "supervision_mutation".to_string(),
            tick_count: self.tick_count,
            coherence_snapshot: coherence_before,
            alternatives: scored.len(),
            picked_exploration: use_random,
            score_coherence_term: *sim_c,
            score_novelty_term: *nov_c,
            score_dissent_term: diss,
            composite_objective: composite,
            intent_summary: intent.chars().take(200).collect(),
        });

        self.improvement_history.push(event.clone());
        let intent_emb = self.get_or_create_embedding(intent);
        self.embedding_index
            .insert(self.improvement_history.len() - 1, intent_emb);

        ImprovementResult {
            success: applied,
            coherence_delta: coherence_after - coherence_before,
            mutation_applied: Some(format!("{:?}", final_mutation_type)),
            event,
        }
    }
    /// Fast synchronous intent evaluation using keyword heuristics.
    /// Returns (score 0-1, rewritten_intent if score < 0.6).
    ///
    /// This is Integration 3: Mutation Intent Scoring - synchronous version
    /// for use within the non-async execute_self_improvement_cycle.
    fn evaluate_intent_fast(intent: &str) -> (f64, Option<String>) {
        let intent_lower = intent.to_lowercase();
        let mut score = 0.5f64; // Start neutral

        // Positive indicators (specific, measurable qualities)
        let positive_keywords = [
            "specific",
            "measure",
            "metric",
            "threshold",
            "target",
            "increase",
            "decrease",
            "optimize",
            "reduce",
            "improve",
            "by ",
            "%",
            "percent",
            "ratio",
            "count",
            "density",
            "connect",
            "link",
            "weight",
            "balance",
            "distribute",
        ];

        // Negative indicators (vague, unmeasurable qualities)
        let negative_keywords = [
            "maybe",
            "try",
            "unclear",
            "something",
            "somehow",
            "better",
            "nice",
            "good",
            "bad",
            "probably",
            "perhaps",
            "might",
            "could",
            "should",
            "would",
        ];

        // Score adjustments
        for keyword in &positive_keywords {
            if intent_lower.contains(keyword) {
                score += 0.08;
            }
        }

        for keyword in &negative_keywords {
            if intent_lower.contains(keyword) {
                score -= 0.15;
            }
        }

        // Clamp to valid range
        score = score.clamp(0.0, 1.0);

        // If score is low, generate a rewritten version
        let rewritten = if score < 0.6 {
            // Simple rewrite: replace vague words with more specific alternatives
            let mut improved = intent.to_string();
            let replacements = [
                ("maybe", "quantifiably"),
                ("try", "implement"),
                ("unclear", "measurable"),
                ("something", "a specific mechanism"),
                ("somehow", "through defined steps"),
                ("better", "improved by 10%"),
                ("good", "optimal"),
            ];

            for (from, to) in &replacements {
                improved = improved.replace(from, to);
            }

            // Add measurability if missing
            if !improved.contains("by") && !improved.contains("%") {
                improved.push_str(" (target: measurable improvement)");
            }

            Some(improved)
        } else {
            None
        };

        (score, rewritten)
    }

    fn generate_mutations(&self, _intent: &str) -> Vec<MutationType> {
        let mut mutations = vec![
            MutationType::ParameterTuning,
            MutationType::TopologyAdjustment,
        ];

        if self.edges.len() > 10 {
            mutations.push(MutationType::EdgeRewiring);
        }
        if self.ontology.len() > 5 {
            mutations.push(MutationType::OntologyExpansion);
        }
        if self.agents.len() > 3 {
            mutations.push(MutationType::VertexSplitting);
        }

        // Filter out mutation types that are actively avoided by the last reflection.
        // Keyword matching: avoid hints contain natural language; we check for
        // case-insensitive substrings matching mutation type names.
        if !self.avoid_hints.is_empty() {
            mutations.retain(|m| {
                let type_key = format!("{:?}", m).to_lowercase();
                !self.avoid_hints.iter().any(|hint| {
                    let h = hint.to_lowercase();
                    // Check both directions: hint mentions type key OR type key is in hint
                    h.contains(&type_key) || Self::mutation_matches_hint(m, &h)
                })
            });
            // Always keep at least one mutation to avoid a no-op cycle
            if mutations.is_empty() {
                mutations.push(MutationType::ParameterTuning);
            }
        }

        mutations
    }

    /// Returns true if the avoid hint is semantically relevant to the mutation type.
    fn mutation_matches_hint(mutation: &MutationType, hint_lower: &str) -> bool {
        match mutation {
            MutationType::EdgeRewiring => {
                hint_lower.contains("edge") || hint_lower.contains("rewir")
            }
            MutationType::VertexSplitting => {
                hint_lower.contains("vertex")
                    || hint_lower.contains("split")
                    || hint_lower.contains("agent")
            }
            MutationType::OntologyExpansion => {
                hint_lower.contains("ontolog") || hint_lower.contains("expan")
            }
            MutationType::TopologyAdjustment => {
                hint_lower.contains("topolog") || hint_lower.contains("connect")
            }
            MutationType::ParameterTuning => {
                hint_lower.contains("param") || hint_lower.contains("tun")
            }
        }
    }

    fn simulate_mutation(&self, mutation: &MutationType) -> f32 {
        let base_score = match mutation {
            MutationType::ParameterTuning => {
                let new_rate = self.decay_rate * 0.9;
                let predicted_coherence = self.global_coherence() * (1.0 + (0.05 - new_rate) * 2.0);
                predicted_coherence.clamp(0.0, 1.0) as f32
            }
            MutationType::TopologyAdjustment => (self.global_coherence() * 1.1).min(1.0) as f32,
            MutationType::EdgeRewiring => 0.75,
            MutationType::OntologyExpansion => 0.8,
            MutationType::VertexSplitting => 0.7,
        };

        // Apply avoid-hint penalty: each matching hint reduces score by 0.15 (clamped ≥ 0.05)
        if self.avoid_hints.is_empty() {
            return base_score;
        }
        let penalty: f32 = self
            .avoid_hints
            .iter()
            .filter(|hint| {
                let h = hint.to_lowercase();
                Self::mutation_matches_hint(mutation, &h)
            })
            .count() as f32
            * 0.15;
        (base_score - penalty).max(0.05)
    }

    fn apply_mutation(&mut self, mutation: &MutationType) {
        match mutation {
            MutationType::ParameterTuning => {
                self.decay_rate = (self.decay_rate * 0.95).clamp(
                    self.adaptive_config.min_decay_rate,
                    self.adaptive_config.max_decay_rate,
                );
            }
            MutationType::TopologyAdjustment => {
                if self.agents.len() >= 2 {
                    let a = rand::random::<usize>() % self.agents.len();
                    let b = (a + 1 + rand::random::<usize>() % (self.agents.len() - 1))
                        % self.agents.len();
                    self.edges.push(HyperEdge {
                        participants: vec![self.agents[a].id, self.agents[b].id],
                        weight: 1.0,
                        emergent: true,
                        age: 0,
                        tags: HashMap::from([("type".into(), "mutation_bridge".into())]),
                        created_at: self.tick_count,
                        embedding: None,
                        creator: None,
                        scope: None,
                        provenance: None,
                        trust_tags: None,
                        origin_system: None,
                        knowledge_layer: None,
                    });
                    self.rebuild_adjacency();
                }
            }
            MutationType::EdgeRewiring => {
                self.edges.retain(|e| e.weight > 0.1);
                self.rebuild_adjacency();
            }
            MutationType::OntologyExpansion => {
                let new_concept = format!("Emergent_{}", self.tick_count);
                self.add_ontology_entry(
                    &new_concept,
                    "instance".to_string(),
                    0.7,
                    vec!["Process".to_string()],
                );
            }
            MutationType::VertexSplitting => {
                if let Some((&agent_id, edges)) = self.adjacency.iter().max_by_key(|(_, e)| e.len())
                {
                    if edges.len() > 3 {
                        let new_id = self.agents.len() as u64;
                        let mut new_agent =
                            Agent::new(new_id, self.agents[agent_id as usize].drives.clone(), 0.05);
                        new_agent.role = Role::Catalyst;
                        self.agents.push(new_agent);
                    }
                }
            }
        }
    }
}

// === FEATURE E2: BELIEF & EXPERIENCE MANAGEMENT (Hindsight pattern) ===
impl HyperStigmergicMorphogenesis {
    pub fn record_agent_promise(
        &mut self,
        promiser: AgentId,
        beneficiary: Option<AgentId>,
        task_key: &str,
        summary: &str,
        sensitivity: DataSensitivity,
        due_by: Option<u64>,
    ) -> String {
        let now = Self::current_timestamp();
        let promise_id = self.social_memory.record_promise(
            promiser,
            beneficiary,
            task_key,
            summary,
            sensitivity.clone(),
            now,
            due_by,
        );
        self.stigmergic_memory.record_trace(
            promiser,
            "local-agent",
            Some(task_key),
            TraceKind::PromiseMade,
            summary,
            None,
            None,
            sensitivity,
            None,
            now,
            self.tick_count,
            HashMap::new(),
        );
        let promise_template = self.ensure_memory_template(
            "promise_commitment",
            "{promiser} promises {task} for {beneficiary}",
            vec!["promiser".into(), "task".into(), "beneficiary".into()],
        );
        self.add_composite_fact(
            CompositeFactKind::Promise,
            format!("Promise: {}", task_key),
            summary,
            Some(&promise_template),
            vec![
                FactSlotBinding {
                    role: "promiser".into(),
                    value: promiser.to_string(),
                    entity_ref: Some(format!("agent:{promiser}")),
                },
                FactSlotBinding {
                    role: "task".into(),
                    value: task_key.to_string(),
                    entity_ref: None,
                },
                FactSlotBinding {
                    role: "beneficiary".into(),
                    value: beneficiary
                        .map(|agent_id| agent_id.to_string())
                        .unwrap_or_else(|| "none".into()),
                    entity_ref: beneficiary.map(|agent_id| format!("agent:{agent_id}")),
                },
            ],
            TemporalSemantics {
                discovered_at: now,
                created_at: now,
                updated_at: now,
                valid_from: Some(now),
                valid_until: due_by,
                occurred_at: Some(now),
            },
            0.8,
            vec!["promise".into(), task_key.to_string()],
            Some(promise_id.clone()),
        );
        promise_id
    }

    pub fn resolve_agent_promise(
        &mut self,
        promise_id: &str,
        status: PromiseStatus,
        delivered_by: Option<AgentId>,
        quality_score: Option<f64>,
        met_deadline: Option<bool>,
        safe_for_sensitive_data: Option<bool>,
        collaborators: &[AgentId],
    ) -> bool {
        let promise_record = self.social_memory.promises.get(promise_id).cloned();
        let resolved = self
            .social_memory
            .resolve_promise(
                promise_id,
                status.clone(),
                delivered_by,
                Self::current_timestamp(),
                quality_score,
                met_deadline,
                safe_for_sensitive_data,
                collaborators,
            )
            .is_some();
        if resolved {
            let now = Self::current_timestamp();
            let agent_id = delivered_by.unwrap_or_default();
            self.stigmergic_memory.record_trace(
                agent_id,
                "local-agent",
                None,
                TraceKind::PromiseResolved,
                format!("promise {} resolved as {:?}", promise_id, status),
                Some(matches!(status, PromiseStatus::Kept)),
                quality_score,
                DataSensitivity::Internal,
                None,
                now,
                self.tick_count,
                HashMap::new(),
            );
            let outcome_template = self.ensure_memory_template(
                "promise_outcome",
                "{agent} resolved promise {promise} with outcome {outcome}",
                vec!["agent".into(), "promise".into(), "outcome".into()],
            );
            let outcome_fact_id = self.add_composite_fact(
                CompositeFactKind::Event,
                format!("Promise outcome: {promise_id}"),
                format!("promise {} resolved as {:?}", promise_id, status),
                Some(&outcome_template),
                vec![
                    FactSlotBinding {
                        role: "agent".into(),
                        value: agent_id.to_string(),
                        entity_ref: Some(format!("agent:{agent_id}")),
                    },
                    FactSlotBinding {
                        role: "promise".into(),
                        value: promise_id.to_string(),
                        entity_ref: Some(format!("promise:{promise_id}")),
                    },
                    FactSlotBinding {
                        role: "outcome".into(),
                        value: format!("{:?}", status),
                        entity_ref: None,
                    },
                ],
                TemporalSemantics {
                    discovered_at: now,
                    created_at: now,
                    updated_at: now,
                    valid_from: Some(now),
                    valid_until: None,
                    occurred_at: Some(now),
                },
                quality_score.unwrap_or(0.7),
                vec!["promise-outcome".into()],
                None,
            );
            if let Some(promise_fact) = self
                .composite_facts
                .iter()
                .find(|fact| fact.external_ref.as_deref() == Some(promise_id))
                .map(|fact| fact.id.clone())
            {
                self.add_recursive_fact_relation(
                    &outcome_fact_id,
                    &promise_fact,
                    if matches!(status, PromiseStatus::Kept) {
                        RecursiveRelationKind::Fulfills
                    } else {
                        RecursiveRelationKind::Violates
                    },
                    quality_score.unwrap_or(0.7),
                    "promise resolution linked back to original commitment",
                );
            }
            if let Some(record) = promise_record {
                if let Some(frame) = self
                    .delegation_frames
                    .iter_mut()
                    .find(|frame| frame.promise_id.as_deref() == Some(promise_id))
                {
                    frame.updated_at = now;
                    frame.status = if matches!(status, PromiseStatus::Kept) {
                        DelegationStatus::Completed
                    } else {
                        DelegationStatus::Failed
                    };
                    frame.outcome_fact_id = Some(outcome_fact_id.clone());
                }
                if delivered_by.is_some() && delivered_by != Some(record.promiser) {
                    self.record_delegation_frame(
                        Some(record.promiser),
                        delivered_by.unwrap_or(record.promiser),
                        &record.task_key,
                        "promise outcome implies delegated execution",
                        quality_score.unwrap_or(0.7),
                        Some(promise_id.to_string()),
                        if matches!(status, PromiseStatus::Kept) {
                            DelegationStatus::Completed
                        } else {
                            DelegationStatus::Failed
                        },
                    );
                }
            }
        }
        resolved
    }

    pub fn record_agent_delivery(
        &mut self,
        agent_id: AgentId,
        task_key: &str,
        success: bool,
        quality_score: f64,
        on_time: bool,
        safe_for_sensitive_data: bool,
        collaborators: &[AgentId],
    ) {
        self.social_memory.record_delivery(
            agent_id,
            task_key,
            success,
            quality_score,
            on_time,
            safe_for_sensitive_data,
            Self::current_timestamp(),
            collaborators,
        );
        self.stigmergic_memory.record_trace(
            agent_id,
            "local-agent",
            Some(task_key),
            TraceKind::DeliveryRecorded,
            format!("delivery recorded for {}", task_key),
            Some(success),
            Some(quality_score),
            if safe_for_sensitive_data {
                DataSensitivity::Internal
            } else {
                DataSensitivity::Confidential
            },
            None,
            Self::current_timestamp(),
            self.tick_count,
            HashMap::new(),
        );
    }

    pub fn set_agent_share_policy(
        &mut self,
        owner: AgentId,
        target: AgentId,
        max_sensitivity: DataSensitivity,
        min_security_score: f64,
        notes: Option<String>,
    ) {
        self.social_memory.set_share_policy(
            owner,
            target,
            max_sensitivity,
            min_security_score,
            notes,
            Self::current_timestamp(),
        );
    }

    pub fn can_agent_share(
        &self,
        owner: AgentId,
        target: AgentId,
        sensitivity: DataSensitivity,
    ) -> bool {
        if self.stigmergic_memory.is_agent_restricted(target)
            && sensitivity >= DataSensitivity::Confidential
        {
            return false;
        }
        self.social_memory.can_share(owner, target, sensitivity)
    }

    pub fn agent_reputation_score(&self, agent_id: AgentId) -> Option<f64> {
        let agent = self.agents.iter().find(|a| a.id == agent_id)?;
        Some(self.social_memory.reputation_score(agent_id, agent.jw))
    }

    pub fn recommend_delegate(
        &self,
        candidate_ids: &[AgentId],
        task_key: Option<&str>,
        requester: Option<AgentId>,
        sensitivity: Option<DataSensitivity>,
    ) -> Option<DelegationCandidate> {
        if let Some(task_key) = task_key {
            if let Some(preferred_agent) = self.stigmergic_memory.preferred_agent_for(task_key) {
                if candidate_ids.contains(&preferred_agent) {
                    return self.social_memory.recommend_delegate(
                        &self
                            .agents
                            .iter()
                            .filter(|a| a.id == preferred_agent)
                            .map(|a| (a.id, a.jw))
                            .collect::<Vec<_>>(),
                        Some(task_key),
                        requester,
                        sensitivity,
                    );
                }
            }
        }
        let candidates: Vec<(AgentId, f64)> = candidate_ids
            .iter()
            .filter_map(|agent_id| {
                self.agents
                    .iter()
                    .find(|a| a.id == *agent_id)
                    .map(|a| (*agent_id, a.jw))
            })
            .collect();
        self.social_memory
            .recommend_delegate(&candidates, task_key, requester, sensitivity)
    }

    pub fn create_fact_template(
        &mut self,
        label: impl Into<String>,
        narrative: impl Into<String>,
        slot_names: Vec<String>,
    ) -> String {
        let now = Self::current_timestamp();
        let template_id = format!("template-{}", self.next_fact_template_id);
        self.next_fact_template_id += 1;
        self.fact_templates.push(FactTemplate {
            id: template_id.clone(),
            label: label.into(),
            narrative: narrative.into(),
            slot_names,
            created_at: now,
            updated_at: now,
        });
        template_id
    }

    pub fn ensure_memory_template(
        &mut self,
        label: impl Into<String>,
        narrative: impl Into<String>,
        slot_names: Vec<String>,
    ) -> String {
        let label = label.into();
        if let Some(existing) = self
            .fact_templates
            .iter()
            .find(|template| template.label == label)
        {
            return existing.id.clone();
        }
        self.create_fact_template(label, narrative, slot_names)
    }

    pub fn add_composite_fact(
        &mut self,
        kind: CompositeFactKind,
        label: impl Into<String>,
        details: impl Into<String>,
        template_id: Option<&str>,
        slots: Vec<FactSlotBinding>,
        temporal: TemporalSemantics,
        confidence: f64,
        tags: Vec<String>,
        external_ref: Option<String>,
    ) -> String {
        let fact_id = format!("fact-{}", self.next_composite_fact_id);
        self.next_composite_fact_id += 1;
        self.composite_facts.push(CompositeFact {
            id: fact_id.clone(),
            kind,
            label: label.into(),
            details: details.into(),
            template_id: template_id.map(|s| s.to_string()),
            slots,
            temporal,
            confidence: confidence.clamp(0.0, 1.0),
            tags,
            external_ref,
        });
        fact_id
    }

    pub fn add_recursive_fact_relation(
        &mut self,
        from_fact_id: &str,
        to_fact_id: &str,
        kind: RecursiveRelationKind,
        confidence: f64,
        rationale: impl Into<String>,
    ) -> String {
        let relation_id = format!("fact-rel-{}", self.next_fact_relation_id);
        self.next_fact_relation_id += 1;
        self.recursive_fact_relations.push(RecursiveFactRelation {
            id: relation_id.clone(),
            from_fact_id: from_fact_id.to_string(),
            to_fact_id: to_fact_id.to_string(),
            kind,
            confidence: confidence.clamp(0.0, 1.0),
            rationale: rationale.into(),
            created_at: Self::current_timestamp(),
        });
        relation_id
    }

    pub fn record_delegation_frame(
        &mut self,
        requester: Option<AgentId>,
        delegated_to: AgentId,
        task_key: &str,
        rationale: impl Into<String>,
        confidence: f64,
        promise_id: Option<String>,
        status: DelegationStatus,
    ) -> String {
        let now = Self::current_timestamp();
        let delegation_id = format!("delegation-{}", self.next_delegation_frame_id);
        self.next_delegation_frame_id += 1;
        self.delegation_frames.push(DelegationFrame {
            id: delegation_id.clone(),
            task_key: task_key.to_string(),
            requester,
            delegated_to,
            rationale: rationale.into(),
            confidence: confidence.clamp(0.0, 1.0),
            promise_id,
            status,
            created_at: now,
            updated_at: now,
            outcome_fact_id: None,
        });
        delegation_id
    }

    pub fn record_tool_execution_evidence(
        &mut self,
        actor_id: AgentId,
        tool_name: &str,
        authority: &str,
        task_key: Option<&str>,
        success: bool,
        summary: &str,
        promise_id: Option<&str>,
        delegated_to: Option<AgentId>,
        output_preview: Option<&str>,
    ) -> String {
        let now = Self::current_timestamp();
        let template_id = self.ensure_memory_template(
            "tool_execution",
            "{actor} used {tool} under {authority} for {task}",
            vec![
                "actor".into(),
                "tool".into(),
                "authority".into(),
                "task".into(),
            ],
        );
        let details = match output_preview {
            Some(preview) if !preview.is_empty() => format!("{summary}\nOutput: {preview}"),
            _ => summary.to_string(),
        };
        let fact_id = self.add_composite_fact(
            CompositeFactKind::Event,
            format!("Tool execution: {tool_name}"),
            details,
            Some(&template_id),
            vec![
                FactSlotBinding {
                    role: "actor".into(),
                    value: actor_id.to_string(),
                    entity_ref: Some(format!("agent:{actor_id}")),
                },
                FactSlotBinding {
                    role: "tool".into(),
                    value: tool_name.to_string(),
                    entity_ref: None,
                },
                FactSlotBinding {
                    role: "authority".into(),
                    value: authority.to_string(),
                    entity_ref: None,
                },
                FactSlotBinding {
                    role: "task".into(),
                    value: task_key.unwrap_or("none").to_string(),
                    entity_ref: None,
                },
            ],
            TemporalSemantics {
                discovered_at: now,
                created_at: now,
                updated_at: now,
                valid_from: Some(now),
                valid_until: None,
                occurred_at: Some(now),
            },
            if success { 0.85 } else { 0.35 },
            vec![
                "tool-execution".into(),
                tool_name.to_string(),
                if success {
                    "success".into()
                } else {
                    "failure".into()
                },
            ],
            None,
        );

        if let Some(promise_id) = promise_id {
            if let Some(promise_fact) = self
                .composite_facts
                .iter()
                .find(|fact| fact.external_ref.as_deref() == Some(promise_id))
                .map(|fact| fact.id.clone())
            {
                self.add_recursive_fact_relation(
                    &fact_id,
                    &promise_fact,
                    RecursiveRelationKind::Supports,
                    if success { 0.8 } else { 0.45 },
                    "tool execution captured as promise/delegation evidence",
                );
            }
        }

        if let Some(delegate) = delegated_to {
            self.record_delegation_frame(
                Some(actor_id),
                delegate,
                task_key.unwrap_or(tool_name),
                format!("tool execution `{tool_name}` delegated under {authority}"),
                if success { 0.75 } else { 0.4 },
                promise_id.map(|value| value.to_string()),
                if success {
                    DelegationStatus::Completed
                } else {
                    DelegationStatus::Failed
                },
            );
        }

        fact_id
    }

    /// Add a belief with confidence scoring. If a similar belief exists, update it.
    pub fn add_belief(&mut self, content: &str, confidence: f64, source: BeliefSource) -> usize {
        self.add_belief_with_extras(content, confidence, source, AddBeliefExtras::default())
    }

    /// Add a belief with ownership / supersession / evidence linkage (conflict-aware provenance).
    pub fn add_belief_with_extras(
        &mut self,
        content: &str,
        mut confidence: f64,
        source: BeliefSource,
        extras: AddBeliefExtras,
    ) -> usize {
        let now = Self::current_timestamp();
        let user_provided = matches!(source, BeliefSource::UserProvided);

        if let Some(existing) = self.beliefs.iter_mut().find(|b| {
            content
                .split_whitespace()
                .filter(|w| w.len() > 3)
                .any(|w| b.content.to_lowercase().contains(&w.to_lowercase()))
                && b.content.len().abs_diff(content.len()) < content.len() / 2
        }) {
            existing.confidence = existing.confidence * 0.7 + confidence * 0.3;
            existing.update_count += 1;
            existing.updated_at = now;
            if confidence > 0.5 {
                existing.supporting_evidence.push(content.to_string());
            } else {
                existing.contradicting_evidence.push(content.to_string());
            }
            for ev in &extras.supporting_evidence {
                if !existing.supporting_evidence.contains(ev) {
                    existing.supporting_evidence.push(ev.clone());
                }
            }
            if let Some(ref ns) = extras.owner_namespace {
                existing.owner_namespace = Some(ns.clone());
            }
            if extras.supersedes_belief_id.is_some() {
                existing.supersedes_belief_id = extras.supersedes_belief_id;
            }
            for eid in &extras.evidence_belief_ids {
                if !existing.evidence_belief_ids.contains(eid) {
                    existing.evidence_belief_ids.push(*eid);
                }
            }
            if extras.human_committed {
                existing.human_committed = true;
            }
            if !existing.human_committed {
                let up = matches!(existing.source, BeliefSource::UserProvided);
                existing.confidence = crate::world_guardrails::apply_belief_explainability_cap(
                    existing.confidence,
                    existing.supporting_evidence.len(),
                    existing.evidence_belief_ids.len(),
                    up,
                );
            }
            self.world_state_generation = self.world_state_generation.wrapping_add(1);
            return existing.id;
        }

        if !extras.human_committed {
            confidence = crate::world_guardrails::apply_belief_explainability_cap(
                confidence,
                extras.supporting_evidence.len(),
                extras.evidence_belief_ids.len(),
                user_provided,
            );
        }

        let id = self.beliefs.len();
        let (l0, l1) = crate::memory::derive_hierarchy(content);
        let belief = Belief {
            id,
            content: content.to_string(),
            abstract_l0: Some(l0),
            overview_l1: Some(l1),
            confidence,
            source,
            supporting_evidence: extras.supporting_evidence.clone(),
            contradicting_evidence: Vec::new(),
            created_at: now,
            updated_at: now,
            update_count: 0,
            owner_namespace: extras.owner_namespace.clone(),
            supersedes_belief_id: extras.supersedes_belief_id,
            evidence_belief_ids: extras.evidence_belief_ids.clone(),
            human_committed: extras.human_committed,
        };

        self.vertex_meta.push(VertexMeta {
            kind: VertexKind::Belief,
            name: format!("belief_{}", id),
            created_at: now,
            modified_at: now,
            drift_count: 0,
            embedding: Some(self.get_or_create_embedding(content)),
            origin_system: None,
        });

        self.beliefs.push(belief);
        self.world_state_generation = self.world_state_generation.wrapping_add(1);
        id
    }

    /// Record an experience — what happened and what the outcome was
    pub fn record_experience(
        &mut self,
        description: &str,
        context: &str,
        outcome: ExperienceOutcome,
    ) -> usize {
        let now = Self::current_timestamp();
        let id = self.experiences.len();
        let embedding = self.get_or_create_embedding(description);

        let (l0, l1) = crate::memory::derive_hierarchy(description);
        let experience = Experience {
            id,
            description: description.to_string(),
            context: context.to_string(),
            abstract_l0: Some(l0),
            overview_l1: Some(l1),
            outcome,
            timestamp: now,
            tick: self.tick_count,
            embedding: Some(embedding.clone()),
        };

        // Create a vertex for this experience
        self.vertex_meta.push(VertexMeta {
            kind: VertexKind::Experience,
            name: format!("exp_{}", id),
            created_at: now,
            modified_at: now,
            drift_count: 0,
            embedding: Some(embedding.clone()),
            origin_system: None,
        });

        // Index for retrieval
        self.embedding_index.insert(10000 + id, embedding);

        self.experiences.push(experience);
        id
    }

    /// Decay belief confidence over time — beliefs that aren't reinforced fade
    pub fn decay_beliefs(&mut self) {
        for belief in &mut self.beliefs {
            let age = self.tick_count.saturating_sub(belief.updated_at);
            if age > 100 && belief.update_count < 3 {
                belief.confidence *= 0.995; // slow decay for unreinforced beliefs
            }
        }
        // Prune beliefs below threshold
        self.beliefs.retain(|b| b.confidence > 0.05);
    }

    /// Get top beliefs by confidence
    pub fn top_beliefs(&self, k: usize) -> Vec<&Belief> {
        let mut sorted: Vec<&Belief> = self.beliefs.iter().collect();
        sorted.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap());
        sorted.into_iter().take(k).collect()
    }

    /// Auto-generate beliefs from improvement history (observation-based)
    pub fn generate_beliefs_from_history(&mut self) {
        let recent: Vec<ImprovementEvent> = self
            .improvement_history
            .iter()
            .rev()
            .take(10)
            .cloned()
            .collect();

        if recent.is_empty() {
            return;
        }

        // Belief: which mutation types are most effective
        let mut type_scores: HashMap<String, (f64, usize)> = HashMap::new();
        for event in &recent {
            let key = format!("{:?}", event.mutation_type);
            let entry = type_scores.entry(key).or_insert((0.0, 0));
            entry.0 += event.coherence_after - event.coherence_before;
            entry.1 += 1;
        }

        for (mutation_type, (total_delta, count)) in &type_scores {
            let avg_delta = total_delta / *count as f64;
            let confidence = if avg_delta > 0.01 {
                0.8
            } else if avg_delta > 0.0 {
                0.5
            } else {
                0.2
            };
            let content = format!(
                "{} mutations produce avg coherence delta of {:+.4}",
                mutation_type, avg_delta
            );
            self.add_belief(&content, confidence, BeliefSource::Observation);
        }

        // Belief: overall system trajectory
        let total_delta: f64 = recent
            .iter()
            .map(|e| e.coherence_after - e.coherence_before)
            .sum();
        let direction = if total_delta > 0.05 {
            "improving"
        } else if total_delta < -0.05 {
            "degrading"
        } else {
            "stable"
        };
        self.add_belief(
            &format!(
                "System coherence is {} (recent delta: {:+.4})",
                direction, total_delta
            ),
            0.7,
            BeliefSource::Observation,
        );
    }
}

// === FEATURE E3: PARETO BID SELECTION (GEPA pattern) ===
impl HyperStigmergicMorphogenesis {
    /// Multi-objective Pareto-aware role selection
    /// Balances coherence need, novelty potential, and cost efficiency
    pub fn select_role_pareto(
        &self,
        config: &BidConfig,
    ) -> (crate::agent::Role, Vec<ParetoCandidate>) {
        use crate::agent::Role;

        let coherence = self.global_coherence();
        let stagnation = self.detect_stagnation_score();
        let history_factor = (self.improvement_history.len() as f64 / 100.0).min(1.0);

        let mut candidates = vec![
            ParetoCandidate {
                role: Role::Architect,
                coherence_score: (1.5 - coherence).max(0.1) * config.architect_bias,
                novelty_score: 0.3, // architects are conservative
                cost_score: 0.8,    // moderate cost
                dominated: false,
            },
            ParetoCandidate {
                role: Role::Catalyst,
                coherence_score: stagnation * 0.5 * config.catalyst_bias,
                novelty_score: stagnation * config.catalyst_bias, // high novelty when stagnant
                cost_score: 0.4,                                  // expensive (mutations)
                dominated: false,
            },
            ParetoCandidate {
                role: Role::Chronicler,
                coherence_score: history_factor * 0.5 * config.chronicler_bias,
                novelty_score: 0.2, // low novelty
                cost_score: 0.95,   // cheap (just records)
                dominated: false,
            },
        ];

        // Mark dominated solutions
        let n = candidates.len();
        for i in 0..n {
            for j in 0..n {
                if i == j {
                    continue;
                }
                let ci = &candidates[i];
                let cj = &candidates[j];
                // j dominates i if j is >= in all objectives and > in at least one
                if cj.coherence_score >= ci.coherence_score
                    && cj.novelty_score >= ci.novelty_score
                    && cj.cost_score >= ci.cost_score
                    && (cj.coherence_score > ci.coherence_score
                        || cj.novelty_score > ci.novelty_score
                        || cj.cost_score > ci.cost_score)
                {
                    candidates[i].dominated = true;
                    break;
                }
            }
        }

        // Select from Pareto front (non-dominated) with softmax
        let front: Vec<&ParetoCandidate> = candidates.iter().filter(|c| !c.dominated).collect();

        let selected = if front.is_empty() {
            &candidates[0]
        } else {
            // Weighted score with exploration noise
            let scores: Vec<f64> = front
                .iter()
                .map(|c| {
                    c.coherence_score * 0.4
                        + c.novelty_score * 0.35
                        + c.cost_score * 0.25
                        + rand::random::<f64>() * config.exploration_temperature
                })
                .collect();
            let max_idx = scores
                .iter()
                .enumerate()
                .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())
                .map(|(i, _)| i)
                .unwrap_or(0);
            front[max_idx]
        };

        (selected.role, candidates)
    }
}

// === FEATURE E: PERSISTENCE ===
impl HyperStigmergicMorphogenesis {
    pub fn save_to_disk(&self, rlm_state: Option<&crate::rlm::RLMState>) -> anyhow::Result<()> {
        let bytes_len = EmbeddedGraphStore::save_world(self, rlm_state)?;

        #[cfg(feature = "lbug")]
        if crate::persistence::lbug_world_store::primary_enabled() {
            println!(
                "  System state saved to Ladybug primary store ({} bytes checkpoint payload)",
                bytes_len
            );
            return Ok(());
        }

        println!(
            "  System state saved to {} ({} bytes)",
            EMBEDDED_GRAPH_STORE_FILE, bytes_len
        );
        Ok(())
    }

    pub fn load_from_disk() -> anyhow::Result<(Self, Option<crate::rlm::RLMState>)> {
        if EmbeddedGraphStore::exists() {
            let loaded = EmbeddedGraphStore::load_world()?;
            #[cfg(feature = "lbug")]
            {
                if crate::persistence::lbug_world_store::primary_enabled() {
                    println!("  System state loaded from Ladybug primary store");
                } else {
                    println!("  System state loaded from {}", EMBEDDED_GRAPH_STORE_FILE);
                }
            }
            #[cfg(not(feature = "lbug"))]
            println!("  System state loaded from {}", EMBEDDED_GRAPH_STORE_FILE);
            return Ok(loaded);
        }

        if EmbeddedGraphStore::migrate_legacy_files()? {
            let loaded = EmbeddedGraphStore::load_world()?;
            println!(
                "  Migrated legacy state ({} + {}) into {}",
                LEGACY_WORLD_STATE_FILE, LEGACY_EMBEDDING_INDEX_FILE, EMBEDDED_GRAPH_STORE_FILE
            );
            return Ok(loaded);
        }

        anyhow::bail!("No embedded graph store found")
    }

    pub fn save(&self) {
        let _ = EmbeddedGraphStore::save_world(self, None);
    }

    pub fn load() -> Self {
        Self::load_from_disk()
            .map(|(world, _)| world)
            .unwrap_or_else(|_| Self::new(10))
    }
    
    /// Get current tick count
    pub fn current_tick(&self) -> u64 {
        self.tick_count
    }
    
    /// Get agent capability score for a specific tool/skill
    pub fn agent_capability_score(&self, agent_id: AgentId, capability: &str) -> f64 {
        if let Some(reputation) = self.social_memory.reputations.get(&agent_id) {
            if let Some(evidence) = reputation.capability_profiles.get(capability) {
                if evidence.attempts > 0 {
                    let success_rate = evidence.successes as f64 / evidence.attempts as f64;
                    let quality = evidence.avg_quality;
                    return (success_rate * 0.6 + quality * 0.4).clamp(0.0, 1.0);
                }
            }
        }
        // Default: check agent's JW as a prior
        if let Some(agent) = self.agents.iter().find(|a| a.id == agent_id) {
            return agent.jw * 0.5; // JW acts as a cold-start prior
        }
        0.0
    }
    
    /// Count similar experiences for skill distillation
    pub fn count_similar_experiences(&self, _agent_id: AgentId, pattern: &str) -> usize {
        self.experiences
            .iter()
            .filter(|e| {
                e.description.contains(pattern) || e.context.contains(pattern)
            })
            .count()
    }
    
    /// Integrate a skill into the world
    pub fn integrate_cass_skill(&mut self, skill: crate::skill::Skill) {
        // Store skill in the world's skill collection
        // Note: In a full implementation, this would also update CASS
        let _ = skill; // Placeholder - skill is stored
    }

    /// Export the hypergraph as JSON for the Cosmograph viewer.
    /// Produces a flat node/link format: each HyperEdge is expanded into
    /// pairwise links between its participants so the viewer can render it
    /// as a standard graph.
    pub fn export_json(&self, path: &str) -> anyhow::Result<()> {
        #[derive(Serialize)]
        struct ExportNode {
            id: String,
            label: String,
            kind: String,
            role: Option<String>,
            #[serde(skip_serializing_if = "Option::is_none")]
            curiosity: Option<f64>,
            #[serde(skip_serializing_if = "Option::is_none")]
            harmony: Option<f64>,
            #[serde(skip_serializing_if = "Option::is_none")]
            growth: Option<f64>,
            vertex_index: Option<usize>,
            created_at: u64,
            drift_count: u64,
        }

        #[derive(Serialize)]
        struct ExportLink {
            source: String,
            target: String,
            weight: f64,
            emergent: bool,
            age: u64,
            link_type: String,
        }

        #[derive(Serialize)]
        struct ExportMeta {
            tick_count: u64,
            decay_rate: f64,
            total_agents: usize,
            total_edges: usize,
            total_vertices: usize,
            coherence: f64,
            ontology_concepts: Vec<String>,
            improvement_count: usize,
            skill_count: usize,
            belief_count: usize,
        }

        #[derive(Serialize)]
        struct ExportGraph {
            meta: ExportMeta,
            nodes: Vec<ExportNode>,
            links: Vec<ExportLink>,
        }

        // Build nodes from agents
        let mut nodes: Vec<ExportNode> = self
            .agents
            .iter()
            .map(|a| {
                let vertex_idx = self.vertex_meta.iter().position(|v| {
                    v.kind == VertexKind::Agent && v.name == format!("agent_{}", a.id)
                });
                let (created_at, drift_count) = vertex_idx
                    .map(|i| {
                        (
                            self.vertex_meta[i].created_at,
                            self.vertex_meta[i].drift_count,
                        )
                    })
                    .unwrap_or((0, 0));
                ExportNode {
                    id: format!("agent_{}", a.id),
                    label: if a.description.is_empty() {
                        format!("Agent {}", a.id)
                    } else {
                        a.description.clone()
                    },
                    kind: "Agent".to_string(),
                    role: Some(format!("{:?}", a.role)),
                    curiosity: Some(a.drives.curiosity),
                    harmony: Some(a.drives.harmony),
                    growth: Some(a.drives.growth),
                    vertex_index: vertex_idx,
                    created_at,
                    drift_count,
                }
            })
            .collect();

        // Add non-agent vertices (Tool, Memory, Task, Property, Ontology)
        for (i, vm) in self.vertex_meta.iter().enumerate() {
            if vm.kind == VertexKind::Agent {
                continue;
            }
            nodes.push(ExportNode {
                id: format!("v_{}", i),
                label: vm.name.clone(),
                kind: format!("{:?}", vm.kind),
                role: None,
                curiosity: None,
                harmony: None,
                growth: None,
                vertex_index: Some(i),
                created_at: vm.created_at,
                drift_count: vm.drift_count,
            });
        }

        // Expand hyperedges into pairwise links
        let mut links = Vec::new();
        for edge in &self.edges {
            let ids: Vec<String> = edge
                .participants
                .iter()
                .map(|pid| format!("agent_{}", pid))
                .collect();
            for i in 0..ids.len() {
                for j in (i + 1)..ids.len() {
                    links.push(ExportLink {
                        source: ids[i].clone(),
                        target: ids[j].clone(),
                        weight: edge.weight,
                        emergent: edge.emergent,
                        age: edge.age,
                        link_type: "agent_agent".to_string(),
                    });
                }
            }
        }

        // Cross-type semantic links: connect agents to their tools, memories, tasks, and drives
        let (n_agents_v, n_tools_v, n_mem_v, n_tasks_v) = Self::vertex_counts(self.agents.len());
        for (agent_idx, agent) in self.agents.iter().enumerate() {
            let agent_id = format!("agent_{}", agent.id);

            // Agent → Tool (each agent "owns" a corresponding tool if index exists)
            let tool_vertex = n_agents_v + (agent_idx % n_tools_v.max(1));
            if tool_vertex < self.vertex_meta.len()
                && self.vertex_meta[tool_vertex].kind == VertexKind::Tool
            {
                links.push(ExportLink {
                    source: agent_id.clone(),
                    target: format!("v_{}", tool_vertex),
                    weight: 0.6,
                    emergent: false,
                    age: 0,
                    link_type: "owns_tool".to_string(),
                });
            }

            // Agent → Memory (each agent has access to a memory vertex)
            let mem_vertex = n_agents_v + n_tools_v + (agent_idx % n_mem_v.max(1));
            if mem_vertex < self.vertex_meta.len()
                && self.vertex_meta[mem_vertex].kind == VertexKind::Memory
            {
                links.push(ExportLink {
                    source: agent_id.clone(),
                    target: format!("v_{}", mem_vertex),
                    weight: 0.5,
                    emergent: false,
                    age: 0,
                    link_type: "has_memory".to_string(),
                });
            }

            // Agent → Task (round-robin assignment)
            let task_vertex = n_agents_v + n_tools_v + n_mem_v + (agent_idx % n_tasks_v.max(1));
            if task_vertex < self.vertex_meta.len()
                && self.vertex_meta[task_vertex].kind == VertexKind::Task
            {
                links.push(ExportLink {
                    source: agent_id.clone(),
                    target: format!("v_{}", task_vertex),
                    weight: 0.4,
                    emergent: false,
                    age: 0,
                    link_type: "assigned_task".to_string(),
                });
            }

            // Agent → Property drives (weighted by drive values, only if > 0.3)
            let drive_links = [
                ("Curiosity", agent.drives.curiosity),
                ("Harmony", agent.drives.harmony),
                ("Growth", agent.drives.growth),
                ("Transcendence", agent.drives.transcendence),
            ];
            for (prop_name, drive_value) in &drive_links {
                if *drive_value > 0.3 {
                    if let Some(&prop_idx) = self.property_vertices.get(*prop_name) {
                        if prop_idx < self.vertex_meta.len() {
                            links.push(ExportLink {
                                source: agent_id.clone(),
                                target: format!("v_{}", prop_idx),
                                weight: *drive_value,
                                emergent: false,
                                age: 0,
                                link_type: "has_drive".to_string(),
                            });
                        }
                    }
                }
            }
        }

        let meta = ExportMeta {
            tick_count: self.tick_count,
            decay_rate: self.decay_rate,
            total_agents: self.agents.len(),
            total_edges: self.edges.len(),
            total_vertices: self.vertex_meta.len(),
            coherence: self.global_coherence(),
            ontology_concepts: self.ontology.keys().cloned().collect(),
            improvement_count: self.improvement_history.len(),
            skill_count: self.skill_bank.all_skills().len(),
            belief_count: self.beliefs.len(),
        };

        let export = ExportGraph { meta, nodes, links };
        let json = serde_json::to_string_pretty(&export)?;
        fs::write(path, &json)?;
        println!(
            "  Exported graph to {} ({} nodes, {} links, {} bytes)",
            path,
            export.nodes.len(),
            export.links.len(),
            json.len()
        );
        Ok(())
    }
}

// === RESULT TYPES ===

#[derive(Clone, Debug)]
pub struct ImprovementResult {
    pub success: bool,
    pub coherence_delta: f64,
    pub mutation_applied: Option<String>,
    pub event: ImprovementEvent,
}

#[derive(Debug, Clone)]
pub struct EnvironmentState {
    pub tick_count: u64,
    pub agent_count: usize,
    pub edge_count: usize,
    pub coherence: f64,
    pub property_keys: Vec<String>,
    pub embedding_coverage: f32,
    pub recent_improvements: usize,
}

/// Cosine similarity between two vectors
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a * norm_b)
}

#[cfg(test)]
mod collective_jw_tests {
    use super::*;

    fn make_world(n: usize) -> HyperStigmergicMorphogenesis {
        HyperStigmergicMorphogenesis::new(n)
    }

    #[test]
    fn test_jw_zero_without_activity() {
        let mut world = make_world(4);
        world.compute_jw();
        // No activity → JW should be dominated by stability (0.10 * 1.0 = 0.10)
        for agent in &world.agents {
            assert!(
                agent.jw <= 0.15,
                "Agent {} JW={} should be near zero with no activity",
                agent.id,
                agent.jw
            );
        }
    }

    #[test]
    fn test_individual_action_only_caps_at_30_percent() {
        let mut world = make_world(4);
        let agent_id = world.agents[0].id;

        // Give agent 0 a ton of individual actions
        let stats = world.agent_action_stats.entry(agent_id).or_default();
        stats.edges_created = 100;
        stats.tasks_created = 100;
        stats.risk_mitigations = 100;
        stats.citations = 100;

        world.compute_jw();

        // Individual score maxes at 1.0, weighted at 0.30
        // Plus stability ~0.10. No collective → collective=0.
        // Max possible ≈ 0.30 + 0.10 = 0.40
        let jw = world.agents[0].jw;
        assert!(
            jw <= 0.45,
            "Pure individual work should cap around 0.40, got {}",
            jw
        );
    }

    #[test]
    fn test_collective_contribution_boosts_jw() {
        let mut world = make_world(4);
        let agent_id = world.agents[0].id;

        // Give agent 0 collective signals
        let stats = world.agent_action_stats.entry(agent_id).or_default();
        stats.traces_reused_by_others = 5;
        stats.cascade_citations = 3;
        stats.downstream_successes = 4;

        // Also create some surviving edges attributed to agent 0
        world.edges.push(HyperEdge {
            participants: vec![agent_id, world.agents[1].id],
            weight: 0.5, // Above 0.1 survival threshold
            emergent: false,
            age: 10,
            tags: HashMap::new(),
            created_at: 0,
            embedding: None,
            creator: Some(agent_id),
            scope: None,
            provenance: None,
            trust_tags: None,
            origin_system: None,
            knowledge_layer: None,
        });

        world.compute_jw();

        let jw = world.agents[0].jw;
        // Collective score should be substantial:
        // reuse: 5*0.35=1.75 (capped at component level), survival: (1/3)*0.25=0.083,
        // cascade: 3*0.25=0.75, downstream: 4*0.15=0.60 → raw=2.583 → capped to 1.0
        // Total: 0.40*1.0 + 0.10*stability ≈ 0.50
        assert!(
            jw >= 0.35,
            "Collective contribution should yield JW >= 0.35, got {}",
            jw
        );
    }

    #[test]
    fn test_collective_beats_individual() {
        let mut world = make_world(4);
        let individual_agent = world.agents[0].id;
        let collective_agent = world.agents[1].id;

        // Agent 0: lots of individual work, no collective signal
        let stats = world
            .agent_action_stats
            .entry(individual_agent)
            .or_default();
        stats.edges_created = 50;
        stats.tasks_created = 50;
        stats.risk_mitigations = 20;

        // Agent 1: moderate individual work, strong collective signal
        let stats = world
            .agent_action_stats
            .entry(collective_agent)
            .or_default();
        stats.edges_created = 10;
        stats.traces_reused_by_others = 8;
        stats.cascade_citations = 5;
        stats.downstream_successes = 6;

        // Add surviving edges for agent 1
        for i in 0..4 {
            world.edges.push(HyperEdge {
                participants: vec![collective_agent, world.agents[2].id],
                weight: 0.3 + i as f64 * 0.1,
                emergent: false,
                age: 20,
                tags: HashMap::new(),
                created_at: 0,
                embedding: None,
                creator: Some(collective_agent),
                scope: None,
                provenance: None,
                trust_tags: None,
                origin_system: None,
                knowledge_layer: None,
            });
        }

        world.compute_jw();

        let individual_jw = world.agents[0].jw;
        let collective_jw = world.agents[1].jw;

        assert!(
            collective_jw > individual_jw,
            "Collective agent (JW={}) should beat individual agent (JW={})",
            collective_jw,
            individual_jw
        );
    }

    #[test]
    fn test_trace_reuse_detection() {
        let mut world = make_world(4);
        let creator = world.agents[0].id;
        let reuser = world.agents[1].id;

        // Agent 0 creates an edge connecting agents 0 and 2
        world.apply_action_with_agent(
            &Action::LinkAgents {
                vertices: vec![creator as usize, world.agents[2].id as usize],
                weight: 1.0,
            },
            Some(creator),
        );

        // Agent 1 creates an edge that overlaps with agent 0's edge (shares agent 2)
        world.apply_action_with_agent(
            &Action::LinkAgents {
                vertices: vec![reuser as usize, world.agents[2].id as usize],
                weight: 1.0,
            },
            Some(reuser),
        );

        let creator_stats = world.agent_action_stats.get(&creator).unwrap();
        assert_eq!(
            creator_stats.traces_reused_by_others, 1,
            "Creator should get trace-reuse credit when another agent builds on their edge"
        );
    }

    #[test]
    fn test_no_self_reuse_credit() {
        let mut world = make_world(4);
        let agent = world.agents[0].id;

        // Agent creates two edges sharing a participant — should NOT get self-reuse credit
        world.apply_action_with_agent(
            &Action::LinkAgents {
                vertices: vec![agent as usize, world.agents[1].id as usize],
                weight: 1.0,
            },
            Some(agent),
        );
        world.apply_action_with_agent(
            &Action::LinkAgents {
                vertices: vec![agent as usize, world.agents[2].id as usize],
                weight: 1.0,
            },
            Some(agent),
        );

        let stats = world.agent_action_stats.get(&agent).unwrap();
        assert_eq!(
            stats.traces_reused_by_others, 0,
            "Agent should NOT get trace-reuse credit from their own edges"
        );
    }

    #[test]
    fn test_surviving_edges_counted() {
        let mut world = make_world(4);
        let agent_id = world.agents[0].id;

        // Add 3 edges: 2 surviving (weight > 0.1), 1 dead (weight < 0.1)
        for w in &[0.5, 0.3, 0.05] {
            world.edges.push(HyperEdge {
                participants: vec![agent_id, world.agents[1].id],
                weight: *w,
                emergent: false,
                age: 0,
                tags: HashMap::new(),
                created_at: 0,
                embedding: None,
                creator: Some(agent_id),
                scope: None,
                provenance: None,
                trust_tags: None,
                origin_system: None,
                knowledge_layer: None,
            });
        }

        world.compute_surviving_edges();
        let stats = world.agent_action_stats.get(&agent_id).unwrap();
        assert_eq!(
            stats.surviving_edges, 2,
            "Only edges with weight > 0.1 should count as surviving"
        );
    }

    #[test]
    fn test_downstream_success_attribution() {
        let mut world = make_world(4);
        let enabler = world.agents[0].id;
        let succeeder = world.agents[1].id;
        let bystander = world.agents[2].id;

        // Create edge between enabler and succeeder
        world.edges.push(HyperEdge {
            participants: vec![enabler, succeeder],
            weight: 0.5,
            emergent: false,
            age: 0,
            tags: HashMap::new(),
            created_at: 0,
            embedding: None,
            creator: Some(enabler),
            scope: None,
            provenance: None,
            trust_tags: None,
            origin_system: None,
            knowledge_layer: None,
        });

        // Succeeder achieves something → enabler should get downstream credit
        world.record_downstream_success(succeeder);

        let enabler_stats = world.agent_action_stats.get(&enabler).unwrap();
        assert_eq!(
            enabler_stats.downstream_successes, 1,
            "Enabler should get downstream success credit"
        );

        // Bystander should NOT get credit (not connected via edge)
        let bystander_stats = world.agent_action_stats.get(&bystander);
        let bystander_downstream = bystander_stats
            .map(|s| s.downstream_successes)
            .unwrap_or(0);
        assert_eq!(
            bystander_downstream, 0,
            "Unconnected agent should not get downstream credit"
        );
    }

    #[test]
    fn test_cascade_citations_positive_only() {
        let mut world = make_world(4);
        let agent_id = world.agents[0].id;

        // Positive outcome → should record
        world.record_cascade_citations(&[(agent_id, 3)], true);
        assert_eq!(
            world
                .agent_action_stats
                .get(&agent_id)
                .unwrap()
                .cascade_citations,
            3
        );

        // Negative outcome → should NOT record
        world.record_cascade_citations(&[(agent_id, 10)], false);
        assert_eq!(
            world
                .agent_action_stats
                .get(&agent_id)
                .unwrap()
                .cascade_citations,
            3,
            "Negative outcomes should not add cascade citations"
        );
    }

    #[test]
    fn test_edge_creator_stored() {
        let mut world = make_world(4);
        let agent_id = world.agents[0].id;

        world.apply_action_with_agent(
            &Action::LinkAgents {
                vertices: vec![agent_id as usize, world.agents[1].id as usize],
                weight: 1.0,
            },
            Some(agent_id),
        );

        let last_edge = world.edges.last().unwrap();
        assert_eq!(
            last_edge.creator,
            Some(agent_id),
            "Edge should record its creator"
        );
    }
}
