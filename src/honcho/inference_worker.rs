//! Async cross-session user inference worker.
//!
//! After each session this worker:
//!   1. Reads the session transcript from `~/.hsmii/memory/journal/YYYY-MM-DD.md`.
//!   2. Runs an LLM inference pass to extract psychological insights about the user.
//!   3. Merges the patch into the peer's `UserRepresentation` (file + EntitySummary).
//!   4. Upserts an `EntitySummary` entry into `HybridMemory` keyed by the peer entity tag.
//!
//! The worker is fire-and-forget: call `spawn_post_session` from the agent turn
//! handler and let it complete in the background without blocking the response.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use crate::honcho::user_representation::{UserRepresentation, UserRepresentationPatch};
use crate::memory::{HybridMemory, MemoryNetwork};
use crate::ollama_client::{OllamaClient, OllamaConfig};

// ── HonchoInferenceWorker ─────────────────────────────────────────────────────

/// Stateless worker — all state lives in files and the shared `HybridMemory`.
#[derive(Clone)]
pub struct HonchoInferenceWorker {
    /// `~/.hsmii/honcho/`
    honcho_home: PathBuf,
    /// `~/.hsmii/` root (for journal discovery)
    hsmii_home: PathBuf,
    /// Shared HybridMemory for EntitySummary upserts.
    pub hybrid_memory: Arc<RwLock<HybridMemory>>,
    /// LLM used for inference (separate config so it doesn't contend with main agent).
    llm_config: OllamaConfig,
}

impl HonchoInferenceWorker {
    pub fn new(hsmii_home: impl Into<PathBuf>, hybrid_memory: Arc<RwLock<HybridMemory>>) -> Self {
        let hsmii_home: PathBuf = hsmii_home.into();
        let honcho_home = hsmii_home.join("honcho");

        let mut cfg = OllamaConfig::default();
        // Use a smaller, faster model for background inference if configured.
        if let Ok(m) = std::env::var("HSM_HONCHO_MODEL") {
            let t = m.trim().to_string();
            if !t.is_empty() {
                cfg.model = t;
            }
        }
        cfg.max_tokens = 1024;
        cfg.temperature = 0.15;
        cfg.latency_budget_ms = 120_000; // 2 min — this runs in the background

        Self {
            honcho_home,
            hsmii_home,
            hybrid_memory,
            llm_config: cfg,
        }
    }

    // ── Public API ────────────────────────────────────────────────────────────

    /// Fire-and-forget: spawn the post-session inference as a background task.
    ///
    /// Call this at the end of a session (or after a configurable number of turns).
    /// The caller is not blocked; failures are logged and silently dropped.
    pub fn spawn_post_session(self, peer_id: String, transcript: String, message_count: u32) {
        tokio::spawn(async move {
            if let Err(e) = self
                .run_post_session_inference(&peer_id, &transcript, message_count)
                .await
            {
                warn!(peer = %peer_id, err = %e, "honcho inference pass failed");
            }
        });
    }

    /// Convenience: read today's journal, then spawn inference.
    pub fn spawn_post_session_from_journal(self, peer_id: String) {
        tokio::spawn(async move {
            match self.load_todays_journal().await {
                Ok((transcript, count)) if !transcript.is_empty() => {
                    if let Err(e) = self
                        .run_post_session_inference(&peer_id, &transcript, count)
                        .await
                    {
                        warn!(peer = %peer_id, err = %e, "honcho journal inference failed");
                    }
                }
                Ok(_) => debug!(peer = %peer_id, "honcho: empty journal, skipping"),
                Err(e) => warn!(peer = %peer_id, err = %e, "honcho: journal read failed"),
            }
        });
    }

    /// Load the `UserRepresentation` for a peer at session start.
    ///
    /// Returns `UserRepresentation::empty()` if the peer is new.
    /// Also queries `HybridMemory` EntitySummary for supplementary beliefs.
    pub async fn load_peer_context(&self, peer_id: &str) -> Result<UserRepresentation> {
        // Fast path: JSON file
        let mut repr = UserRepresentation::load(&self.honcho_home, peer_id).await?;

        // Enrich with any EntitySummary entries from HybridMemory
        // (these may have been added by older sessions before the JSON file existed)
        let mem = self.hybrid_memory.read().await;
        let entity_tag = format!("peer:{peer_id}");
        let es_entries: Vec<_> = mem
            .by_network(MemoryNetwork::EntitySummary)
            .into_iter()
            .filter(|e| e.entities.contains(&entity_tag))
            .collect();

        if !es_entries.is_empty() && repr.communication_style.is_empty() {
            // Seed the representation from the oldest EntitySummary entry
            let oldest = es_entries.iter().min_by_key(|e| e.timestamp).unwrap();
            repr.communication_style = oldest
                .abstract_l0
                .clone()
                .unwrap_or_else(|| oldest.content.chars().take(200).collect());
        }

        Ok(repr)
    }

    // ── Core inference logic ──────────────────────────────────────────────────

