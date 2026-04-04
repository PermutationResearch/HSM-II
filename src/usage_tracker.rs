//! Per-Tenant Usage Tracking for HSM-II SaaS
//!
//! Tracks API calls, LLM token consumption, and channel publishes per tenant
//! per day. Counters are held in memory and flushed to disk periodically.

use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

// ═══════════════════════════════════════════════════════════════════
// Section 1: Usage data model
// ═══════════════════════════════════════════════════════════════════

/// Daily usage counters for a single tenant.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TenantUsage {
    /// API calls per date (YYYY-MM-DD → count).
    pub api_calls: HashMap<String, u64>,
    /// LLM tokens consumed per date.
    pub llm_tokens: HashMap<String, u64>,
    /// Channel publishes per date.
    pub publishes: HashMap<String, u64>,
}

impl TenantUsage {
    /// Total API calls today.
    pub fn api_calls_today(&self) -> u64 {
        let today = today_key();
        *self.api_calls.get(&today).unwrap_or(&0)
    }

    /// Total API calls this month.
    pub fn api_calls_this_month(&self) -> u64 {
        let prefix = month_prefix();
        self.api_calls
            .iter()
            .filter(|(k, _)| k.starts_with(&prefix))
            .map(|(_, v)| v)
            .sum()
    }

    /// Total LLM tokens this month.
    pub fn llm_tokens_this_month(&self) -> u64 {
        let prefix = month_prefix();
        self.llm_tokens
            .iter()
            .filter(|(k, _)| k.starts_with(&prefix))
            .map(|(_, v)| v)
            .sum()
    }

    /// Total publishes this month.
    pub fn publishes_this_month(&self) -> u64 {
        let prefix = month_prefix();
        self.publishes
            .iter()
            .filter(|(k, _)| k.starts_with(&prefix))
            .map(|(_, v)| v)
            .sum()
    }

    /// Prune entries older than `days` days.
    pub fn prune(&mut self, days: usize) {
        let cutoff = Utc::now() - chrono::Duration::days(days as i64);
        let cutoff_key = cutoff.format("%Y-%m-%d").to_string();
        self.api_calls.retain(|k, _| k >= &cutoff_key);
        self.llm_tokens.retain(|k, _| k >= &cutoff_key);
        self.publishes.retain(|k, _| k >= &cutoff_key);
    }
}

// ═══════════════════════════════════════════════════════════════════
// Section 2: UsageTracker — per-tenant counters
// ═══════════════════════════════════════════════════════════════════

/// Tracks usage metrics per tenant with periodic disk persistence.
pub struct UsageTracker {
    /// In-memory counters: tenant_id → TenantUsage.
    counters: Arc<RwLock<HashMap<String, TenantUsage>>>,
    /// Base directory for persistence (`~/.hsmii`).
    base_dir: PathBuf,
}

impl UsageTracker {
    /// Create a new usage tracker, loading existing data from disk.
    pub fn new(base_dir: &Path) -> Self {
        let counters = Self::load_from_disk(base_dir).unwrap_or_default();
        Self {
            counters: Arc::new(RwLock::new(counters)),
            base_dir: base_dir.to_path_buf(),
        }
    }

    /// Record one API call for a tenant.
    pub async fn record_api_call(&self, tenant_id: &str) {
        let today = today_key();
        let mut counters = self.counters.write().await;
        let usage = counters.entry(tenant_id.to_string()).or_default();
        *usage.api_calls.entry(today).or_insert(0) += 1;
    }

    /// Record LLM token consumption for a tenant.
    pub async fn record_llm_tokens(&self, tenant_id: &str, tokens: u64) {
        let today = today_key();
        let mut counters = self.counters.write().await;
        let usage = counters.entry(tenant_id.to_string()).or_default();
        *usage.llm_tokens.entry(today).or_insert(0) += tokens;
    }

    /// Record a channel publish for a tenant.
    pub async fn record_publish(&self, tenant_id: &str) {
        let today = today_key();
        let mut counters = self.counters.write().await;
        let usage = counters.entry(tenant_id.to_string()).or_default();
        *usage.publishes.entry(today).or_insert(0) += 1;
    }

    /// Get usage data for a tenant.
    pub async fn get_usage(&self, tenant_id: &str) -> TenantUsage {
        let counters = self.counters.read().await;
        counters.get(tenant_id).cloned().unwrap_or_default()
    }

    /// Check if a tenant has exceeded their daily API call limit.
    pub async fn check_daily_limit(&self, tenant_id: &str, max_per_day: u32) -> bool {
        let counters = self.counters.read().await;
        if let Some(usage) = counters.get(tenant_id) {
            usage.api_calls_today() < max_per_day as u64
        } else {
            true // no usage yet
        }
    }

    /// Flush all counters to disk.
    pub async fn flush(&self) -> Result<()> {
        let usage_dir = self.base_dir.join("usage");
        std::fs::create_dir_all(&usage_dir)?;

        let counters = self.counters.read().await;
        for (tenant_id, usage) in counters.iter() {
            let path = usage_dir.join(format!("{}.json", tenant_id));
            let json = serde_json::to_string_pretty(usage)?;
            std::fs::write(path, json)?;
        }

        info!(tenant_count = counters.len(), "Usage data flushed to disk");
        Ok(())
    }

