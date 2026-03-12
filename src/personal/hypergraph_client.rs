//! Hypergraph Client - Connect Personal Agent to existing HSM-II backend
//!
//! This allows the personal agent to optionally use a running
//! hypergraphd/hyper-stigmergy backend for advanced coordination.

use anyhow::{anyhow, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::{info, warn};

/// Client for connecting to running HSM-II backend
pub struct HypergraphClient {
    client: Client,
    base_url: String,
}

impl HypergraphClient {
    /// Create new client
    pub fn new(base_url: impl Into<String>) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            base_url: base_url.into(),
        }
    }

    /// Check if backend is available
    pub async fn is_available(&self) -> bool {
        match self
            .client
            .get(format!("{}/api/status", self.base_url))
            .send()
            .await
        {
            Ok(resp) => resp.status().is_success(),
            Err(_) => false,
        }
    }

    /// Get current hypergraph state
    pub async fn get_state(&self) -> Result<HypergraphState> {
        let resp = self
            .client
            .get(format!("{}/api/state", self.base_url))
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(anyhow!("Failed to get state: {}", resp.status()));
        }

        let state: HypergraphState = resp.json().await?;
        Ok(state)
    }

    /// Get coherence metric
    pub async fn get_coherence(&self) -> Result<f64> {
        let state = self.get_state().await?;
        Ok(state.coherence)
    }

    /// Inject event into hypergraph
    pub async fn inject_event(&self, event: HyperStigmergicEvent) -> Result<()> {
        let resp = self
            .client
            .post(format!("{}/api/events", self.base_url))
            .json(&event)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(anyhow!("Failed to inject event: {}", resp.status()));
        }

        Ok(())
    }

    /// Get DKS population stats
    pub async fn get_dks_stats(&self) -> Result<DKSStats> {
        let resp = self
            .client
            .get(format!("{}/api/dks/stats", self.base_url))
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(anyhow!("Failed to get DKS stats: {}", resp.status()));
        }

        let stats: DKSStats = resp.json().await?;
        Ok(stats)
    }

    /// Request Council deliberation
    pub async fn council_deliberate(&self, proposal: CouncilProposal) -> Result<CouncilDecision> {
        let resp = self
            .client
            .post(format!("{}/api/council/deliberate", self.base_url))
            .json(&proposal)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(anyhow!("Council deliberation failed: {}", resp.status()));
        }

        let decision: CouncilDecision = resp.json().await?;
        Ok(decision)
    }

    /// Spawn DKS agent
    pub async fn spawn_agent(&self, config: AgentConfig) -> Result<AgentId> {
        let resp = self
            .client
            .post(format!("{}/api/dks/spawn", self.base_url))
            .json(&config)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(anyhow!("Failed to spawn agent: {}", resp.status()));
        }

        let result: SpawnResult = resp.json().await?;
        Ok(result.agent_id)
    }
}

impl Default for HypergraphClient {
    fn default() -> Self {
        Self::new("http://127.0.0.1:9000")
    }
}

/// Hypergraph state snapshot
#[derive(Clone, Debug, Deserialize)]
pub struct HypergraphState {
    pub coherence: f64,
    pub agent_count: usize,
    pub edge_count: usize,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// Stigmergic event for injection
#[derive(Clone, Debug, Serialize)]
pub struct HyperStigmergicEvent {
    pub source: String,
    pub event_type: String,
    pub payload: serde_json::Value,
    pub coherence: f64,
}

/// DKS statistics
#[derive(Clone, Debug, Deserialize)]
pub struct DKSStats {
    pub population_size: usize,
    pub average_persistence: f64,
    pub replication_rate: f64,
    pub generation: u64,
}

/// Council proposal
#[derive(Clone, Debug, Serialize)]
pub struct CouncilProposal {
    pub proposal_type: String,
    pub description: String,
    pub context: serde_json::Value,
}

/// Council decision
#[derive(Clone, Debug, Deserialize)]
pub struct CouncilDecision {
    pub action: String,
    pub confidence: f64,
    pub coherence: f64,
    pub reasoning: String,
}

/// Agent configuration
#[derive(Clone, Debug, Serialize)]
pub struct AgentConfig {
    pub role: String,
    pub capabilities: Vec<String>,
    pub energy_budget: f64,
}

/// Agent identifier
#[derive(Clone, Debug, Deserialize)]
pub struct AgentId(pub String);

/// Spawn result
#[derive(Clone, Debug, Deserialize)]
struct SpawnResult {
    agent_id: AgentId,
}

/// Extension to PersonalAgent for hypergraph integration
impl super::PersonalAgent {
    /// Try to connect to running hypergraph backend
    pub async fn connect_hypergraph(&mut self, url: Option<String>) -> Result<bool> {
        let client = if let Some(url) = url {
            HypergraphClient::new(url)
        } else {
            HypergraphClient::default()
        };

        if client.is_available().await {
            info!("Connected to HSM-II backend at {}", client.base_url);

            // Get initial state
            let state = client.get_state().await?;
            info!(
                "Hypergraph state: {} agents, coherence {:.3}",
                state.agent_count, state.coherence
            );

            // Store client for later use
            // self.hypergraph = Some(client);

            Ok(true)
        } else {
            warn!("No HSM-II backend found at {}", client.base_url);
            warn!("Personal agent will run in standalone mode");
            Ok(false)
        }
    }

    /// Use Council for important decisions (if backend available)
    pub async fn council_decide(&self, _proposal: CouncilProposal) -> Option<CouncilDecision> {
        // if let Some(client) = &self.hypergraph {
        //     match client.council_deliberate(proposal).await {
        //         Ok(decision) => Some(decision),
        //         Err(e) => {
        //             warn!("Council deliberation failed: {}", e);
        //             None
        //         }
        //     }
        // } else {
        //     None
        // }
        None // Placeholder until integration complete
    }
}
