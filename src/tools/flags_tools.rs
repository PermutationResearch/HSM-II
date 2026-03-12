//! Feature Flag Tools for Agents
//! 
//! These tools allow agents to:
//! - Create feature flags
//! - Check flag status
//! - Execute with progressive rollout
//! - Emergency rollback

use crate::flags::{FeatureFlag, FlagMetadata, FlagStore, EvaluationContext, Operator, TargetingRule};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;

/// Tool: Create a new feature flag
pub struct CreateFlagTool {
    flag_store: Arc<FlagStore>,
}

impl CreateFlagTool {
    pub fn new(flag_store: Arc<FlagStore>) -> Self {
        Self { flag_store }
    }
}

#[async_trait]
impl super::Tool for CreateFlagTool {
    fn name(&self) -> &str {
        "create_feature_flag"
    }
    
    fn description(&self) -> &str {
        "Create a new feature flag for progressive rollout of agent capabilities. \
         The flag starts disabled (0% rollout) and can be gradually enabled."
    }
    
    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "key": {
                    "type": "string",
                    "description": "Unique identifier for the flag (e.g., 'semantic_search_v2')"
                },
                "description": {
                    "type": "string", 
                    "description": "Human-readable description of what this flag controls"
                },
                "initial_rollout": {
                    "type": "number",
                    "description": "Initial rollout percentage (0-100). Recommended: start at 5%",
                    "default": 5.0
                },
                "target_cohorts": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Optional: limit to specific cohorts (e.g., ['beta', 'internal'])",
                    "default": []
                },
                "rollback_on_error": {
                    "type": "boolean",
                    "description": "Auto-rollback if error threshold exceeded",
                    "default": true
                },
                "error_threshold": {
                    "type": "number",
                    "description": "Error rate threshold for auto-rollback (0.0-1.0)",
                    "default": 0.05
                }
            },
            "required": ["key", "description"]
        })
    }
    
    async fn execute(&self, params: Value) -> super::ToolOutput {
        let key = params.get("key").and_then(|v| v.as_str()).unwrap_or("");
        let description = params.get("description").and_then(|v| v.as_str()).unwrap_or("");
        let initial_rollout = params.get("initial_rollout").and_then(|v| v.as_f64()).unwrap_or(5.0);
        let rollback_on_error = params.get("rollback_on_error").and_then(|v| v.as_bool()).unwrap_or(true);
        let error_threshold = params.get("error_threshold").and_then(|v| v.as_f64()).or(Some(0.05));
        
        if key.is_empty() {
            return super::ToolOutput::error("Flag key is required");
        }
        
        // Build targeting rules if cohorts specified
        let targeting_rules: Vec<TargetingRule> = if let Some(cohorts) = params.get("target_cohorts").and_then(|v| v.as_array()) {
            let cohort_list: Vec<String> = cohorts.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect();
            
            if !cohort_list.is_empty() {
                vec![TargetingRule {
                    attribute: "cohort".to_string(),
                    operator: Operator::In,
                    value: serde_json::json!(cohort_list),
                }]
            } else {
                vec![]
            }
        } else {
            vec![]
        };
        
        let has_targeting = !targeting_rules.is_empty();
        
        let flag = FeatureFlag {
            key: key.to_string(),
            enabled: initial_rollout > 0.0,
            rollout_percentage: initial_rollout,
            targeting_rules,
            metadata: FlagMetadata {
                created_by: "agent".to_string(),
                created_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
                description: description.to_string(),
                rollback_on_error,
                error_threshold,
            },
        };
        
        self.flag_store.set_flag(flag).await;
        
        let cohort_msg = if has_targeting {
            "targeted cohorts".to_string()
        } else {
            "all users".to_string()
        };
        
        super::ToolOutput::success(format!(
            "✅ Created feature flag '{}'\n- Description: {}\n- Initial rollout: {:.0}% for {}\n- Auto-rollback: {} (threshold: {:.0}%)",
            key,
            description,
            initial_rollout,
            cohort_msg,
            if rollback_on_error { "enabled" } else { "disabled" },
            error_threshold.unwrap_or(0.05) * 100.0
        ))
    }
}

/// Tool: Check if a feature flag is enabled
pub struct CheckFlagTool {
    flag_store: Arc<FlagStore>,
}

impl CheckFlagTool {
    pub fn new(flag_store: Arc<FlagStore>) -> Self {
        Self { flag_store }
    }
}

#[async_trait]
impl super::Tool for CheckFlagTool {
    fn name(&self) -> &str {
        "check_feature_flag"
    }
    
