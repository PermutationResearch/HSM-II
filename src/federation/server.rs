use std::sync::Arc;
use tokio::sync::RwLock;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
    routing::{delete, get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use tower_http::cors::CorsLayer;

use super::types::*;
use crate::hyper_stigmergy::HyperStigmergicMorphogenesis;
use crate::meta_graph::MetaGraph;

/// Shared application state for the federation HTTP server.
#[derive(Clone)]
pub struct FederationState {
    pub meta_graph: Arc<RwLock<MetaGraph>>,
    pub world: Arc<RwLock<HyperStigmergicMorphogenesis>>,
    pub current_tick: Arc<RwLock<u64>>,
}

/// The federation HTTP server exposing cross-instance APIs.
pub struct FederationServer;

impl FederationServer {
    /// Build the axum router with all federation endpoints.
    pub fn router(state: FederationState) -> Router {
        Router::new()
            .route("/hyperedges", post(import_hyperedges))
            .route("/hyperedges", get(query_hyperedges))
            .route("/subscribe", post(add_subscription))
            .route("/subscribe/{system_id}", delete(remove_subscription))
            .route("/system/info", get(system_info))
            .route("/consensus/vote", post(submit_consensus_vote))
            .route("/trust/{system_id}", get(get_trust_score))
            .route("/meta/layers", get(query_meta_layers))
            .layer(CorsLayer::permissive())
            .with_state(state)
    }

    /// Start the federation server on the given address.
    pub async fn serve(addr: &str, state: FederationState) -> anyhow::Result<()> {
        let router = Self::router(state);
        let listener = tokio::net::TcpListener::bind(addr).await?;
        tracing::info!("Federation server listening on {}", addr);
        axum::serve(listener, router).await?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Request / response types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct ImportRequest {
    edges: Vec<HyperedgeInjectionRequest>,
    from_system: SystemId,
}

#[derive(Serialize)]
struct ImportResponse {
    result: ImportResult,
    injected: Vec<InjectedEdge>,
}

#[derive(Deserialize)]
struct HyperedgeQuery {
    edge_types: Option<String>, // comma-separated
    min_trust: Option<f64>,
    min_layer: Option<String>, // "Raw", "Distilled", "Validated", "Meta"
}

#[derive(Deserialize)]
struct SubscribeRequest {
    subscriber_system: SystemId,
    callback_url: String,
    filter: SubscriptionFilter,
}

#[derive(Deserialize)]
struct VoteRequest {
    vote: CrossSystemVote,
}

#[derive(Deserialize)]
struct MetaLayerQuery {
    min_layer: Option<String>,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// POST /hyperedges — Import edges from a remote system (trust-gated).
async fn import_hyperedges(
    State(state): State<FederationState>,
    Json(req): Json<ImportRequest>,
) -> Result<Json<ImportResponse>, StatusCode> {
    let tick = *state.current_tick.read().await;
    let mut mg = state.meta_graph.write().await;

    let result = mg.import_remote_edges(&req.edges, &req.from_system, tick);

    // Build per-edge result list
    let injected: Vec<InjectedEdge> = req
        .edges
        .iter()
        .enumerate()
        .map(|(i, _edge)| {
            let accepted = i < result.imported;
            InjectedEdge {
                edge_id: if accepted {
                    mg.shared_edges
                        .get(mg.shared_edges.len().saturating_sub(result.imported) + i)
                        .map(|e| e.id.clone())
                        .unwrap_or_default()
                } else {
                    String::new()
                },
                accepted,
                reason: if accepted {
                    "imported".to_string()
                } else {
                    "rejected or merged".to_string()
                },
            }
        })
        .collect();

    Ok(Json(ImportResponse { result, injected }))
}

/// GET /hyperedges — Query shared edges with optional filters.
async fn query_hyperedges(
    State(state): State<FederationState>,
    Query(params): Query<HyperedgeQuery>,
) -> Json<Vec<SharedEdge>> {
    let mg = state.meta_graph.read().await;

    let filter = SubscriptionFilter {
        edge_types: params
            .edge_types
            .map(|s| s.split(',').map(|t| t.trim().to_string()).collect()),
        min_trust: params.min_trust,
        domains: None,
        min_layer: params.min_layer.and_then(|s| parse_layer(&s)),
    };

    let edges: Vec<SharedEdge> = mg.query_shared(&filter).into_iter().cloned().collect();
    Json(edges)
}

/// POST /subscribe — Register a subscription for push-based notifications.
async fn add_subscription(
    State(state): State<FederationState>,
    Json(req): Json<SubscribeRequest>,
) -> StatusCode {
    let tick = *state.current_tick.read().await;
    let mut mg = state.meta_graph.write().await;

    mg.add_subscription(Subscription {
        subscriber_system: req.subscriber_system,
        callback_url: req.callback_url,
        filter: req.filter,
        created_at: tick,
    });

    StatusCode::OK
}

/// DELETE /subscribe/:system_id — Remove a subscription.
async fn remove_subscription(
    State(state): State<FederationState>,
    Path(system_id): Path<SystemId>,
) -> StatusCode {
    let mut mg = state.meta_graph.write().await;
    mg.remove_subscription(&system_id);
    StatusCode::OK
}

/// GET /system/info — Return this system's basic info.
async fn system_info(State(state): State<FederationState>) -> Json<SystemInfo> {
    let mg = state.meta_graph.read().await;
    let world = state.world.read().await;
    let tick = *state.current_tick.read().await;

    Json(SystemInfo {
        system_id: mg.local_system_id.clone(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        vertex_count: world.agents.len(),
        edge_count: world.edges.len(),
        uptime_ticks: tick,
    })
}

/// POST /consensus/vote — Submit a cross-system consensus vote.
async fn submit_consensus_vote(
    State(state): State<FederationState>,
    Json(req): Json<VoteRequest>,
) -> StatusCode {
    let tick = *state.current_tick.read().await;
    let mut mg = state.meta_graph.write().await;

    // Record the vote as a trust interaction (voting is a positive signal)
    let local_id = mg.local_system_id.clone();
    mg.trust_graph
        .record_success(&local_id, &req.vote.voter_system, tick);

    // Store the vote in pending imports metadata for later processing
    // The conductor's federation_tick will pick this up
    let vote_edge = HyperedgeInjectionRequest {
        vertices: vec![req.vote.skill_id.clone(), req.vote.voter_system.clone()],
        edge_type: "consensus_vote".to_string(),
        scope: EdgeScope::Shared,
        trust_tags: vec!["vote".to_string()],
        provenance: Provenance {
            origin_system: req.vote.voter_system.clone(),
            created_at: tick,
            hop_chain: vec![],
        },
        weight: req.vote.confidence,
        embedding: None,
        metadata: {
            let mut m = std::collections::HashMap::new();
            m.insert("verdict".to_string(), format!("{:?}", req.vote.verdict));
            m.insert("evidence".to_string(), req.vote.evidence.clone());
            m
        },
    };

    mg.pending_imports.push_back(vote_edge);
    StatusCode::OK
}

/// GET /trust/:system_id — Query the trust score for a remote system.
async fn get_trust_score(
    State(state): State<FederationState>,
    Path(system_id): Path<SystemId>,
) -> Json<TrustScoreResponse> {
    let mg = state.meta_graph.read().await;
    let score = mg.trust_graph.get_trust(&mg.local_system_id, &system_id);
    let edge = mg
        .trust_graph
        .edges
        .get(&(mg.local_system_id.clone(), system_id.clone()));

    Json(TrustScoreResponse {
        from_system: mg.local_system_id.clone(),
        to_system: system_id,
        score,
        successful_imports: edge.map(|e| e.successful_imports).unwrap_or(0),
        failed_imports: edge.map(|e| e.failed_imports).unwrap_or(0),
    })
}

#[derive(Serialize)]
struct TrustScoreResponse {
    from_system: SystemId,
    to_system: SystemId,
    score: f64,
    successful_imports: u64,
    failed_imports: u64,
}

/// GET /meta/layers — Query meta-hyperedges, optionally filtered by minimum layer.
async fn query_meta_layers(
    State(state): State<FederationState>,
    Query(params): Query<MetaLayerQuery>,
) -> Json<MetaLayerResponse> {
    let mg = state.meta_graph.read().await;

    let meta_hyperedges = mg.detect_meta_hyperedges();

    // Count edges by layer
    let mut layer_counts = std::collections::HashMap::new();
    for edge in &mg.shared_edges {
        *layer_counts
            .entry(format!("{:?}", edge.layer))
            .or_insert(0usize) += 1;
    }

    // If min_layer filter is specified, filter shared edges
    let filtered_edges: Vec<SharedEdge> = if let Some(ref min_layer_str) = params.min_layer {
        if let Some(min_layer) = parse_layer(min_layer_str) {
            mg.shared_edges
                .iter()
                .filter(|e| e.layer >= min_layer)
                .cloned()
                .collect()
        } else {
            mg.shared_edges.clone()
        }
    } else {
        mg.shared_edges.clone()
    };

    Json(MetaLayerResponse {
        meta_hyperedges,
        layer_counts,
        edges: filtered_edges,
        promoted_count: mg.promoted_edges.len(),
    })
}

#[derive(Serialize)]
struct MetaLayerResponse {
    meta_hyperedges: Vec<MetaHyperedge>,
    layer_counts: std::collections::HashMap<String, usize>,
    edges: Vec<SharedEdge>,
    promoted_count: usize,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn parse_layer(s: &str) -> Option<KnowledgeLayer> {
    match s.to_lowercase().as_str() {
        "raw" => Some(KnowledgeLayer::Raw),
        "distilled" => Some(KnowledgeLayer::Distilled),
        "validated" => Some(KnowledgeLayer::Validated),
        "meta" => Some(KnowledgeLayer::Meta),
        _ => None,
    }
}
