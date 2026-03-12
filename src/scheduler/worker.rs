//! Job Worker for HSM-II Scheduler
//!
//! Provides specialized workers for different job types.

use super::{Job, JobHandler, JobResult, JobType};
use async_trait::async_trait;
use tracing::{info, warn};

/// Default job handler that routes jobs to appropriate processors
pub struct JobWorker {
    agent_handler: Option<Box<dyn AgentTaskHandler>>,
    heartbeat_handler: Option<Box<dyn HeartbeatHandler>>,
    custom_handlers: std::collections::HashMap<String, Box<dyn CustomJobHandler>>,
}

#[async_trait]
pub trait AgentTaskHandler: Send + Sync {
    async fn handle_agent_task(&self, payload: &str) -> JobResult;
}

#[async_trait]
pub trait HeartbeatHandler: Send + Sync {
    async fn handle_heartbeat(&self, payload: &str) -> JobResult;
}

#[async_trait]
pub trait CustomJobHandler: Send + Sync {
    async fn handle(&self, payload: &str) -> JobResult;
}

impl JobWorker {
    pub fn new() -> Self {
        Self {
            agent_handler: None,
            heartbeat_handler: None,
            custom_handlers: std::collections::HashMap::new(),
        }
    }

    pub fn with_agent_handler(mut self, handler: Box<dyn AgentTaskHandler>) -> Self {
        self.agent_handler = Some(handler);
        self
    }

    pub fn with_heartbeat_handler(mut self, handler: Box<dyn HeartbeatHandler>) -> Self {
        self.heartbeat_handler = Some(handler);
        self
    }

    pub fn with_custom_handler(mut self, name: &str, handler: Box<dyn CustomJobHandler>) -> Self {
        self.custom_handlers.insert(name.to_string(), handler);
        self
    }
}

#[async_trait]
impl JobHandler for JobWorker {
    async fn handle_job(&self, job: &Job) -> JobResult {
        info!(
            job_id = %job.id,
            job_type = %job.job_type,
            "Processing job"
        );

        match &job.job_type {
            JobType::AgentTask => {
                if let Some(handler) = &self.agent_handler {
                    handler.handle_agent_task(&job.payload).await
                } else {
                    warn!("No agent task handler registered");
                    JobResult::failure("No agent task handler registered")
                }
            }
            JobType::Heartbeat => {
                if let Some(handler) = &self.heartbeat_handler {
                    handler.handle_heartbeat(&job.payload).await
                } else {
                    // Default heartbeat just logs
                    info!("Heartbeat job executed");
                    JobResult::success("Heartbeat completed")
                }
            }
            JobType::FederationSync => {
                info!("Federation sync job executed");
                JobResult::success("Federation sync completed")
            }
            JobType::Maintenance => {
                info!("Maintenance job executed");
                JobResult::success("Maintenance completed")
            }
            JobType::Custom(name) => {
                if let Some(handler) = self.custom_handlers.get(name) {
                    handler.handle(&job.payload).await
                } else {
                    warn!(job_type = %name, "No custom handler registered for job type");
                    JobResult::failure(format!("No handler for custom job type: {}", name))
                }
            }
        }
    }
}

impl Default for JobWorker {
    fn default() -> Self {
        Self::new()
    }
}

/// Example agent task handler implementation
pub struct DefaultAgentHandler;

#[async_trait]
impl AgentTaskHandler for DefaultAgentHandler {
    async fn handle_agent_task(&self, payload: &str) -> JobResult {
        info!(payload, "Executing agent task");
        
        // Here you would:
        // 1. Parse the payload for task details
        // 2. Create/invoke an agent
        // 3. Execute the task
        // 4. Return the result
        
        JobResult::success("Agent task completed")
            .with_output(format!("Processed: {}", payload))
    }
}

/// Example heartbeat handler implementation
pub struct DefaultHeartbeatHandler;

#[async_trait]
impl HeartbeatHandler for DefaultHeartbeatHandler {
    async fn handle_heartbeat(&self, _payload: &str) -> JobResult {
        let start = std::time::Instant::now();
        
        // Perform health checks
        info!("Running system heartbeat");
        
        // Check various system components
        // - Hypergraph connectivity
        // - LLM provider health
        // - Disk space
        // - Memory usage
        
        let duration = start.elapsed().as_millis() as u64;
        
        JobResult::success("System healthy")
            .with_duration(duration)
    }
}
