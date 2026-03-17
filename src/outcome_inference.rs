//! Automatic Outcome Inference for HSM-II SaaS
//!
//! Solves the cold-start problem: feedback loops only work if outcomes get reported,
//! but humans rarely bother with explicit `POST /tasks/:id/outcome`. This module
//! infers outcomes from observable API behavioral signals so the DreamAdvisor
//! and routing system improve even when nobody reports anything.
//!
//! # Signal Tiers
//!
//! | Tier       | Source                          | Confidence | Human Input |
//! |------------|---------------------------------|------------|-------------|
//! | Explicit   | `POST /tasks/:id/outcome`       | 1.0        | Yes         |
//! | Inferred   | API behavioral patterns          | 0.4        | No          |
//! | Decay      | No signal after timeout           | 0.15       | No          |
//!
//! # Behavioral Signals
//!
//! The system already sees enough to guess outcomes:
//! - Task submitted → same role queried again shortly after → user probably used the output
//! - Task submitted → same task re-submitted → first routing was probably wrong
//! - Task submitted → brand context updated right after → user wasn't happy
//! - Task submitted → campaign created in same domain → output was useful
//! - No follow-up activity at all → neutral-to-negative (they ignored it)

use crate::autonomous_team::BusinessRole;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info};

// ═══════════════════════════════════════════════════════════════════
// Section 1: Task Event Tracking
// ═══════════════════════════════════════════════════════════════════

/// What happened in the API — we track these to infer outcomes.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum TaskEvent {
    /// Task was submitted and routed to a role.
    Submitted {
        task_id: String,
        description: String,
        assigned_role: BusinessRole,
        domain: Option<String>,
        timestamp: DateTime<Utc>,
    },
    /// User queried a specific role (GET /team/:role) — shows interest.
    RoleQueried {
        role: BusinessRole,
        timestamp: DateTime<Utc>,
    },
    /// Same or very similar task re-submitted — implies first attempt failed.
    Resubmitted {
        original_task_id: String,
        new_task_id: String,
        timestamp: DateTime<Utc>,
    },
    /// Brand context was updated — could indicate dissatisfaction with output.
    BrandUpdated {
        timestamp: DateTime<Utc>,
    },
    /// Campaign created — positive signal if domain matches recent task.
    CampaignCreated {
        domain: String,
        timestamp: DateTime<Utc>,
    },
    /// Explicit outcome reported by human — gold standard.
    ExplicitOutcome {
        task_id: String,
        success: bool,
        quality: f64,
        timestamp: DateTime<Utc>,
    },
}

/// A pending task that hasn't received explicit outcome yet.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PendingTask {
    pub task_id: String,
    pub description: String,
    pub assigned_role: BusinessRole,
    pub domain: Option<String>,
    pub submitted_at: DateTime<Utc>,
    /// Accumulated behavioral signals about this task.
    pub signals: Vec<BehavioralSignal>,
    /// Whether an explicit outcome was reported (overrides inference).
    pub explicit_outcome: Option<InferredOutcome>,
}

/// A single behavioral signal observed after task submission.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BehavioralSignal {
    pub kind: SignalKind,
    pub timestamp: DateTime<Utc>,
}

/// Types of behavioral signals we can detect.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum SignalKind {
    /// User queried the assigned role after task — likely used the output.
    RoleFollowUp,
    /// User submitted a new task to the same role — role is working well.
    SameRoleReuse,
    /// User re-submitted very similar task — first attempt failed.
    TaskResubmission,
    /// Brand context changed shortly after — possible dissatisfaction.
    BrandChangeAfterTask,
    /// Campaign created in same domain — output was useful enough to act on.
    CampaignInDomain,
    /// Another task in same domain submitted — domain is active.
    DomainActivity,
}

/// The inferred outcome for a task.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InferredOutcome {
    pub success: bool,
    pub quality: f64,
    /// How confident we are in this inference [0.0, 1.0].
    pub confidence: f64,
    pub source: OutcomeSource,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum OutcomeSource {
    /// Human explicitly reported via API.
    Explicit,
    /// Inferred from behavioral signals.
    Inferred,
    /// No signal after timeout — decay default.
    DecayDefault,
}

// ═══════════════════════════════════════════════════════════════════
// Section 2: OutcomeInferenceEngine
// ═══════════════════════════════════════════════════════════════════

