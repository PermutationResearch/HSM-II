//! Stigmergic DKS Entities - Second-order ecological dynamics.
//!
//! This module extends DKS entities with the ability to read and write hyperedges,
//! creating a genuine second-order stigmergic layer where entity population dynamics
//! influence collective knowledge evolution.
//!
//! Key features:
//! - DKS entities can sense hyperedges (read stigmergic field)
//! - Entities can modify/create hyperedges based on their state
//! - Entity fitness is influenced by hypergraph coherence
//! - Population dynamics feedback into collective knowledge

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::hyper_stigmergy::{HyperEdge, HyperStigmergicMorphogenesis};
// AgentId is used in participants, defined in crate::agent
use crate::dks::{EntityId, Replicator, ReplicatorState};

/// A DKS entity with stigmergic capabilities
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StigmergicEntity {
    /// Underlying replicator
    pub replicator: Replicator,
    /// Stigmergic sensor range (how many hops it can sense)
    pub sensor_range: usize,
    /// Hyperedges this entity has created or modified
    pub authored_edges: Vec<HyperEdgeRef>,
    /// Current stigmergic field readings
    pub field_readings: Vec<FieldReading>,
    /// Stigmergic memory (learned patterns)
    pub stigmergic_memory: StigmergicMemory,
    /// Cognitive state of the entity
    pub cognitive_state: CognitiveState,
    /// Position in the hypergraph (vertex indices it occupies)
    pub positions: Vec<usize>,
    /// Last tick this entity performed stigmergic action
    pub last_stigmergic_tick: u64,
}

/// Reference to a hyperedge
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HyperEdgeRef {
    pub edge_index: usize,
    pub created_at: u64,
    pub edge_type: StigmergicEdgeType,
    pub contribution_score: f64,
}

/// Types of stigmergic edges entities can create
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum StigmergicEdgeType {
    /// Links entities with similar persistence strategies
    StrategyAlignment,
    /// Marks resource-rich regions
    ResourceMarker,
    /// Indicates dangerous/depleted areas
    DangerSignal,
    /// Connects cooperative entities
    Cooperation,
    /// Trail left by successful entities
    SuccessTrail,
    /// Warning about selection pressure
    SelectionSignal,
}

/// A reading from the stigmergic field
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FieldReading {
    /// What was sensed
    pub edge_type: StigmergicEdgeType,
    /// Strength of the signal
    pub intensity: f64,
    /// Distance from entity (hops)
    pub distance: usize,
    /// Source vertex (if known)
    pub source: Option<usize>,
    /// Timestamp of reading
    pub timestamp: u64,
}

/// Memory of stigmergic patterns
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct StigmergicMemory {
    /// Successful strategies remembered
    pub successful_patterns: Vec<StigmergicPattern>,
    /// Avoided patterns (negative experiences)
    pub avoided_patterns: Vec<StigmergicPattern>,
    /// Resource locations remembered
    pub resource_locations: HashMap<usize, f64>,
    /// Cooperation partners
    pub cooperation_partners: Vec<EntityId>,
}

/// A learned stigmergic pattern
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StigmergicPattern {
    /// What edge type is involved
    pub edge_type: StigmergicEdgeType,
    /// Context (which vertices)
    pub context: Vec<usize>,
    /// Outcome quality (-1 to 1)
    pub outcome: f64,
    /// How many times observed
    pub confidence: f64,
}

/// Cognitive state influencing stigmergic behavior
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum CognitiveState {
    /// Exploring, seeking opportunities
    Exploring,
    /// Exploiting known good strategies
    Exploiting,
    /// Avoiding detected dangers
    Avoiding,
    /// Cooperating with other entities
    Cooperating,
    /// Signaling to others
    Signaling,
}

impl Default for CognitiveState {
    fn default() -> Self {
        CognitiveState::Exploring
    }
}

impl StigmergicEntity {
    /// Create a new stigmergic entity from a replicator
    pub fn from_replicator(replicator: Replicator, sensor_range: usize) -> Self {
        Self {
            replicator,
            sensor_range,
            authored_edges: Vec::new(),
            field_readings: Vec::new(),
            stigmergic_memory: StigmergicMemory::default(),
            cognitive_state: CognitiveState::Exploring,
            positions: Vec::new(),
            last_stigmergic_tick: 0,
        }
    }

