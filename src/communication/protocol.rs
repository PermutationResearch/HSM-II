//! Message protocol definitions.

use super::{MessageId, Target};
use crate::agent::AgentId;
use serde::{Deserialize, Serialize};

/// Envelope wrapping a message with routing information
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MessageEnvelope {
    pub id: MessageId,
    pub sender: AgentId,
    pub recipient: Target,
    pub content: String,
    pub message_type: String,
    pub priority: MessagePriority,
    pub timestamp: u64,
    pub ttl: u32,
    pub hop_count: u32,
    pub routing: RoutingStrategy,
    pub delivery_guarantee: DeliveryGuarantee,
}

impl MessageEnvelope {
    pub fn new(sender: AgentId, message: super::Message, recipient: Target) -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Self {
            id: format!("msg_{}_{}", sender, timestamp),
            sender,
            recipient,
            content: message.content,
            message_type: message.message_type.to_string(),
            priority: message.priority,
            timestamp,
            ttl: 10,
            hop_count: 0,
            routing: RoutingStrategy::Flood,
            delivery_guarantee: DeliveryGuarantee::BestEffort,
        }
    }

    /// Check if message is addressed to a specific agent
    pub fn is_addressed_to(&self, agent_id: AgentId) -> bool {
        match self.recipient {
            Target::Agent(id) => id == agent_id,
            Target::Broadcast => true,
            Target::Swarm => true,
        }
    }

    /// Check if recipient matches (for filtering)
    pub fn recipient_matches(&self, agent_id: AgentId) -> bool {
        self.is_addressed_to(agent_id)
    }

    /// Get message type as string
    pub fn message_type(&self) -> String {
        self.message_type.clone()
    }

    /// Create acknowledgment envelope
    pub fn create_ack(&self) -> Self {
        let mut ack = self.clone();
        ack.id = format!("{}_ack", self.id);
        ack.recipient = Target::Agent(self.sender);
        ack.content = format!("ACK: {}", self.id);
        ack.message_type = "ack".to_string();
        ack
    }
}

/// Message priority levels
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, PartialOrd)]
pub enum MessagePriority {
    Critical = 0,
    High = 1,
    Normal = 2,
    Low = 3,
}

impl Default for MessagePriority {
    fn default() -> Self {
        MessagePriority::Normal
    }
}

/// Routing strategy for message delivery
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum RoutingStrategy {
    /// Broadcast to all reachable nodes
    Flood,
    /// Forward to specific neighbors
    Directed,
    /// Gossip-style epidemic spread
    Epidemic,
    /// Follow gradient field
    Gradient,
    /// Neural network learned routing
    Learned,
}

/// Delivery guarantee level
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum DeliveryGuarantee {
    /// No guarantees
    BestEffort,
    /// At-least-once delivery
    AtLeastOnce,
    /// At-most-once delivery
    AtMostOnce,
    /// Exactly-once delivery
    ExactlyOnce,
}
