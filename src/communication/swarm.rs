//! Swarm communication for collective behavior.
//!
//! Implements:
//! - Stigmergic fields (indirect coordination via environment)
//! - Waggle dance (location-based communication)
//! - Flocking behaviors (alignment, cohesion, separation)

use super::{FieldType, MessageEnvelope, Position};
use crate::agent::AgentId;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

/// Swarm communication for collective behavior
pub struct SwarmCommunication {
    /// Stigmergic fields
    fields: HashMap<FieldType, StigmergicField>,
    /// Recent waggle dances
    dances: VecDeque<WaggleDance>,
    /// Agent positions
    agent_positions: HashMap<AgentId, Position>,
    /// Swarm messages
    message_queue: VecDeque<MessageEnvelope>,
}

impl SwarmCommunication {
    pub fn new() -> Self {
        let mut fields = HashMap::new();

        // Initialize default fields
        for field_type in [
            FieldType::Resource,
            FieldType::Danger,
            FieldType::Exploration,
            FieldType::Trail,
            FieldType::Nest,
        ] {
            fields.insert(field_type, StigmergicField::new(field_type));
        }

        Self {
            fields,
            dances: VecDeque::with_capacity(100),
            agent_positions: HashMap::new(),
            message_queue: VecDeque::new(),
        }
    }

    /// Broadcast a message to the swarm
    pub fn broadcast(&mut self, message: MessageEnvelope) {
        self.message_queue.push_back(message);
    }

    /// Retrieve messages for an agent
    pub fn retrieve_messages(&mut self, agent_id: AgentId) -> Vec<MessageEnvelope> {
        self.retrieve_messages_limited(agent_id, usize::MAX)
    }

    /// Retrieve messages with an upper bound to keep per-tick work bounded.
    pub fn retrieve_messages_limited(
        &mut self,
        agent_id: AgentId,
        limit: usize,
    ) -> Vec<MessageEnvelope> {
        let mut messages = Vec::new();

        // Get messages from queue
        while let Some(msg) = self.message_queue.pop_front() {
            if messages.len() >= limit {
                self.message_queue.push_front(msg);
                break;
            }
            if msg.recipient_matches(agent_id) {
                messages.push(msg);
            }
        }

        // Add field-based "messages"
        if let Some(pos) = self.agent_positions.get(&agent_id) {
            for (field_type, field) in &self.fields {
                if messages.len() >= limit {
                    break;
                }
                let value = field.value_at(*pos);
                if value > 0.5 {
                    messages.push(self.create_field_message(*field_type, *pos, value));
                }
            }
        }

        messages
    }

    /// Update agent position
    pub fn update_position(&mut self, agent_id: AgentId, position: Position) {
        self.agent_positions.insert(agent_id, position);
    }

    /// Get field value at position
    pub fn get_field_value(&self, field_type: FieldType, position: Position) -> f64 {
        self.fields
            .get(&field_type)
            .map(|f| f.value_at(position))
            .unwrap_or(0.0)
    }

    /// Deposit pheromone in field
    pub fn deposit_pheromone(&mut self, field_type: FieldType, position: Position, strength: f64) {
        if let Some(field) = self.fields.get_mut(&field_type) {
            field.deposit(position, strength);
        }
    }

    /// Perform waggle dance to communicate location
    pub fn perform_waggle_dance(&mut self, agent_id: AgentId, location: Position, quality: f64) {
        let dance = WaggleDance {
            performer: agent_id,
            location,
            quality,
            timestamp: current_timestamp(),
        };

        self.dances.push_back(dance);

        // Limit dance history
        while self.dances.len() > 100 {
            self.dances.pop_front();
        }
    }

    /// Get recent dances relevant to position
    pub fn get_relevant_dances(&self, position: Position, radius: f64) -> Vec<&WaggleDance> {
        self.dances
            .iter()
            .filter(|d| d.location.distance(&position) < radius)
            .collect()
    }

    /// Calculate flocking forces for an agent
    pub fn calculate_flocking_forces(&self, agent_id: AgentId) -> FlockingForces {
        let position = match self.agent_positions.get(&agent_id) {
            Some(p) => *p,
            None => return FlockingForces::default(),
        };

        let mut separation = Position::new(0.0, 0.0, 0.0);
        let mut alignment = Position::new(0.0, 0.0, 0.0);
        let mut cohesion = Position::new(0.0, 0.0, 0.0);

        let separation_radius = 5.0;
        let alignment_radius = 10.0;
        let cohesion_radius = 20.0;

        let mut neighbor_count = 0;

        for (other_id, other_pos) in &self.agent_positions {
            if *other_id == agent_id {
                continue;
            }

            let distance = position.distance(other_pos);

            // Separation: avoid crowding
            if distance < separation_radius && distance > 0.0 {
                let diff = Position::new(
                    position.x - other_pos.x,
                    position.y - other_pos.y,
                    position.z - other_pos.z,
                );
                let scale = 1.0 / distance;
                separation.x += diff.x * scale;
                separation.y += diff.y * scale;
                separation.z += diff.z * scale;
            }

            // Alignment and cohesion
            if distance < alignment_radius {
                alignment.x += other_pos.x;
                alignment.y += other_pos.y;
                alignment.z += other_pos.z;
                neighbor_count += 1;
            }

            if distance < cohesion_radius {
                cohesion.x += other_pos.x;
                cohesion.y += other_pos.y;
                cohesion.z += other_pos.z;
            }
        }

        if neighbor_count > 0 {
            // Average alignment
            alignment.x /= neighbor_count as f64;
            alignment.y /= neighbor_count as f64;
            alignment.z /= neighbor_count as f64;

            // Average cohesion
            cohesion.x /= neighbor_count as f64;
            cohesion.y /= neighbor_count as f64;
            cohesion.z /= neighbor_count as f64;

            // Steer towards center
            cohesion.x -= position.x;
            cohesion.y -= position.y;
            cohesion.z -= position.z;
        }

        FlockingForces {
            separation,
            alignment,
            cohesion,
        }
    }

