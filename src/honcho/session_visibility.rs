//! Configurable session visibility — per-session control of which participants
//! see which messages.
//!
//! In HSM-II all agents currently see the same context. `SessionVisibility`
//! adds a visibility matrix so a private sidebar between two participants
//! (e.g. an orchestrator and one specialist) is not broadcast to the whole
//! session. The visibility config is stored per-session under
//! `~/.hsmii/honcho/sessions/<session_id>.json`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};

// ── VisibilityMatrix ──────────────────────────────────────────────────────────

/// Maps each peer to the set of peers whose messages it can see.
///
/// An absent key means "sees everyone" (default / open session).
/// An empty `HashSet` means "sees no-one's messages" (isolated observer).
///
/// Example:
/// ```json
/// { "agent_council_3": ["orchestrator", "user_alice"] }
/// ```
/// `agent_council_3` only sees messages from `orchestrator` and `user_alice`.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct VisibilityMatrix(HashMap<String, HashSet<String>>);

impl VisibilityMatrix {
    pub fn new() -> Self {
        Self(HashMap::new())
    }

    /// Restrict `observer` to seeing only `visible_from` peers.
    pub fn restrict(
        &mut self,
        observer: impl Into<String>,
        visible_from: impl IntoIterator<Item = impl Into<String>>,
    ) {
        self.0.insert(
            observer.into(),
            visible_from.into_iter().map(|s| s.into()).collect(),
        );
    }

    /// Open `observer` back up to see all participants.
    pub fn open(&mut self, observer: &str) {
        self.0.remove(observer);
    }

    /// Returns `true` if `observer` can see a message sent by `sender`.
    pub fn can_see(&self, observer: &str, sender: &str) -> bool {
        // If no restriction configured, everyone sees everyone.
        match self.0.get(observer) {
            None => true,
            Some(allowed) => allowed.contains(sender),
        }
    }

    /// Filter a message list to only the messages `observer` can see.
    pub fn filter_messages<'a>(
        &self,
        observer: &str,
        messages: impl Iterator<Item = &'a SessionMessage>,
    ) -> Vec<&'a SessionMessage> {
        messages
            .filter(|m| self.can_see(observer, &m.sender_id))
            .collect()
    }
}

// ── SessionMessage ─────────────────────────────────────────────────────────────

/// A message within a visibility-controlled session.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionMessage {
    pub id: String,
    pub sender_id: String,
    pub content: String,
    pub timestamp: u64,
    pub role: String, // "user" | "assistant" | "system"
}

// ── SessionVisibility ─────────────────────────────────────────────────────────

/// Full visibility configuration for a single session.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionVisibility {
    pub session_id: String,
    /// All peer IDs participating in this session.
    pub participants: Vec<String>,
    /// Per-observer visibility restrictions (empty = fully open).
    pub matrix: VisibilityMatrix,
    /// Unix timestamp when this config was created.
    pub created_at: u64,
}

impl SessionVisibility {
    /// Create a new fully-open session config.
    pub fn new_open(session_id: impl Into<String>, participants: Vec<String>) -> Self {
        Self {
            session_id: session_id.into(),
            participants,
            matrix: VisibilityMatrix::new(),
            created_at: now_secs(),
        }
    }

    /// Convenience: only `observer` and `allowed_senders` can communicate privately.
    ///
    /// Adds a bilateral restriction: observer sees only allowed_senders, and each
    /// allowed_sender sees only observer + each other.
    pub fn add_private_channel(&mut self, observer: &str, allowed_senders: &[&str]) {
        // observer sees only allowed_senders
        self.matrix
            .restrict(observer, allowed_senders.iter().copied());
        // each allowed_sender also sees observer
        for &sender in allowed_senders {
            let mut visible: HashSet<String> = allowed_senders
                .iter()
                .filter(|&&s| s != sender)
                .map(|&s| s.to_string())
                .collect();
            visible.insert(observer.to_string());
            self.matrix.0.insert(sender.to_string(), visible);
        }
    }

    /// Build the filtered message list for `observer`.
    pub fn messages_for<'a>(
        &self,
        observer: &str,
        all_messages: &'a [SessionMessage],
    ) -> Vec<&'a SessionMessage> {
        self.matrix.filter_messages(observer, all_messages.iter())
    }

    // ── Persistence ──────────────────────────────────────────────────────

    fn session_dir(honcho_home: &Path) -> PathBuf {
        honcho_home.join("sessions")
    }

    fn file_path(honcho_home: &Path, session_id: &str) -> PathBuf {
        Self::session_dir(honcho_home).join(format!("{}.json", sanitize_id(session_id)))
    }

    pub async fn load(honcho_home: &Path, session_id: &str) -> Result<Option<Self>> {
        let path = Self::file_path(honcho_home, session_id);
        if !path.exists() {
            return Ok(None);
        }
        let raw = tokio::fs::read_to_string(&path).await?;
        Ok(Some(serde_json::from_str(&raw)?))
    }

    pub async fn save(&self, honcho_home: &Path) -> Result<()> {
        let dir = Self::session_dir(honcho_home);
        tokio::fs::create_dir_all(&dir).await?;
        let path = Self::file_path(honcho_home, &self.session_id);
        let json = serde_json::to_string_pretty(self)?;
        crate::fs_atomic::write_atomic(&path, json.as_bytes())?;
        Ok(())
    }
}

// ── helpers ───────────────────────────────────────────────────────────────────

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn sanitize_id(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}
