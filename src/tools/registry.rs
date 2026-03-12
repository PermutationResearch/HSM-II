//! Tool Registry - manages all available tools

use std::collections::HashMap;
use std::sync::Arc;
use serde_json::Value;
use tokio::time::{Duration, Instant};
use tracing::{debug, info, warn};

use super::{Tool, ToolCall, ToolCallResult, ToolOutput};

/// Registry of all available tools
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
    /// Tool execution timeout
    timeout: Duration,
    /// Track tool usage statistics
    stats: HashMap<String, ToolStats>,
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
        Self {
            tools: HashMap::new(),
            timeout: Duration::from_secs(60),
            stats: HashMap::new(),
        }
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
    
    /// Execute a single tool call
    pub async fn execute(&mut self, call: ToolCall) -> ToolCallResult {
        let start = Instant::now();
        let timestamp = chrono::Utc::now();
        
        let output = if let Some(tool) = self.tools.get(&call.name) {
            debug!("Executing tool: {} with params: {:?}", call.name, call.parameters);
            
            // Execute with timeout
            match tokio::time::timeout(self.timeout, tool.execute(call.parameters.clone())).await {
                Ok(result) => {
                    self.update_stats(&call.name, result.success, start.elapsed());
                    result
                }
                Err(_) => {
                    warn!("Tool {} timed out after {:?}", call.name, self.timeout);
                    self.update_stats(&call.name, false, start.elapsed());
                    ToolOutput::error(format!("Tool timed out after {}s", self.timeout.as_secs()))
                }
            }
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
        let mut registry = ToolRegistry::new();
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
}
