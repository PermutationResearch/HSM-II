# Job Scheduler for HSM-II

The HSM-II job scheduler provides cron-like scheduling, delayed job execution, and background task processing.

## Features

- ✅ **Cron jobs**: Schedule recurring tasks with standard cron expressions
- ✅ **Delayed execution**: Schedule one-time jobs for future execution
- ✅ **Priority queues**: Critical/High/Normal/Low priority levels
- ✅ **Multiple workers**: Parallel job processing
- ✅ **Persistent jobs**: Jobs survive restarts (stored in SQLite)
- ✅ **Job types**: Built-in types + custom job handlers

## Quick Start

### Basic Usage

```rust
use hyper_stigmergy::scheduler::{JobScheduler, SchedulerConfig, Job, JobType};
use hyper_stigmergy::scheduler::worker::{JobWorker, DefaultAgentHandler};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Create a job worker
    let worker = JobWorker::new()
        .with_agent_handler(Box::new(DefaultAgentHandler));
    
    // Create scheduler
    let mut scheduler = JobScheduler::new(
        SchedulerConfig::default(),
        Arc::new(worker)
    );
    
    // Start the scheduler
    scheduler.start().await?;
    
    // Schedule a job
    let job = Job::new(
        JobType::AgentTask,
        &serde_json::json!({
            "task": "Generate daily report",
            "agent": "report_generator"
        })
    )?;
    
    scheduler.schedule(job).await?;
    
    Ok(())
}
```

### Cron Jobs

Schedule recurring tasks:

```rust
use hyper_stigmergy::scheduler::CronJob;

// Every 5 minutes
let job = CronJob::new(
    "heartbeat",
    "0 */5 * * * *",  // Cron expression
    JobType::Heartbeat,
    &serde_json::json!({"check": "system_health"})
)?;

scheduler.schedule_cron(job).await?;
```

### Delayed Jobs

Schedule a job for a specific time:

```rust
use chrono::{Utc, Duration};

let job = Job::new(JobType::AgentTask, &task_payload)?;
let run_at = Utc::now() + Duration::hours(1);

scheduler.schedule_at(job, run_at).await?;
```

## Cron Expression Format

HSM-II uses the standard cron format:

```
┌───────────── Seconds (0-59)
│ ┌───────────── Minutes (0-59)
│ │ ┌───────────── Hours (0-23)
│ │ │ ┌───────────── Day of month (1-31)
│ │ │ │ ┌───────────── Month (1-12)
│ │ │ │ │ ┌───────────── Day of week (0-7, 0=Sunday)
│ │ │ │ │ │
* * * * * *
```

### Examples

| Expression | Description |
|------------|-------------|
| `0 */5 * * * *` | Every 5 minutes |
| `0 0 * * * *` | Every hour |
| `0 0 9 * * *` | Every day at 9:00 AM |
| `0 0 9 * * 1` | Every Monday at 9:00 AM |
| `0 0 0 1 * *` | First day of every month |

## Integration with Personal Agent

The personal agent can use the scheduler for background tasks:

```rust
// In your personal agent setup
let scheduler_config = SchedulerConfig {
    database_path: "~/.hsmii/jobs.db".to_string(),
    worker_count: 4,
    enable_retry: true,
    max_concurrent: 10,
};

let worker = JobWorker::new()
    .with_heartbeat_handler(Box::new(MyHeartbeatHandler))
    .with_agent_handler(Box::new(MyAgentHandler));

let mut scheduler = JobScheduler::new(scheduler_config, Arc::new(worker));
scheduler.start().await?;

// Schedule daily report
let report_job = CronJob::new(
    "daily_report",
    "0 0 9 * * *",  // 9 AM daily
    JobType::AgentTask,
    &serde_json::json!({
        "type": "generate_report",
        "email": "admin@example.com"
    })
)?;
scheduler.schedule_cron(report_job).await?;
```

## Job Types

### Built-in Types

- `JobType::AgentTask` - Execute an agent task
- `JobType::Heartbeat` - System health checks
- `JobType::FederationSync` - Sync with federated nodes
- `JobType::Maintenance` - Cleanup and maintenance tasks
- `JobType::Custom(name)` - Your custom job types

