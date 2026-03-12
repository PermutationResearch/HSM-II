//! Hermes Bridge - Integration between HSM-II and Hermes Agent
//!
//! This crate provides a bridge between HSM-II (Rust) and Hermes Agent (Python),
//! enabling HSM-II to leverage Hermes's tool ecosystem, persistent memory,
//! and multi-platform gateways.

use anyhow::{anyhow, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::{debug, info, warn};
use uuid::Uuid;

pub mod client;
pub mod skill_converter;
pub mod types;

pub use client::{HermesClient, HermesClientBuilder};
pub use skill_converter::SkillConverter;
pub use types::*;

/// Configuration for Hermes Bridge
#[derive(Clone, Debug)]
pub struct BridgeConfig {
    /// Hermes Agent endpoint URL
    pub endpoint: String,
    /// Request timeout
    pub timeout: Duration,
    /// Default toolsets to enable
    pub default_toolsets: Vec<String>,
    /// Maximum turns per execution
    pub max_turns: u32,
    /// Retry configuration
    pub max_retries: u32,
    pub retry_delay: Duration,
}

impl Default for BridgeConfig {
    fn default() -> Self {
        Self {
            endpoint: "http://localhost:8000".to_string(),
            timeout: Duration::from_secs(60),
            default_toolsets: vec![
                "web".to_string(),
                "terminal".to_string(),
                "skills".to_string(),
            ],
            max_turns: 20,
            max_retries: 3,
            retry_delay: Duration::from_secs(1),
        }
    }
}

/// Main bridge interface
pub struct HermesBridge {
    config: BridgeConfig,
    client: Client,
}

impl HermesBridge {
    /// Create a new bridge instance
    pub fn new(config: BridgeConfig) -> Result<Self> {
        let client = Client::builder()
            .timeout(config.timeout)
            .build()
            .map_err(|e| anyhow!("Failed to create HTTP client: {}", e))?;

        info!("HermesBridge initialized with endpoint: {}", config.endpoint);
        
        Ok(Self { config, client })
    }

    /// Execute a task through Hermes Agent
    pub async fn execute(&self, request: ExecutionRequest) -> Result<ExecutionResponse> {
        let task_id = request.task_id.clone().unwrap_or_else(|| Uuid::new_v4().to_string());
        info!("Executing task {} via Hermes", task_id);

        let hermes_request = HermesApiRequest {
            task_id: task_id.clone(),
            prompt: request.prompt,
            toolsets: request.toolsets.unwrap_or_else(|| self.config.default_toolsets.clone()),
            max_turns: request.max_turns.unwrap_or(self.config.max_turns),
            context: request.context.map(|ctx| HermesContext {
                memory: ctx.memory,
                user_profile: ctx.user_profile,
                hsmii_state: ctx.hsmii_state,
            }),
            system_prompt: request.system_prompt,
        };

        let url = format!("{}/api/v1/execute", self.config.endpoint);
        
        let response = self
            .execute_with_retry(&url, &hermes_request)
            .await?;

        info!("Task {} completed with status: {:?}", task_id, response.status);
        
        Ok(ExecutionResponse {
            task_id,
            result: response.result,
            tool_calls: response.tool_calls,
            trajectory: response.trajectory,
            status: response.status.into(),
            metadata: response.metadata,
        })
    }

    /// Execute with retry logic
    async fn execute_with_retry(
        &self,
        url: &str,
        request: &HermesApiRequest,
    ) -> Result<HermesApiResponse> {
        let mut last_error = None;

        for attempt in 0..self.config.max_retries {
            match self.execute_once(url, request).await {
                Ok(response) => return Ok(response),
                Err(e) => {
                    warn!("Attempt {} failed: {}", attempt + 1, e);
                    last_error = Some(e);
                    
                    if attempt < self.config.max_retries - 1 {
                        tokio::time::sleep(self.config.retry_delay).await;
                    }
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow!("All retry attempts failed")))
    }

    /// Single execution attempt
    async fn execute_once(
        &self,
        url: &str,
        request: &HermesApiRequest,
    ) -> Result<HermesApiResponse> {
        debug!("Sending request to Hermes: {}", url);
        
        let response = self
            .client
            .post(url)
            .json(request)
            .send()
            .await
            .map_err(|e| anyhow!("Request failed: {}", e))?;

        let status = response.status();
        
        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(anyhow!("Hermes returned {}: {}", status, error_text));
        }

        let hermes_response: HermesApiResponse = response
            .json()
            .await
            .map_err(|e| anyhow!("Failed to parse response: {}", e))?;

        Ok(hermes_response)
    }

    /// Health check
    pub async fn health_check(&self) -> Result<HermesHealth> {
        let url = format!("{}/api/v1/health", self.config.endpoint);
        
        let response = self
            .client
            .get(&url)
            .timeout(Duration::from_secs(5))
            .send()
            .await
            .map_err(|e| anyhow!("Health check failed: {}", e))?;

        let health: HermesHealth = response
            .json()
            .await
            .map_err(|e| anyhow!("Failed to parse health response: {}", e))?;

        Ok(health)
    }

    /// Sync skills with Hermes
    pub async fn sync_skills(&self, skills: Vec<HermesSkill>) -> Result<SkillSyncResult> {
        let url = format!("{}/api/v1/skills/sync", self.config.endpoint);
        
        let response = self
            .client
            .post(&url)
            .json(&skills)
            .send()
            .await
            .map_err(|e| anyhow!("Skill sync failed: {}", e))?;

        let result: SkillSyncResult = response
            .json()
            .await
            .map_err(|e| anyhow!("Failed to parse sync result: {}", e))?;

        info!(
            "Skill sync completed: {} imported, {} exported",
            result.imported.len(),
            result.exported.len()
        );

        Ok(result)
    }

    /// Get available toolsets from Hermes
    pub async fn get_available_toolsets(&self) -> Result<Vec<String>> {
        let url = format!("{}/api/v1/toolsets", self.config.endpoint);
        
        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| anyhow!("Failed to get toolsets: {}", e))?;

        let toolsets: Vec<String> = response
            .json()
            .await
            .map_err(|e| anyhow!("Failed to parse toolsets: {}", e))?;

        Ok(toolsets)
    }
}

/// Internal API types for Hermes communication
#[derive(Serialize, Debug)]
struct HermesApiRequest {
    task_id: String,
    prompt: String,
    toolsets: Vec<String>,
    max_turns: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    context: Option<HermesContext>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system_prompt: Option<String>,
}

#[derive(Deserialize, Debug)]
struct HermesApiResponse {
    #[allow(dead_code)]
    task_id: String,
    result: String,
    tool_calls: Vec<ToolCall>,
    trajectory: Vec<Turn>,
    status: String,
    #[serde(default)]
    metadata: Option<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_bridge_creation() {
        let config = BridgeConfig::default();
        let bridge = HermesBridge::new(config);
        assert!(bridge.is_ok());
    }
}