    fn description(&self) -> &str {
        "Check if a feature flag is enabled for a specific context (agent, user, cohort)."
    }
    
    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "key": {
                    "type": "string",
                    "description": "Flag key to check"
                },
                "agent_id": {
                    "type": "string",
                    "description": "Optional: Agent ID for targeting"
                },
                "user_id": {
                    "type": "string",
                    "description": "Optional: User ID for targeting"
                },
                "cohort": {
                    "type": "string",
                    "description": "Optional: Cohort (beta, alpha, internal, etc.)"
                }
            },
            "required": ["key"]
        })
    }
    
    async fn execute(&self, params: Value) -> super::ToolOutput {
        let key = params.get("key").and_then(|v| v.as_str()).unwrap_or("");
        
        if key.is_empty() {
            return super::ToolOutput::error("Flag key is required");
        }
        
        let ctx = EvaluationContext {
            agent_id: params.get("agent_id").and_then(|v| v.as_str()).map(|s| s.to_string()),
            user_id: params.get("user_id").and_then(|v| v.as_str()).map(|s| s.to_string()),
            cohort: params.get("cohort").and_then(|v| v.as_str()).map(|s| s.to_string()),
            ..Default::default()
        };
        
        let enabled = self.flag_store.evaluate(key, &ctx).await;
        
        // Get stats if available
        let stats_msg = if let Some(stats) = self.flag_store.get_stats(key).await {
            format!(
                "\n📊 Stats: {}/{} evaluations enabled ({:.1}% error rate)",
                stats.enabled_count,
                stats.total_evaluations,
                stats.error_rate * 100.0
            )
        } else {
            String::new()
        };
        
        super::ToolOutput::success(format!(
            "Flag '{}' is {}",
            key,
            if enabled { "✅ ENABLED" } else { "❌ DISABLED" }
        ) + &stats_msg)
    }
}

/// Tool: Update flag rollout percentage
pub struct UpdateRolloutTool {
    flag_store: Arc<FlagStore>,
}

impl UpdateRolloutTool {
    pub fn new(flag_store: Arc<FlagStore>) -> Self {
        Self { flag_store }
    }
}

#[async_trait]
impl super::Tool for UpdateRolloutTool {
    fn name(&self) -> &str {
        "update_flag_rollout"
    }
    
    fn description(&self) -> &str {
        "Update the rollout percentage for a feature flag. Use this to gradually increase \
         or decrease exposure (e.g., 5% → 25% → 50% → 100%)."
    }
    
    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "key": {
                    "type": "string",
                    "description": "Flag key to update"
                },
                "rollout_percentage": {
                    "type": "number",
                    "description": "New rollout percentage (0-100)"
                }
            },
            "required": ["key", "rollout_percentage"]
        })
    }
    
    async fn execute(&self, params: Value) -> super::ToolOutput {
        let key = params.get("key").and_then(|v| v.as_str()).unwrap_or("");
        let percentage = params.get("rollout_percentage").and_then(|v| v.as_f64()).unwrap_or(0.0);
        
        if key.is_empty() {
            return super::ToolOutput::error("Flag key is required");
        }
        
        if percentage < 0.0 || percentage > 100.0 {
            return super::ToolOutput::error("Rollout percentage must be between 0 and 100");
        }
        
        // Get existing flag and update it
        let existing = self.flag_store.get_stats(key).await;
        
        if existing.is_none() {
            return super::ToolOutput::error(&format!("Flag '{}' not found", key));
        }
        
        let existing_flag = existing.unwrap().flag;
        let updated_flag = FeatureFlag {
            enabled: percentage > 0.0,
            rollout_percentage: percentage,
            ..existing_flag
        };
        
        self.flag_store.set_flag(updated_flag).await;
        
        // Progress bar visualization
        let filled = (percentage / 2.0) as usize;
        let empty = 50 - filled;
        let bar = "█".repeat(filled) + &"░".repeat(empty);
        
        super::ToolOutput::success(format!(
            "🚀 Updated rollout for '{}':\n[{}] {:.0}%",
            key, bar, percentage
        ))
    }
}

/// Tool: Emergency rollback
pub struct EmergencyRollbackTool {
    flag_store: Arc<FlagStore>,
}

impl EmergencyRollbackTool {
    pub fn new(flag_store: Arc<FlagStore>) -> Self {
        Self { flag_store }
    }
}

#[async_trait]
impl super::Tool for EmergencyRollbackTool {
    fn name(&self) -> &str {
        "emergency_rollback"
    }
    
    fn description(&self) -> &str {
        "EMERGENCY: Immediately disable a feature flag. Use when a deployment is causing \
         errors, downtime, or unexpected behavior. This takes effect instantly across all nodes."
    }
    
    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "key": {
                    "type": "string",
                    "description": "Flag key to rollback"
                },
                "reason": {
                    "type": "string",
                    "description": "Reason for rollback (for audit log)"
                }
            },
            "required": ["key"]
        })
    }
    
    async fn execute(&self, params: Value) -> super::ToolOutput {
        let key = params.get("key").and_then(|v| v.as_str()).unwrap_or("");
        let reason = params.get("reason").and_then(|v| v.as_str()).unwrap_or("No reason provided");
        
        if key.is_empty() {
            return super::ToolOutput::error("Flag key is required");
        }
        
        if self.flag_store.rollback(key).await {
            super::ToolOutput::success(format!(
                "🚨 EMERGENCY ROLLBACK COMPLETE\n\nFlag '{}' has been immediately disabled.\nReason: {}\n\nAll traffic is now routed to the stable code path.",
                key, reason
            ))
        } else {
            super::ToolOutput::error(&format!("Flag '{}' not found", key))
        }
    }
}