/// Configuration for the inference engine.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InferenceConfig {
    /// How long to wait before inferring a decay-default outcome (seconds).
    pub decay_timeout_secs: u64,
    /// Confidence weight for inferred signals (vs explicit = 1.0).
    pub inferred_confidence: f64,
    /// Confidence weight for decay-default outcomes.
    pub decay_confidence: f64,
    /// Window (seconds) after task submission in which signals count.
    pub signal_window_secs: u64,
    /// Similarity threshold for detecting re-submissions (word overlap ratio).
    pub resubmission_threshold: f64,
}

impl Default for InferenceConfig {
    fn default() -> Self {
        Self {
            decay_timeout_secs: 3600,       // 1 hour
            inferred_confidence: 0.4,
            decay_confidence: 0.15,
            signal_window_secs: 1800,       // 30 min window for signals
            resubmission_threshold: 0.6,    // 60% word overlap = resubmission
        }
    }
}

/// Per-tenant outcome inference engine.
///
/// Tracks pending tasks, accumulates behavioral signals, and periodically
/// sweeps to infer outcomes for tasks that never got explicit feedback.
pub struct OutcomeInferenceEngine {
    /// Pending tasks per tenant: tenant_id → Vec<PendingTask>.
    pending: Arc<RwLock<HashMap<String, Vec<PendingTask>>>>,
    config: InferenceConfig,
    base_dir: PathBuf,
}

impl OutcomeInferenceEngine {
    pub fn new(base_dir: &Path, config: InferenceConfig) -> Self {
        let pending = Self::load_from_disk(base_dir).unwrap_or_default();
        Self {
            pending: Arc::new(RwLock::new(pending)),
            config,
            base_dir: base_dir.to_path_buf(),
        }
    }

    pub fn with_defaults(base_dir: &Path) -> Self {
        Self::new(base_dir, InferenceConfig::default())
    }

    // ── Event Recording ──────────────────────────────────────────

    /// Record that a task was submitted and routed.
    pub async fn record_task_submitted(
        &self,
        tenant_id: &str,
        task_id: String,
        description: String,
        assigned_role: BusinessRole,
        domain: Option<String>,
    ) {
        let mut pending = self.pending.write().await;
        let tasks = pending.entry(tenant_id.to_string()).or_default();

        tasks.push(PendingTask {
            task_id,
            description,
            assigned_role,
            domain,
            submitted_at: Utc::now(),
            signals: Vec::new(),
            explicit_outcome: None,
        });

        // Cap pending tasks per tenant to prevent unbounded growth.
        if tasks.len() > 500 {
            tasks.drain(..tasks.len() - 500);
        }
    }

    /// Record that a user queried a specific role.
    pub async fn record_role_query(&self, tenant_id: &str, role: BusinessRole) {
        let mut pending = self.pending.write().await;
        let now = Utc::now();

        if let Some(tasks) = pending.get_mut(tenant_id) {
            for task in tasks.iter_mut() {
                if task.explicit_outcome.is_some() {
                    continue;
                }
                if task.assigned_role == role && self.in_signal_window(task.submitted_at, now) {
                    task.signals.push(BehavioralSignal {
                        kind: SignalKind::RoleFollowUp,
                        timestamp: now,
                    });
                }
            }
        }
    }

    /// Record that a task was re-submitted (similar description detected).
    pub async fn record_task_resubmission(
        &self,
        tenant_id: &str,
        new_task_id: &str,
        new_description: &str,
    ) {
        let mut pending = self.pending.write().await;

        if let Some(tasks) = pending.get_mut(tenant_id) {
            let now = Utc::now();
            // Find pending tasks with similar descriptions.
            let similar_ids: Vec<String> = tasks
                .iter()
                .filter(|t| {
                    t.explicit_outcome.is_none()
                        && t.task_id != new_task_id
                        && word_overlap_ratio(&t.description, new_description)
                            >= self.config.resubmission_threshold
                })
                .map(|t| t.task_id.clone())
                .collect();

            for task in tasks.iter_mut() {
                if similar_ids.contains(&task.task_id) {
                    task.signals.push(BehavioralSignal {
                        kind: SignalKind::TaskResubmission,
                        timestamp: now,
                    });
                }
            }
        }
    }