### Custom Job Handlers

```rust
use hyper_stigmergy::scheduler::worker::CustomJobHandler;
use hyper_stigmergy::scheduler::JobResult;

struct EmailJobHandler;

#[async_trait]
impl CustomJobHandler for EmailJobHandler {
    async fn handle(&self, payload: &str) -> JobResult {
        let data: serde_json::Value = match serde_json::from_str(payload) {
            Ok(v) => v,
            Err(e) => return JobResult::failure(format!("Invalid payload: {}", e)),
        };
        
        let to = data["to"].as_str().unwrap_or("default@example.com");
        let subject = data["subject"].as_str().unwrap_or("No subject");
        
        // Send email logic here...
        
        JobResult::success("Email sent")
            .with_output(format!("Sent to: {}", to))
    }
}

// Register the handler
let worker = JobWorker::new()
    .with_custom_handler("send_email", Box::new(EmailJobHandler));
```

## Monitoring

### Check Job Status

```rust
// Get pending job count
let pending = scheduler.pending_count().await;
println!("Pending jobs: {}", pending);

// List all cron jobs
let cron_jobs = scheduler.list_cron_jobs().await;
for job in cron_jobs {
    println!("{}: next run at {:?}", job.name, job.next_run);
}
```

### Job Results

Jobs return a `JobResult`:

```rust
pub struct JobResult {
    pub success: bool,
    pub message: String,
    pub output: Option<String>,
    pub duration_ms: u64,
}
```

## Configuration Options

```rust
SchedulerConfig {
    database_path: "jobs.db".to_string(),  // SQLite database path
    worker_count: 4,                        // Number of worker threads
    enable_retry: true,                     // Retry failed jobs
    max_concurrent: 10,                     // Max concurrent jobs
}
```

## Example: Complete Setup

```rust
use hyper_stigmergy::scheduler::*;
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Setup
    let config = SchedulerConfig {
        database_path: "hsmii_jobs.db".to_string(),
        worker_count: 2,
        ..Default::default()
    };
    
    let worker = JobWorker::new();
    let mut scheduler = JobScheduler::new(config, Arc::new(worker));
    
    // Start
    scheduler.start().await?;
    
    // Schedule jobs
    
    // 1. Heartbeat every 5 minutes
    scheduler.schedule_cron(CronJob::new(
        "heartbeat",
        "0 */5 * * * *",
        JobType::Heartbeat,
        &serde_json::json!({})
    )?).await?;
    
    // 2. Daily report at 9 AM
    scheduler.schedule_cron(CronJob::new(
        "daily_report",
        "0 0 9 * * *",
        JobType::AgentTask,
        &serde_json::json!({
            "task": "Generate daily summary",
            "output": "email"
        })
    )?).await?;
    
    // 3. One-time job in 1 hour
    let delayed_job = Job::new(
        JobType::Maintenance,
        &serde_json::json!({"cleanup": "old_logs"})
    )?;
    scheduler.schedule_at(
        delayed_job,
        chrono::Utc::now() + chrono::Duration::hours(1)
    ).await?;
    
    // Run forever
    tokio::signal::ctrl_c().await?;
    scheduler.shutdown().await?;
    
    Ok(())
}
```

## Integration with Telegram/Discord

Combine the scheduler with chat platforms for scheduled notifications:

```rust
// Schedule a job that sends Telegram messages
let telegram_job = CronJob::new(
    "daily_briefing",
    "0 0 8 * * *",  // 8 AM daily
    JobType::Custom("telegram_notify".to_string()),
    &serde_json::json!({
        "chat_id": "123456789",
        "message": "Good morning! Here's your daily briefing..."
    })
)?;

scheduler.schedule_cron(telegram_job).await?;
```

## Troubleshooting

### Jobs not running

- Check scheduler is started: `scheduler.start().await?`
- Verify worker count > 0
- Check logs for errors

### Cron jobs not triggering

- Verify cron expression format (6 fields: sec min hour day month dow)
- Check system time is correct
- Enable debug logging to see scheduler ticks

### Database errors

- Ensure the directory for `database_path` exists
- Check file permissions
- Try an absolute path instead of relative
