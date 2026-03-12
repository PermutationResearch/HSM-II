use std::collections::{HashMap, VecDeque};

use crate::embedding_index::InMemoryEmbeddingIndex;
use crate::federation::conflict::{ConflictMediator, ConflictRecord, ConflictResolution};
use crate::federation::trust::TrustGraph;
use crate::federation::types::{
    EdgeScope, FederationConfig, HyperedgeInjectionRequest, ImportResult, KnowledgeLayer,
    MetaHyperedge, PromotedEdge, Provenance, SharedEdge, SharedVertexMeta, Subscription,
    SubscriptionFilter, SystemId, SystemInfo,
};
use crate::hyper_stigmergy::HyperStigmergicMorphogenesis;

/// The shared meta-hypergraph H* — the central federation integration point.
/// Each federated instance projects its shared-scope edges into H*, and
/// H* mediates cross-system knowledge, conflict resolution, and promotion.
pub struct MetaGraph {
    pub shared_edges: Vec<SharedEdge>,
    pub shared_vertex_meta: HashMap<String, SharedVertexMeta>,
    pub shared_embedding_index: InMemoryEmbeddingIndex,
    pub known_systems: HashMap<SystemId, SystemInfo>,
    pub trust_graph: TrustGraph,
    pub promoted_edges: Vec<PromotedEdge>,
    pub conflict_history: Vec<ConflictRecord>,
    pub subscriptions: Vec<Subscription>,
    pub pending_imports: VecDeque<HyperedgeInjectionRequest>,
    pub conflict_mediator: ConflictMediator,
    pub local_system_id: SystemId,
}

impl MetaGraph {
    pub fn new(config: &FederationConfig) -> Self {
        let mut trust_graph = TrustGraph::default();
        trust_graph.default_trust = config.trust_threshold;

        // Bootstrap trust for known peers
        for peer in &config.known_peers {
            trust_graph.bootstrap_peer(&config.system_id, peer, 0);
        }

        Self {
            shared_edges: Vec::new(),
            shared_vertex_meta: HashMap::new(),
            shared_embedding_index: InMemoryEmbeddingIndex::new(768),
            known_systems: HashMap::new(),
            trust_graph,
            promoted_edges: Vec::new(),
            conflict_history: Vec::new(),
            subscriptions: Vec::new(),
            pending_imports: VecDeque::new(),
            conflict_mediator: ConflictMediator::default(),
            local_system_id: config.system_id.clone(),
        }
    }

    /// Project local shared-scope edges from the world into the shared meta-graph.
    pub fn project_to_shared(
        &mut self,
        world: &HyperStigmergicMorphogenesis,
        system_id: &SystemId,
    ) {
        for edge in &world.edges {
            let is_shared = match &edge.scope {
                Some(EdgeScope::Shared) => true,
                Some(EdgeScope::Restricted(systems)) => systems.contains(system_id),
                _ => false,
            };

            if !is_shared {
                continue;
            }

            // Check if this edge already exists in shared (by provenance match)
            let already_exists = self.shared_edges.iter().any(|se| {
                se.provenance.origin_system == *system_id
                    && se.vertices
                        == edge
                            .participants
                            .iter()
                            .map(|p| format!("agent_{}", p))
                            .collect::<Vec<_>>()
            });

            if already_exists {
                continue;
            }

            let shared_edge = SharedEdge {
                id: uuid::Uuid::new_v4().to_string(),
                vertices: edge
                    .participants
                    .iter()
                    .map(|p| format!("agent_{}", p))
                    .collect(),
                edge_type: edge
                    .tags
                    .get("type")
                    .cloned()
                    .unwrap_or_else(|| "agent_agent".to_string()),
                weight: edge.weight,
                provenance: edge.provenance.clone().unwrap_or(Provenance {
                    origin_system: system_id.clone(),
                    created_at: edge.created_at,
                    hop_chain: vec![],
                }),
                layer: edge.knowledge_layer.clone().unwrap_or(KnowledgeLayer::Raw),
                contributing_systems: vec![system_id.clone()],
                trust_tags: edge.trust_tags.clone().unwrap_or_default(),
                embedding: edge.embedding.clone(),
                usage_count: 0,
                success_count: 0,
            };

            // Index embedding if available
            if let Some(ref emb) = shared_edge.embedding {
                self.shared_embedding_index
                    .insert(self.shared_edges.len(), emb.clone());
            }

            self.shared_edges.push(shared_edge);
        }
    }