    /// Record that brand context was updated.
    pub async fn record_brand_update(&self, tenant_id: &str) {
        let mut pending = self.pending.write().await;
        let now = Utc::now();

        if let Some(tasks) = pending.get_mut(tenant_id) {
            for task in tasks.iter_mut() {
                if task.explicit_outcome.is_some() {
                    continue;
                }
                if self.in_signal_window(task.submitted_at, now) {
                    task.signals.push(BehavioralSignal {
                        kind: SignalKind::BrandChangeAfterTask,
                        timestamp: now,
                    });
                }
            }
        }
    }

    /// Record that a campaign was created.
    pub async fn record_campaign_created(&self, tenant_id: &str, domain: &str) {
        let mut pending = self.pending.write().await;
        let now = Utc::now();

        if let Some(tasks) = pending.get_mut(tenant_id) {
            for task in tasks.iter_mut() {
                if task.explicit_outcome.is_some() {
                    continue;
                }
                // Match domain or check if task description relates to campaign domain.
                let domain_match = task.domain.as_deref() == Some(domain)
                    || task.description.to_lowercase().contains(&domain.to_lowercase());
                if domain_match && self.in_signal_window(task.submitted_at, now) {
                    task.signals.push(BehavioralSignal {
                        kind: SignalKind::CampaignInDomain,
                        timestamp: now,
                    });
                }
            }
        }
    }

    /// Record an explicit outcome (marks the task as resolved).
    pub async fn record_explicit_outcome(
        &self,
        tenant_id: &str,
        task_id: &str,
        success: bool,
        quality: f64,
    ) {
        let mut pending = self.pending.write().await;

        if let Some(tasks) = pending.get_mut(tenant_id) {
            if let Some(task) = tasks.iter_mut().find(|t| t.task_id == task_id) {
                task.explicit_outcome = Some(InferredOutcome {
                    success,
                    quality,
                    confidence: 1.0,
                    source: OutcomeSource::Explicit,
                });
            }
        }
    }

    // ── Outcome Sweep ────────────────────────────────────────────

    /// Sweep all tenants, infer outcomes for tasks past the signal window,
    /// and return a list of (tenant_id, task_id, role, domain, outcome) tuples
    /// ready to be fed into the DreamAdvisor.
    pub async fn sweep(&self) -> Vec<InferredTaskOutcome> {
        let mut results = Vec::new();
        let now = Utc::now();
        let mut pending = self.pending.write().await;

        for (tenant_id, tasks) in pending.iter_mut() {
            let mut resolved_indices = Vec::new();

            for (i, task) in tasks.iter().enumerate() {
                // Skip tasks that already have explicit outcomes — they were
                // already fed to the dream advisor via the normal API path.
                if task.explicit_outcome.is_some()
                    && task.explicit_outcome.as_ref().unwrap().source == OutcomeSource::Explicit
                {
                    resolved_indices.push(i);
                    continue;
                }

                let age_secs = (now - task.submitted_at).num_seconds().max(0) as u64;

                // Only infer if past the signal window.
                if age_secs < self.config.signal_window_secs {
                    continue;
                }

                let outcome = if age_secs >= self.config.decay_timeout_secs
                    && task.signals.is_empty()
                {
                    // No signals at all after timeout → decay default.
                    InferredOutcome {
                        success: false,
                        quality: 0.3,
                        confidence: self.config.decay_confidence,
                        source: OutcomeSource::DecayDefault,
                    }
                } else if !task.signals.is_empty() {
                    // Has signals — compute from behavioral evidence.
                    self.infer_from_signals(&task.signals)
                } else {
                    // Still within timeout, has no signals — skip for now.
                    continue;
                };

                results.push(InferredTaskOutcome {
                    tenant_id: tenant_id.clone(),
                    task_id: task.task_id.clone(),
                    assigned_role: task.assigned_role,
                    domain: task
                        .domain
                        .clone()
                        .unwrap_or_else(|| extract_domain(&task.description)),
                    outcome,
                });

                resolved_indices.push(i);
            }

            // Remove resolved tasks (iterate in reverse to preserve indices).
            for i in resolved_indices.into_iter().rev() {
                tasks.remove(i);
            }
        }

        if !results.is_empty() {
            debug!(
                inferred_count = results.len(),
                "Outcome inference sweep completed"
            );
        }

        results
    }

