//! Job Scheduler for HSM-II
//!
//! Provides cron-like scheduling, delayed job execution, and persistent
//! background task processing using SQLite storage.

use anyhow::Result;
use chrono::{DateTime, Utc};
use cron::Schedule as CronSchedule;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

pub mod gardening;
pub mod worker;

pub use worker::JobWorker;

/// Job types supported by the scheduler
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum JobType {
    /// Execute an agent task
    AgentTask,
    /// Run a heartbeat check
    Heartbeat,
    /// Sync federation data
    FederationSync,
    /// Clean up old data
    Maintenance,
    /// Custom job with arbitrary payload
    Custom(String),
}

impl std::fmt::Display for JobType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JobType::AgentTask => write!(f, "agent_task"),
            JobType::Heartbeat => write!(f, "heartbeat"),
            JobType::FederationSync => write!(f, "federation_sync"),
            JobType::Maintenance => write!(f, "maintenance"),
            JobType::Custom(s) => write!(f, "custom:{}", s),
        }
    }
}

/// Job priority levels
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum JobPriority {
    Critical,
    High,
    Normal,
    Low,
}

impl JobPriority {
    pub fn as_i32(&self) -> i32 {
        match self {
            JobPriority::Critical => 100,
            JobPriority::High => 75,
            JobPriority::Normal => 50,
            JobPriority::Low => 25,
        }
    }
}

/// Job payload for the scheduler
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Job {
    /// Unique job identifier
    pub id: String,
    /// Type of job
    pub job_type: JobType,
    /// Job priority
    pub priority: JobPriority,
    /// Job payload (JSON string)
    pub payload: String,
    /// When the job was created
    pub created_at: DateTime<Utc>,
    /// Maximum retry attempts
    pub max_retries: i32,
    /// Job metadata
    pub metadata: HashMap<String, String>,
}

impl Job {
    /// Create a new job
    pub fn new(job_type: JobType, payload: impl Serialize) -> Result<Self> {
        Ok(Self {
            id: uuid::Uuid::new_v4().to_string(),
            job_type,
            priority: JobPriority::Normal,
            payload: serde_json::to_string(&payload)?,
            created_at: Utc::now(),
            max_retries: 3,
            metadata: HashMap::new(),
        })
    }

    /// Set priority
    pub fn with_priority(mut self, priority: JobPriority) -> Self {
        self.priority = priority;
        self
    }

    /// Set max retries
    pub fn with_retries(mut self, retries: i32) -> Self {
        self.max_retries = retries;
        self
    }

    /// Add metadata
    pub fn with_metadata(mut self, key: &str, value: &str) -> Self {
        self.metadata.insert(key.to_string(), value.to_string());
        self
    }

    /// Parse payload to a specific type
    pub fn parse_payload<T: for<'de> Deserialize<'de>>(&self) -> Result<T> {
        Ok(serde_json::from_str(&self.payload)?)
    }
}

/// Cron job definition
#[derive(Clone, Debug)]
pub struct CronJob {
    /// Job name/identifier
    pub name: String,
    /// Cron expression (e.g., "0 */5 * * * *" for every 5 minutes)
    pub schedule: CronSchedule,
    /// Job type
    pub job_type: JobType,
    /// Job payload
    pub payload: String,
    /// Whether the job is enabled
    pub enabled: bool,
    /// Last run time
    pub last_run: Option<DateTime<Utc>>,
    /// Next scheduled run
    pub next_run: Option<DateTime<Utc>>,
}

impl CronJob {
    /// Create a new cron job
    pub fn new(name: &str, cron_expr: &str, job_type: JobType, payload: impl Serialize) -> Result<Self> {
        let schedule = CronSchedule::from_str(cron_expr)?;
        let next_run = schedule.upcoming(Utc).next();
        
        Ok(Self {
            name: name.to_string(),
            schedule,
            job_type,
            payload: serde_json::to_string(&payload)?,
            enabled: true,
            last_run: None,
            next_run,
        })
    }

    /// Calculate next run time
    pub fn update_next_run(&mut self) {
        if self.enabled {
            self.next_run = self.schedule.upcoming(Utc).next();
        }
    }
}

/// Job execution result
#[derive(Clone, Debug)]
pub struct JobResult {
    pub success: bool,
    pub message: String,
    pub output: Option<String>,
    pub duration_ms: u64,
}

impl JobResult {
    pub fn success(message: impl Into<String>) -> Self {
        Self {
            success: true,
            message: message.into(),
            output: None,
            duration_ms: 0,
        }
    }

