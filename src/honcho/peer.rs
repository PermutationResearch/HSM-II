//! Unified Peer abstraction — humans and AI agents as identical participants.
//!
//! HSM-II previously had separate systems for user modeling (`UserMd`) and agent
//! reputation (`SocialMemory`). `Peer` unifies both behind a single abstraction so
//! the same inference, visibility, and context-packing logic applies regardless of
//! whether the participant is a human or an AI agent.

use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

// ── PeerKind ─────────────────────────────────────────────────────────────────

/// Whether a peer is a human user or an AI agent participant.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum PeerKind {
    /// A human interacting through a gateway (Telegram, Discord, web, CLI).
    Human,
    /// An AI agent participating in a multi-agent session.
    Agent,
}

impl std::fmt::Display for PeerKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PeerKind::Human => write!(f, "human"),
            PeerKind::Agent => write!(f, "agent"),
        }
    }
}

// ── Peer ─────────────────────────────────────────────────────────────────────

/// A unified participant in an HSM-II session — human or agent.
///
/// Both kinds share the same identity representation so `HonchoInferenceWorker`,
/// `SessionVisibility`, and `PackedContextBuilder` can treat them uniformly.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Peer {
    /// Stable opaque identifier (e.g. Telegram user-id, agent UUID).
    pub id: String,
    /// Whether this peer is a human or an AI agent.
    pub kind: PeerKind,
    /// Short handle used in conversation (e.g. "@alice", "council_agent_3").
    pub handle: String,
    /// Human-readable display name.
    pub display_name: String,
    /// Unix timestamp of first interaction.
    pub created_at: u64,
    /// Unix timestamp of most recent interaction.
    pub last_seen: u64,
    /// Platform / gateway this peer is reachable through.
    #[serde(default)]
    pub platform: Option<String>,
    /// For agent peers: the role within the HSM-II council.
    #[serde(default)]
    pub agent_role: Option<String>,
}

impl Peer {
    /// Create a new human peer with `now` timestamps.
    pub fn new_human(id: impl Into<String>, handle: impl Into<String>) -> Self {
        let now = now_secs();
        let id = id.into();
        let handle = handle.into();
        Self {
            display_name: handle.clone(),
            id,
            kind: PeerKind::Human,
            handle,
            created_at: now,
            last_seen: now,
            platform: None,
            agent_role: None,
        }
    }

    /// Create a new agent peer with `now` timestamps.
    pub fn new_agent(id: impl Into<String>, handle: impl Into<String>, role: impl Into<String>) -> Self {
        let now = now_secs();
        let id = id.into();
        let handle = handle.into();
        Self {
            display_name: handle.clone(),
            id,
            kind: PeerKind::Agent,
            handle,
            created_at: now,
            last_seen: now,
            platform: None,
            agent_role: Some(role.into()),
        }
    }

    /// Touch the `last_seen` timestamp.
    pub fn touch(&mut self) {
        self.last_seen = now_secs();
    }

    /// Entity tag used when storing into `HybridMemory` EntitySummary network.
    pub fn entity_tag(&self) -> String {
        format!("peer:{}", self.id)
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
