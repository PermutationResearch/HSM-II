# Anti-Fragile Agent Infrastructure: Beyond MCP

## The Vision: From Fragile to Anti-Fragile

```
MCP Pattern (Fragile)          API/CLI + Flags Pattern (Anti-Fragile)
─────────────────────          ──────────────────────────────────────
Single node server             Distributed compute layer
┌─────────────────┐           ┌─────────┐ ┌─────────┐ ┌─────────┐
│   MCP Server    │           │ Agent 1 │ │ Agent 2 │ │ Agent 3 │
│  (monolithic)   │           │  [🟢]   │ │  [🟢]   │ │  [🔴]   │
│                 │           └────┬────┘ └────┬────┘ └────┬────┘
│ Crashes = Down  │                │           │           │
└─────────────────┘           ┌────┴───────────┴───────────┴────┐
                              │      Load Balancer + Health     │
                              │   Unhealthy? Route to healthy   │
                              └─────────────────────────────────┘
                                     ↓
                              ┌─────────────┐
                              │ Feature Flags│
                              │  (progressive│
                              │   rollout)   │
                              └─────────────┘
```

## Why Move Away from MCP?

### MCP Limitations
1. **Single point of failure** - One server crashes, everything stops
2. **Tight coupling** - Client and server must stay in sync
3. **No progressive rollout** - All-or-nothing deployments
4. **Stateful connections** - Harder to scale horizontally

### API/CLI + Flags Advantages
1. **Stateless** - Any node can handle any request
2. **Horizontal scaling** - Add nodes as load increases
3. **Fault isolation** - One bad function doesn't kill the system
4. **Progressive rollout** - Flags control feature exposure
5. **Observability** - Each API call is independently trackable

---

## Implementation: The Flags-Native Agent System

### 1. Feature Flag Service

```rust
// src/flags/mod.rs

use std::collections::HashMap;
use serde::{Deserialize, Serialize};

/// Feature flag with targeting rules
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FeatureFlag {
    pub key: String,
    pub enabled: bool,
    pub rollout_percentage: f64, // 0.0 to 100.0
    pub targeting_rules: Vec<TargetingRule>,
    pub metadata: FlagMetadata,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TargetingRule {
    pub attribute: String, // "user_id", "agent_type", "cohort"
    pub operator: Operator, // Eq, Neq, In, NotIn, Gt, Lt
    pub value: serde_json::Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FlagMetadata {
    pub created_by: String,
    pub created_at: u64,
    pub description: String,
    pub rollback_on_error: bool,
}

pub enum Operator {
    Eq, Neq, In, NotIn, Gt, Lt, Contains,
}

/// Flag evaluation context
#[derive(Clone, Debug)]
pub struct EvaluationContext {
    pub user_id: Option<String>,
    pub agent_id: Option<String>,
    pub agent_type: Option<String>,
    pub cohort: Option<String>,
    pub custom_attributes: HashMap<String, serde_json::Value>,
}

impl FeatureFlag {
    /// Check if flag is enabled for this context
    pub fn is_enabled(&self, ctx: &EvaluationContext) -> bool {
        // Check hard enable/disable first
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
            let hash = self.hash_context(ctx);
            let bucket = (hash % 100) as f64;
            return bucket < self.rollout_percentage;
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
        hasher.finish() % 100
    }
}

impl TargetingRule {
    fn matches(&self, ctx: &EvaluationContext) -> bool {
        let attr_value = match self.attribute.as_str() {
            "user_id" => ctx.user_id.as_ref().map(|v| serde_json::Value::String(v.clone())),
            "agent_id" => ctx.agent_id.as_ref().map(|v| serde_json::Value::String(v.clone())),
            "agent_type" => ctx.agent_type.as_ref().map(|v| serde_json::Value::String(v.clone())),
            "cohort" => ctx.cohort.as_ref().map(|v| serde_json::Value::String(v.clone())),
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
            // ... other operators
            _ => false,
        }
    }
}
```

### 2. Distributed Compute Layer