    pub fn failure(message: impl Into<String>) -> Self {
        Self {
            success: false,
            message: message.into(),
            output: None,
            duration_ms: 0,
        }
    }

    pub fn with_output(mut self, output: impl Into<String>) -> Self {
        self.output = Some(output.into());
        self
    }

    pub fn with_duration(mut self, ms: u64) -> Self {
        self.duration_ms = ms;
        self
    }
}

/// Job handler trait - implement this to process jobs
#[async_trait::async_trait]
pub trait JobHandler: Send + Sync {
    async fn handle_job(&self, job: &Job) -> JobResult;
}

/// Scheduler configuration
#[derive(Clone, Debug)]
pub struct SchedulerConfig {
    /// Database path for job storage
    pub database_path: String,
    /// Number of worker threads
    pub worker_count: usize,
    /// Enable retry on failure
    pub enable_retry: bool,
    /// Max concurrent jobs
    pub max_concurrent: usize,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            database_path: "hsmii_jobs.db".to_string(),
            worker_count: 4,
            enable_retry: true,
            max_concurrent: 10,
        }
    }
}

/// Simple in-memory job queue for HSM-II
/// 
/// Note: This is a lightweight implementation. For production with high
/// volume, consider using apalis with Redis or a proper message queue.
pub struct JobScheduler {
    config: SchedulerConfig,
    job_queue: Arc<RwLock<Vec<Job>>>,
    cron_jobs: Arc<RwLock<HashMap<String, CronJob>>>,
    handler: Arc<dyn JobHandler>,
    shutdown_tx: Option<tokio::sync::broadcast::Sender<()>>,
}

impl JobScheduler {
    /// Create a new job scheduler
    pub fn new(config: SchedulerConfig, handler: Arc<dyn JobHandler>) -> Self {
        Self {
            config,
            job_queue: Arc::new(RwLock::new(Vec::new())),
            cron_jobs: Arc::new(RwLock::new(HashMap::new())),
            handler,
            shutdown_tx: None,
        }
    }

    /// Start the scheduler
    pub async fn start(&mut self) -> Result<()> {
        let (shutdown_tx, _) = tokio::sync::broadcast::channel(1);
        self.shutdown_tx = Some(shutdown_tx);

        // Start cron job scheduler
        self.start_cron_scheduler().await;

        // Start job workers
        self.start_workers().await;

        info!("Job scheduler started with {} workers", self.config.worker_count);
        Ok(())
    }

    /// Schedule a one-time job
    pub async fn schedule(&self, job: Job) -> Result<()> {
        let job_id = job.id.clone();
        let job_type = job.job_type.clone();
        let mut queue = self.job_queue.write().await;
        queue.push(job);
        drop(queue);
        info!(job_id = %job_id, job_type = %job_type, "Job scheduled");
        Ok(())
    }

    /// Schedule a job to run at a specific time
    pub async fn schedule_at(&self, job: Job, run_at: DateTime<Utc>) -> Result<()> {
        let delay = run_at.signed_duration_since(Utc::now());
        
        if delay.num_seconds() <= 0 {
            // Run immediately
            self.schedule(job).await?;
        } else {
            // Schedule for later
            let queue = self.job_queue.clone();
            let job_id = job.id.clone();
            let job_type = job.job_type.clone();
            
            tokio::spawn(async move {
                tokio::time::sleep(delay.to_std().unwrap_or(tokio::time::Duration::from_secs(0))).await;
                let mut q = queue.write().await;
                q.push(job);
                info!(job_id = %job_id, job_type = %job_type, "Delayed job enqueued");
            });
        }
        
        Ok(())
    }

    /// Schedule a recurring cron job
    pub async fn schedule_cron(&self, cron_job: CronJob) -> Result<()> {
        let mut jobs = self.cron_jobs.write().await;
        info!(name = %cron_job.name, cron = %cron_job.schedule, "Cron job registered");
        jobs.insert(cron_job.name.clone(), cron_job);
        Ok(())
    }

    /// Remove a cron job
    pub async fn remove_cron(&self, name: &str) -> Result<()> {
        let mut jobs = self.cron_jobs.write().await;
        jobs.remove(name);
        info!(name, "Cron job removed");
        Ok(())
    }

    /// Get all cron jobs
    pub async fn list_cron_jobs(&self) -> Vec<CronJob> {
        let jobs = self.cron_jobs.read().await;
        jobs.values().cloned().collect()
    }

