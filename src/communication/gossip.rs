//! Gossip protocol for epidemic information spread.
//!
//! Implements the rumor mongering algorithm:
//! - Hot rumors: Actively being spread
//! - Cold rumors: Known but not actively spread
//! - Resolved: Acknowledged by recipient

use super::{GossipStats, MessageEnvelope, MessageId};
use crate::agent::AgentId;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

/// Gossip protocol implementation
pub struct GossipProtocol {
    config: GossipConfig,
    /// All known rumors with their state
    rumors: HashMap<MessageId, RumorState>,
    /// Hot rumors (actively spreading)
    hot_rumors: Vec<MessageId>,
    /// Cold rumors (known but not spreading)
    cold_rumors: Vec<MessageId>,
    /// Outgoing message queue
    outgoing: VecDeque<MessageEnvelope>,
    /// Message handlers by type
    handlers: HashMap<String, Box<dyn Fn(&MessageEnvelope) + Send>>,
    /// Statistics
    stats: GossipStats,
}

/// Configuration for gossip protocol
#[derive(Clone, Debug)]
pub struct GossipConfig {
    /// Number of peers to gossip to per round
    pub fanout: usize,
    /// TTL for messages
    pub default_ttl: u32,
    /// How often to process hot rumors
    pub hot_process_interval: u32,
    /// Max hot rumors before demotion
    pub max_hot_rumors: usize,
    /// Probability of forwarding a cold rumor
    pub cold_forward_probability: f64,
}

impl Default for GossipConfig {
    fn default() -> Self {
        Self {
            fanout: 3,
            default_ttl: 10,
            hot_process_interval: 5,
            max_hot_rumors: 100,
            cold_forward_probability: 0.1,
        }
    }
}