    /// Start a background sweep loop that runs every `interval_secs` seconds.
    ///
    /// Returns inferred outcomes via the callback, which should feed them
    /// into each tenant's TeamOrchestrator.
    pub fn start_sweep_loop<F>(self: &Arc<Self>, interval_secs: u64, callback: F)
    where
        F: Fn(Vec<InferredTaskOutcome>) + Send + Sync + 'static,
    {
        let engine = Arc::clone(self);
        tokio::spawn(async move {
            let mut interval =
                tokio::time::interval(tokio::time::Duration::from_secs(interval_secs));
            loop {
                interval.tick().await;
                let outcomes = engine.sweep().await;
                if !outcomes.is_empty() {
                    info!(
                        count = outcomes.len(),
                        "Auto-inferred task outcomes from behavioral signals"
                    );
                    callback(outcomes);
                }
                // Also flush to disk periodically.
                if let Err(e) = engine.flush().await {
                    tracing::warn!(error = %e, "Failed to flush inference state");
                }
            }
        });
    }

    // ── Signal Interpretation ────────────────────────────────────

    fn infer_from_signals(&self, signals: &[BehavioralSignal]) -> InferredOutcome {
        // Score each signal type.
        let mut positive_score: f64 = 0.0;
        let mut negative_score: f64 = 0.0;
        let mut signal_count = 0u32;

        for signal in signals {
            signal_count += 1;
            match signal.kind {
                SignalKind::RoleFollowUp => {
                    // User looked at the role after task — engaged with output.
                    positive_score += 0.3;
                }
                SignalKind::SameRoleReuse => {
                    // Sent another task to same role — trusts that role.
                    positive_score += 0.4;
                }
                SignalKind::CampaignInDomain => {
                    // Created a campaign in the task's domain — strong positive.
                    positive_score += 0.6;
                }
                SignalKind::DomainActivity => {
                    // General domain activity — mild positive.
                    positive_score += 0.15;
                }
                SignalKind::TaskResubmission => {
                    // Re-submitted same task — first attempt failed.
                    negative_score += 0.7;
                }
                SignalKind::BrandChangeAfterTask => {
                    // Changed brand context right after — mildly negative.
                    // Could be refining, not necessarily unhappy, so small weight.
                    negative_score += 0.2;
                }
            }
        }

        // Net score normalized to [0.0, 1.0].
        let raw = positive_score - negative_score;
        let quality = (raw / signal_count.max(1) as f64).clamp(0.0, 1.0);
        let success = quality > 0.35;

        InferredOutcome {
            success,
            quality,
            confidence: self.config.inferred_confidence,
            source: OutcomeSource::Inferred,
        }
    }

    fn in_signal_window(&self, submitted_at: DateTime<Utc>, now: DateTime<Utc>) -> bool {
        let age = (now - submitted_at).num_seconds().max(0) as u64;
        age <= self.config.signal_window_secs
    }

    // ── Persistence ──────────────────────────────────────────────

    pub async fn flush(&self) -> anyhow::Result<()> {
        let inference_dir = self.base_dir.join("inference");
        std::fs::create_dir_all(&inference_dir)?;

        let pending = self.pending.read().await;
        let json = serde_json::to_string_pretty(&*pending)?;
        std::fs::write(inference_dir.join("pending_tasks.json"), json)?;
        Ok(())
    }

    fn load_from_disk(base_dir: &Path) -> anyhow::Result<HashMap<String, Vec<PendingTask>>> {
        let path = base_dir.join("inference").join("pending_tasks.json");
        if !path.exists() {
            return Ok(HashMap::new());
        }
        let data = std::fs::read_to_string(path)?;
        let map: HashMap<String, Vec<PendingTask>> = serde_json::from_str(&data)?;
        Ok(map)
    }
}

/// A resolved inferred outcome, ready to be fed to the DreamAdvisor.
#[derive(Clone, Debug)]
pub struct InferredTaskOutcome {
    pub tenant_id: String,
    pub task_id: String,
    pub assigned_role: BusinessRole,
    pub domain: String,
    pub outcome: InferredOutcome,
}

// ═══════════════════════════════════════════════════════════════════
// Section 3: Utility Functions
// ═══════════════════════════════════════════════════════════════════

/// Compute word overlap ratio between two strings.
/// Returns [0.0, 1.0] — ratio of shared words to total unique words.
fn word_overlap_ratio(a: &str, b: &str) -> f64 {
    let lower_a = a.to_lowercase();
    let lower_b = b.to_lowercase();
    let words_a: std::collections::HashSet<&str> = lower_a.split_whitespace().collect();
    let words_b: std::collections::HashSet<&str> = lower_b.split_whitespace().collect();

    if words_a.is_empty() && words_b.is_empty() {
        return 1.0;
    }

    let intersection = words_a.intersection(&words_b).count();
    let union = words_a.union(&words_b).count();

    if union == 0 {
        return 0.0;
    }

    intersection as f64 / union as f64
}