    /// Import remote edges with trust gating and conflict detection.
    pub fn import_remote_edges(
        &mut self,
        requests: &[HyperedgeInjectionRequest],
        from_system: &SystemId,
        tick: u64,
    ) -> ImportResult {
        let mut result = ImportResult::default();

        for req in requests {
            // Trust gate
            if !self.trust_graph.meets_threshold(
                &self.local_system_id,
                from_system,
                self.trust_graph.default_trust,
            ) {
                result.rejected += 1;
                self.trust_graph
                    .record_failure(&self.local_system_id, from_system, tick);
                continue;
            }

            // Check for scope compatibility
            match &req.scope {
                EdgeScope::Restricted(allowed) if !allowed.contains(&self.local_system_id) => {
                    result.rejected += 1;
                    continue;
                }
                EdgeScope::Local => {
                    result.rejected += 1;
                    continue;
                }
                _ => {}
            }

            // Build shared edge from request
            let mut provenance = req.provenance.clone();
            provenance
                .hop_chain
                .push((self.local_system_id.clone(), tick));

            let shared_edge = SharedEdge {
                id: uuid::Uuid::new_v4().to_string(),
                vertices: req.vertices.clone(),
                edge_type: req.edge_type.clone(),
                weight: req.weight,
                provenance,
                layer: KnowledgeLayer::Raw,
                contributing_systems: vec![from_system.clone()],
                trust_tags: req.trust_tags.clone(),
                embedding: req.embedding.clone(),
                usage_count: 0,
                success_count: 0,
            };

            // Conflict detection against existing edges
            let conflicts = ConflictMediator::detect_conflicts(
                &[&self.shared_edges[..], &[shared_edge.clone()]].concat(),
            );

            if !conflicts.is_empty() {
                result.conflicts += conflicts.len();
                // Resolve each conflict
                for (a_idx, b_idx) in &conflicts {
                    if *b_idx == self.shared_edges.len() {
                        // New edge conflicts with existing
                        let resolution = self.conflict_mediator.resolve(
                            &self.shared_edges[*a_idx],
                            &shared_edge,
                            &self.local_system_id,
                            &self.trust_graph,
                        );

                        self.conflict_history.push(ConflictRecord {
                            edge_a_id: self.shared_edges[*a_idx].id.clone(),
                            edge_b_id: shared_edge.id.clone(),
                            resolution: resolution.clone(),
                            resolved_at: tick,
                        });

                        match resolution {
                            ConflictResolution::PreferB { .. } | ConflictResolution::KeepBoth => {
                                // Accept the new edge
                            }
                            ConflictResolution::PreferA { .. } => {
                                result.rejected += 1;
                                continue;
                            }
                            ConflictResolution::Merge { merged_weight } => {
                                self.shared_edges[*a_idx].weight = merged_weight;
                                if !self.shared_edges[*a_idx]
                                    .contributing_systems
                                    .contains(from_system)
                                {
                                    self.shared_edges[*a_idx]
                                        .contributing_systems
                                        .push(from_system.clone());
                                }
                                result.imported += 1;
                                self.trust_graph.record_success(
                                    &self.local_system_id,
                                    from_system,
                                    tick,
                                );
                                continue;
                            }
                        }
                    }
                }
            }

            // Index and add
            if let Some(ref emb) = shared_edge.embedding {
                self.shared_embedding_index
                    .insert(self.shared_edges.len(), emb.clone());
            }

            self.shared_edges.push(shared_edge);
            result.imported += 1;
            self.trust_graph
                .record_success(&self.local_system_id, from_system, tick);
        }

        result
    }

