//! Tool Registry - manages all available tools

use std::collections::HashMap;
use std::sync::Arc;
use serde_json::Value;
use tokio::time::{Duration, Instant};
use tracing::{debug, info, warn};

use super::{Tool, ToolCall, ToolCallResult, ToolOutput};
use super::tool_permissions::ToolPermissionContext;

/// Registry of all available tools
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
    /// Tool execution timeout
    timeout: Duration,
    /// Track tool usage statistics
    stats: HashMap<String, ToolStats>,
    /// ECC-style allowlist / prefix blocklist
    permissions: ToolPermissionContext,
    /// When set, firewall denials append `tool_denied` rows here
    audit_trail: Option<crate::personal::task_trail::TaskTrail>,
}

#[derive(Clone, Debug, Default)]
pub struct ToolStats {
    pub calls: u64,
    pub successes: u64,
    pub failures: u64,
    pub avg_duration_ms: f64,
}

impl ToolRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self::new_with_permissions(ToolPermissionContext::from_env())
    }

    pub fn new_with_permissions(permissions: ToolPermissionContext) -> Self {
        Self {
            tools: HashMap::new(),
            timeout: Duration::from_secs(60),
            stats: HashMap::new(),
            permissions,
            audit_trail: None,
        }
    }

    /// Share the same [`TaskTrail`] as [`crate::personal::EnhancedPersonalAgent`] for unified JSONL audit.
    pub fn set_audit_trail(&mut self, trail: Option<crate::personal::task_trail::TaskTrail>) {
        self.audit_trail = trail;
    }

    pub fn permissions(&self) -> &ToolPermissionContext {
        &self.permissions
    }

    pub fn set_permissions(&mut self, permissions: ToolPermissionContext) {
        self.permissions = permissions;
    }
    
    /// Create registry with default HSM-II tools
    pub fn with_default_tools() -> Self {
        let mut registry = Self::new();
        
        // Register default tools
        registry.register(Arc::new(super::web_search::WebSearchTool::new()));
        registry.register(Arc::new(super::file_tools::ReadTool::new()));
        registry.register(Arc::new(super::file_tools::WriteTool::new()));
        registry.register(Arc::new(super::file_tools::EditTool::new()));
        registry.register(Arc::new(super::shell_tools::BashTool::new()));
        registry.register(Arc::new(super::shell_tools::GrepTool::new()));
        registry.register(Arc::new(super::shell_tools::FindTool::new()));
        
        info!("Registered {} default tools", registry.tools.len());
        registry
    }
    
    /// Register a tool
    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        let name = tool.name().to_string();
        info!("Registering tool: {}", name);
        self.tools.insert(name.clone(), tool);
        self.stats.entry(name).or_default();
    }
    
    /// Get a tool by name
    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.get(name).cloned()
    }
    
    /// Check if a tool exists
    pub fn has(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }
    
    /// List all available tools
    pub fn list_tools(&self) -> Vec<(&str, &str)> {
        self.tools
            .values()
            .map(|t| (t.name(), t.description()))
            .collect()
    }
    
    /// Get tool schemas for LLM function calling
    pub fn get_schemas(&self) -> Vec<Value> {
        self.tools
            .values()
            .map(|t| {
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": t.name(),
                        "description": t.description(),
                        "parameters": t.parameters_schema()
                    }
                })
            })
            .collect()
    }
    
    /// Execute a single tool call (no timeout - let tools complete)
    pub async fn execute(&mut self, call: ToolCall) -> ToolCallResult {
        let start = Instant::now();
        let timestamp = chrono::Utc::now();

        if let Err(reason) = self.permissions.check(&call.name) {
            warn!(target: "hsm_tool_firewall", tool = %call.name, %reason, "blocked");
            if let Some(ref trail) = self.audit_trail {
                if let Err(e) = trail.append_tool_denied(&call.name, &reason).await {
                    warn!("task trail append tool_denied failed: {}", e);
                }
            }
            return ToolCallResult {
                call,
                output: ToolOutput::error(format!("Tool blocked by policy: {reason}")),
                duration_ms: start.elapsed().as_millis() as u64,
                timestamp,
            };
        }

        let output = if let Some(tool) = self.tools.get(&call.name) {
            debug!("Executing tool: {} with params: {:?}", call.name, call.parameters);

            // Execute without timeout
            let result = tool.execute(call.parameters.clone()).await;
            self.update_stats(&call.name, result.success, start.elapsed());
            result
        } else {
            warn!("Tool not found: {}", call.name);
            ToolOutput::error(format!("Tool '{}' not found", call.name))
        };
        
        ToolCallResult {
            call,
            output,
            duration_ms: start.elapsed().as_millis() as u64,
            timestamp,
        }
    }
    
    /// Execute multiple tool calls in sequence
    pub async fn execute_all(&mut self, calls: Vec<ToolCall>) -> Vec<ToolCallResult> {
        let mut results = Vec::new();
        for call in calls {
            results.push(self.execute(call).await);
        }
        results
    }
    
    /// Get statistics for all tools
    pub fn get_stats(&self) -> &HashMap<String, ToolStats> {
        &self.stats
    }
    
    /// Update tool statistics
    fn update_stats(&mut self, name: &str, success: bool, duration: Duration) {
        if let Some(stats) = self.stats.get_mut(name) {
            stats.calls += 1;
            if success {
                stats.successes += 1;
            } else {
                stats.failures += 1;
            }
            // Update rolling average
            let duration_ms = duration.as_millis() as f64;
            stats.avg_duration_ms = (stats.avg_duration_ms * (stats.calls - 1) as f64 + duration_ms) 
                / stats.calls as f64;
        }
    }
    
    /// Set execution timeout
    pub fn set_timeout(&mut self, timeout: Duration) {
        self.timeout = timeout;
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::with_default_tools()
    }
}