    /// Periodic maintenance (decay fields)
    pub fn tick(&mut self) {
        for field in self.fields.values_mut() {
            field.decay();
        }

        // Expire old dances
        let now = current_timestamp();
        while let Some(dance) = self.dances.front() {
            if now - dance.timestamp > 300 {
                // 5 minutes
                self.dances.pop_front();
            } else {
                break;
            }
        }
    }

    fn create_field_message(
        &self,
        field_type: FieldType,
        position: Position,
        value: f64,
    ) -> MessageEnvelope {
        use super::protocol::MessagePriority;
        use super::{Message, MessageType};

        let content = format!(
            "{:?} field detected at {:?} with strength {}",
            field_type, position, value
        );

        MessageEnvelope::new(
            0, // System agent
            Message::new(MessageType::StigmergicSignal, content)
                .with_priority(MessagePriority::Low),
            super::Target::Swarm,
        )
    }
}

impl Default for SwarmCommunication {
    fn default() -> Self {
        Self::new()
    }
}

/// Stigmergic field for indirect coordination
pub struct StigmergicField {
    _field_type: FieldType,
    /// Grid of pheromone values
    grid: HashMap<GridCoord, f64>,
    /// Decay rate per tick
    decay_rate: f64,
    /// Diffusion rate
    _diffusion_rate: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct GridCoord {
    x: i64,
    y: i64,
    z: i64,
}

impl StigmergicField {
    pub fn new(field_type: FieldType) -> Self {
        Self {
            _field_type: field_type,
            grid: HashMap::new(),
            decay_rate: 0.05,
            _diffusion_rate: 0.1,
        }
    }

    /// Get value at continuous position
    pub fn value_at(&self, position: Position) -> f64 {
        let coord = position_to_grid(position);

        // Bilinear interpolation from nearest grid points
        let mut total = 0.0;
        let mut weight_sum = 0.0;

        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    let neighbor = GridCoord {
                        x: coord.x + dx,
                        y: coord.y + dy,
                        z: coord.z + dz,
                    };

                    if let Some(&value) = self.grid.get(&neighbor) {
                        let dist =
                            ((dx as f64).powi(2) + (dy as f64).powi(2) + (dz as f64).powi(2))
                                .sqrt();
                        let weight = 1.0 / (1.0 + dist);
                        total += value * weight;
                        weight_sum += weight;
                    }
                }
            }
        }

        if weight_sum > 0.0 {
            total / weight_sum
        } else {
            0.0
        }
    }

    /// Deposit pheromone at position
    pub fn deposit(&mut self, position: Position, strength: f64) {
        let coord = position_to_grid(position);
        *self.grid.entry(coord).or_insert(0.0) += strength;
    }

    /// Apply decay and diffusion
    pub fn decay(&mut self) {
        // Decay
        for value in self.grid.values_mut() {
            *value *= 1.0 - self.decay_rate;
        }

        // Remove negligible values
        self.grid.retain(|_, v| *v > 0.01);
    }
}

/// Waggle dance communicating resource location
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WaggleDance {
    pub performer: AgentId,
    pub location: Position,
    pub quality: f64,
    pub timestamp: u64,
}

/// Flocking behavior forces
#[derive(Clone, Copy, Debug, Default)]
pub struct FlockingForces {
    pub separation: Position,
    pub alignment: Position,
    pub cohesion: Position,
}

impl FlockingForces {
    /// Combined force vector
    pub fn combined(
        &self,
        separation_weight: f64,
        alignment_weight: f64,
        cohesion_weight: f64,
    ) -> Position {
        Position::new(
            self.separation.x * separation_weight
                + self.alignment.x * alignment_weight
                + self.cohesion.x * cohesion_weight,
            self.separation.y * separation_weight
                + self.alignment.y * alignment_weight
                + self.cohesion.y * cohesion_weight,
            self.separation.z * separation_weight
                + self.alignment.z * alignment_weight
                + self.cohesion.z * cohesion_weight,
        )
    }
}

fn position_to_grid(position: Position) -> GridCoord {
    GridCoord {
        x: (position.x / 10.0) as i64,
        y: (position.y / 10.0) as i64,
        z: (position.z / 10.0) as i64,
    }
}

fn current_timestamp() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
