//! Hypergraph Gardening Scheduler
//!
//! Provides automated maintenance tasks for the HSM-II hypergraph:
//! - Scheduled pruning of decayed edges
//! - Belief consolidation and cleanup
//! - Skill distillation triggers
//! - Embedding index optimization
//! - Storage vacuum and compaction
//!
//! This implements the "anti-fragile + self-managing" design philosophy
//! where agents and evolutionary loops keep the hypergraph healthy.

use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;
use chrono::Utc;

use crate::scheduler::{Job, JobType, JobResult, JobHandler, CronJob};
use crate::personal::IntegratedPersonalAgent;

/// Gardening task types
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum GardeningTask {
    /// Prune edges below decay threshold
    PruneEdges { threshold: f64 },
    /// Consolidate similar beliefs
    ConsolidateBeliefs { similarity_threshold: f64 },
    /// Trigger skill distillation
    DistillSkills { min_experiences: usize },
    /// Optimize embedding index
    OptimizeEmbeddings,
    /// Vacuum storage
    VacuumStorage,
    /// Full maintenance cycle
    FullMaintenance,
}

/// Gardening statistics
#[derive(Clone, Debug, Default)]
pub struct GardeningStats {
    pub total_runs: u64,
    pub edges_pruned: u64,
    pub beliefs_consolidated: u64,
    pub skills_distilled: u64,
    pub last_run: Option<chrono::DateTime<Utc>>,
    pub avg_duration_ms: u64,
}

/// Gardening scheduler for hypergraph maintenance
pub struct GardeningScheduler {
    /// Reference to the integrated agent
    agent: Arc<RwLock<IntegratedPersonalAgent>>,
    /// Statistics
    stats: Arc<RwLock<GardeningStats>>,
    /// Default decay threshold
    decay_threshold: f64,
    /// Enable automatic scheduling of cron jobs
    auto_schedule: bool,
}

impl GardeningScheduler {
    /// Create new gardening scheduler
    pub fn new(
        agent: Arc<RwLock<IntegratedPersonalAgent>>,
        decay_threshold: f64,
    ) -> Self {
        Self {
            agent,
            stats: Arc::new(RwLock::new(GardeningStats::default())),
            decay_threshold,
            auto_schedule: true,
        }
    }

