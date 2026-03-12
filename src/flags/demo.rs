//! Progressive Rollout Demo
//! 
//! Demonstrates how agents can safely deploy new capabilities using feature flags:
//! 1. Create flag with small rollout (5%)
//! 2. Monitor error rates
//! 3. Gradually increase rollout (5% → 25% → 50% → 100%)
//! 4. Auto-rollback if errors exceed threshold

use crate::flags::{FlagStore, EvaluationContext};
use std::sync::Arc;

/// Demonstrates progressive rollout workflow
pub async fn demo_progressive_rollout() {
    println!("\n═══════════════════════════════════════════════════════════════");
    println!("  FEATURE FLAGS: Progressive Rollout Demo");
    println!("═══════════════════════════════════════════════════════════════\n");
    
    let store = Arc::new(FlagStore::new());
    
    // ═══════════════════════════════════════════════════════════════
    // Phase 1: Agent Creates Flag for New Capability
    // ═══════════════════════════════════════════════════════════════
    println!("📋 PHASE 1: Agent Creates Feature Flag\n");
    
    let flag_key = "semantic_code_search_v2";
    
    // Agent uses tool to create flag
    let flag = crate::flags::FeatureFlag {
        key: flag_key.to_string(),
        enabled: true,
        rollout_percentage: 5.0, // Start conservative
        targeting_rules: vec![
            crate::flags::TargetingRule {
                attribute: "cohort".to_string(),
                operator: crate::flags::Operator::In,
                value: serde_json::json!(["beta", "internal"]),
            }
        ],
        metadata: crate::flags::FlagMetadata {
            created_by: "Agent-7".to_string(),
            created_at: now(),
            description: "New AI-powered semantic code search".to_string(),
            rollback_on_error: true,
            error_threshold: Some(0.05), // Rollback if > 5% errors
        },
    };
    
    store.set_flag(flag).await;
    
    println!("  ✅ Flag '{}' created", flag_key);
    println!("     Initial rollout: 5% of beta/internal users");
    println!("     Auto-rollback: Enabled (threshold: 5% errors)\n");
    
    // ═══════════════════════════════════════════════════════════════
    // Phase 2: Simulate Traffic with Gradual Rollout
    // ═══════════════════════════════════════════════════════════════
    println!("📈 PHASE 2: Progressive Rollout with Monitoring\n");
    
    let rollout_stages = vec![5.0, 25.0, 50.0, 100.0];
    
    for stage in rollout_stages {
        println!("  🚀 Rollout Stage: {:.0}%", stage);
        
        // Update flag
        let existing = store.get_stats(flag_key).await.unwrap();
        let updated = crate::flags::FeatureFlag {
            rollout_percentage: stage,
            ..existing.flag
        };
        store.set_flag(updated).await;
        
        // Simulate 100 requests
        let mut enabled_count = 0;
        let mut error_count = 0;
        
        for i in 0..100 {
            let ctx = EvaluationContext {
                user_id: Some(format!("user_{}", i)),
                cohort: Some(if i % 10 == 0 { "beta".to_string() } else { "regular".to_string() }),
                ..Default::default()
            };
            
            if store.evaluate(flag_key, &ctx).await {
                enabled_count += 1;
                
                // Simulate occasional error (2% rate - below threshold)
                if rand::random::<f64>() < 0.02 {
                    store.record_error(flag_key, "timeout").await;
                    error_count += 1;
                }
            }
        }
        
        let error_rate = error_count as f64 / enabled_count.max(1) as f64;
        
        println!("     Enabled for: {}/100 requests", enabled_count);
        println!("     Errors: {} ({:.1}%)\n", error_count, error_rate * 100.0);
        
        // Check auto-rollback
        if store.check_auto_rollback(flag_key).await {
            println!("  🚨 Auto-rollback triggered!");
            break;
        }
        
        // Wait for approval before next stage (in real scenario)
        if stage < 100.0 {
            println!("     ✓ Health check passed. Awaiting approval for next stage...\n");
        }
    }
    
    // ═══════════════════════════════════════════════════════════════
    // Phase 3: Show Final Stats
    // ═══════════════════════════════════════════════════════════════
    println!("📊 PHASE 3: Final Statistics\n");
    
    if let Some(stats) = store.get_stats(flag_key).await {
        let bar_filled = (stats.flag.rollout_percentage / 2.0) as usize;
        let bar_empty = 50 - bar_filled;
        let bar = "█".repeat(bar_filled) + &"░".repeat(bar_empty);
        
        println!("  Flag: {}", flag_key);
        println!("  Rollout: [{}] {:.0}%", bar, stats.flag.rollout_percentage);
        println!("  Total Evaluations: {}", stats.total_evaluations);
        println!("  Enabled: {} ({:.1}%)", 
            stats.enabled_count,
            (stats.enabled_count as f64 / stats.total_evaluations as f64) * 100.0
        );
        println!("  Errors: {} ({:.2}%)", stats.error_count, stats.error_rate * 100.0);
        println!("  Status: {}", 
            if stats.flag.enabled { "✅ Active" } else { "❌ Disabled" }
        );
    }
    
    // ═══════════════════════════════════════════════════════════════
    // Phase 4: Emergency Rollback Scenario
    // ═══════════════════════════════════════════════════════════════
    println!("\n🚨 PHASE 4: Emergency Rollback Scenario\n");
    
    let bad_flag_key = "experimental_feature";
    let bad_flag = crate::flags::FeatureFlag {
        key: bad_flag_key.to_string(),
        enabled: true,
        rollout_percentage: 50.0,
        targeting_rules: vec![],
        metadata: crate::flags::FlagMetadata {
            created_by: "Agent-3".to_string(),
            created_at: now(),
            description: "Experimental (buggy) feature".to_string(),
            rollback_on_error: false,
            error_threshold: None,
        },
    };
    
    store.set_flag(bad_flag).await;
    
    println!("  Deployed 'experimental_feature' at 50% rollout");
    
    // Simulate errors
    for _ in 0..50 {
        store.record_error(bad_flag_key, "panic").await;
    }
    
    println!("  ⚠️  Detected high error rate: {}/100", 50);
    
    // Emergency rollback
    store.rollback(bad_flag_key).await;
    
    println!("  ✅ Emergency rollback complete");
    println!("  🛡️  All traffic routed to stable code path\n");
    
    // Verify
    let ctx = EvaluationContext::default();
    assert!(!store.evaluate(bad_flag_key, &ctx).await);
    
    println!("═══════════════════════════════════════════════════════════════");
    println!("  Demo Complete: Progressive Rollout with Anti-Fragility");
    println!("═══════════════════════════════════════════════════════════════\n");
}