/// Extract a rough domain key from a task description.
/// Takes the most distinctive noun-like words.
fn extract_domain(description: &str) -> String {
    const STOP_WORDS: &[&str] = &[
        "a", "an", "the", "and", "or", "but", "in", "on", "at", "to", "for", "of", "with",
        "by", "from", "as", "is", "was", "are", "were", "be", "been", "being", "have", "has",
        "had", "do", "does", "did", "will", "would", "could", "should", "may", "might",
        "shall", "can", "this", "that", "these", "those", "i", "we", "you", "he", "she",
        "it", "they", "my", "our", "your", "his", "her", "its", "their", "about", "up",
        "into", "through", "during", "before", "after", "above", "below", "between",
        "under", "again", "further", "then", "once", "here", "there", "when", "where",
        "why", "how", "all", "each", "every", "both", "few", "more", "most", "other",
        "some", "such", "no", "nor", "not", "only", "own", "same", "so", "than", "too",
        "very", "just", "because", "if", "while", "new", "write", "create", "make",
        "build", "implement", "design", "develop", "please",
    ];

    let lower = description.to_lowercase();
    let words: Vec<&str> = lower
        .split_whitespace()
        .filter(|w| w.len() > 2 && !STOP_WORDS.contains(w))
        .collect();

    if words.is_empty() {
        return "general".to_string();
    }

    // Take up to 3 most distinctive words.
    words.into_iter().take(3).collect::<Vec<_>>().join("_")
}

