use super::types::*;
use crate::meta_graph::MetaGraph;

/// Outbound federation client for communicating with remote instances.
#[derive(Clone)]
pub struct FederationClient {
    http: reqwest::Client,
    pub known_peers: Vec<String>,
    pub local_system_id: SystemId,
}

impl FederationClient {
    pub fn new(local_system_id: SystemId, known_peers: Vec<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            known_peers,
            local_system_id,
        }
    }

    /// Inject edges into a remote peer.
    pub async fn inject_edges(
        &self,
        peer_url: &str,
        edges: Vec<HyperedgeInjectionRequest>,
    ) -> anyhow::Result<ImportResult> {
        let url = format!("{}/hyperedges", peer_url.trim_end_matches('/'));

        let body = serde_json::json!({
            "edges": edges,
            "from_system": self.local_system_id,
        });

        let resp = self
            .http
            .post(&url)
            .json(&body)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await?;

        if !resp.status().is_success() {
            anyhow::bail!("inject_edges failed: HTTP {}", resp.status());
        }

        #[derive(serde::Deserialize)]
        struct ImportResponse {
            result: ImportResult,
        }

        let parsed: ImportResponse = resp.json().await?;
        Ok(parsed.result)
    }

    /// Register a subscription on a remote peer.
    pub async fn subscribe(
        &self,
        peer_url: &str,
        callback_url: &str,
        filter: SubscriptionFilter,
    ) -> anyhow::Result<()> {
        let url = format!("{}/subscribe", peer_url.trim_end_matches('/'));

        let body = serde_json::json!({
            "subscriber_system": self.local_system_id,
            "callback_url": callback_url,
            "filter": filter,
        });

        let resp = self
            .http
            .post(&url)
            .json(&body)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await?;

        if !resp.status().is_success() {
            anyhow::bail!("subscribe failed: HTTP {}", resp.status());
        }

        Ok(())
    }

    /// Poll a remote peer for shared edges matching a filter (pull-based).
    pub async fn poll_updates(
        &self,
        peer_url: &str,
        filter: &SubscriptionFilter,
    ) -> anyhow::Result<Vec<SharedEdge>> {
        let url = format!("{}/hyperedges", peer_url.trim_end_matches('/'));

        let mut query_params = Vec::new();
        if let Some(ref types) = filter.edge_types {
            query_params.push(("edge_types".to_string(), types.join(",")));
        }
        if let Some(min_trust) = filter.min_trust {
            query_params.push(("min_trust".to_string(), min_trust.to_string()));
        }
        if let Some(ref min_layer) = filter.min_layer {
            query_params.push(("min_layer".to_string(), format!("{:?}", min_layer)));
        }

        let resp = self
            .http
            .get(&url)
            .query(&query_params)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await?;

        if !resp.status().is_success() {
            anyhow::bail!("poll_updates failed: HTTP {}", resp.status());
        }

        let edges: Vec<SharedEdge> = resp.json().await?;
        Ok(edges)
    }

    /// Submit a consensus vote to a remote peer.
    pub async fn submit_consensus_vote(
        &self,
        peer_url: &str,
        vote: CrossSystemVote,
    ) -> anyhow::Result<()> {
        let url = format!("{}/consensus/vote", peer_url.trim_end_matches('/'));

        let body = serde_json::json!({ "vote": vote });

        let resp = self
            .http
            .post(&url)
            .json(&body)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await?;

        if !resp.status().is_success() {
            anyhow::bail!("submit_consensus_vote failed: HTTP {}", resp.status());
        }

        Ok(())
    }

    /// Query system info from a remote peer.
    pub async fn get_system_info(&self, peer_url: &str) -> anyhow::Result<SystemInfo> {
        let url = format!("{}/system/info", peer_url.trim_end_matches('/'));

        let resp = self
            .http
            .get(&url)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await?;

        if !resp.status().is_success() {
            anyhow::bail!("get_system_info failed: HTTP {}", resp.status());
        }

        let info: SystemInfo = resp.json().await?;
        Ok(info)
    }

    /// Query trust score from a remote peer.
    pub async fn get_trust_score(
        &self,
        peer_url: &str,
        target_system: &SystemId,
    ) -> anyhow::Result<f64> {
        let url = format!("{}/trust/{}", peer_url.trim_end_matches('/'), target_system);

        let resp = self
            .http
            .get(&url)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await?;

        if !resp.status().is_success() {
            anyhow::bail!("get_trust_score failed: HTTP {}", resp.status());
        }

        #[derive(serde::Deserialize)]
        struct TrustResponse {
            score: f64,
        }

        let parsed: TrustResponse = resp.json().await?;
        Ok(parsed.score)
    }

    /// Broadcast new shared edges to all subscribers by pushing to their callback URLs.
    pub async fn broadcast_to_subscribers(
        &self,
        meta_graph: &MetaGraph,
        new_edges: &[SharedEdge],
    ) -> usize {
        let mut delivered = 0usize;

        for edge in new_edges {
            let matching_subs = meta_graph.matching_subscriptions(edge);

            for sub in matching_subs {
                let url = &sub.callback_url;
                let payload = serde_json::json!({
                    "edges": [edge],
                    "from_system": self.local_system_id,
                });

                match self
                    .http
                    .post(url)
                    .json(&payload)
                    .timeout(std::time::Duration::from_secs(5))
                    .send()
                    .await
                {
                    Ok(resp) if resp.status().is_success() => {
                        delivered += 1;
                    }
                    Ok(resp) => {
                        tracing::warn!(
                            "Broadcast to {} failed: HTTP {}",
                            sub.subscriber_system,
                            resp.status()
                        );
                    }
                    Err(e) => {
                        tracing::warn!("Broadcast to {} error: {}", sub.subscriber_system, e);
                    }
                }
            }
        }

        delivered
    }

    /// Broadcast to all known peers (fan-out injection).
    pub async fn broadcast_edges_to_peers(
        &self,
        edges: Vec<HyperedgeInjectionRequest>,
    ) -> Vec<(String, anyhow::Result<ImportResult>)> {
        let mut results = Vec::new();

        for peer in &self.known_peers {
            let result = self.inject_edges(peer, edges.clone()).await;
            results.push((peer.clone(), result));
        }

        results
    }
}
