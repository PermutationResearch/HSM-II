//! Communication protocols for multi-agent coordination.
//!
//! Provides:
//! - Gossip protocol for epidemic information spread
//! - Swarm communication for collective behavior
//! - Stigmergic fields for indirect coordination

use crate::agent::AgentId;
use serde::{Deserialize, Serialize};

pub mod gossip;
pub mod message;
pub mod protocol;
pub mod swarm;

pub use gossip::{GossipConfig, GossipProtocol, RumorState, RumorStatus};
pub use message::{Message, MessageMetadata, MessageType};
pub use protocol::{DeliveryGuarantee, MessageEnvelope, MessagePriority, RoutingStrategy};
pub use swarm::{FlockingForces, StigmergicField, SwarmCommunication, WaggleDance};

/// Communication hub managing all protocols
pub struct CommunicationHub {
    gossip: GossipProtocol,
    swarm: SwarmCommunication,
    message_log: Vec<MessageEnvelope>,
    local_agent_id: AgentId,
    max_message_history: usize,
}

impl CommunicationHub {
    pub fn new(agent_id: AgentId, config: CommunicationConfig) -> Self {
        Self {
            gossip: GossipProtocol::new(config.gossip),
            swarm: SwarmCommunication::new(),
            message_log: Vec::new(),
            local_agent_id: agent_id,
            max_message_history: config.max_message_history,
        }
    }

    /// Send a message via appropriate protocol
    pub fn send(&mut self, message: Message, target: Target) -> anyhow::Result<MessageId> {
        self.send_from(self.local_agent_id, message, target)
    }

    /// Send as an explicit sender (used for lightweight multi-agent simulation).
    pub fn send_from(
        &mut self,
        sender: AgentId,
        message: Message,
        target: Target,
    ) -> anyhow::Result<MessageId> {
        let envelope = MessageEnvelope::new(sender, message, target);

        let id = envelope.id.clone();

        match target {
            Target::Agent(_) | Target::Broadcast => {
                self.gossip.submit_message(envelope.clone());
            }
            Target::Swarm => {
                self.swarm.broadcast(envelope.clone());
            }
        }

        self.message_log.push(envelope);
        if self.message_log.len() > self.max_message_history {
            let overflow = self.message_log.len() - self.max_message_history;
            self.message_log.drain(0..overflow);
        }
        Ok(id)
    }

    /// Receive messages for this agent
    pub fn receive(&mut self) -> Vec<MessageEnvelope> {
        self.receive_limited(usize::MAX)
    }

    /// Receive with bounded work per tick.
    pub fn receive_limited(&mut self, max_messages: usize) -> Vec<MessageEnvelope> {
        let mut received = Vec::new();

        // Get gossip messages
        received.extend(
            self.gossip
                .retrieve_messages_limited(self.local_agent_id, max_messages),
        );

        // Get swarm messages
        if received.len() < max_messages {
            received.extend(self.swarm.retrieve_messages_limited(
                self.local_agent_id,
                max_messages.saturating_sub(received.len()),
            ));
        }

        if received.len() > max_messages {
            received.truncate(max_messages);
        }

        received
    }

    /// Tick the communication protocols (for periodic maintenance)
    pub fn tick(&mut self) {
        self.gossip.tick();
        self.swarm.tick();
    }

    /// Get gossip statistics
    pub fn gossip_stats(&self) -> GossipStats {
        self.gossip.stats()
    }

    /// Get swarm field values at position
    pub fn get_field_value(&self, field_type: FieldType, position: Position) -> f64 {
        self.swarm.get_field_value(field_type, position)
    }

    /// Deposit pheromone in stigmergic field
    pub fn deposit_pheromone(&mut self, field_type: FieldType, position: Position, strength: f64) {
        self.swarm.deposit_pheromone(field_type, position, strength);
    }

    /// Perform waggle dance to communicate location
    pub fn waggle_dance(&mut self, resource_location: Position, quality: f64) {
        self.swarm
            .perform_waggle_dance(self.local_agent_id, resource_location, quality);
    }

    /// Update agent position in swarm (for testing)
    pub fn update_swarm_position(&mut self, agent_id: AgentId, position: Position) {
        self.swarm.update_position(agent_id, position);
    }

    /// Calculate flocking forces for an agent (for testing)
    pub fn calculate_flocking_forces(&self, agent_id: AgentId) -> FlockingForces {
        self.swarm.calculate_flocking_forces(agent_id)
    }

    /// Get swarm component (for testing)
    pub fn swarm(&self) -> &SwarmCommunication {
        &self.swarm
    }

    /// Get mutable swarm component (for testing)
    pub fn swarm_mut(&mut self) -> &mut SwarmCommunication {
        &mut self.swarm
    }

    /// Recent messages (bounded, most recent last).
    pub fn recent_messages(&self, limit: usize) -> Vec<MessageEnvelope> {
        if limit == 0 || self.message_log.is_empty() {
            return Vec::new();
        }
        let start = self.message_log.len().saturating_sub(limit);
        self.message_log[start..].to_vec()
    }
}

/// Communication configuration
#[derive(Clone, Debug)]
pub struct CommunicationConfig {
    pub gossip: GossipConfig,
    pub enable_swarm: bool,
    pub max_message_history: usize,
}

impl Default for CommunicationConfig {
    fn default() -> Self {
        Self {
            gossip: GossipConfig::default(),
            enable_swarm: true,
            max_message_history: 10000,
        }
    }
}

/// Message target
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum Target {
    /// Direct to agent
    Agent(AgentId),
    /// Broadcast to all
    Broadcast,
    /// Swarm communication
    Swarm,
}

/// Message identifier
pub type MessageId = String;

/// Position in n-dimensional space
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Position {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Position {
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    pub fn distance(&self, other: &Position) -> f64 {
        ((self.x - other.x).powi(2) + (self.y - other.y).powi(2) + (self.z - other.z).powi(2))
            .sqrt()
    }
}

/// Types of stigmergic fields
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum FieldType {
    Resource,
    Danger,
    Exploration,
    Trail,
    Nest,
}

/// Gossip protocol statistics
#[derive(Clone, Debug, Default)]
pub struct GossipStats {
    pub rumors_active: usize,
    pub rumors_resolved: usize,
    pub messages_sent: u64,
    pub messages_received: u64,
}