```rust
// src/compute/mod.rs

use std::sync::Arc;
use tokio::sync::RwLock;
use std::collections::HashMap;

/// A compute node that can execute agent tasks
pub struct ComputeNode {
    pub id: String,
    pub health: NodeHealth,
    pub capabilities: Vec<String>,
    pub current_load: usize,
    pub max_concurrent: usize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NodeHealth {
    Healthy,
    Degraded,
    Unhealthy,
}

/// Distributed compute layer with fault tolerance
pub struct ComputeLayer {
    nodes: Arc<RwLock<HashMap<String, ComputeNode>>>,
    circuit_breakers: Arc<RwLock<HashMap<String, CircuitBreaker>>>,
}

pub struct CircuitBreaker {
    failures: u32,
    last_failure: Option<std::time::Instant>,
    state: CircuitState,
    threshold: u32,
    reset_timeout: std::time::Duration,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CircuitState {
    Closed,    // Normal operation
    Open,      // Failing, reject requests
    HalfOpen,  // Testing if recovered
}

impl ComputeLayer {
    pub async fn execute<F, Fut, T>(
        &self,
        task: F,
        preferred_node: Option<String>,
    ) -> Result<T, ComputeError>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<T, anyhow::Error>>,
    {
        // Try preferred node first
        if let Some(node_id) = preferred_node {
            if let Ok(result) = self.try_execute_on_node(&node_id, &task).await {
                return Ok(result);
            }
        }
        
        // Fall back to any healthy node
        let nodes = self.nodes.read().await;
        let healthy_nodes: Vec<_> = nodes
            .values()
            .filter(|n| n.health == NodeHealth::Healthy)
            .filter(|n| n.current_load < n.max_concurrent)
            .collect();
        
        drop(nodes); // Release read lock
        
        // Try each healthy node with circuit breaker
        for node in healthy_nodes {
            if self.is_circuit_closed(&node.id).await {
                match self.try_execute_on_node(&node.id, &task).await {
                    Ok(result) => {
                        self.record_success(&node.id).await;
                        return Ok(result);
                    }
                    Err(_) => {
                        self.record_failure(&node.id).await;
                    }
                }
            }
        }
        
        Err(ComputeError::NoHealthyNodes)
    }
    
    async fn is_circuit_closed(&self, node_id: &str) -> bool {
        let breakers = self.circuit_breakers.read().await;
        match breakers.get(node_id) {
            Some(cb) => cb.state == CircuitState::Closed,
            None => true,
        }
    }
    
    async fn record_failure(&self, node_id: &str) {
        let mut breakers = self.circuit_breakers.write().await;
        let cb = breakers.entry(node_id.to_string()).or_insert_with(|| CircuitBreaker {
            failures: 0,
            last_failure: None,
            state: CircuitState::Closed,
            threshold: 5,
            reset_timeout: std::time::Duration::from_secs(30),
        });
        
        cb.failures += 1;
        cb.last_failure = Some(std::time::Instant::now());
        
        if cb.failures >= cb.threshold {
            cb.state = CircuitState::Open;
            tracing::warn!("Circuit breaker OPEN for node {}", node_id);
        }
    }
    
    async fn record_success(&self, node_id: &str) {
        let mut breakers = self.circuit_breakers.write().await;
        if let Some(cb) = breakers.get_mut(node_id) {
            if cb.state == CircuitState::HalfOpen {
                cb.state = CircuitState::Closed;
                cb.failures = 0;
                tracing::info!("Circuit breaker CLOSED for node {}", node_id);
            }
        }
    }
}

#[derive(Debug)]
pub enum ComputeError {
    NoHealthyNodes,
    AllNodesFailed,
    Timeout,
}
```

### 3. Agent with Flag Awareness