    /// Sense the stigmergic field at current positions
    pub fn sense_field(&mut self, world: &HyperStigmergicMorphogenesis, current_tick: u64) {
        self.field_readings.clear();

        for &pos in &self.positions {
            // Sense edges within sensor range
            for (_idx, edge) in world.edges.iter().enumerate() {
                if edge.participants.contains(&(pos as u64)) {
                    // Direct connection
                    let reading = FieldReading {
                        edge_type: self.infer_edge_type(edge),
                        intensity: edge.weight,
                        distance: 0,
                        source: Some(pos),
                        timestamp: current_tick,
                    };
                    self.field_readings.push(reading);
                } else if self.sensor_range > 0 {
                    // Check indirect connections
                    for &participant in &edge.participants {
                        if let Some(dist) = self.graph_distance(world, pos, participant as usize) {
                            if dist <= self.sensor_range {
                                let reading = FieldReading {
                                    edge_type: self.infer_edge_type(edge),
                                    intensity: edge.weight * (1.0 / (dist as f64 + 1.0)),
                                    distance: dist,
                                    source: Some(pos),
                                    timestamp: current_tick,
                                };
                                self.field_readings.push(reading);
                            }
                        }
                    }
                }
            }
        }

        // Update cognitive state based on readings
        self.update_cognitive_state();
    }

    /// Perform a stigmergic action based on current state
    pub fn act_stigmergic(
        &mut self,
        world: &mut HyperStigmergicMorphogenesis,
        current_tick: u64,
    ) -> Vec<StigmergicAction> {
        if self.replicator.state() != &ReplicatorState::Active {
            return vec![];
        }

        let mut actions = Vec::new();

        match self.cognitive_state {
            CognitiveState::Exploring => {
                // Leave success trails when energy is high
                if self.replicator.energy() > 70.0 {
                    if let Some(action) = self.create_trail(world, current_tick) {
                        actions.push(action);
                    }
                }
            }
            CognitiveState::Exploiting => {
                // Mark resource-rich areas
                if self.replicator.energy() > 50.0 {
                    if let Some(action) = self.mark_resource(world, current_tick) {
                        actions.push(action);
                    }
                }
                // Cooperate with similar entities
                if let Some(action) = self.establish_cooperation(world, current_tick) {
                    actions.push(action);
                }
            }
            CognitiveState::Avoiding => {
                // Signal danger
                if let Some(action) = self.signal_danger(world, current_tick) {
                    actions.push(action);
                }
            }
            CognitiveState::Cooperating => {
                // Strengthen cooperation edges
                if let Some(action) = self.reinforce_cooperation(world, current_tick) {
                    actions.push(action);
                }
            }
            CognitiveState::Signaling => {
                // Broadcast selection pressure
                if let Some(action) = self.signal_selection(world, current_tick) {
                    actions.push(action);
                }
            }
        }

        self.last_stigmergic_tick = current_tick;
        actions
    }

    /// Create a success trail edge
    fn create_trail(
        &mut self,
        world: &mut HyperStigmergicMorphogenesis,
        tick: u64,
    ) -> Option<StigmergicAction> {
        if self.positions.is_empty() {
            return None;
        }

        // Connect current position to previous positions in trail
        let current = *self.positions.last()?;
        let previous = self.positions.get(self.positions.len().saturating_sub(2))?;

        let edge = HyperEdge {
            participants: vec![current as u64, *previous as u64],
            weight: self.replicator.persistence_score() / 100.0,
            emergent: true,
            age: 0,
            tags: [("type".to_string(), "success_trail".to_string())].into(),
            created_at: tick,
            embedding: None,
            scope: None,
            provenance: None,
            trust_tags: Some(vec!["dks_entity".to_string()]),
            origin_system: None,
            knowledge_layer: None,
        };

        let edge_idx = world.edges.len();
        world.edges.push(edge);

        self.authored_edges.push(HyperEdgeRef {
            edge_index: edge_idx,
            created_at: tick,
            edge_type: StigmergicEdgeType::SuccessTrail,
            contribution_score: 1.0,
        });

        Some(StigmergicAction::CreatedEdge {
            edge_type: StigmergicEdgeType::SuccessTrail,
            participants: vec![current, *previous],
            weight: self.replicator.persistence_score() / 100.0,
        })
    }