    /// Align a local vertex name with shared vertices by embedding similarity.
    pub fn align_by_embedding(
        &self,
        local_name: &str,
        local_embedding: &[f32],
        threshold: f32,
    ) -> Vec<&SharedVertexMeta> {
        self.shared_vertex_meta
            .values()
            .filter(|svm| {
                if let Some(ref emb) = svm.embedding {
                    cosine_similarity_f32(local_embedding, emb) >= threshold
                } else {
                    // Fall back to name matching
                    svm.name.to_lowercase().contains(&local_name.to_lowercase())
                        || svm
                            .aliases
                            .iter()
                            .any(|(_, alias)| alias.to_lowercase() == local_name.to_lowercase())
                }
            })
            .collect()
    }

    /// Promote an edge to a higher knowledge layer.
    pub fn promote_edge(&mut self, edge_idx: usize, reason: &str, tick: u64) {
        if edge_idx >= self.shared_edges.len() {
            return;
        }

        let current_layer = self.shared_edges[edge_idx].layer.clone();
        let next_layer = match current_layer {
            KnowledgeLayer::Raw => KnowledgeLayer::Distilled,
            KnowledgeLayer::Distilled => KnowledgeLayer::Validated,
            KnowledgeLayer::Validated => KnowledgeLayer::Meta,
            KnowledgeLayer::Meta => return, // Already at top
        };

        self.promoted_edges.push(PromotedEdge {
            edge_index: edge_idx,
            promoted_at_tick: tick,
            from_layer: current_layer,
            to_layer: next_layer.clone(),
            reason: reason.to_string(),
        });

        self.shared_edges[edge_idx].layer = next_layer;
    }

    /// Query shared edges with optional filters.
    pub fn query_shared(&self, filter: &SubscriptionFilter) -> Vec<&SharedEdge> {
        self.shared_edges
            .iter()
            .filter(|edge| {
                // Filter by edge type
                if let Some(ref types) = filter.edge_types {
                    if !types.contains(&edge.edge_type) {
                        return false;
                    }
                }

                // Filter by minimum trust
                if let Some(min_trust) = filter.min_trust {
                    let trust = self
                        .trust_graph
                        .get_trust(&self.local_system_id, &edge.provenance.origin_system);
                    if trust < min_trust {
                        return false;
                    }
                }

                // Filter by minimum layer
                if let Some(ref min_layer) = filter.min_layer {
                    if edge.layer < *min_layer {
                        return false;
                    }
                }

                true
            })
            .collect()
    }

    /// Detect emergent meta-hyperedges from patterns across shared edges.
    pub fn detect_meta_hyperedges(&self) -> Vec<MetaHyperedge> {
        let mut meta_edges = Vec::new();

        // Pattern 1: Consensus — multiple systems contributed to similar edges
        let mut vertex_system_map: HashMap<Vec<String>, Vec<(usize, Vec<SystemId>)>> =
            HashMap::new();
        for (i, edge) in self.shared_edges.iter().enumerate() {
            let mut key = edge.vertices.clone();
            key.sort();
            vertex_system_map
                .entry(key)
                .or_default()
                .push((i, edge.contributing_systems.clone()));
        }

        for (_vertices, entries) in &vertex_system_map {
            if entries.len() < 2 {
                continue;
            }
            let all_systems: Vec<SystemId> = entries
                .iter()
                .flat_map(|(_, systems)| systems.iter().cloned())
                .collect::<std::collections::HashSet<_>>()
                .into_iter()
                .collect();

            if all_systems.len() >= 2 {
                let edge_indices: Vec<usize> = entries.iter().map(|(i, _)| *i).collect();
                let agreement = edge_indices.len() as f64 / all_systems.len() as f64;

                meta_edges.push(MetaHyperedge::Consensus {
                    systems: all_systems,
                    shared_edge_indices: edge_indices,
                    agreement_score: agreement.min(1.0),
                });
            }
        }

        // Pattern 2: Synthesis — dense cross-domain co-occurrence
        let mut domain_system_edges: HashMap<String, Vec<(usize, SystemId)>> = HashMap::new();
        for (i, edge) in self.shared_edges.iter().enumerate() {
            let domain = edge.edge_type.clone();
            domain_system_edges
                .entry(domain)
                .or_default()
                .push((i, edge.provenance.origin_system.clone()));
        }

        let domains_with_multi_system: Vec<(String, Vec<(usize, SystemId)>)> = domain_system_edges
            .into_iter()
            .filter(|(_, entries)| {
                let unique_systems: std::collections::HashSet<&SystemId> =
                    entries.iter().map(|(_, s)| s).collect();
                unique_systems.len() >= 2
            })
            .collect();

        if domains_with_multi_system.len() >= 2 {
            let all_systems: Vec<SystemId> = domains_with_multi_system
                .iter()
                .flat_map(|(_, entries)| entries.iter().map(|(_, s)| s.clone()))
                .collect::<std::collections::HashSet<_>>()
                .into_iter()
                .collect();
            let domains: Vec<String> = domains_with_multi_system
                .iter()
                .map(|(d, _)| d.clone())
                .collect();
            let edge_indices: Vec<usize> = domains_with_multi_system
                .iter()
                .flat_map(|(_, entries)| entries.iter().map(|(i, _)| *i))
                .collect();

            meta_edges.push(MetaHyperedge::Synthesis {
                systems: all_systems,
                domains,
                edge_indices,
            });
        }

        meta_edges
    }