```rust
// src/agent/flags_aware.rs

use crate::flags::{FeatureFlag, EvaluationContext};

/// Trait for agents that can check feature flags
#[async_trait::async_trait]
pub trait FlagsAwareAgent {
    /// Check if a feature is enabled for this agent
    async fn is_feature_enabled(&self, flag_key: &str) -> bool;
    
    /// Get evaluation context for this agent
    fn flag_context(&self) -> EvaluationContext;
    
    /// Execute with feature flag check
    async fn execute_with_flag<F, Fut, T>(
        &self,
        flag_key: &str,
        default_fn: F,
        flagged_fn: F,
    ) -> Result<T, anyhow::Error>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<T, anyhow::Error>>;
}

/// Example: Code review agent with progressive rollout
pub struct ProgressiveCodeReviewAgent {
    agent_id: String,
    flag_service: Arc<FlagService>,
}

#[async_trait::async_trait]
impl FlagsAwareAgent for ProgressiveCodeReviewAgent {
    async fn is_feature_enabled(&self, flag_key: &str) -> bool {
        let ctx = self.flag_context();
        self.flag_service.evaluate(flag_key, &ctx).await
    }
    
    fn flag_context(&self) -> EvaluationContext {
        EvaluationContext {
            user_id: None,
            agent_id: Some(self.agent_id.clone()),
            agent_type: Some("code_review".to_string()),
            cohort: Some("beta_testers".to_string()),
            custom_attributes: Default::default(),
        }
    }
    
    async fn execute_with_flag<F, Fut, T>(
        &self,
        flag_key: &str,
        default_fn: F,
        flagged_fn: F,
    ) -> Result<T, anyhow::Error>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<T, anyhow::Error>>,
    {
        if self.is_feature_enabled(flag_key).await {
            // New code path (progressive rollout)
            flagged_fn().await
        } else {
            // Old code path (stable)
            default_fn().await
        }
    }
}

impl ProgressiveCodeReviewAgent {
    pub async fn review_code(&self, code: &str) -> Result<CodeReview, anyhow::Error> {
        // Check if new AI-powered analysis is enabled
        self.execute_with_flag(
            "new_semantic_analysis",
            || self.legacy_review(code),      // 90% of traffic
            || self.ai_enhanced_review(code), // 10% of traffic (beta)
        ).await
    }
    
    async fn legacy_review(&self, code: &str) -> Result<CodeReview, anyhow::Error> {
        // Stable, proven code review logic
        Ok(CodeReview {
            issues: vec![],
            confidence: 0.8,
        })
    }
    
    async fn ai_enhanced_review(&self, code: &str) -> Result<CodeReview, anyhow::Error> {
        // New, experimental AI analysis
        // If this fails, flag can be immediately toggled off
        let analysis = self.deep_semantic_analysis(code).await?;
        Ok(CodeReview {
            issues: analysis.issues,
            confidence: analysis.confidence,
        })
    }
}
```

### 4. VC Flags Integration (Version Control for Flags)

