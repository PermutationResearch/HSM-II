//! High-level client for HSM-II integration

use crate::{
    BridgeConfig, ExecutionRequest, ExecutionResponse, ExecutionStatus, HermesBridge,
    HermesHealth, SkillConverter, SkillSyncResult,
};
use anyhow::{anyhow, Result};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

/// High-level client for HSM-II to use Hermes capabilities
pub struct HermesClient {
    bridge: HermesBridge,
    converter: SkillConverter,
    /// Cache of available toolsets
    toolsets: Arc<RwLock<Vec<String>>>,
    /// Health status cache
    health: Arc<RwLock<Option<HermesHealth>>>,
}

impl HermesClient {
    /// Create a new Hermes client
    pub fn new(config: BridgeConfig) -> Result<Self> {
        let bridge = HermesBridge::new(config)?;
        let converter = SkillConverter::new();

        Ok(Self {
            bridge,
            converter,
            toolsets: Arc::new(RwLock::new(Vec::new())),
            health: Arc::new(RwLock::new(None)),
        })
    }

    /// Initialize the client (fetch toolsets, check health)
    pub async fn initialize(&self) -> Result<()> {
        info!("Initializing Hermes client...");

        // Check health
        let health = self.bridge.health_check().await?;
        info!("Hermes health: {} (v{})", health.status, health.version);
        *self.health.write().await = Some(health.clone());

        // Fetch available toolsets
        let toolsets = self.bridge.get_available_toolsets().await?;
        info!("Available toolsets: {:?}", toolsets);
        *self.toolsets.write().await = toolsets;

        Ok(())
    }

    /// Execute a simple task
    pub async fn execute(&self, prompt: impl Into<String>) -> Result<String> {
        let request = ExecutionRequest::builder(prompt).build();
        let response = self.bridge.execute(request).await?;

        match response.status {
            ExecutionStatus::Success => Ok(response.result),
            ExecutionStatus::PartialSuccess => {
                warn!("Task completed with partial success");
                Ok(response.result)
            }
            ExecutionStatus::Failed => Err(anyhow!("Task failed: {}", response.result)),
            ExecutionStatus::Timeout => Err(anyhow!("Task timed out")),
            ExecutionStatus::Cancelled => Err(anyhow!("Task was cancelled")),
        }
    }

    /// Execute with full control
    pub async fn execute_full(&self, request: ExecutionRequest) -> Result<ExecutionResponse> {
        self.bridge.execute(request).await
    }

    /// Execute a web search
    pub async fn web_search(&self, query: impl Into<String>) -> Result<String> {
        let request = ExecutionRequest::builder(format!(
            "Search the web for: {}. Provide a comprehensive summary of the top results.",
            query.into()
        ))
        .toolsets(vec!["web".to_string()])
        .max_turns(10)
        .build();

        let response = self.bridge.execute(request).await?;
        Ok(response.result)
    }

    /// Execute terminal command (sandboxed)
    pub async fn terminal_command(
        &self,
        command: impl Into<String>,
        working_dir: Option<impl Into<String>>,
    ) -> Result<String> {
        let prompt = if let Some(dir) = working_dir {
            format!(
                "Execute in directory '{}': {}. Return the output.",
                dir.into(),
                command.into()
            )
        } else {
            format!("Execute: {}. Return the output.", command.into())
        };

        let request = ExecutionRequest::builder(prompt)
            .toolsets(vec!["terminal".to_string()])
            .max_turns(5)
            .build();

        let response = self.bridge.execute(request).await?;
        Ok(response.result)
    }

    /// Read a file via Hermes
    pub async fn read_file(&self, path: impl Into<String>) -> Result<String> {
        let request = ExecutionRequest::builder(format!(
            "Read the file at '{}' and return its contents.",
            path.into()
        ))
        .toolsets(vec!["terminal".to_string()])
        .max_turns(3)
        .build();

        let response = self.bridge.execute(request).await?;
        Ok(response.result)
    }