    /// Add a subscription for push-based edge notifications.
    pub fn add_subscription(&mut self, subscription: Subscription) {
        // Remove existing subscription from same system
        self.subscriptions
            .retain(|s| s.subscriber_system != subscription.subscriber_system);
        self.subscriptions.push(subscription);
    }

    /// Remove a subscription by system ID.
    pub fn remove_subscription(&mut self, system_id: &SystemId) {
        self.subscriptions
            .retain(|s| &s.subscriber_system != system_id);
    }

    /// Find subscriptions that match a given edge.
    pub fn matching_subscriptions(&self, edge: &SharedEdge) -> Vec<&Subscription> {
        self.subscriptions
            .iter()
            .filter(|sub| {
                let filter = &sub.filter;

                if let Some(ref types) = filter.edge_types {
                    if !types.contains(&edge.edge_type) {
                        return false;
                    }
                }

                if let Some(min_trust) = filter.min_trust {
                    let trust = self
                        .trust_graph
                        .get_trust(&self.local_system_id, &edge.provenance.origin_system);
                    if trust < min_trust {
                        return false;
                    }
                }

                if let Some(ref min_layer) = filter.min_layer {
                    if edge.layer < *min_layer {
                        return false;
                    }
                }

                true
            })
            .collect()
    }

    /// Process all pending import requests.
    pub fn process_pending_imports(&mut self, tick: u64) -> ImportResult {
        let mut total_result = ImportResult::default();

        while let Some(req) = self.pending_imports.pop_front() {
            let from = req.provenance.origin_system.clone();
            let result = self.import_remote_edges(&[req], &from, tick);
            total_result.imported += result.imported;
            total_result.rejected += result.rejected;
            total_result.conflicts += result.conflicts;
        }

        total_result
    }

    /// Auto-promote edges that have been used by multiple systems and are old enough.
    pub fn auto_promote(&mut self, tick: u64, promote_after: u64) {
        let indices_to_promote: Vec<usize> = self
            .shared_edges
            .iter()
            .enumerate()
            .filter(|(_, edge)| {
                edge.contributing_systems.len() >= 2
                    && edge.layer == KnowledgeLayer::Raw
                    && tick.saturating_sub(edge.provenance.created_at) >= promote_after
            })
            .map(|(i, _)| i)
            .collect();

        for idx in indices_to_promote {
            self.promote_edge(idx, "auto-promoted: multi-system reuse", tick);
        }
    }
}

fn cosine_similarity_f32(a: &[f32], b: &[f32]) -> f32 {
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