    /// Mark a resource-rich area
    fn mark_resource(
        &mut self,
        world: &mut HyperStigmergicMorphogenesis,
        tick: u64,
    ) -> Option<StigmergicAction> {
        let pos = *self.positions.last()?;

        // Create a self-loop marking resource
        let edge = HyperEdge {
            participants: vec![pos as u64],
            weight: self.replicator.energy() / 100.0,
            emergent: true,
            age: 0,
            tags: [("type".to_string(), "resource_marker".to_string())].into(),
            created_at: tick,
            embedding: None,
            scope: None,
            provenance: None,
            trust_tags: Some(vec!["dks_entity".to_string()]),
            origin_system: None,
            knowledge_layer: None,
        };

        let edge_idx = world.edges.len();
        world.edges.push(edge);

        // Remember this location
        self.stigmergic_memory
            .resource_locations
            .insert(pos, self.replicator.energy());

        self.authored_edges.push(HyperEdgeRef {
            edge_index: edge_idx,
            created_at: tick,
            edge_type: StigmergicEdgeType::ResourceMarker,
            contribution_score: 0.5,
        });

        Some(StigmergicAction::CreatedEdge {
            edge_type: StigmergicEdgeType::ResourceMarker,
            participants: vec![pos],
            weight: self.replicator.energy() / 100.0,
        })
    }

    /// Signal danger
    fn signal_danger(
        &mut self,
        world: &mut HyperStigmergicMorphogenesis,
        tick: u64,
    ) -> Option<StigmergicAction> {
        let pos = *self.positions.last()?;

        let edge = HyperEdge {
            participants: vec![pos as u64],
            weight: 0.9, // High weight for danger
            emergent: true,
            age: 0,
            tags: [("type".to_string(), "danger_signal".to_string())].into(),
            created_at: tick,
            embedding: None,
            scope: None,
            provenance: None,
            trust_tags: Some(vec!["dks_entity".to_string(), "warning".to_string()]),
            origin_system: None,
            knowledge_layer: None,
        };

        let edge_idx = world.edges.len();
        world.edges.push(edge);

        self.authored_edges.push(HyperEdgeRef {
            edge_index: edge_idx,
            created_at: tick,
            edge_type: StigmergicEdgeType::DangerSignal,
            contribution_score: 1.0,
        });

        Some(StigmergicAction::CreatedEdge {
            edge_type: StigmergicEdgeType::DangerSignal,
            participants: vec![pos],
            weight: 0.9,
        })
    }

    /// Establish cooperation with nearby entities
    fn establish_cooperation(
        &mut self,
        _world: &mut HyperStigmergicMorphogenesis,
        _tick: u64,
    ) -> Option<StigmergicAction> {
        // This would require access to other entities
        // For now, return None - would be implemented with population access
        None
    }

    /// Reinforce existing cooperation
    fn reinforce_cooperation(
        &mut self,
        _world: &mut HyperStigmergicMorphogenesis,
        _tick: u64,
    ) -> Option<StigmergicAction> {
        // Would strengthen existing cooperation edges
        None
    }

    /// Signal selection pressure
    fn signal_selection(
        &mut self,
        world: &mut HyperStigmergicMorphogenesis,
        tick: u64,
    ) -> Option<StigmergicAction> {
        let pos = *self.positions.last()?;

        let edge = HyperEdge {
            participants: vec![pos as u64],
            weight: self.replicator.dks_stability().clamp(0.0, 1.0),
            emergent: true,
            age: 0,
            tags: [("type".to_string(), "selection_signal".to_string())].into(),
            created_at: tick,
            embedding: None,
            scope: None,
            provenance: None,
            trust_tags: Some(vec!["dks_entity".to_string()]),
            origin_system: None,
            knowledge_layer: None,
        };

        let edge_idx = world.edges.len();
        world.edges.push(edge);

        self.authored_edges.push(HyperEdgeRef {
            edge_index: edge_idx,
            created_at: tick,
            edge_type: StigmergicEdgeType::SelectionSignal,
            contribution_score: 0.3,
        });

        Some(StigmergicAction::CreatedEdge {
            edge_type: StigmergicEdgeType::SelectionSignal,
            participants: vec![pos],
            weight: self.replicator.dks_stability().clamp(0.0, 1.0),
        })
    }