/// Tool: Get flag statistics and health
pub struct FlagStatsTool {
    flag_store: Arc<FlagStore>,
}

impl FlagStatsTool {
    pub fn new(flag_store: Arc<FlagStore>) -> Self {
        Self { flag_store }
    }
}

#[async_trait]
impl super::Tool for FlagStatsTool {
    fn name(&self) -> &str {
        "get_flag_stats"
    }
    
    fn description(&self) -> &str {
        "Get detailed statistics for a feature flag including evaluation counts, \
         error rates, and health status."
    }
    
    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "key": {
                    "type": "string",
                    "description": "Flag key to get stats for"
                }
            },
            "required": ["key"]
        })
    }
    
    async fn execute(&self, params: Value) -> super::ToolOutput {
        let key = params.get("key").and_then(|v| v.as_str()).unwrap_or("");
        
        if key.is_empty() {
            return super::ToolOutput::error("Flag key is required");
        }
        
        match self.flag_store.get_stats(key).await {
            Some(stats) => {
                let health = if stats.error_rate > 0.1 {
                    "🔴 CRITICAL"
                } else if stats.error_rate > 0.05 {
                    "🟠 WARNING"
                } else {
                    "🟢 HEALTHY"
                };
                
                super::ToolOutput::success(format!(
                    "📊 Flag Stats: {}\n\nStatus: {}\nRollout: {:.0}%\nEvaluations: {}\nEnabled: {} ({:.1}%)\nErrors: {} ({:.2}%)\nDescription: {}",
                    key,
                    health,
                    stats.flag.rollout_percentage,
                    stats.total_evaluations,
                    stats.enabled_count,
                    (stats.enabled_count as f64 / stats.total_evaluations.max(1) as f64) * 100.0,
                    stats.error_count,
                    stats.error_rate * 100.0,
                    stats.flag.metadata.description
                ))
            }
            None => super::ToolOutput::error(&format!("Flag '{}' not found or has no stats", key))
        }
    }
}

/// Bundle all flag tools
pub fn get_flag_tools(flag_store: Arc<FlagStore>) -> Vec<Box<dyn super::Tool>> {
    vec![
        Box::new(CreateFlagTool::new(flag_store.clone())),
        Box::new(CheckFlagTool::new(flag_store.clone())),
        Box::new(UpdateRolloutTool::new(flag_store.clone())),
        Box::new(EmergencyRollbackTool::new(flag_store.clone())),
        Box::new(FlagStatsTool::new(flag_store.clone())),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::Tool;
    
    #[tokio::test]
    async fn test_create_flag_tool() {
        let store = Arc::new(FlagStore::new());
        let tool = CreateFlagTool::new(store.clone());
        
        let result = tool.execute(serde_json::json!({
            "key": "test_feature",
            "description": "A test feature",
            "initial_rollout": 10.0
        })).await;
        
        assert!(result.success);
        assert!(result.result.contains("test_feature"));
        assert!(result.result.contains("10%"));
    }
    
    #[tokio::test]
    async fn test_check_flag_tool() {
        let store = Arc::new(FlagStore::new());
        let create_tool = CreateFlagTool::new(store.clone());
        let check_tool = CheckFlagTool::new(store.clone());
        
        // Create flag first
        create_tool.execute(serde_json::json!({
            "key": "check_test",
            "initial_rollout": 100.0
        })).await;
        
        // Check it
        let result = check_tool.execute(serde_json::json!({
            "key": "check_test"
        })).await;
        
        assert!(result.success);
        assert!(result.result.contains("ENABLED"));
    }
    
    #[tokio::test]
    async fn test_emergency_rollback_tool() {
        let store = Arc::new(FlagStore::new());
        let create_tool = CreateFlagTool::new(store.clone());
        let rollback_tool = EmergencyRollbackTool::new(store.clone());
        let check_tool = CheckFlagTool::new(store.clone());
        
        // Create and enable flag
        create_tool.execute(serde_json::json!({
            "key": "rollback_test",
            "initial_rollout": 100.0
        })).await;
        
        // Verify enabled
        let check = check_tool.execute(serde_json::json!({
            "key": "rollback_test"
        })).await;
        assert!(check.result.contains("ENABLED"));
        
        // Rollback
        let result = rollback_tool.execute(serde_json::json!({
            "key": "rollback_test",
            "reason": "Test rollback"
        })).await;
        
        assert!(result.success);
        assert!(result.result.contains("EMERGENCY ROLLBACK"));
        
        // Verify disabled
        let check = check_tool.execute(serde_json::json!({
            "key": "rollback_test"
        })).await;
        assert!(check.result.contains("DISABLED"));
    }
}
