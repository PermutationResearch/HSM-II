//! Feature Flags for Agent Anti-Fragility
//!
//! This module implements feature flags for progressive rollout,
//! allowing agents to safely deploy new capabilities with automatic
//! rollback on errors.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Feature flag with targeting rules and percentage rollout
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FeatureFlag {
    pub key: String,
    pub enabled: bool,
    pub rollout_percentage: f64,
    pub targeting_rules: Vec<TargetingRule>,
    pub metadata: FlagMetadata,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TargetingRule {
    pub attribute: String,
    pub operator: Operator,
    pub value: serde_json::Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operator {
    Eq,
    Neq,
    In,
    NotIn,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FlagMetadata {
    pub created_by: String,
    pub created_at: u64,
    pub description: String,
    pub rollback_on_error: bool,
    pub error_threshold: Option<f64>, // Rollback if error rate exceeds this
}

/// Context for flag evaluation
#[derive(Clone, Debug, Default)]
pub struct EvaluationContext {
    pub user_id: Option<String>,
    pub agent_id: Option<String>,
    pub agent_type: Option<String>,
    pub cohort: Option<String>,
    pub custom_attributes: HashMap<String, serde_json::Value>,
}

/// In-memory flag store
pub struct FlagStore {
    flags: Arc<RwLock<HashMap<String, FeatureFlag>>>,
    metrics: Arc<RwLock<FlagMetrics>>,
}

#[derive(Clone, Debug, Default)]
pub struct FlagMetrics {
    evaluations: HashMap<String, u64>,
    enabled_count: HashMap<String, u64>,
    errors: HashMap<String, Vec<FlagErrorEvent>>,
}

#[derive(Clone, Debug)]
pub struct FlagErrorEvent {
    pub timestamp: u64,
    pub error_message: String,
}

impl FeatureFlag {
    /// Evaluate if flag is enabled for given context
    pub fn is_enabled(&self, ctx: &EvaluationContext) -> bool {
        if !self.enabled {
            return false;
        }

        // Check targeting rules
        for rule in &self.targeting_rules {
            if !rule.matches(ctx) {
                return false;
            }
        }

        // Percentage rollout
        if self.rollout_percentage < 100.0 {
            let bucket = self.hash_context(ctx);
            return ((bucket % 100) as f64) < self.rollout_percentage;
        }

        true
    }

    fn hash_context(&self, ctx: &EvaluationContext) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        self.key.hash(&mut hasher);
        ctx.user_id.hash(&mut hasher);
        ctx.agent_id.hash(&mut hasher);
        hasher.finish()
    }
}

impl TargetingRule {
    fn matches(&self, ctx: &EvaluationContext) -> bool {
        let attr_value = match self.attribute.as_str() {
            "user_id" => ctx
                .user_id
                .as_ref()
                .map(|v| serde_json::Value::String(v.clone())),
            "agent_id" => ctx
                .agent_id
                .as_ref()
                .map(|v| serde_json::Value::String(v.clone())),
            "agent_type" => ctx
                .agent_type
                .as_ref()
                .map(|v| serde_json::Value::String(v.clone())),
            "cohort" => ctx
                .cohort
                .as_ref()
                .map(|v| serde_json::Value::String(v.clone())),
            _ => ctx.custom_attributes.get(&self.attribute).cloned(),
        };

        match (attr_value, &self.operator) {
            (Some(av), Operator::Eq) => av == self.value,
            (Some(av), Operator::Neq) => av != self.value,
            (Some(av), Operator::In) => {
                if let Some(arr) = self.value.as_array() {
                    arr.contains(&av)
                } else {
                    false
                }
            }
            (Some(av), Operator::NotIn) => {
                if let Some(arr) = self.value.as_array() {
                    !arr.contains(&av)
                } else {
                    true
                }
            }
            _ => false,
        }
    }
}

impl FlagStore {
    pub fn new() -> Self {
        Self {
            flags: Arc::new(RwLock::new(HashMap::new())),
            metrics: Arc::new(RwLock::new(FlagMetrics::default())),
        }
    }

    /// Add or update a flag
    pub async fn set_flag(&self, flag: FeatureFlag) {
        let mut flags = self.flags.write().await;
        flags.insert(flag.key.clone(), flag);
    }

    /// Evaluate flag for context
    pub async fn evaluate(&self, flag_key: &str, ctx: &EvaluationContext) -> bool {
        let flags = self.flags.read().await;
        let enabled = flags
            .get(flag_key)
            .map(|f| f.is_enabled(ctx))
            .unwrap_or(false);

        drop(flags);

        // Record metrics
        let mut metrics = self.metrics.write().await;
        *metrics.evaluations.entry(flag_key.to_string()).or_insert(0) += 1;
        if enabled {
            *metrics
                .enabled_count
                .entry(flag_key.to_string())
                .or_insert(0) += 1;
        }

        enabled
    }

    /// Emergency rollback - immediately disable flag
    pub async fn rollback(&self, flag_key: &str) -> bool {
        let mut flags = self.flags.write().await;
        if let Some(flag) = flags.get_mut(flag_key) {
            flag.enabled = false;
            flag.rollout_percentage = 0.0;
            tracing::warn!("🚨 EMERGENCY ROLLBACK: Flag '{}' disabled", flag_key);
            true
        } else {
            false
        }
    }

    /// Check if flag should auto-rollback based on error rate
    pub async fn check_auto_rollback(&self, flag_key: &str) -> bool {
        let flags = self.flags.read().await;
        let flag = match flags.get(flag_key) {
            Some(f) => f.clone(),
            None => return false,
        };
        drop(flags);

        let threshold = match flag.metadata.error_threshold {
            Some(t) => t,
            None => return false,
        };

        let metrics = self.metrics.read().await;
        let errors = metrics.errors.get(flag_key).map(|v| v.len()).unwrap_or(0) as f64;
        let total = metrics.evaluations.get(flag_key).copied().unwrap_or(0) as f64;

        if total > 0.0 && errors / total > threshold {
            drop(metrics);
            tracing::warn!(
                "Auto-rollback triggered for '{}': error rate {:.2}% > threshold {:.2}%",
                flag_key,
                (errors / total) * 100.0,
                threshold * 100.0
            );
            self.rollback(flag_key).await;
            true
        } else {
            false
        }
    }

    /// Record an error for a flag
    pub async fn record_error(&self, flag_key: &str, error: &str) {
        let mut metrics = self.metrics.write().await;
        let event = FlagErrorEvent {
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            error_message: error.to_string(),
        };
        metrics
            .errors
            .entry(flag_key.to_string())
            .or_default()
            .push(event);
    }

    /// Get flag statistics
    pub async fn get_stats(&self, flag_key: &str) -> Option<FlagStats> {
        let metrics = self.metrics.read().await;
        let flags = self.flags.read().await;

        let flag = flags.get(flag_key)?;
        let evaluations = metrics.evaluations.get(flag_key).copied().unwrap_or(0);
        let enabled = metrics.enabled_count.get(flag_key).copied().unwrap_or(0);
        let errors = metrics.errors.get(flag_key).map(|v| v.len()).unwrap_or(0);

        Some(FlagStats {
            flag: flag.clone(),
            total_evaluations: evaluations,
            enabled_count: enabled,
            disabled_count: evaluations - enabled,
            error_count: errors,
            error_rate: if evaluations > 0 {
                errors as f64 / evaluations as f64
            } else {
                0.0
            },
        })
    }
}

#[derive(Clone, Debug)]
pub struct FlagStats {
    pub flag: FeatureFlag,
    pub total_evaluations: u64,
    pub enabled_count: u64,
    pub disabled_count: u64,
    pub error_count: usize,
    pub error_rate: f64,
}

/// Trait for agents that check feature flags
#[async_trait::async_trait]
pub trait FlagsAware {
    async fn is_feature_enabled(&self, flag_key: &str, store: &FlagStore) -> bool;
    fn flag_context(&self) -> EvaluationContext;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_flag_basic() {
        let store = FlagStore::new();
        let flag = FeatureFlag {
            key: "test_flag".to_string(),
            enabled: true,
            rollout_percentage: 100.0,
            targeting_rules: vec![],
            metadata: FlagMetadata {
                created_by: "test".to_string(),
                created_at: 0,
                description: "Test flag".to_string(),
                rollback_on_error: true,
                error_threshold: Some(0.1),
            },
        };

        store.set_flag(flag).await;

        let ctx = EvaluationContext::default();
        assert!(store.evaluate("test_flag", &ctx).await);
    }

    #[tokio::test]
    async fn test_percentage_rollout() {
        let store = FlagStore::new();
        let flag = FeatureFlag {
            key: "rollout_flag".to_string(),
            enabled: true,
            rollout_percentage: 50.0, // 50% rollout
            targeting_rules: vec![],
            metadata: FlagMetadata {
                created_by: "test".to_string(),
                created_at: 0,
                description: "Rollout test".to_string(),
                rollback_on_error: false,
                error_threshold: None,
            },
        };

        store.set_flag(flag).await;

        // With 50% rollout, roughly half should be enabled
        let mut enabled_count = 0;
        for i in 0..100 {
            let ctx = EvaluationContext {
                user_id: Some(format!("user_{}", i)),
                ..Default::default()
            };
            if store.evaluate("rollout_flag", &ctx).await {
                enabled_count += 1;
            }
        }

        // Should be close to 50 (with some variance due to hashing)
        assert!(
            enabled_count >= 40 && enabled_count <= 60,
            "Expected ~50 enabled, got {}",
            enabled_count
        );
    }

    #[tokio::test]
    async fn test_targeting_rules() {
        let store = FlagStore::new();
        let flag = FeatureFlag {
            key: "targeted_flag".to_string(),
            enabled: true,
            rollout_percentage: 100.0,
            targeting_rules: vec![TargetingRule {
                attribute: "cohort".to_string(),
                operator: Operator::In,
                value: serde_json::json!(["beta", "alpha"]),
            }],
            metadata: FlagMetadata {
                created_by: "test".to_string(),
                created_at: 0,
                description: "Targeted test".to_string(),
                rollback_on_error: false,
                error_threshold: None,
            },
        };

        store.set_flag(flag).await;

        // Beta user should see flag
        let beta_ctx = EvaluationContext {
            cohort: Some("beta".to_string()),
            ..Default::default()
        };
        assert!(store.evaluate("targeted_flag", &beta_ctx).await);

        // Regular user should not
        let regular_ctx = EvaluationContext {
            cohort: Some("regular".to_string()),
            ..Default::default()
        };
        assert!(!store.evaluate("targeted_flag", &regular_ctx).await);
    }

    #[tokio::test]
    async fn test_emergency_rollback() {
        let store = FlagStore::new();
        let flag = FeatureFlag {
            key: "rollback_test".to_string(),
            enabled: true,
            rollout_percentage: 100.0,
            targeting_rules: vec![],
            metadata: FlagMetadata {
                created_by: "test".to_string(),
                created_at: 0,
                description: "Rollback test".to_string(),
                rollback_on_error: true,
                error_threshold: None,
            },
        };

        store.set_flag(flag).await;

        // Flag is initially enabled
        let ctx = EvaluationContext::default();
        assert!(store.evaluate("rollback_test", &ctx).await);

        // Emergency rollback
        store.rollback("rollback_test").await;

        // Flag is now disabled
        assert!(!store.evaluate("rollback_test", &ctx).await);
    }
}