/// State of a rumor in the gossip protocol
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RumorState {
    pub message: MessageEnvelope,
    pub status: RumorStatus,
    pub hop_count: u32,
    pub created_at: u64,
    pub last_forwarded: Option<u64>,
    pub forward_count: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum RumorStatus {
    Hot,      // Actively spreading
    Cold,     // Known but passive
    Resolved, // Acknowledged
    Expired,  // TTL exceeded
}

impl GossipProtocol {
    pub fn new(config: GossipConfig) -> Self {
        Self {
            config,
            rumors: HashMap::new(),
            hot_rumors: Vec::new(),
            cold_rumors: Vec::new(),
            outgoing: VecDeque::new(),
            handlers: HashMap::new(),
            stats: GossipStats::default(),
        }
    }

    /// Submit a message to be gossiped
    pub fn submit_message(&mut self, message: MessageEnvelope) {
        let id = message.id.clone();

        // Check if we already know this rumor
        if self.rumors.contains_key(&id) {
            return;
        }

        let state = RumorState {
            message: message.clone(),
            status: RumorStatus::Hot,
            hop_count: 0,
            created_at: current_timestamp(),
            last_forwarded: None,
            forward_count: 0,
        };

        self.rumors.insert(id.clone(), state);
        self.hot_rumors.push(id);

        // Manage hot rumor capacity
        if self.hot_rumors.len() > self.config.max_hot_rumors {
            self.demote_oldest_hot_rumor();
        }

        // Queue for immediate forwarding
        self.outgoing.push_back(message);
        self.stats.messages_sent += 1;
    }

    /// Receive a message from another peer
    pub fn receive_message(&mut self, message: MessageEnvelope) {
        let id = message.id.clone();

        // Check if we already know this rumor
        if let Some(existing) = self.rumors.get_mut(&id) {
            // We already know it - mark as resolved
            existing.status = RumorStatus::Resolved;
            return;
        }

        // New rumor - add it
        let state = RumorState {
            message: message.clone(),
            status: RumorStatus::Hot,
            hop_count: message.hop_count + 1,
            created_at: current_timestamp(),
            last_forwarded: None,
            forward_count: 0,
        };

        self.rumors.insert(id.clone(), state);
        self.hot_rumors.push(id);

        self.stats.messages_received += 1;

        // Call handlers
        self.call_handlers(&message);
    }

    /// Retrieve messages addressed to specific agent
    pub fn retrieve_messages(&mut self, agent_id: AgentId) -> Vec<MessageEnvelope> {
        self.retrieve_messages_limited(agent_id, usize::MAX)
    }

    /// Retrieve messages with an upper bound to control CPU/memory work per tick.
    pub fn retrieve_messages_limited(
        &mut self,
        agent_id: AgentId,
        limit: usize,
    ) -> Vec<MessageEnvelope> {
        let mut messages = Vec::new();

        // Check all rumors for messages addressed to this agent
        let now = current_timestamp();

        for (_id, state) in &mut self.rumors {
            if messages.len() >= limit {
                break;
            }
            if state.message.is_addressed_to(agent_id) && state.status != RumorStatus::Expired {
                // Check TTL
                if now - state.created_at > state.message.ttl as u64 {
                    state.status = RumorStatus::Expired;
                    continue;
                }

                messages.push(state.message.clone());
                state.status = RumorStatus::Resolved;
            }
        }

        messages
    }

    /// Get messages to send to peers (for external transport)
    pub fn get_outgoing(&mut self, limit: usize) -> Vec<MessageEnvelope> {
        let mut result = Vec::new();

        // Process hot rumors
        self.process_hot_rumors();

        // Get from outgoing queue
        while result.len() < limit && !self.outgoing.is_empty() {
            if let Some(msg) = self.outgoing.pop_front() {
                result.push(msg);
            }
        }

        result
    }

    /// Process rumors to potentially forward
    fn process_hot_rumors(&mut self) {
        let now = current_timestamp();

        // Process a batch of hot rumors
        let to_process: Vec<MessageId> = self
            .hot_rumors
            .iter()
            .take(self.config.fanout)
            .cloned()
            .collect();

        for id in to_process {
            if let Some(state) = self.rumors.get_mut(&id) {
                // Check if should demote to cold
                if state.forward_count >= self.config.hot_process_interval as u32 {
                    state.status = RumorStatus::Cold;
                    self.cold_rumors.push(id.clone());
                } else {
                    // Forward to more peers
                    self.outgoing.push_back(state.message.clone());
                    state.forward_count += 1;
                    state.last_forwarded = Some(now);
                    self.stats.messages_sent += 1;
                }
            }

            // Remove from hot if now cold
            self.hot_rumors.retain(|rid| rid != &id);
        }
    }

    /// Periodically try to forward cold rumors
    fn process_cold_rumors(&mut self) {
        let now = current_timestamp();

        for id in &self.cold_rumors {
            if rand::random::<f64>() < self.config.cold_forward_probability {
                if let Some(state) = self.rumors.get_mut(id) {
                    self.outgoing.push_back(state.message.clone());
                    state.last_forwarded = Some(now);
                    self.stats.messages_sent += 1;
                }
            }
        }
    }

    /// Demote oldest hot rumor to cold
    fn demote_oldest_hot_rumor(&mut self) {
        if let Some(id) = self.hot_rumors.first().cloned() {
            if let Some(state) = self.rumors.get_mut(&id) {
                state.status = RumorStatus::Cold;
            }
            self.cold_rumors.push(id);
            self.hot_rumors.remove(0);
        }
    }

    /// Register a handler for message types
    pub fn register_handler<F>(&mut self, message_type: &str, handler: F)
    where
        F: Fn(&MessageEnvelope) + Send + 'static,
    {
        self.handlers
            .insert(message_type.to_string(), Box::new(handler));
    }

    /// Call handlers for a message
    fn call_handlers(&self, message: &MessageEnvelope) {
        if let Some(handler) = self.handlers.get(&message.message_type()) {
            handler(message);
        }
    }

    /// Get protocol statistics
    pub fn stats(&self) -> GossipStats {
        let active = self
            .rumors
            .values()
            .filter(|s| s.status == RumorStatus::Hot || s.status == RumorStatus::Cold)
            .count();

        let resolved = self
            .rumors
            .values()
            .filter(|s| s.status == RumorStatus::Resolved)
            .count();

        GossipStats {
            rumors_active: active,
            rumors_resolved: resolved,
            messages_sent: self.stats.messages_sent,
            messages_received: self.stats.messages_received,
        }
    }

    /// Periodic maintenance
    pub fn tick(&mut self) {
        self.process_cold_rumors();

        // Expire old rumors
        let now = current_timestamp();
        for state in self.rumors.values_mut() {
            if now - state.created_at > state.message.ttl as u64 * 60 {
                state.status = RumorStatus::Expired;
            }
        }

        // Clean up expired rumors occasionally
        if self.rumors.len() > 10000 {
            self.rumors
                .retain(|_, state| state.status != RumorStatus::Expired);
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