#[cfg(test)]
mod tests {
    use super::super::tool_permissions::ToolPermissionContext;
    use super::*;
    use serde_json::json;

    struct TestTool;
    
    #[async_trait::async_trait]
    impl Tool for TestTool {
        fn name(&self) -> &str {
            "test_tool"
        }
        
        fn description(&self) -> &str {
            "A test tool"
        }
        
        fn parameters_schema(&self) -> Value {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "input": {"type": "string"}
                },
                "required": ["input"]
            })
        }
        
        async fn execute(&self, params: Value) -> ToolOutput {
            let input = params.get("input").and_then(|v| v.as_str()).unwrap_or("");
            ToolOutput::success(format!("Processed: {}", input))
        }
    }

    #[tokio::test]
    async fn test_registry() {
        let mut registry = ToolRegistry::new_with_permissions(ToolPermissionContext::permissive());
        registry.register(Arc::new(TestTool));

        assert!(registry.has("test_tool"));
        assert!(!registry.has("nonexistent"));

        let call = ToolCall {
            name: "test_tool".to_string(),
            parameters: json!({"input": "hello"}),
            call_id: "1".to_string(),
        };

        let result = registry.execute(call).await;
        assert!(result.output.success);
        assert!(result.output.result.contains("hello"));
    }

    #[tokio::test]
    async fn test_tool_firewall_blocks_prefix() {
        let mut registry =
            ToolRegistry::new_with_permissions(ToolPermissionContext::with_blocked_prefixes([
                "bash",
            ]));
        registry.register(Arc::new(crate::tools::BashTool::new()));

        let call = ToolCall {
            name: "bash".to_string(),
            parameters: json!({"command": "echo hi"}),
            call_id: "1".to_string(),
        };
        let result = registry.execute(call).await;
        assert!(!result.output.success);
        assert!(
            result.output.error.as_deref().unwrap_or("").contains("blocked"),
            "{:?}",
            result.output.error
        );
    }

    #[tokio::test]
    async fn test_register_all_tools() {
        let mut registry = ToolRegistry::new_with_permissions(ToolPermissionContext::permissive());
        super::super::register_all_tools(&mut registry);

        // Should have 60+ tools registered
        let tools = registry.list_tools();
        assert!(tools.len() >= 40, "Expected 40+ tools, got {}", tools.len());

        // Core tools must exist (note: file tools are "read"/"write"/"edit", enhanced versions are "read_file")
        assert!(registry.has("read"), "read missing");
        assert!(registry.has("write"), "write missing");
        assert!(registry.has("read_file"), "read_file (enhanced) missing");
        assert!(registry.has("bash"), "bash missing");
        assert!(registry.has("grep"), "grep missing");
        assert!(registry.has("web_search"), "web_search missing");
        assert!(registry.has("git_status"), "git_status missing");
        assert!(registry.has("calculator"), "calculator missing");
        assert!(registry.has("system_info"), "system_info missing");
    }

    #[tokio::test]
    async fn test_real_tool_execution_read_file() {
        let mut registry = ToolRegistry::new_with_permissions(ToolPermissionContext::permissive());
        super::super::register_all_tools(&mut registry);

        // Execute read_file on Cargo.toml (should exist)
        let call = ToolCall {
            name: "read_file".to_string(),
            parameters: json!({"path": "Cargo.toml"}),
            call_id: "test_read".to_string(),
        };

        let result = registry.execute(call).await;
        assert!(result.output.success, "read_file failed: {:?}", result.output.error);
        assert!(!result.output.result.is_empty(), "read_file returned empty result");
        assert!(result.output.result.contains("[package]"), "Cargo.toml should contain [package]");
    }

    #[tokio::test]
    async fn test_real_tool_execution_calculator() {
        let mut registry = ToolRegistry::new_with_permissions(ToolPermissionContext::permissive());
        super::super::register_all_tools(&mut registry);

        let call = ToolCall {
            name: "calculator".to_string(),
            parameters: json!({"expression": "2 + 2"}),
            call_id: "test_calc".to_string(),
        };

        let result = registry.execute(call).await;
        assert!(result.output.success, "calculator failed: {:?}", result.output.error);
        assert!(result.output.result.contains("4"), "2+2 should equal 4, got: {}", result.output.result);
    }

    #[tokio::test]
    async fn test_real_tool_execution_system_info() {
        let mut registry = ToolRegistry::new_with_permissions(ToolPermissionContext::permissive());
        super::super::register_all_tools(&mut registry);

        let call = ToolCall {
            name: "system_info".to_string(),
            parameters: json!({}),
            call_id: "test_sysinfo".to_string(),
        };

        let result = registry.execute(call).await;
        assert!(result.output.success, "system_info failed: {:?}", result.output.error);
        assert!(!result.output.result.is_empty(), "system_info returned empty result");
    }

    #[tokio::test]
    async fn test_real_tool_execution_grep() {
        let mut registry = ToolRegistry::new_with_permissions(ToolPermissionContext::permissive());
        super::super::register_all_tools(&mut registry);

        let call = ToolCall {
            name: "grep".to_string(),
            parameters: json!({"pattern": "fn main", "path": "src/"}),
            call_id: "test_grep".to_string(),
        };

        let result = registry.execute(call).await;
        assert!(result.output.success, "grep failed: {:?}", result.output.error);
        // Should find at least one fn main in src/
        assert!(!result.output.result.is_empty(), "grep for 'fn main' in src/ returned empty");
    }

    #[tokio::test]
    async fn test_real_tool_execution_list_directory() {
        let mut registry = ToolRegistry::new_with_permissions(ToolPermissionContext::permissive());
        super::super::register_all_tools(&mut registry);

        let call = ToolCall {
            name: "list_directory".to_string(),
            parameters: json!({"path": "src/"}),
            call_id: "test_ls".to_string(),
        };

        let result = registry.execute(call).await;
        assert!(result.output.success, "list_directory failed: {:?}", result.output.error);
        assert!(!result.output.result.is_empty(), "list_directory returned empty");
    }

    #[tokio::test]
    async fn test_real_tool_execution_git_status() {
        let mut registry = ToolRegistry::new_with_permissions(ToolPermissionContext::permissive());
        super::super::register_all_tools(&mut registry);

        let call = ToolCall {
            name: "git_status".to_string(),
            parameters: json!({}),
            call_id: "test_git".to_string(),
        };

        let result = registry.execute(call).await;
        // git_status should succeed if we're in a git repo
        assert!(result.output.success, "git_status failed: {:?}", result.output.error);
    }
}
