//! Capability registry — atomic composable primitives.
//!
//! A Capability is a named unit of work that agents can execute:
//! code generation, research, customer support, financial ops, etc.
//! Each has reliability/cost/performance targets but no fixed UI —
//! the Intelligence Layer composes them dynamically.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub type CapabilityId = String;

// ── CapabilityTarget ─────────────────────────────────────────────────────────

/// Performance contract for a capability.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CapabilityTarget {
    /// Target success rate (0.0–1.0).
    pub reliability: f64,
    /// Target cost per invocation in USD.
    pub cost_per_invocation: f64,
    /// Target p50 latency in milliseconds.
    pub latency_p50_ms: u64,
    /// Max concurrent invocations.
    pub max_concurrency: u32,
}

impl Default for CapabilityTarget {
    fn default() -> Self {
        Self {
            reliability: 0.95,
            cost_per_invocation: 0.01,
            latency_p50_ms: 5000,
            max_concurrency: 4,
        }
    }
}

// ── CapabilityMetrics ────────────────────────────────────────────────────────

/// Observed performance of a capability (updated after each invocation).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CapabilityMetrics {
    pub total_invocations: u64,
    pub successes: u64,
    pub failures: u64,
    pub total_cost_usd: f64,
    pub avg_latency_ms: f64,
    pub last_invoked: u64,
}

impl CapabilityMetrics {
    pub fn reliability(&self) -> f64 {
        if self.total_invocations == 0 {
            return 0.0;
        }
        self.successes as f64 / self.total_invocations as f64
    }

    pub fn record_success(&mut self, cost: f64, latency_ms: u64) {
        self.total_invocations += 1;
        self.successes += 1;
        self.total_cost_usd += cost;
        self.update_latency(latency_ms);
        self.last_invoked = now_secs();
    }

    pub fn record_failure(&mut self, cost: f64, latency_ms: u64) {
        self.total_invocations += 1;
        self.failures += 1;
        self.total_cost_usd += cost;
        self.update_latency(latency_ms);
        self.last_invoked = now_secs();
    }

    fn update_latency(&mut self, latency_ms: u64) {
        let n = self.total_invocations as f64;
        self.avg_latency_ms = self.avg_latency_ms * ((n - 1.0) / n) + (latency_ms as f64 / n);
    }
}

// ── Capability ───────────────────────────────────────────────────────────────

/// An atomic, composable primitive that agents can execute.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Capability {
    pub id: CapabilityId,
    pub name: String,
    pub description: String,

    /// Domain tags (e.g., "engineering", "research", "sales").
    pub domains: Vec<String>,

    /// Performance contract.
    pub target: CapabilityTarget,

    /// Observed metrics.
    pub metrics: CapabilityMetrics,

    /// Which agent(s) can provide this capability.
    pub provider_agents: Vec<String>,

    /// Capabilities this one depends on (for composition ordering).
    pub depends_on: Vec<CapabilityId>,

    /// Whether this capability is currently available.
    pub available: bool,

    /// Tool IDs from the HSM-II tool registry that back this capability.
    pub tool_ids: Vec<String>,
}

impl Capability {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            description: String::new(),
            domains: Vec::new(),
            target: CapabilityTarget::default(),
            metrics: CapabilityMetrics::default(),
            provider_agents: Vec::new(),
            depends_on: Vec::new(),
            available: true,
            tool_ids: Vec::new(),
        }
    }

    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    pub fn with_domains(mut self, domains: Vec<String>) -> Self {
        self.domains = domains;
        self
    }

    pub fn with_providers(mut self, providers: Vec<String>) -> Self {
        self.provider_agents = providers;
        self
    }

    pub fn meets_target(&self) -> bool {
        self.metrics.reliability() >= self.target.reliability
            && self.metrics.avg_latency_ms <= self.target.latency_p50_ms as f64
    }

    pub fn health_score(&self) -> f64 {
        if self.metrics.total_invocations == 0 {
            return 1.0; // Assume healthy until proven otherwise
        }
        let rel = (self.metrics.reliability() / self.target.reliability).min(1.0);
        let lat = if self.metrics.avg_latency_ms > 0.0 {
            (self.target.latency_p50_ms as f64 / self.metrics.avg_latency_ms).min(1.0)
        } else {
            1.0
        };
        rel * 0.7 + lat * 0.3
    }
}

// ── CapabilityRegistry ───────────────────────────────────────────────────────

/// Central registry of all capabilities available to the Intelligence Layer.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CapabilityRegistry {
    capabilities: HashMap<CapabilityId, Capability>,
}

impl CapabilityRegistry {
    pub fn new() -> Self {
        Self {
            capabilities: HashMap::new(),
        }
    }

    /// Seed with the standard Paperclip capability set.
    pub fn with_defaults() -> Self {
        let mut reg = Self::new();
        let defaults = vec![
            (
                "code_engineering",
                "Code & Engineering",
                &["engineering"][..],
            ),
            ("research_data", "Research & Data", &["research", "data"]),
            ("customer_sales", "Customer & Sales", &["customer", "sales"]),
            (
                "finance_ops",
                "Finance & Operations",
                &["finance", "operations"],
            ),
            (
                "content_marketing",
                "Content & Marketing",
                &["marketing", "content"],
            ),
            (
                "quality_compliance",
                "Quality & Compliance",
                &["quality", "compliance"],
            ),
        ];
        for (id, name, domains) in defaults {
            reg.register(
                Capability::new(id, name)
                    .with_domains(domains.iter().map(|s| s.to_string()).collect()),
            );
        }
        reg
    }

    pub fn register(&mut self, cap: Capability) {
        self.capabilities.insert(cap.id.clone(), cap);
    }

    pub fn get(&self, id: &str) -> Option<&Capability> {
        self.capabilities.get(id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut Capability> {
        self.capabilities.get_mut(id)
    }

    pub fn remove(&mut self, id: &str) -> Option<Capability> {
        self.capabilities.remove(id)
    }

    pub fn all(&self) -> impl Iterator<Item = &Capability> {
        self.capabilities.values()
    }

    pub fn by_domain(&self, domain: &str) -> Vec<&Capability> {
        self.capabilities
            .values()
            .filter(|c| c.domains.iter().any(|d| d == domain))
            .collect()
    }

    /// Find capabilities that can fulfill a set of required capability IDs.
    /// Returns (found, missing).
    pub fn resolve(&self, required: &[String]) -> (Vec<&Capability>, Vec<String>) {
        let mut found = Vec::new();
        let mut missing = Vec::new();
        for id in required {
            match self.capabilities.get(id) {
                Some(cap) if cap.available => found.push(cap),
                _ => missing.push(id.clone()),
            }
        }
        (found, missing)
    }

    /// Overall health: fraction of capabilities meeting their targets.
    pub fn health(&self) -> f64 {
        if self.capabilities.is_empty() {
            return 1.0;
        }
        let meeting = self
            .capabilities
            .values()
            .filter(|c| c.meets_target())
            .count();
        meeting as f64 / self.capabilities.len() as f64
    }

    pub fn len(&self) -> usize {
        self.capabilities.len()
    }

    pub fn is_empty(&self) -> bool {
        self.capabilities.is_empty()
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