```rust
// src/flags/vc_flags.rs

/// Version-controlled flags - agents can write flag-native code
pub struct VcFlags {
    git_repo: Arc<RwLock<GitRepo>>, // Flags as code
    runtime_flags: Arc<RwLock<HashMap<String, FeatureFlag>>>,
}

impl VcFlags {
    /// Agent invokes `vc flags` to create a new flag
    pub async fn create_flag(&self, flag_def: FlagDefinition) -> Result<String, FlagError> {
        // 1. Generate flag code
        let flag_code = self.generate_flag_code(&flag_def);
        
        // 2. Create branch
        let branch_name = format!("flags/{}", flag_def.key);
        self.git_repo.write().await.create_branch(&branch_name)?;
        
        // 3. Write flag file
        let file_path = format!("flags/{}.yaml", flag_def.key);
        self.git_repo.write().await.write_file(&file_path, &flag_code)?;
        
        // 4. Create PR for human review
        let pr_url = self.git_repo.write().await.create_pr(
            &branch_name,
            &format!("Add feature flag: {}", flag_def.key),
            &flag_def.description,
        ).await?;
        
        Ok(pr_url)
    }
    
    /// Deploy flag to runtime
    pub async fn deploy_flag(&self, flag_key: &str) -> Result<(), FlagError> {
        // 1. Read from git
        let flag_yaml = self.git_repo.read().await
            .read_file(&format!("flags/{}.yaml", flag_key))?;
        
        // 2. Parse
        let flag: FeatureFlag = serde_yaml::from_str(&flag_yaml)?;
        
        // 3. Deploy to runtime
        self.runtime_flags.write().await.insert(flag_key.to_string(), flag);
        
        tracing::info!("Deployed flag {} to runtime", flag_key);
        Ok(())
    }
    
    /// Emergency rollback
    pub async fn rollback(&self, flag_key: &str) -> Result<(), FlagError> {
        let mut flags = self.runtime_flags.write().await;
        if let Some(flag) = flags.get_mut(flag_key) {
            flag.enabled = false;
            flag.rollout_percentage = 0.0;
            tracing::warn!("EMERGENCY ROLLBACK of flag {}", flag_key);
        }
        Ok(())
    }
    
    fn generate_flag_code(&self, def: &FlagDefinition) -> String {
        format!(r#"# Feature Flag: {}
# Created: {}
# Description: {}

key: {}
enabled: false
rollout_percentage: 0.0

targeting_rules: []

metadata:
  created_by: {}
  rollback_on_error: true
  auto_rollback_threshold: 0.05  # Rollback if error rate > 5%
"#,
            def.key,
            chrono::Utc::now(),
            def.description,
            def.key,
            def.created_by
        )
    }
}

/// Agent can invoke this to write flag-native code
pub async fn agent_creates_flag(
    agent: &impl Agent,
    flag_key: &str,
    description: &str,
    vc_flags: &VcFlags,
) -> Result<String, anyhow::Error> {
    // Agent generates flag definition
    let flag_def = FlagDefinition {
        key: flag_key.to_string(),
        description: description.to_string(),
        created_by: agent.id().to_string(),
        targeting: FlagTargeting::default(),
    };
    
    // Create via version control
    let pr_url = vc_flags.create_flag(flag_def).await?;
    
    Ok(format!(
        "Created feature flag '{}'. Review and merge: {}",
        flag_key, pr_url
    ))
}
```

### 5. CLI Integration for Agents

```rust
// src/tools/flags_tools.rs

/// Tool: Create feature flag
pub struct CreateFlagTool {
    vc_flags: Arc<VcFlags>,
}

#[async_trait::async_trait]
impl Tool for CreateFlagTool {
    fn name(&self) -> &str {
        "create_feature_flag"
    }
    
    fn description(&self) -> &str {
        "Create a new feature flag for progressive rollout. \
         The flag will be created as a PR for review before deployment."
    }
    
    async fn execute(&self, params: Value) -> ToolOutput {
        let flag_key = params.get("key").and_then(|v| v.as_str()).unwrap_or("");
        let description = params.get("description").and_then(|v| v.as_str()).unwrap_or("");
        
        // Agent is creating a flag
        match agent_creates_flag(
            &self.current_agent,
            flag_key,
            description,
            &self.vc_flags
        ).await {
            Ok(msg) => ToolOutput::success(msg),
            Err(e) => ToolOutput::error(format!("Failed to create flag: {}", e)),
        }
    }
}

/// Tool: Check feature flag
pub struct CheckFlagTool {
    flag_service: Arc<FlagService>,
}

#[async_trait::async_trait]
impl Tool for CheckFlagTool {
    fn name(&self) -> &str {
        "check_feature_flag"
    }
    
    async fn execute(&self, params: Value) -> ToolOutput {
        let flag_key = params.get("key").and_then(|v| v.as_str()).unwrap_or("");
        let ctx = EvaluationContext {
            agent_id: params.get("agent_id").and_then(|v| v.as_str()).map(|s| s.to_string()),
            ..Default::default()
        };
        
        let enabled = self.flag_service.evaluate(flag_key, &ctx).await;
        
        ToolOutput::success(format!("Flag '{}' is {}", 
            flag_key,
            if enabled { "ENABLED ✅" } else { "DISABLED ❌" }
        ))
    }
}

/// Tool: Emergency rollback
pub struct RollbackFlagTool {
    vc_flags: Arc<VcFlags>,
}

#[async_trait::async_trait]
impl Tool for RollbackTool {
    fn name(&self) -> &str {
        "emergency_rollback"
    }
    
    fn description(&self) -> &str {
        "EMERGENCY: Immediately disable a feature flag. \
         Use when a deployment is causing errors or downtime."
    }
    
    async fn execute(&self, params: Value) -> ToolOutput {
        let flag_key = params.get("key").and_then(|v| v.as_str()).unwrap_or("");
        
        match self.vc_flags.rollback(flag_key).await {
            Ok(_) => ToolOutput::success(format!(
                "🚨 EMERGENCY ROLLBACK COMPLETE: Flag '{}' has been immediately disabled.",
                flag_key
            )),
            Err(e) => ToolOutput::error(format!("Rollback failed: {}", e)),
        }
    }
}
```