    async fn run_post_session_inference(
        &self,
        peer_id: &str,
        transcript: &str,
        message_count: u32,
    ) -> Result<()> {
        if transcript.trim().is_empty() {
            return Ok(());
        }

        info!(peer = %peer_id, messages = message_count, "honcho: running post-session inference");

        let patch = self.infer_patch(transcript, message_count).await?;

        // 1. Merge into UserRepresentation JSON file
        tokio::fs::create_dir_all(&self.honcho_home).await?;
        let mut repr = UserRepresentation::load(&self.honcho_home, peer_id).await?;
        repr.merge_patch(patch);
        repr.save(&self.honcho_home).await?;

        // 2. Upsert EntitySummary into HybridMemory
        let entity_tag = format!("peer:{peer_id}");
        let summary_content = repr.render_context(2000);
        if !summary_content.is_empty() {
            let mut mem = self.hybrid_memory.write().await;
            // Use a zero-length embedding (placeholder); real embedding would
            // require the embedding engine to be wired in here.
            let embedding = vec![0f32; 768];
            mem.retain(
                &summary_content,
                MemoryNetwork::EntitySummary,
                vec![entity_tag],
                vec!["honcho".to_string(), "user_representation".to_string()],
                0, // tick — not tracked here
                embedding,
            );
        }

        // 3. Persist HybridMemory to disk
        self.save_hybrid_memory().await?;

        info!(peer = %peer_id, session = repr.session_count, "honcho: representation updated");
        Ok(())
    }

    async fn infer_patch(
        &self,
        transcript: &str,
        message_count: u32,
    ) -> Result<UserRepresentationPatch> {
        let llm = OllamaClient::new(self.llm_config.clone());

        let system = r#"You are a user modeling system. Analyze the conversation transcript and extract psychological insights about the HUMAN user (not the AI assistant).

Output ONLY valid JSON matching this schema:
{
  "communication_style": "one paragraph describing how this person communicates",
  "communication_style_confidence": 0.85,
  "goals": [{"description": "...", "confidence": 0.8, "first_seen_session": 1}],
  "frustrations": [{"description": "...", "confidence": 0.7}],
  "preferences": [{"key": "response_length", "value": "concise", "confidence": 0.9}],
  "traits": [{"label": "curious", "evidence": "asked follow-up questions about X", "confidence": 0.75}],
  "message_count": 0
}

Focus on stable traits, not one-off comments. Confidence 0.0–1.0. Omit arrays if nothing meaningful detected. Do not invent data."#;

        let user_msg = format!(
            "## Session transcript ({message_count} messages)\n\n{}\n\nRespond with JSON only.",
            transcript.chars().take(6000).collect::<String>()
        );

        let res = llm.chat(system, &user_msg, &[]).await;

        if res.timed_out || res.text.is_empty() {
            return Err(anyhow::anyhow!("LLM timed out or returned empty"));
        }

        let text = strip_json_fences(&res.text);
        let mut patch: UserRepresentationPatch =
            serde_json::from_str(&text).context("failed to parse inference JSON")?;
        patch.message_count = message_count;

        Ok(patch)
    }

    // ── Journal helpers ───────────────────────────────────────────────────────

    async fn load_todays_journal(&self) -> Result<(String, u32)> {
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let journal_path = self
            .hsmii_home
            .join("memory")
            .join("journal")
            .join(format!("{today}.md"));

        if !journal_path.exists() {
            // Fallback: daily memory file at memory/YYYY-MM-DD.md
            let alt = self.hsmii_home.join("memory").join(format!("{today}.md"));
            if !alt.exists() {
                return Ok((String::new(), 0));
            }
            let content = tokio::fs::read_to_string(&alt).await?;
            let count = count_user_messages(&content);
            return Ok((content, count));
        }

        let content = tokio::fs::read_to_string(&journal_path).await?;
        let count = count_user_messages(&content);
        Ok((content, count))
    }

    // ── HybridMemory persistence ──────────────────────────────────────────────

    /// Path where the honcho HybridMemory is stored.
    pub fn memory_path(&self) -> PathBuf {
        self.honcho_home.join("hybrid_memory.json")
    }

    async fn save_hybrid_memory(&self) -> Result<()> {
        tokio::fs::create_dir_all(&self.honcho_home).await?;
        let mem = self.hybrid_memory.read().await;
        let json = serde_json::to_string_pretty(&*mem)?;
        crate::fs_atomic::write_atomic(&self.memory_path(), json.as_bytes())?;
        Ok(())
    }

    /// Load or create the HybridMemory from disk.
    pub async fn load_or_create_memory(honcho_home: &Path) -> HybridMemory {
        let path = honcho_home.join("hybrid_memory.json");
        if path.exists() {
            if let Ok(raw) = tokio::fs::read_to_string(&path).await {
                if let Ok(mem) = serde_json::from_str::<HybridMemory>(&raw) {
                    return mem;
                }
            }
        }
        HybridMemory::new()
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn strip_json_fences(s: &str) -> String {
    let t = s.trim();
    if let Some(rest) = t.strip_prefix("```") {
        let body = rest.trim_start_matches(|c| c != '\n');
        if let Some(i) = body.find('\n') {
            let after = &body[i + 1..];
            if let Some(end) = after.rfind("```") {
                return after[..end].trim().to_string();
            }
        }
    }
    t.to_string()
}

fn count_user_messages(journal: &str) -> u32 {
    // Journal format from append_turn_journal: "## User\n..." headers
    journal
        .lines()
        .filter(|l| l.starts_with("## User") || l.starts_with("**User**"))
        .count() as u32
}