// ═══════════════════════════════════════════════════════════════════
// Section 4: Tests
// ═══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_word_overlap_ratio() {
        assert!((word_overlap_ratio("write a blog post", "write a blog post") - 1.0).abs() < 0.01);
        assert!((word_overlap_ratio("write a blog post", "write a blog article") - 0.6).abs() < 0.1);
        assert!((word_overlap_ratio("completely different", "nothing alike here") - 0.0).abs() < 0.01);
        assert!((word_overlap_ratio("", "") - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_extract_domain() {
        assert_eq!(extract_domain("write a blog post about product launch"), "blog_post_product");
        assert_eq!(extract_domain("design the homepage UI"), "homepage");
        assert_eq!(extract_domain(""), "general");
    }

    #[tokio::test]
    async fn test_record_and_sweep_positive_signals() {
        let tmp = tempfile::tempdir().unwrap();
        let engine = OutcomeInferenceEngine::new(
            tmp.path(),
            InferenceConfig {
                signal_window_secs: 0, // Immediate inference for testing.
                decay_timeout_secs: 3600,
                ..Default::default()
            },
        );

        // Submit a task.
        engine
            .record_task_submitted(
                "tenant-1",
                "task-1".into(),
                "write a blog post".into(),
                BusinessRole::Writer,
                Some("blog".into()),
            )
            .await;

        // User follows up by querying the writer role.
        engine
            .record_role_query("tenant-1", BusinessRole::Writer)
            .await;

        // User creates a campaign in the blog domain.
        engine
            .record_campaign_created("tenant-1", "blog")
            .await;

        // Sweep — should infer positive outcome.
        let outcomes = engine.sweep().await;
        assert_eq!(outcomes.len(), 1);
        assert!(outcomes[0].outcome.success);
        assert!(outcomes[0].outcome.quality > 0.3);
        assert_eq!(outcomes[0].outcome.source, OutcomeSource::Inferred);
    }

    #[tokio::test]
    async fn test_record_and_sweep_negative_signals() {
        let tmp = tempfile::tempdir().unwrap();
        let engine = OutcomeInferenceEngine::new(
            tmp.path(),
            InferenceConfig {
                signal_window_secs: 0,
                decay_timeout_secs: 3600,
                resubmission_threshold: 0.5,
                ..Default::default()
            },
        );

        // Submit a task.
        engine
            .record_task_submitted(
                "tenant-1",
                "task-1".into(),
                "write a blog post about our product".into(),
                BusinessRole::Writer,
                Some("blog".into()),
            )
            .await;

        // User re-submits the same task.
        engine
            .record_task_resubmission("tenant-1", "task-2", "write a blog post about our product launch")
            .await;

        // Sweep — should infer negative outcome.
        let outcomes = engine.sweep().await;
        assert_eq!(outcomes.len(), 1);
        assert!(!outcomes[0].outcome.success);
        assert_eq!(outcomes[0].outcome.source, OutcomeSource::Inferred);
    }

    #[tokio::test]
    async fn test_decay_default_on_timeout() {
        let tmp = tempfile::tempdir().unwrap();
        let engine = OutcomeInferenceEngine::new(
            tmp.path(),
            InferenceConfig {
                signal_window_secs: 0,
                decay_timeout_secs: 0, // Immediate decay for testing.
                ..Default::default()
            },
        );

        // Submit a task and do nothing.
        engine
            .record_task_submitted(
                "tenant-1",
                "task-1".into(),
                "analyze competitor pricing".into(),
                BusinessRole::Analyst,
                Some("pricing".into()),
            )
            .await;

        // Sweep — should decay-default.
        let outcomes = engine.sweep().await;
        assert_eq!(outcomes.len(), 1);
        assert!(!outcomes[0].outcome.success);
        assert_eq!(outcomes[0].outcome.quality, 0.3);
        assert_eq!(outcomes[0].outcome.confidence, 0.15);
        assert_eq!(outcomes[0].outcome.source, OutcomeSource::DecayDefault);
    }

    #[tokio::test]
    async fn test_explicit_outcome_overrides_inference() {
        let tmp = tempfile::tempdir().unwrap();
        let engine = OutcomeInferenceEngine::new(
            tmp.path(),
            InferenceConfig {
                signal_window_secs: 0,
                decay_timeout_secs: 0,
                ..Default::default()
            },
        );

        // Submit task.
        engine
            .record_task_submitted(
                "tenant-1",
                "task-1".into(),
                "write a blog post".into(),
                BusinessRole::Writer,
                Some("blog".into()),
            )
            .await;

        // Explicit outcome reported.
        engine
            .record_explicit_outcome("tenant-1", "task-1", true, 0.9)
            .await;

        // Sweep — explicit outcomes get cleaned up, not re-inferred.
        let outcomes = engine.sweep().await;
        assert!(outcomes.is_empty()); // Already handled by explicit API.
    }

    #[tokio::test]
    async fn test_persistence_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();

        // Record and flush.
        {
            let engine = OutcomeInferenceEngine::with_defaults(tmp.path());
            engine
                .record_task_submitted(
                    "tenant-1",
                    "task-1".into(),
                    "test persistence".into(),
                    BusinessRole::Developer,
                    None,
                )
                .await;
            engine.flush().await.unwrap();
        }

        // Reload.
        {
            let engine = OutcomeInferenceEngine::with_defaults(tmp.path());
            let pending = engine.pending.read().await;
            assert!(pending.contains_key("tenant-1"));
            assert_eq!(pending["tenant-1"].len(), 1);
            assert_eq!(pending["tenant-1"][0].task_id, "task-1");
        }
    }

    #[tokio::test]
    async fn test_pending_cap_prevents_unbounded_growth() {
        let tmp = tempfile::tempdir().unwrap();
        let engine = OutcomeInferenceEngine::with_defaults(tmp.path());

        // Submit 550 tasks.
        for i in 0..550 {
            engine
                .record_task_submitted(
                    "tenant-1",
                    format!("task-{}", i),
                    "test".into(),
                    BusinessRole::Developer,
                    None,
                )
                .await;
        }

        let pending = engine.pending.read().await;
        assert!(pending["tenant-1"].len() <= 500);
    }

    #[test]
    fn test_signal_scoring_balanced() {
        let config = InferenceConfig::default();
        let engine_signals = vec![
            BehavioralSignal {
                kind: SignalKind::RoleFollowUp,
                timestamp: Utc::now(),
            },
            BehavioralSignal {
                kind: SignalKind::TaskResubmission,
                timestamp: Utc::now(),
            },
        ];

        // One positive (0.3) + one strong negative (0.7) = net -0.4 / 2 = -0.2 → clamped to 0.0
        let engine = OutcomeInferenceEngine::new(
            Path::new("/tmp/test"),
            config,
        );
        let outcome = engine.infer_from_signals(&engine_signals);
        assert!(!outcome.success);
    }
}