---

## Usage Examples

### Example 1: Progressive Rollout of New Agent Capability

```rust
// Agent checks if new feature is enabled
if agent.is_feature_enabled("semantic_code_search").await {
    // New capability (10% of agents)
    results = semantic_search(query).await?;
} else {
    // Old capability (90% of agents)
    results = text_search(query).await?;
}
```

### Example 2: Agent Creates Flag for New Feature

```
User: "Deploy the new security analyzer gradually"

Agent:
1. Creates flag `security_analyzer_v2`
2. Sets rollout: 5% → 25% → 50% → 100%
3. Monitors error rates
4. Auto-rollback if errors > 5%
5. Reports: "Security analyzer deployed to 25% of agents, 
    error rate 0.1%, proceeding to 50%"
```

### Example 3: Circuit Breaker Protects System

```rust
// Node 3 starts failing
for request in requests {
    match compute_layer.execute(task, None).await {
        Ok(result) => return result,
        Err(_) => {
            // Circuit breaker opens for Node 3
            // Subsequent requests route to Nodes 1, 2, 4
            // Node 3 gets time to recover
        }
    }
}
```

---

## Benefits Over MCP

| Aspect | MCP | API/CLI + Flags |
|--------|-----|-----------------|
| **Availability** | Single point of failure | N+1 redundancy |
| **Scaling** | Vertical only | Horizontal |
| **Deployment** | All-or-nothing | Progressive rollout |
| **Risk** | High (one bug = down) | Low (flags contain blast radius) |
| **Recovery** | Manual restart | Automatic circuit breaker |
| **Testing** | Staging only | Production with flags |
| **Agent Safety** | No guardrails | Flags limit AI mistakes |

---

## Implementation Roadmap

### Phase 1: Flags Service (Week 1)
- [ ] Flag evaluation engine
- [ ] Targeting rules
- [ ] Percentage rollout
- [ ] Basic CLI tools

### Phase 2: Distributed Compute (Week 2)
- [ ] Compute node abstraction
- [ ] Load balancer
- [ ] Circuit breakers
- [ ] Health checks

### Phase 3: VC Flags (Week 3)
- [ ] Git-backed flag definitions
- [ ] PR workflow
- [ ] Agent flag creation
- [ ] Emergency rollback

### Phase 4: Agent Integration (Week 4)
- [ ] Flags-aware agents
- [ ] Progressive rollout automation
- [ ] Error rate monitoring
- [ ] Auto-rollback

---

## The Pit of Success

> "Scaling the organizational best practice and discipline to use flags is tricky for humans. We now make it natural and easy for agents."

By giving agents:
1. **Simple APIs** to check flags
2. **CLI tools** to create flags
3. **Automatic rollback** on errors
4. **Circuit breakers** for resilience

We put both engineers and agents into the **pit of success** - the easy path is the safe path.

Anti-fragility becomes the default, not an afterthought.