    /// Prune old entries across all tenants.
    pub async fn prune(&self, retention_days: usize) {
        let mut counters = self.counters.write().await;
        for usage in counters.values_mut() {
            usage.prune(retention_days);
        }
    }

    /// Start a background flush loop that persists every `interval_secs` seconds.
    pub fn start_flush_loop(self: &Arc<Self>, interval_secs: u64) {
        let tracker = Arc::clone(self);
        tokio::spawn(async move {
            let mut interval =
                tokio::time::interval(tokio::time::Duration::from_secs(interval_secs));
            loop {
                interval.tick().await;
                if let Err(e) = tracker.flush().await {
                    tracing::warn!(error = %e, "Failed to flush usage data");
                }
            }
        });
    }

    // ── Internal helpers ───────────────────────────────────────────

    fn load_from_disk(base_dir: &Path) -> Result<HashMap<String, TenantUsage>> {
        let usage_dir = base_dir.join("usage");
        if !usage_dir.exists() {
            return Ok(HashMap::new());
        }

        let mut map = HashMap::new();
        for entry in std::fs::read_dir(&usage_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().map_or(false, |e| e == "json") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    let data = std::fs::read_to_string(&path)?;
                    if let Ok(usage) = serde_json::from_str::<TenantUsage>(&data) {
                        map.insert(stem.to_string(), usage);
                    }
                }
            }
        }

        Ok(map)
    }
}

// ═══════════════════════════════════════════════════════════════════
// Section 3: Utility functions
// ═══════════════════════════════════════════════════════════════════

fn today_key() -> String {
    Utc::now().format("%Y-%m-%d").to_string()
}

fn month_prefix() -> String {
    Utc::now().format("%Y-%m").to_string()
}

// ═══════════════════════════════════════════════════════════════════
// Section 4: Tests
// ═══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_record_api_calls() {
        let tmp = tempfile::tempdir().unwrap();
        let tracker = UsageTracker::new(tmp.path());

        tracker.record_api_call("tenant-1").await;
        tracker.record_api_call("tenant-1").await;
        tracker.record_api_call("tenant-2").await;

        let usage1 = tracker.get_usage("tenant-1").await;
        assert_eq!(usage1.api_calls_today(), 2);

        let usage2 = tracker.get_usage("tenant-2").await;
        assert_eq!(usage2.api_calls_today(), 1);
    }

    #[tokio::test]
    async fn test_record_llm_tokens() {
        let tmp = tempfile::tempdir().unwrap();
        let tracker = UsageTracker::new(tmp.path());

        tracker.record_llm_tokens("tenant-1", 500).await;
        tracker.record_llm_tokens("tenant-1", 300).await;

        let usage = tracker.get_usage("tenant-1").await;
        assert_eq!(usage.llm_tokens_this_month(), 800);
    }

    #[tokio::test]
    async fn test_record_publishes() {
        let tmp = tempfile::tempdir().unwrap();
        let tracker = UsageTracker::new(tmp.path());

        tracker.record_publish("tenant-1").await;
        tracker.record_publish("tenant-1").await;
        tracker.record_publish("tenant-1").await;

        let usage = tracker.get_usage("tenant-1").await;
        assert_eq!(usage.publishes_this_month(), 3);
    }

    #[tokio::test]
    async fn test_check_daily_limit() {
        let tmp = tempfile::tempdir().unwrap();
        let tracker = UsageTracker::new(tmp.path());

        // Should be under limit initially
        assert!(tracker.check_daily_limit("tenant-1", 2).await);

        tracker.record_api_call("tenant-1").await;
        assert!(tracker.check_daily_limit("tenant-1", 2).await);

        tracker.record_api_call("tenant-1").await;
        assert!(!tracker.check_daily_limit("tenant-1", 2).await);
    }

    #[tokio::test]
    async fn test_flush_and_reload() {
        let tmp = tempfile::tempdir().unwrap();

        // Record data and flush
        {
            let tracker = UsageTracker::new(tmp.path());
            tracker.record_api_call("tenant-persist").await;
            tracker.record_llm_tokens("tenant-persist", 1000).await;
            tracker.flush().await.unwrap();
        }

        // Reload from disk
        {
            let tracker = UsageTracker::new(tmp.path());
            let usage = tracker.get_usage("tenant-persist").await;
            assert_eq!(usage.api_calls_today(), 1);
            assert_eq!(usage.llm_tokens_this_month(), 1000);
        }
    }

    #[tokio::test]
    async fn test_unknown_tenant_returns_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let tracker = UsageTracker::new(tmp.path());

        let usage = tracker.get_usage("nonexistent").await;
        assert_eq!(usage.api_calls_today(), 0);
        assert_eq!(usage.llm_tokens_this_month(), 0);
    }

    #[test]
    fn test_tenant_usage_prune() {
        let mut usage = TenantUsage::default();
        usage.api_calls.insert("2020-01-01".to_string(), 100);
        usage.api_calls.insert(today_key(), 5);

        usage.prune(30); // keep only last 30 days
        assert_eq!(usage.api_calls.len(), 1); // old entry pruned
        assert_eq!(*usage.api_calls.get(&today_key()).unwrap(), 5);
    }
}