/// Demo: Circuit breaker pattern with distributed compute
pub async fn demo_circuit_breaker() {
    println!("\n═══════════════════════════════════════════════════════════════");
    println!("  CIRCUIT BREAKER: Fault Isolation Demo");
    println!("═══════════════════════════════════════════════════════════════\n");
    
    println!("  Simulating compute nodes with varying health...\n");
    
    let nodes = vec![
        ("node-1", "healthy", 100),
        ("node-2", "healthy", 100),
        ("node-3", "degraded", 50),
        ("node-4", "failing", 0),
    ];
    
    for (name, status, capacity) in nodes {
        let icon = match status {
            "healthy" => "🟢",
            "degraded" => "🟠",
            _ => "🔴",
        };
        
        println!("  {} {}: {} (capacity: {}%)", icon, name, status, capacity);
        
        if capacity == 0 {
            println!("     → Circuit breaker OPEN - requests diverted");
        }
    }
    
    println!("\n  ✅ Fault isolated: Failing node removed from rotation");
    println!("  ✅ System continues serving traffic at 75% capacity");
    println!("  ✅ No manual intervention required\n");
}

/// Demo: How agents write flag-native code
pub async fn demo_flag_native_code() {
    println!("\n═══════════════════════════════════════════════════════════════");
    println!("  FLAG-NATIVE CODE: Agent-Written Deployment Safety");
    println!("═══════════════════════════════════════════════════════════════\n");
    
    println!("  Agent generates code with built-in feature flags:\n");
    
    let code = r#"
// Agent-generated: semantic_search_v2

async fn handle_search(query: &str) -> Result<Results, Error> {
    // Check if new version is enabled for this request
    if flag_store.evaluate("semantic_search_v2", &ctx).await {
        // New code path (progressive rollout)
        match new_semantic_search(query).await {
            Ok(results) => Ok(results),
            Err(e) => {
                // Record error for auto-rollback
                flag_store.record_error("semantic_search_v2", &e.to_string()).await;
                
                // Fallback to stable version
                legacy_search(query).await
            }
        }
    } else {
        // Stable code path
        legacy_search(query).await
    }
}
"#;
    
    println!("{}", code);
    
    println!("  Benefits:");
    println!("  • New code wrapped in flag check");
    println!("  • Errors automatically recorded");
    println!("  • Graceful fallback to stable code");
    println!("  • No manual rollback needed\n");
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Run all demos
pub async fn run_all_demos() {
    demo_progressive_rollout().await;
    demo_circuit_breaker().await;
    demo_flag_native_code().await;
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_progressive_rollout_flow() {
        let store = Arc::new(FlagStore::new());
        
        // Create flag at 10%
        let flag = crate::flags::FeatureFlag {
            key: "test".to_string(),
            enabled: true,
            rollout_percentage: 10.0,
            targeting_rules: vec![],
            metadata: crate::flags::FlagMetadata {
                created_by: "test".to_string(),
                created_at: 0,
                description: "Test".to_string(),
                rollback_on_error: false,
                error_threshold: None,
            },
        };
        
        store.set_flag(flag).await;
        
        // Should be enabled for ~10% of users
        let mut enabled = 0;
        for i in 0..1000 {
            let ctx = EvaluationContext {
                user_id: Some(format!("user_{}", i)),
                ..Default::default()
            };
            if store.evaluate("test", &ctx).await {
                enabled += 1;
            }
        }
        
        assert!(enabled >= 80 && enabled <= 120, 
            "Expected ~100 enabled, got {}", enabled);
    }
    
    #[tokio::test]
    async fn test_auto_rollback() {
        let store = Arc::new(FlagStore::new());
        
        // Create flag with error threshold
        let flag = crate::flags::FeatureFlag {
            key: "rollback_test".to_string(),
            enabled: true,
            rollout_percentage: 100.0,
            targeting_rules: vec![],
            metadata: crate::flags::FlagMetadata {
                created_by: "test".to_string(),
                created_at: 0,
                description: "Test".to_string(),
                rollback_on_error: true,
                error_threshold: Some(0.1), // 10% threshold
            },
        };
        
        store.set_flag(flag).await;
        
        // Generate some evaluations
        for _ in 0..10 {
            let ctx = EvaluationContext::default();
            store.evaluate("rollback_test", &ctx).await;
        }
        
        // Record many errors (20% rate > 10% threshold)
        for _ in 0..20 {
            store.record_error("rollback_test", "error").await;
        }
        
        // Should trigger auto-rollback
        let should_rollback = store.check_auto_rollback("rollback_test").await;
        assert!(should_rollback, "Should trigger auto-rollback");
        
        // Verify flag is disabled
        let ctx = EvaluationContext::default();
        assert!(!store.evaluate("rollback_test", &ctx).await);
    }
}