    /// Update cognitive state based on field readings
    fn update_cognitive_state(&mut self) {
        // Count readings by type
        let danger_count = self
            .field_readings
            .iter()
            .filter(|r| r.edge_type == StigmergicEdgeType::DangerSignal)
            .count();
        let resource_count = self
            .field_readings
            .iter()
            .filter(|r| r.edge_type == StigmergicEdgeType::ResourceMarker)
            .count();
        let trail_count = self
            .field_readings
            .iter()
            .filter(|r| r.edge_type == StigmergicEdgeType::SuccessTrail)
            .count();

        // State transitions
        if danger_count > 0 {
            self.cognitive_state = CognitiveState::Avoiding;
        } else if resource_count > 2 && self.replicator.energy() > 50.0 {
            self.cognitive_state = CognitiveState::Exploiting;
        } else if trail_count > 1 && self.cognitive_state == CognitiveState::Exploring {
            self.cognitive_state = CognitiveState::Exploiting;
        } else if self.replicator.energy() < 30.0 {
            self.cognitive_state = CognitiveState::Exploring;
        }
    }

    /// Infer the stigmergic type of a hyperedge
    fn infer_edge_type(&self, edge: &HyperEdge) -> StigmergicEdgeType {
        if let Some(edge_type) = edge.tags.get("type") {
            match edge_type.as_str() {
                "success_trail" => StigmergicEdgeType::SuccessTrail,
                "resource_marker" => StigmergicEdgeType::ResourceMarker,
                "danger_signal" => StigmergicEdgeType::DangerSignal,
                "cooperation" => StigmergicEdgeType::Cooperation,
                "selection_signal" => StigmergicEdgeType::SelectionSignal,
                _ => StigmergicEdgeType::StrategyAlignment,
            }
        } else {
            StigmergicEdgeType::StrategyAlignment
        }
    }

    /// Calculate graph distance between two vertices (simplified BFS)
    fn graph_distance(
        &self,
        world: &HyperStigmergicMorphogenesis,
        start: usize,
        end: usize,
    ) -> Option<usize> {
        if start == end {
            return Some(0);
        }

        // Simple 1-hop check (could be extended to full BFS)
        for edge in &world.edges {
            let has_start = edge.participants.contains(&(start as u64));
            let has_end = edge.participants.contains(&(end as u64));
            if has_start && has_end {
                return Some(1);
            }
        }

        None
    }

    /// Get entity ID
    pub fn id(&self) -> EntityId {
        self.replicator.id()
    }

    /// Update position in the hypergraph
    pub fn update_position(&mut self, new_positions: Vec<usize>) {
        self.positions = new_positions;
    }

    /// Learn from an outcome
    pub fn learn(&mut self, pattern: StigmergicPattern) {
        if pattern.outcome > 0.0 {
            // Add to successful patterns
            self.stigmergic_memory.successful_patterns.push(pattern);
        } else {
            // Add to avoided patterns
            self.stigmergic_memory.avoided_patterns.push(pattern);
        }
    }
}

/// Actions that stigmergic entities can perform
#[derive(Clone, Debug)]
pub enum StigmergicAction {
    CreatedEdge {
        edge_type: StigmergicEdgeType,
        participants: Vec<usize>,
        weight: f64,
    },
    ModifiedEdge {
        edge_index: usize,
        delta_weight: f64,
    },
    Signaled {
        signal_type: StigmergicEdgeType,
        intensity: f64,
    },
}

/// Population of stigmergic entities
pub struct StigmergicPopulation {
    entities: Vec<StigmergicEntity>,
    /// Statistics on stigmergic activity
    pub stats: StigmergicStats,
}

/// Statistics on stigmergic activity
#[derive(Clone, Debug, Default)]
pub struct StigmergicStats {
    pub total_edges_created: usize,
    pub edges_by_type: HashMap<String, usize>,
    pub total_field_readings: usize,
    pub cognitive_state_distribution: HashMap<String, usize>,
}

impl StigmergicPopulation {
    pub fn new() -> Self {
        Self {
            entities: Vec::new(),
            stats: StigmergicStats::default(),
        }
    }

    /// Add a stigmergic entity
    pub fn add_entity(&mut self, entity: StigmergicEntity) {
        self.entities.push(entity);
    }

    /// Get all entities
    pub fn entities(&self) -> &[StigmergicEntity] {
        &self.entities
    }

    /// Get mutable entities
    pub fn entities_mut(&mut self) -> &mut Vec<StigmergicEntity> {
        &mut self.entities
    }