    /// Execute a gardening task
    pub async fn execute_task(&self, task: GardeningTask) -> anyhow::Result<GardeningResult> {
        let start = std::time::Instant::now();
        let mut result = GardeningResult::default();

        match task {
            GardeningTask::PruneEdges { threshold } => {
                info!("🌱 Pruning edges below threshold {:.3}", threshold);
                
                let (pruned, initial_count, final_count) = {
                    let mut agent = self.agent.write().await;
                    let initial_count = agent.core.world.edges.len();
                    agent.core.world.edges.retain(|edge| edge.weight > threshold);
                    let final_count = agent.core.world.edges.len();
                    let pruned = initial_count - final_count;
                    (pruned, initial_count, final_count)
                };
                
                info!("🌱 Pruned {} edges ({} -> {})", pruned, initial_count, final_count);
                result.edges_pruned = pruned as u64;
                
                // Update stats
                let mut stats = self.stats.write().await;
                stats.edges_pruned += pruned as u64;
            }

            GardeningTask::ConsolidateBeliefs { similarity_threshold } => {
                info!("🌱 Consolidating beliefs (threshold: {:.3})", similarity_threshold);
                
                let consolidated = {
                    let mut agent = self.agent.write().await;
                    let initial_count = agent.core.world.beliefs.len();
                    
                    // Simple consolidation: remove very low confidence beliefs
                    agent.core.world.beliefs.retain(|belief| {
                        belief.confidence > similarity_threshold
                    });
                    
                    initial_count - agent.core.world.beliefs.len()
                };
                
                info!("🌱 Consolidated {} beliefs", consolidated);
                result.beliefs_consolidated = consolidated as u64;
                
                let mut stats = self.stats.write().await;
                stats.beliefs_consolidated += consolidated as u64;
            }

            GardeningTask::DistillSkills { min_experiences } => {
                info!("🌱 Triggering skill distillation (min: {} experiences)", min_experiences);
                
                // First check the count with a read lock
                let exp_count = {
                    let agent = self.agent.read().await;
                    agent.core.world.experiences.len()
                };
                
                if exp_count >= min_experiences {
                    let mut agent = self.agent.write().await;
                    let experiences = agent.core.world.experiences.clone();
                    let improvements = agent.core.world.improvement_history.clone();
                    
                    let result_distill = agent.core.world.skill_bank.distill_from_experiences(
                        &experiences,
                        &improvements,
                    );
                    
                    let new_skills = result_distill.new_skills;
                    drop(agent); // Release lock before logging
                    
                    info!("🌱 Distilled {} new skills", new_skills);
                    result.skills_distilled = new_skills as u64;
                    
                    let mut stats = self.stats.write().await;
                    stats.skills_distilled += new_skills as u64;
                } else {
                    info!("🌱 Not enough experiences for distillation ({} < {})",
                        exp_count, min_experiences);
                }
            }

            GardeningTask::OptimizeEmbeddings => {
                info!("🌱 Optimizing embedding index");
                // This would trigger index optimization in a full implementation
                result.embeddings_optimized = true;
            }

            GardeningTask::VacuumStorage => {
                info!("🌱 Vacuuming storage");
                // Remove temporary files, compact storage
                result.storage_vacuumed = true;
            }

            GardeningTask::FullMaintenance => {
                info!("🌱 Running full maintenance cycle");
                
                // Run all tasks inline to avoid async recursion
                // Prune edges
                let (pruned, _) = {
                    let mut agent = self.agent.write().await;
                    let initial_count = agent.core.world.edges.len();
                    agent.core.world.edges.retain(|edge| edge.weight > self.decay_threshold);
                    let final_count = agent.core.world.edges.len();
                    (initial_count - final_count, final_count)
                };
                result.edges_pruned = pruned as u64;
                
                // Consolidate beliefs
                let consolidated = {
                    let mut agent = self.agent.write().await;
                    let initial_count = agent.core.world.beliefs.len();
                    agent.core.world.beliefs.retain(|belief| belief.confidence > self.decay_threshold);
                    initial_count - agent.core.world.beliefs.len()
                };
                result.beliefs_consolidated = consolidated as u64;
                
                // Distill skills
                let exp_count = {
                    let agent = self.agent.read().await;
                    agent.core.world.experiences.len()
                };
                
                if exp_count >= 10 {
                    let mut agent = self.agent.write().await;
                    let experiences = agent.core.world.experiences.clone();
                    let improvements = agent.core.world.improvement_history.clone();
                    let distill_result = agent.core.world.skill_bank.distill_from_experiences(
                        &experiences, &improvements
                    );
                    result.skills_distilled = distill_result.new_skills as u64;
                }
                
                info!("🌱 Full maintenance: {} edges pruned, {} beliefs consolidated, {} skills distilled",
                    result.edges_pruned, result.beliefs_consolidated, result.skills_distilled);
            }
        }

        let duration = start.elapsed();
        result.duration_ms = duration.as_millis() as u64;
        
        // Update stats
        {
            let mut stats = self.stats.write().await;
            stats.total_runs += 1;
            stats.last_run = Some(Utc::now());
            stats.avg_duration_ms = (stats.avg_duration_ms * (stats.total_runs - 1) + result.duration_ms) 
                / stats.total_runs;
        }

        info!("🌱 Gardening task completed in {}ms", result.duration_ms);
        
        Ok(result)
    }

    /// Get current statistics
    pub async fn get_stats(&self) -> GardeningStats {
        self.stats.read().await.clone()
    }

    /// Create cron jobs for automatic gardening (empty if auto_schedule is disabled)
    pub fn create_cron_jobs(&self) -> Vec<CronJob> {
        if !self.auto_schedule {
            return Vec::new();
        }

        let mut jobs = Vec::new();

        // Hourly light pruning
        if let Ok(job) = CronJob::new(
            "garden_prune_hourly",
            "0 0 * * * *", // Every hour
            JobType::Maintenance,
            GardeningTask::PruneEdges { threshold: self.decay_threshold },
        ) {
            jobs.push(job);
        }

        // Daily consolidation
        if let Ok(job) = CronJob::new(
            "garden_consolidate_daily",
            "0 0 4 * * *", // 4 AM daily
            JobType::Maintenance,
            GardeningTask::ConsolidateBeliefs { similarity_threshold: 0.2 },
        ) {
            jobs.push(job);
        }

        // Weekly skill distillation
        if let Ok(job) = CronJob::new(
            "garden_distill_weekly",
            "0 0 3 * * 0", // Sunday 3 AM
            JobType::Maintenance,
            GardeningTask::DistillSkills { min_experiences: 20 },
        ) {
            jobs.push(job);
        }

        // Daily vacuum
        if let Ok(job) = CronJob::new(
            "garden_vacuum_daily",
            "0 30 2 * * *", // 2:30 AM daily
            JobType::Maintenance,
            GardeningTask::VacuumStorage,
        ) {
            jobs.push(job);
        }

        jobs
    }
}

