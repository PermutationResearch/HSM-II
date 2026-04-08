//! UserRepresentation — pre-synthesized metamemory about a specific peer.
//!
//! Unlike HSM-II's `UserMd` (static markdown you manually update) or the
//! end-of-turn extract (single-session bullets), a `UserRepresentation` is a
//! continuously enriched insight document that gets richer with every session.
//!
//! It is stored in two places:
//!   1. `~/.hsmii/honcho/peers/<peer_id>.json` — fast JSON load at session start.
//!   2. The `EntitySummary` network of `HybridMemory` — searchable via RRF recall.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use serde::{Deserialize, Serialize};

// ── Inferred insight types ────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InferredGoal {
    pub description: String,
    /// 0.0–1.0 confidence from the LLM inference pass.
    pub confidence: f64,
    /// Session count when first observed.
    pub first_seen_session: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InferredPreference {
    /// Short key, e.g. "response_length" or "code_style".
    pub key: String,
    /// Observed value, e.g. "concise" or "typed Python".
    pub value: String,
    pub confidence: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InferredTrait {
    /// Trait label, e.g. "curious", "deadline-driven", "avoids boilerplate".
    pub label: String,
    /// Representative evidence quote from the transcript.
    pub evidence: String,
    pub confidence: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InferredFrustration {
    pub description: String,
    pub confidence: f64,
}

// ── UserRepresentation ────────────────────────────────────────────────────────

/// Continuously-enriched psychological profile of a peer, distilled from
/// all observed session transcripts.
///
/// The worker in `inference_worker.rs` upserts into this struct after each
/// session; the personal agent queries it at session start to ground context.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UserRepresentation {
    /// Matches `Peer::id`.
    pub peer_id: String,

    /// One-paragraph narrative of how this peer communicates
    /// (formal/informal, verbose/terse, technical depth, etc.).
    pub communication_style: String,

    /// What the peer is trying to accomplish — accumulated across sessions.
    pub goals: Vec<InferredGoal>,

    /// Recurring friction points observed in transcripts.
    pub frustrations: Vec<InferredFrustration>,

    /// Stable preferences that shape how responses should be crafted.
    pub preferences: Vec<InferredPreference>,

    /// Personality / cognitive traits inferred from language patterns.
    pub traits: Vec<InferredTrait>,

    /// Unix timestamp of the last inference pass.
    pub last_updated: u64,

    /// How many completed sessions have contributed to this representation.
    pub session_count: u32,

    /// Total messages seen across all sessions.
    pub total_messages: u32,

    /// Aggregate confidence: mean of all inferred item confidences.
    pub confidence: f64,
}

impl UserRepresentation {
    /// Empty representation for a peer we've never seen before.
    pub fn empty(peer_id: impl Into<String>) -> Self {
        Self {
            peer_id: peer_id.into(),
            communication_style: String::new(),
            goals: Vec::new(),
            frustrations: Vec::new(),
            preferences: Vec::new(),
            traits: Vec::new(),
            last_updated: 0,
            session_count: 0,
            total_messages: 0,
            confidence: 0.0,
        }
    }

    /// Render as a compact markdown block suitable for injecting into system prompt.
    ///
    /// Stays under `max_bytes` by trimming lower-confidence items first.
    pub fn render_context(&self, max_bytes: usize) -> String {
        if self.communication_style.is_empty() && self.goals.is_empty() {
            return String::new();
        }

        let mut buf = format!(
            "## User Representation (peer: {})\n\
             > Inferred across {} sessions | confidence {:.0}%\n\n",
            self.peer_id,
            self.session_count,
            self.confidence * 100.0
        );

        if !self.communication_style.is_empty() {
            buf.push_str(&format!(
                "**Communication style**: {}\n\n",
                self.communication_style
            ));
        }

        if !self.goals.is_empty() {
            buf.push_str("**Goals**:\n");
            let mut goals = self.goals.clone();
            goals.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap());
            for g in goals.iter().take(5) {
                buf.push_str(&format!(
                    "- {} _(conf {:.0}%)_\n",
                    g.description,
                    g.confidence * 100.0
                ));
            }
            buf.push('\n');
        }

        if !self.preferences.is_empty() {
            buf.push_str("**Preferences**:\n");
            for p in &self.preferences {
                buf.push_str(&format!("- **{}**: {}\n", p.key, p.value));
            }
            buf.push('\n');
        }

        if !self.traits.is_empty() {
            buf.push_str("**Traits**: ");
            let labels: Vec<&str> = self.traits.iter().map(|t| t.label.as_str()).collect();
            buf.push_str(&labels.join(", "));
            buf.push_str("\n\n");
        }

        if !self.frustrations.is_empty() {
            buf.push_str("**Known frustrations**:\n");
            for f in &self.frustrations {
                buf.push_str(&format!("- {}\n", f.description));
            }
            buf.push('\n');
        }

        // Trim to byte budget
        if buf.len() > max_bytes {
            buf.truncate(max_bytes);
            buf.push_str("\n... [truncated]");
        }

        buf
    }

    /// Merge a newly-inferred patch into this representation.
    ///
    /// Goals / frustrations / preferences / traits are merged by deduplication
    /// (highest confidence wins for duplicate keys). Communication style and
    /// confidence are updated from the patch if the patch confidence is higher.
    pub fn merge_patch(&mut self, patch: UserRepresentationPatch) {
        // Communication style: keep the higher-confidence version
        if patch.communication_style_confidence > self.confidence
            || self.communication_style.is_empty()
        {
            self.communication_style = patch.communication_style;
        }

        // Goals: upsert by description similarity (simple prefix match)
        'outer_goal: for new_goal in patch.goals {
            for existing in &mut self.goals {
                if goals_similar(&existing.description, &new_goal.description) {
                    if new_goal.confidence > existing.confidence {
                        existing.confidence = new_goal.confidence;
                        existing.description = new_goal.description;
                    }
                    continue 'outer_goal;
                }
            }
            self.goals.push(new_goal);
        }

        // Frustrations: upsert
        'outer_frus: for new_f in patch.frustrations {
            for existing in &mut self.frustrations {
                if goals_similar(&existing.description, &new_f.description) {
                    if new_f.confidence > existing.confidence {
                        *existing = new_f;
                    }
                    continue 'outer_frus;
                }
            }
            self.frustrations.push(new_f);
        }

        // Preferences: upsert by key
        for new_p in patch.preferences {
            if let Some(existing) = self.preferences.iter_mut().find(|p| p.key == new_p.key) {
                if new_p.confidence > existing.confidence {
                    *existing = new_p;
                }
            } else {
                self.preferences.push(new_p);
            }
        }

        // Traits: upsert by label
        for new_t in patch.traits {
            if let Some(existing) = self.traits.iter_mut().find(|t| t.label == new_t.label) {
                if new_t.confidence > existing.confidence {
                    *existing = new_t;
                }
            } else {
                self.traits.push(new_t);
            }
        }

        self.session_count += 1;
        self.total_messages += patch.message_count;
        self.last_updated = now_secs();

        // Recompute aggregate confidence
        let all: Vec<f64> = self
            .goals
            .iter()
            .map(|g| g.confidence)
            .chain(self.frustrations.iter().map(|f| f.confidence))
            .chain(self.preferences.iter().map(|p| p.confidence))
            .chain(self.traits.iter().map(|t| t.confidence))
            .collect();
        if !all.is_empty() {
            self.confidence = all.iter().sum::<f64>() / all.len() as f64;
        }
    }

    // ── Persistence ──────────────────────────────────────────────────────

    /// Directory for peer JSON files.
    pub fn peers_dir(honcho_home: &Path) -> PathBuf {
        honcho_home.join("peers")
    }

    /// Path for this peer's JSON file.
    pub fn file_path(honcho_home: &Path, peer_id: &str) -> PathBuf {
        Self::peers_dir(honcho_home).join(format!("{}.json", sanitize_id(peer_id)))
    }

    /// Load from disk, returning `empty()` if the file doesn't exist yet.
    pub async fn load(honcho_home: &Path, peer_id: &str) -> Result<Self> {
        let path = Self::file_path(honcho_home, peer_id);
        if !path.exists() {
            return Ok(Self::empty(peer_id));
        }
        let raw = tokio::fs::read_to_string(&path).await?;
        Ok(serde_json::from_str(&raw)?)
    }

    /// Persist to disk atomically.
    pub async fn save(&self, honcho_home: &Path) -> Result<()> {
        let dir = Self::peers_dir(honcho_home);
        tokio::fs::create_dir_all(&dir).await?;
        let path = Self::file_path(honcho_home, &self.peer_id);
        let json = serde_json::to_string_pretty(self)?;
        crate::fs_atomic::write_atomic(&path, json.as_bytes())?;
        Ok(())
    }
}

/// A diff produced by the LLM inference pass for a single session.
#[derive(Debug, Deserialize)]
pub struct UserRepresentationPatch {
    pub communication_style: String,
    pub communication_style_confidence: f64,
    pub goals: Vec<InferredGoal>,
    pub frustrations: Vec<InferredFrustration>,
    pub preferences: Vec<InferredPreference>,
    pub traits: Vec<InferredTrait>,
    /// Number of user messages in the session that produced this patch.
    pub message_count: u32,
}

// ── helpers ───────────────────────────────────────────────────────────────────

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Fuzzy similarity check: two descriptions are "similar" if one starts with
/// the first 30 chars of the other (fast, allocation-free approximation).
fn goals_similar(a: &str, b: &str) -> bool {
    let prefix_len = 30.min(a.len()).min(b.len());
    a[..prefix_len].to_lowercase() == b[..prefix_len].to_lowercase()
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