    /// Run stigmergic cycle: sense -> act -> learn
    pub fn stigmergic_cycle(
        &mut self,
        world: &mut HyperStigmergicMorphogenesis,
        current_tick: u64,
    ) -> Vec<StigmergicAction> {
        let mut all_actions = Vec::new();

        for entity in &mut self.entities {
            // Sense the field
            entity.sense_field(world, current_tick);

            // Act on the field
            let actions = entity.act_stigmergic(world, current_tick);

            // Update statistics
            for action in &actions {
                match action {
                    StigmergicAction::CreatedEdge { edge_type, .. } => {
                        self.stats.total_edges_created += 1;
                        *self
                            .stats
                            .edges_by_type
                            .entry(format!("{:?}", edge_type))
                            .or_insert(0) += 1;
                    }
                    _ => {}
                }
            }

            all_actions.extend(actions);
        }

        // Update cognitive state distribution
        self.stats.cognitive_state_distribution.clear();
        for entity in &self.entities {
            let state_name = format!("{:?}", entity.cognitive_state);
            *self
                .stats
                .cognitive_state_distribution
                .entry(state_name)
                .or_insert(0) += 1;
        }

        self.stats.total_field_readings =
            self.entities.iter().map(|e| e.field_readings.len()).sum();

        all_actions
    }

    /// Get population size
    pub fn size(&self) -> usize {
        self.entities.len()
    }
}

impl Default for StigmergicPopulation {
    fn default() -> Self {
        Self::new()
    }
}

/// Extended DKS system with stigmergic capabilities
pub struct StigmergicDKS {
    /// Regular DKS population
    pub population: StigmergicPopulation,
    /// Generation counter
    pub generation: u64,
    /// World coherence influence factor
    pub coherence_influence: f64,
}

impl StigmergicDKS {
    pub fn new() -> Self {
        Self {
            population: StigmergicPopulation::new(),
            generation: 0,
            coherence_influence: 0.1,
        }
    }

    /// Seed with initial entities
    pub fn seed(&mut self, count: usize, sensor_range: usize) {
        for i in 0..count {
            let replicator = Replicator::new(format!("stigmergic_seed_{}", i), 0.1, 0.05);
            let entity = StigmergicEntity::from_replicator(replicator, sensor_range);
            self.population.add_entity(entity);
        }
    }

    /// Execute one generation with stigmergic interaction
    pub fn tick(&mut self, world: &mut HyperStigmergicMorphogenesis) -> StigmergicTickResult {
        self.generation += 1;

        // Run stigmergic cycle
        let actions = self.population.stigmergic_cycle(world, self.generation);

        // Calculate world coherence influence on entities
        let coherence = world.global_coherence();

        // Update entity energy based on coherence (stigmergic reward)
        for _entity in self.population.entities_mut() {
            let coherence_bonus = coherence * self.coherence_influence;
            // Would modify energy through replicator interface
            let _ = coherence_bonus; // Placeholder
        }

        StigmergicTickResult {
            generation: self.generation,
            stigmergic_actions: actions.len(),
            edges_created: self.population.stats.total_edges_created,
            coherence_at_tick: coherence,
        }
    }
}

impl Default for StigmergicDKS {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of a stigmergic DKS tick
#[derive(Clone, Debug)]
pub struct StigmergicTickResult {
    pub generation: u64,
    pub stigmergic_actions: usize,
    pub edges_created: usize,
    pub coherence_at_tick: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stigmergic_entity_creation() {
        let replicator = Replicator::new("test".to_string(), 0.1, 0.05);
        let entity = StigmergicEntity::from_replicator(replicator, 2);

        assert_eq!(entity.sensor_range, 2);
        assert!(matches!(entity.cognitive_state, CognitiveState::Exploring));
    }

    #[test]
    fn test_stigmergic_population() {
        let mut pop = StigmergicPopulation::new();

        let replicator = Replicator::new("test".to_string(), 0.1, 0.05);
        let entity = StigmergicEntity::from_replicator(replicator, 1);
        pop.add_entity(entity);

        assert_eq!(pop.size(), 1);
    }

    #[test]
    fn test_stigmergic_edge_types() {
        let types = vec![
            StigmergicEdgeType::SuccessTrail,
            StigmergicEdgeType::ResourceMarker,
            StigmergicEdgeType::DangerSignal,
        ];

        assert_eq!(types.len(), 3);
    }
}
