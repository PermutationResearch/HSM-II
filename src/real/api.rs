use serde::{Deserialize, Serialize};

use crate::{Action, Agent, HyperStigmergicMorphogenesis, Role};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AgentSnapshot {
    pub id: u64,
    pub role: Role,
    pub curiosity: f64,
    pub harmony: f64,
    pub growth: f64,
    pub transcendence: f64,
    pub learning_rate: f64,
    pub bid_bias: f64,
    pub jw: f64,
}

impl From<&Agent> for AgentSnapshot {
    fn from(agent: &Agent) -> Self {
        Self {
            id: agent.id,
            role: agent.role,
            curiosity: agent.drives.curiosity,
            harmony: agent.drives.harmony,
            growth: agent.drives.growth,
            transcendence: agent.drives.transcendence,
            learning_rate: agent.learning_rate,
            bid_bias: agent.bid_bias,
            jw: agent.jw,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WorldSnapshot {
    pub tick: u64,
    pub coherence: f64,
    pub agents: Vec<AgentSnapshot>,
    pub edge_count: usize,
    pub emergent_edge_count: usize,
}

impl From<&HyperStigmergicMorphogenesis> for WorldSnapshot {
    fn from(world: &HyperStigmergicMorphogenesis) -> Self {
        Self {
            tick: world.tick_count,
            coherence: world.global_coherence(),
            agents: world.agents.iter().map(AgentSnapshot::from).collect(),
            edge_count: world.edges.len(),
            emergent_edge_count: world.emergent_edge_count(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ApplyActionRequest {
    pub action: Action,
    #[serde(default)]
    pub agent_id: Option<u64>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ApplyActionResponse {
    pub snapshot: WorldSnapshot,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TickResponse {
    pub snapshot: WorldSnapshot,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BidSubmission {
    pub agent_id: u64,
    pub role: Role,
    pub bid: f64,
    pub objectives: Objectives,
    pub action: Action,
    pub rationale: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Objectives {
    /// Higher is better. Represents expected coherence gain.
    pub coherence: f64,
    /// Higher is better. Represents expected novelty gain.
    pub novelty: f64,
    /// Higher is better. Represents safety / risk reduction.
    pub safety: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DecisionResult {
    pub chosen: Option<BidSubmission>,
    pub snapshot: Option<WorldSnapshot>,
    pub ticked: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct GrpoUpdateRequest {
    pub rewards: Vec<GrpoReward>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct GrpoReward {
    pub agent_id: u64,
    pub reward: f64,
}
