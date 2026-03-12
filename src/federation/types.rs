use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::consensus::ConsensusVerdict;

/// Unique identifier for a federated system instance.
pub type SystemId = String;

/// Scope of a hyperedge — controls visibility in the federation.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum EdgeScope {
    /// Only visible within the originating instance.
    Local,
    /// Shared across all federated instances.
    Shared,
    /// Shared only with specific systems.
    Restricted(Vec<SystemId>),
}

impl Default for EdgeScope {
    fn default() -> Self {
        EdgeScope::Local
    }
}

/// Provenance chain tracking where an edge originated and how it traveled.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Provenance {
    pub origin_system: SystemId,
    pub created_at: u64,
    /// Chain of (system_id, timestamp) hops this edge has traveled through.
    pub hop_chain: Vec<(SystemId, u64)>,
}

/// Knowledge abstraction layer — edges get promoted through layers as they're validated.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum KnowledgeLayer {
    /// Layer 0: Raw observations and edges.
    Raw,
    /// Layer 1: Distilled patterns from multiple observations.
    Distilled,
    /// Layer 2: Validated by consensus across agents.
    Validated,
    /// Layer 3: Meta-level cross-system knowledge.
    Meta,
}

impl Default for KnowledgeLayer {
    fn default() -> Self {
        KnowledgeLayer::Raw
    }
}

/// Configuration for federation networking.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FederationConfig {
    pub system_id: SystemId,
    pub listen_addr: String,
    pub known_peers: Vec<String>,
    /// Minimum trust score to import edges from a remote system.
    pub trust_threshold: f64,
    /// Number of ticks before a shared edge is eligible for promotion.
    pub auto_promote_after: u64,
}

impl Default for FederationConfig {
    fn default() -> Self {
        Self {
            system_id: uuid::Uuid::new_v4().to_string(),
            listen_addr: "0.0.0.0:8787".to_string(),
            known_peers: Vec::new(),
            trust_threshold: 0.3,
            auto_promote_after: 50,
        }
    }
}

/// A request to inject hyperedges from a remote system.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HyperedgeInjectionRequest {
    pub vertices: Vec<String>,
    pub edge_type: String,
    pub scope: EdgeScope,
    pub trust_tags: Vec<String>,
    pub provenance: Provenance,
    pub weight: f64,
    pub embedding: Option<Vec<f32>>,
    pub metadata: HashMap<String, String>,
}

/// Result of a single injected edge.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InjectedEdge {
    pub edge_id: String,
    pub accepted: bool,
    pub reason: String,
}

/// Summary of an import operation.
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct ImportResult {
    pub imported: usize,
    pub rejected: usize,
    pub conflicts: usize,
}

/// Filter for subscription-based edge notifications.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SubscriptionFilter {
    pub edge_types: Option<Vec<String>>,
    pub min_trust: Option<f64>,
    pub domains: Option<Vec<String>>,
    pub min_layer: Option<KnowledgeLayer>,
}

/// A registered subscription from a remote system.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Subscription {
    pub subscriber_system: SystemId,
    pub callback_url: String,
    pub filter: SubscriptionFilter,
    pub created_at: u64,
}

/// Information about a federated system instance.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SystemInfo {
    pub system_id: SystemId,
    pub version: String,
    pub vertex_count: usize,
    pub edge_count: usize,
    pub uptime_ticks: u64,
}

/// A cross-system consensus vote on a skill.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CrossSystemVote {
    pub voter_system: SystemId,
    pub skill_id: String,
    pub verdict: ConsensusVerdict,
    pub confidence: f64,
    pub evidence: String,
}

/// A bid from a remote agent (used in cross-system ACPO aggregation).
#[derive(Clone, Debug)]
pub struct RemoteAgentBid {
    pub system_id: SystemId,
    pub role: String,
    pub bid_value: f64,
    pub confidence: f64,
}

/// A shared edge in the MetaGraph (H*).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SharedEdge {
    pub id: String,
    pub vertices: Vec<String>,
    pub edge_type: String,
    pub weight: f64,
    pub provenance: Provenance,
    pub layer: KnowledgeLayer,
    pub contributing_systems: Vec<SystemId>,
    pub trust_tags: Vec<String>,
    pub embedding: Option<Vec<f32>>,
    pub usage_count: u64,
    pub success_count: u64,
}

/// Metadata for a shared vertex in the MetaGraph.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SharedVertexMeta {
    pub name: String,
    pub origin_system: SystemId,
    /// Cross-system name mappings (system_id, local_name).
    pub aliases: Vec<(SystemId, String)>,
    pub embedding: Option<Vec<f32>>,
}

/// A promoted edge that has been elevated to a higher knowledge layer.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PromotedEdge {
    pub edge_index: usize,
    pub promoted_at_tick: u64,
    pub from_layer: KnowledgeLayer,
    pub to_layer: KnowledgeLayer,
    pub reason: String,
}

/// A detected meta-hyperedge (emergent cross-system pattern).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum MetaHyperedge {
    /// Multiple systems proposed similar solutions.
    Consensus {
        systems: Vec<SystemId>,
        shared_edge_indices: Vec<usize>,
        agreement_score: f64,
    },
    /// Dense cross-domain co-occurrence across systems.
    Synthesis {
        systems: Vec<SystemId>,
        domains: Vec<String>,
        edge_indices: Vec<usize>,
    },
}