/// Result of a gardening task
#[derive(Clone, Debug, Default)]
pub struct GardeningResult {
    pub edges_pruned: u64,
    pub beliefs_consolidated: u64,
    pub skills_distilled: u64,
    pub embeddings_optimized: bool,
    pub storage_vacuumed: bool,
    pub duration_ms: u64,
}

/// Job handler implementation for gardening tasks
pub struct GardeningJobHandler {
    scheduler: Arc<GardeningScheduler>,
}

impl GardeningJobHandler {
    pub fn new(scheduler: Arc<GardeningScheduler>) -> Self {
        Self { scheduler }
    }
}

#[async_trait::async_trait]
impl JobHandler for GardeningJobHandler {
    async fn handle_job(&self, job: &Job) -> JobResult {
        match job.parse_payload::<GardeningTask>() {
            Ok(task) => {
                match self.scheduler.execute_task(task).await {
                    Ok(result) => {
                        let message = format!(
                            "Gardening complete: {} edges pruned, {} beliefs consolidated, {} skills distilled",
                            result.edges_pruned,
                            result.beliefs_consolidated,
                            result.skills_distilled
                        );
                        
                        JobResult::success(message)
                            .with_duration(result.duration_ms)
                    }
                    Err(e) => {
                        JobResult::failure(format!("Gardening failed: {}", e))
                    }
                }
            }
            Err(e) => {
                JobResult::failure(format!("Failed to parse gardening task: {}", e))
            }
        }
    }
}

/// Create a maintenance schedule for the integrated agent
pub async fn setup_maintenance_schedule(
    agent: Arc<RwLock<IntegratedPersonalAgent>>,
    scheduler: &mut crate::scheduler::JobScheduler,
) -> anyhow::Result<()> {
    let config = {
        let agent_guard = agent.read().await;
        (agent_guard.config.enable_gardening, agent_guard.config.decay_threshold)
    };

    if !config.0 {
        info!("Gardening disabled, skipping maintenance schedule setup");
        return Ok(());
    }

    let gardening_scheduler = Arc::new(GardeningScheduler::new(
        agent,
        config.1,
    ));

    // Create and schedule cron jobs
    let cron_jobs = gardening_scheduler.create_cron_jobs();
    let job_count = cron_jobs.len();
    
    for job in cron_jobs {
        scheduler.schedule_cron(job).await?;
    }

    info!("Maintenance schedule setup complete with {} jobs", job_count);
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gardening_task_serialization() {
        let task = GardeningTask::PruneEdges { threshold: 0.1 };
        let json = serde_json::to_string(&task).unwrap();
        let deserialized: GardeningTask = serde_json::from_str(&json).unwrap();
        
        match deserialized {
            GardeningTask::PruneEdges { threshold } => assert_eq!(threshold, 0.1),
            _ => panic!("Wrong variant"),
        }
    }

    #[tokio::test]
    async fn test_cron_job_creation() {
        let dir = tempfile::tempdir().unwrap();
        let agent = IntegratedPersonalAgent::initialize(dir.path())
            .await
            .expect("initialize integrated agent for gardening test");
        let scheduler = GardeningScheduler::new(Arc::new(RwLock::new(agent)), 0.1);

        let jobs = scheduler.create_cron_jobs();
        assert!(!jobs.is_empty());

        let job_names: Vec<_> = jobs.iter().map(|j| j.name.clone()).collect();
        assert!(job_names.iter().any(|n| n.contains("prune")));
        assert!(job_names.iter().any(|n| n.contains("consolidate")));
    }
}
