//! DRI Registry — maps Directly Responsible Individuals to their owned
//! domains and authority scope.
//!
//! A DRI is a temporary or persistent owner of a cross-cutting outcome.
//! They have explicit authority to: spawn sub-agents, reallocate budgets,
//! pause work, and create/cancel goals within their domain.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── Authority ────────────────────────────────────────────────────────────────

/// What a DRI is allowed to do within their domain.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DriAuthority {
    /// Can spawn new sub-agents.
    pub can_spawn_agents: bool,
    /// Can reallocate budget across agents in their domain.
    pub can_reallocate_budget: bool,
    /// Can pause/resume goals and tasks.
    pub can_pause_work: bool,
    /// Can create new goals.
    pub can_create_goals: bool,
    /// Can cancel goals.
    pub can_cancel_goals: bool,
    /// Max monthly budget (cents) this DRI can allocate without higher approval.
    pub budget_authority_cents: i64,
    /// Max number of agents this DRI can have active simultaneously.
    pub max_agents: u32,
}

impl Default for DriAuthority {
    fn default() -> Self {
        Self {
            can_spawn_agents: true,
            can_reallocate_budget: true,
            can_pause_work: true,
            can_create_goals: true,
            can_cancel_goals: true,
            budget_authority_cents: 100_000, // $1,000 default
            max_agents: 10,
        }
    }
}

// ── DriEntry ─────────────────────────────────────────────────────────────────

/// A single DRI registration.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DriEntry {
    /// Unique DRI ID (usually matches agent_ref).
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// The domain(s) this DRI owns (e.g., "customer_retention", "cost_optimization").
    pub domains: Vec<String>,
    /// Authority scope.
    pub authority: DriAuthority,
    /// Whether this is a persistent DRI or time-boxed (e.g., 90-day sprint).
    pub tenure: DriTenure,
    /// Agent ref in company_os (for routing).
    pub agent_ref: String,
    /// Goals currently owned by this DRI.
    pub owned_goal_ids: Vec<String>,
    /// Agents currently reporting to this DRI.
    pub managed_agent_refs: Vec<String>,
    /// When this DRI was registered.
    pub created_at: u64,
    /// When this DRI was last active.
    pub last_active: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DriTenure {
    /// Permanent DRI — owns this domain indefinitely.
    Persistent,
    /// Time-boxed (e.g., sprint, incident response).
    TimeBound {
        start: u64,
        end: u64,
    },
}

impl DriEntry {
    pub fn new(id: impl Into<String>, name: impl Into<String>, agent_ref: impl Into<String>) -> Self {
        let now = now_secs();
        Self {
            id: id.into(),
            name: name.into(),
            domains: Vec::new(),
            authority: DriAuthority::default(),
            tenure: DriTenure::Persistent,
            agent_ref: agent_ref.into(),
            owned_goal_ids: Vec::new(),
            managed_agent_refs: Vec::new(),
            created_at: now,
            last_active: now,
        }
    }

    pub fn with_domains(mut self, domains: Vec<String>) -> Self {
        self.domains = domains;
        self
    }

    pub fn with_authority(mut self, authority: DriAuthority) -> Self {
        self.authority = authority;
        self
    }

    pub fn with_tenure(mut self, tenure: DriTenure) -> Self {
        self.tenure = tenure;
        self
    }

    pub fn is_active(&self) -> bool {
        match &self.tenure {
            DriTenure::Persistent => true,
            DriTenure::TimeBound { start, end } => {
                let now = now_secs();
                now >= *start && now <= *end
            }
        }
    }

    pub fn owns_domain(&self, domain: &str) -> bool {
        self.domains.iter().any(|d| d == domain)
    }

    pub fn touch(&mut self) {
        self.last_active = now_secs();
    }
}

// ── DriRegistry ──────────────────────────────────────────────────────────────

/// Central registry of all DRIs. The Intelligence Layer queries this to route
/// escalations and assign cross-cutting goals.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DriRegistry {
    entries: HashMap<String, DriEntry>,
    /// Domain → DRI ID index for fast lookup.
    domain_index: HashMap<String, Vec<String>>,
}

impl DriRegistry {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            domain_index: HashMap::new(),
        }
    }

    pub fn register(&mut self, entry: DriEntry) {
        for domain in &entry.domains {
            self.domain_index
                .entry(domain.clone())
                .or_default()
                .push(entry.id.clone());
        }
        self.entries.insert(entry.id.clone(), entry);
    }

    pub fn get(&self, id: &str) -> Option<&DriEntry> {
        self.entries.get(id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut DriEntry> {
        self.entries.get_mut(id)
    }

    pub fn remove(&mut self, id: &str) -> Option<DriEntry> {
        if let Some(entry) = self.entries.remove(id) {
            for domain in &entry.domains {
                if let Some(ids) = self.domain_index.get_mut(domain) {
                    ids.retain(|i| i != id);
                }
            }
            Some(entry)
        } else {
            None
        }
    }

    /// Find the best DRI for a given domain. Prefers active, persistent DRIs
    /// with the most recent activity.
    pub fn find_for_domain(&self, domain: &str) -> Option<&DriEntry> {
        let ids = self.domain_index.get(domain)?;
        ids.iter()
            .filter_map(|id| self.entries.get(id))
            .filter(|e| e.is_active())
            .max_by_key(|e| e.last_active)
    }

    /// All active DRIs.
    pub fn active(&self) -> Vec<&DriEntry> {
        self.entries.values().filter(|e| e.is_active()).collect()
    }

    pub fn all(&self) -> impl Iterator<Item = &DriEntry> {
        self.entries.values()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Assign a goal to the appropriate DRI based on required capability domains.
    /// Returns the DRI entry if found.
    pub fn route_goal(&self, domains: &[String]) -> Option<&DriEntry> {
        // Try exact domain match first
        for domain in domains {
            if let Some(dri) = self.find_for_domain(domain) {
                return Some(dri);
            }
        }
        // Fallback: any active DRI
        self.active().into_iter().next()
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