    /// Get pending job count
    pub async fn pending_count(&self) -> usize {
        let queue = self.job_queue.read().await;
        queue.len()
    }

    /// Shutdown the scheduler
    pub async fn shutdown(&self) -> Result<()> {
        if let Some(tx) = &self.shutdown_tx {
            let _ = tx.send(());
        }
        info!("Job scheduler shutting down");
        Ok(())
    }

    /// Start the cron job scheduler loop
    async fn start_cron_scheduler(&self) {
        let cron_jobs = self.cron_jobs.clone();
        let job_queue = self.job_queue.clone();
        let mut shutdown_rx = self.shutdown_tx.as_ref().unwrap().subscribe();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(30));

            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        let now = Utc::now();
                        let mut jobs = cron_jobs.write().await;

                        for (name, cron_job) in jobs.iter_mut() {
                            if !cron_job.enabled {
                                continue;
                            }

                            if let Some(next_run) = cron_job.next_run {
                                if now >= next_run {
                                    // Create and schedule the job
                                    let job = Job {
                                        id: uuid::Uuid::new_v4().to_string(),
                                        job_type: cron_job.job_type.clone(),
                                        priority: JobPriority::Normal,
                                        payload: cron_job.payload.clone(),
                                        created_at: now,
                                        max_retries: 3,
                                        metadata: {
                                            let mut m = HashMap::new();
                                            m.insert("cron_name".to_string(), name.clone());
                                            m
                                        },
                                    };

                                    let mut queue = job_queue.write().await;
                                    queue.push(job);
                                    drop(queue);
                                    
                                    info!(cron_name = %name, "Cron job triggered");

                                    cron_job.last_run = Some(now);
                                    cron_job.update_next_run();
                                }
                            }
                        }
                    }
                    _ = shutdown_rx.recv() => {
                        info!("Cron scheduler shutting down");
                        break;
                    }
                }
            }
        });
    }

    /// Start job workers
    async fn start_workers(&self) {
        let handler = self.handler.clone();
        let job_queue = self.job_queue.clone();

        // Create worker pool
        for i in 0..self.config.worker_count {
            let handler = handler.clone();
            let queue = job_queue.clone();
            let mut shutdown_rx = self.shutdown_tx.as_ref().unwrap().subscribe();

            tokio::spawn(async move {
                info!(worker_id = i, "Job worker started");

                loop {
                    tokio::select! {
                        _ = tokio::time::sleep(tokio::time::Duration::from_millis(100)) => {
                            // Try to get a job
                            let job = {
                                let mut q = queue.write().await;
                                if !q.is_empty() {
                                    // Sort by priority (highest first)
                                    q.sort_by(|a, b| b.priority.as_i32().cmp(&a.priority.as_i32()));
                                    q.pop()
                                } else {
                                    None
                                }
                            };

                            if let Some(job) = job {
                                let start = std::time::Instant::now();
                                let result = handler.handle_job(&job).await;
                                let duration = start.elapsed().as_millis() as u64;

                                if result.success {
                                    info!(
                                        job_id = %job.id,
                                        job_type = %job.job_type,
                                        duration_ms = duration,
                                        "Job completed successfully"
                                    );
                                } else {
                                    warn!(
                                        job_id = %job.id,
                                        job_type = %job.job_type,
                                        error = %result.message,
                                        "Job failed"
                                    );
                                    
                                    // Retry logic could go here
                                }
                            }
                        }
                        _ = shutdown_rx.recv() => {
                            info!(worker_id = i, "Job worker shutting down");
                            break;
                        }
                    }
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Serialize, Deserialize, Debug)]
    struct TestPayload {
        message: String,
    }

    #[test]
    fn test_job_creation() {
        let payload = TestPayload {
            message: "Hello".to_string(),
        };
        let job = Job::new(JobType::Custom("test".to_string()), &payload).unwrap();
        
        assert_eq!(job.job_type.to_string(), "custom:test");
        assert_eq!(job.priority, JobPriority::Normal);
        
        let parsed: TestPayload = job.parse_payload().unwrap();
        assert_eq!(parsed.message, "Hello");
    }

    #[test]
    fn test_cron_job_parsing() {
        let cron = CronJob::new(
            "test_job",
            "0 */5 * * * *", // Every 5 minutes
            JobType::Heartbeat,
            &TestPayload { message: "ping".to_string() },
        );
        
        assert!(cron.is_ok());
        let job = cron.unwrap();
        assert_eq!(job.name, "test_job");
        assert!(job.enabled);
        assert!(job.next_run.is_some());
    }
}