    /// Write a file via Hermes
    pub async fn write_file(
        &self,
        path: impl Into<String>,
        content: impl Into<String>,
    ) -> Result<()> {
        let request = ExecutionRequest::builder(format!(
            "Write the following content to '{}':\n\n{}",
            path.into(),
            content.into()
        ))
        .toolsets(vec!["terminal".to_string()])
        .max_turns(3)
        .build();

        let response = self.bridge.execute(request).await?;
        
        if response.status == ExecutionStatus::Success {
            Ok(())
        } else {
            Err(anyhow!("Failed to write file: {}", response.result))
        }
    }

    /// Spawn a subagent for parallel work
    pub async fn spawn_subagent(
        &self,
        task: impl Into<String>,
        context: Option<serde_json::Value>,
    ) -> Result<String> {
        let mut request = ExecutionRequest::builder(format!(
            "Spawn a subagent to handle this task independently: {}",
            task.into()
        ))
        .toolsets(vec!["delegation".to_string()])
        .max_turns(15)
        .build();

        if let Some(ctx) = context {
            request.context = Some(crate::HSMIIContext {
                memory: std::collections::HashMap::new(),
                user_profile: crate::UserProfile {
                    name: "HSM-II".to_string(),
                    expertise: vec!["agent_coordination".to_string()],
                    preferences: std::collections::HashMap::new(),
                },
                hsmii_state: ctx,
            });
        }

        let response = self.bridge.execute(request).await?;
        Ok(response.result)
    }

    /// Schedule a cron job
    pub async fn schedule_job(
        &self,
        schedule: impl Into<String>,
        task: impl Into<String>,
    ) -> Result<String> {
        let request = ExecutionRequest::builder(format!(
            "Schedule a cron job with schedule '{}' to: {}",
            schedule.into(),
            task.into()
        ))
        .toolsets(vec!["cron".to_string()])
        .max_turns(5)
        .build();

        let response = self.bridge.execute(request).await?;
        Ok(response.result)
    }

    /// Sync skills with Hermes
    pub async fn sync_skills(&self, skills: Vec<crate::CASSSkill>) -> Result<SkillSyncResult> {
        let hermes_skills = self.converter.cass_batch_to_hermes(&skills);
        self.bridge.sync_skills(hermes_skills).await
    }

    /// Get cached health status
    pub async fn health(&self) -> Option<HermesHealth> {
        self.health.read().await.clone()
    }

    /// Refresh health status
    pub async fn refresh_health(&self) -> Result<HermesHealth> {
        let health = self.bridge.health_check().await?;
        *self.health.write().await = Some(health.clone());
        Ok(health)
    }

    /// Get cached toolsets
    pub async fn toolsets(&self) -> Vec<String> {
        self.toolsets.read().await.clone()
    }

    /// Check if a specific toolset is available
    pub async fn has_toolset(&self, name: &str) -> bool {
        self.toolsets.read().await.contains(&name.to_string())
    }

    /// Refresh toolset cache
    pub async fn refresh_toolsets(&self) -> Result<Vec<String>> {
        let toolsets = self.bridge.get_available_toolsets().await?;
        *self.toolsets.write().await = toolsets.clone();
        Ok(toolsets)
    }
}

/// Builder for HermesClient
pub struct HermesClientBuilder {
    config: BridgeConfig,
}

impl HermesClientBuilder {
    pub fn new() -> Self {
        Self {
            config: BridgeConfig::default(),
        }
    }

    pub fn endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.config.endpoint = endpoint.into();
        self
    }

    pub fn timeout_secs(mut self, secs: u64) -> Self {
        self.config.timeout = std::time::Duration::from_secs(secs);
        self
    }

    pub fn default_toolsets(mut self, toolsets: Vec<String>) -> Self {
        self.config.default_toolsets = toolsets;
        self
    }

    pub fn max_turns(mut self, turns: u32) -> Self {
        self.config.max_turns = turns;
        self
    }

    pub fn build(self) -> Result<HermesClient> {
        HermesClient::new(self.config)
    }
}

impl Default for HermesClientBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_builder() {
        let client = HermesClientBuilder::new()
            .endpoint("http://test:8000")
            .timeout_secs(30)
            .max_turns(10)
            .build();

        // Client creation should succeed (connection happens during operations)
        assert!(client.is_ok());
    }
}
