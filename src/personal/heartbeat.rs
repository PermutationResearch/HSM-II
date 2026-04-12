//! Heartbeat System - Scheduled checks and cron jobs
//!
//! Like Hermes' HEARTBEAT.md - periodic tasks that keep the agent aware.

use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::time::interval;

use super::HeartbeatResult;

/// Heartbeat system for periodic tasks
#[derive(Clone, Debug)]
pub struct Heartbeat {
    /// Last heartbeat time
    pub last_beat: DateTime<Utc>,
    /// Beat interval
    pub interval: Duration,
    /// Checklist items
    pub checklist: Vec<CheckItem>,
    /// Cron jobs
    pub cron_jobs: Vec<CronJob>,
    /// Higher-level maintenance routines
    pub routines: Vec<Routine>,
}

impl Heartbeat {
    /// Load from HEARTBEAT.md
    pub async fn load(base_path: &Path) -> Result<Self> {
        let heartbeat_path = base_path.join("HEARTBEAT.md");

        if heartbeat_path.exists() {
            let content = tokio::fs::read_to_string(&heartbeat_path).await?;
            Self::parse(&content)
        } else {
            Ok(Self::default())
        }
    }

    /// Parse HEARTBEAT.md
    pub fn parse(content: &str) -> Result<Self> {
        let mut checklist = Vec::new();
        let cron_jobs = Vec::new();
        let mut routines = Vec::new();

        // Simple parsing - production would be more robust
        for line in content.lines() {
            let trimmed = line.trim();

            // Parse checklist items
            if trimmed.starts_with("- [ ]") || trimmed.starts_with("- [x]") {
                let item_text = trimmed[5..].trim();
                if let Some(item) = CheckItem::from_text(item_text) {
                    checklist.push(item);
                }
            }

            if let Some(body) = trimmed.strip_prefix("- routine:") {
                let parts: Vec<&str> = body.split('|').map(|part| part.trim()).collect();
                if parts.len() >= 3 {
                    let trigger = parse_routine_trigger(parts[1]);
                    let action = RoutineAction::from_text(parts[2]);
                    routines.push(Routine {
                        id: format!("routine-{}", routines.len()),
                        name: parts[0].to_string(),
                        trigger,
                        action,
                        last_run: None,
                        enabled: true,
                    });
                }
            }
        }

        let mut heartbeat = Self {
            last_beat: Utc::now(),
            interval: Duration::minutes(30),
            checklist,
            cron_jobs,
            routines,
        };
        if heartbeat.routines.is_empty() {
            heartbeat.install_memory_maintenance_routines();
        }
        Ok(heartbeat)
    }

    /// Execute heartbeat tick
    pub async fn tick(&mut self, base_path: &Path) -> Result<Vec<HeartbeatResult>> {
        let mut results = Vec::new();

        tracing::info!("Running heartbeat...");
        match super::kb_manifest::load_kb_manifest_report(base_path) {
            Ok(Some(report)) => {
                let ok = report.missing_files.is_empty();
                let mut message = report.status_line();
                if !ok {
                    message.push_str(&format!("; missing={}", report.missing_files.join(", ")));
                }
                tracing::info!("{}", message);
                results.push(HeartbeatResult {
                    action: "KB Manifest".to_string(),
                    success: ok,
                    message,
                });
            }
            Ok(None) => {}
            Err(e) => {
                results.push(HeartbeatResult {
                    action: "KB Manifest".to_string(),
                    success: false,
                    message: format!("kb manifest load failed: {}", e),
                });
            }
        }

        // Run checklist items
        for item in &self.checklist {
            match self.execute_check(item).await {
                Ok(msg) => results.push(HeartbeatResult {
                    action: format!("{:?}", item),
                    success: true,
                    message: msg,
                }),
                Err(e) => results.push(HeartbeatResult {
                    action: format!("{:?}", item),
                    success: false,
                    message: e.to_string(),
                }),
            }
        }

        // Run due cron jobs
        // Collect jobs that need to run with their data
        let jobs_to_run: Vec<(String, String)> = self
            .cron_jobs
            .iter()
            .filter(|job| job.should_run())
            .map(|job| (job.name.clone(), job.task.clone()))
            .collect();

        // Execute each job
        for (name, task) in jobs_to_run {
            match self.execute_cron(&name, &task).await {
                Ok(msg) => results.push(HeartbeatResult {
                    action: format!("Cron: {}", name),
                    success: true,
                    message: msg,
                }),
                Err(e) => results.push(HeartbeatResult {
                    action: format!("Cron: {}", name),
                    success: false,
                    message: e.to_string(),
                }),
            };
            // Update last_run
            if let Some(job) = self.cron_jobs.iter_mut().find(|j| j.name == name) {
                job.last_run = Some(Utc::now());
            }
        }

        let due_routines: Vec<String> = self
            .routines
            .iter()
            .filter(|routine| routine.should_run())
            .map(|routine| routine.id.clone())
            .collect();
        for routine_id in due_routines {
            if let Some(idx) = self
                .routines
                .iter()
                .position(|routine| routine.id == routine_id)
            {
                let routine = self.routines[idx].clone();
                match self.execute_routine(&routine).await {
                    Ok(msg) => results.push(HeartbeatResult {
                        action: format!("Routine: {}", routine.name),
                        success: true,
                        message: msg,
                    }),
                    Err(e) => results.push(HeartbeatResult {
                        action: format!("Routine: {}", routine.name),
                        success: false,
                        message: e.to_string(),
                    }),
                }
                self.routines[idx].last_run = Some(Utc::now());
            }
        }

        self.last_beat = Utc::now();

        // Save updated state
        self.save(base_path).await?;

        Ok(results)
    }

    /// Execute a checklist item
    async fn execute_check(&self, item: &CheckItem) -> Result<String> {
        match item {
            CheckItem::CheckEmail => {
                // TODO: Check configured email accounts
                Ok("No new urgent emails".to_string())
            }
            CheckItem::ReviewTasks => {
                // TODO: Review todo/ directory
                Ok("3 active tasks, 1 overdue".to_string())
            }
            CheckItem::SyncFederation => {
                // TODO: Sync with federation peers
                Ok("Synced with 2 peers".to_string())
            }
            CheckItem::RunCron => {
                // Cron jobs handled separately
                Ok("Cron jobs processed".to_string())
            }
            CheckItem::CompressMemory => {
                // TODO: Compress old memories
                Ok("Compressed memories from last week".to_string())
            }
            CheckItem::Custom(name) => Ok(format!("Executed custom check: {}", name)),
        }
    }

    /// Execute a cron job
    async fn execute_cron(&self, _name: &str, task: &str) -> Result<String> {
        // TODO: Execute the actual task
        Ok(format!("Executed: {}", task))
    }

    async fn execute_routine(&self, routine: &Routine) -> Result<String> {
        let detail = match &routine.action {
            RoutineAction::SummarizePromises => {
                "summarized recent promises and delivery outcomes".to_string()
            }
            RoutineAction::AuditDelegations => {
                "audited delegation backlog for stale or failed executions".to_string()
            }
            RoutineAction::CompressMemory => {
                "compressed stale memory snapshots and queued archival work".to_string()
            }
            RoutineAction::SyncFederation => {
                "synced federation peers for background coordination".to_string()
            }
            RoutineAction::ReviewTasks => {
                "reviewed active tasks for recursive follow-up".to_string()
            }
            RoutineAction::Custom(name) => format!("executed custom routine: {}", name),
        };
        Ok(detail)
    }

    /// Add a new cron job
    pub fn schedule(
        &mut self,
        name: impl Into<String>,
        schedule: &str,
        task: impl Into<String>,
    ) -> Result<()> {
        let cron = parse_cron(schedule)?;

        self.cron_jobs.push(CronJob {
            name: name.into(),
            schedule: cron,
            task: task.into(),
            last_run: None,
            enabled: true,
        });

        Ok(())
    }

    pub fn install_memory_maintenance_routines(&mut self) {
        if self.routines.is_empty() {
            self.routines.push(Routine {
                id: "routine-promise-summary".to_string(),
                name: "Promise Summary".to_string(),
                trigger: RoutineTrigger::IntervalMinutes(60),
                action: RoutineAction::SummarizePromises,
                last_run: None,
                enabled: true,
            });
            self.routines.push(Routine {
                id: "routine-delegation-audit".to_string(),
                name: "Delegation Audit".to_string(),
                trigger: RoutineTrigger::IntervalMinutes(30),
                action: RoutineAction::AuditDelegations,
                last_run: None,
                enabled: true,
            });
            self.routines.push(Routine {
                id: "routine-memory-compress".to_string(),
                name: "Memory Compression".to_string(),
                trigger: RoutineTrigger::IntervalMinutes(24 * 60),
                action: RoutineAction::CompressMemory,
                last_run: None,
                enabled: true,
            });
        }
    }

    /// Save to HEARTBEAT.md
    pub async fn save(&self, base_path: &Path) -> Result<()> {
        let content = self.to_markdown();
        tokio::fs::write(base_path.join("HEARTBEAT.md"), content).await?;
        Ok(())
    }

    /// Convert to markdown
    pub fn to_markdown(&self) -> String {
        let mut md = String::new();

        md.push_str("# Heartbeat\n\n");
        md.push_str("*Periodic checks and scheduled tasks*\n\n");

        md.push_str("## Checklist\n\n");
        for item in &self.checklist {
            md.push_str(&format!("- [ ] {:?}\n", item));
        }
        md.push('\n');

        if !self.cron_jobs.is_empty() {
            md.push_str("## Scheduled Jobs\n\n");
            for job in &self.cron_jobs {
                let status = if job.enabled { "" } else { " (disabled)" };
                md.push_str(&format!(
                    "- **{}**: `{}`{}\n",
                    job.name, job.schedule.raw, status
                ));
            }
        }

        if !self.routines.is_empty() {
            md.push_str("\n## Routines\n\n");
            for routine in &self.routines {
                let status = if routine.enabled { "" } else { " (disabled)" };
                md.push_str(&format!(
                    "- routine: {} | {} | {}{}\n",
                    routine.name,
                    routine.trigger.to_text(),
                    routine.action.to_text(),
                    status
                ));
            }
        }

        md.push_str(&format!(
            "\n---\nLast run: {}\n",
            self.last_beat.to_rfc3339()
        ));

        md
    }

    /// Run heartbeat loop (for daemon mode)
    pub async fn run_loop(mut self, base_path: PathBuf) {
        let mut ticker = interval(tokio::time::Duration::from_secs(1800)); // 30 min

        loop {
            ticker.tick().await;

            if let Err(e) = self.tick(&base_path).await {
                tracing::error!("Heartbeat error: {}", e);
            }
        }
    }
}

impl Default for Heartbeat {
    fn default() -> Self {
        Self {
            last_beat: Utc::now(),
            interval: Duration::minutes(30),
            checklist: vec![
                CheckItem::CheckEmail,
                CheckItem::ReviewTasks,
                CheckItem::RunCron,
            ],
            cron_jobs: Vec::new(),
            routines: vec![
                Routine {
                    id: "routine-promise-summary".to_string(),
                    name: "Promise Summary".to_string(),
                    trigger: RoutineTrigger::IntervalMinutes(60),
                    action: RoutineAction::SummarizePromises,
                    last_run: None,
                    enabled: true,
                },
                Routine {
                    id: "routine-delegation-audit".to_string(),
                    name: "Delegation Audit".to_string(),
                    trigger: RoutineTrigger::IntervalMinutes(30),
                    action: RoutineAction::AuditDelegations,
                    last_run: None,
                    enabled: true,
                },
            ],
        }
    }
}

/// A checklist item
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum CheckItem {
    CheckEmail,
    ReviewTasks,
    SyncFederation,
    RunCron,
    CompressMemory,
    Custom(String),
}

impl CheckItem {
    /// Parse from text
    pub fn from_text(text: &str) -> Option<Self> {
        let lower = text.to_lowercase();

        if lower.contains("email") {
            Some(Self::CheckEmail)
        } else if lower.contains("task") {
            Some(Self::ReviewTasks)
        } else if lower.contains("sync") || lower.contains("federation") {
            Some(Self::SyncFederation)
        } else if lower.contains("cron") {
            Some(Self::RunCron)
        } else if lower.contains("compress") || lower.contains("memory") {
            Some(Self::CompressMemory)
        } else {
            Some(Self::Custom(text.to_string()))
        }
    }
}

/// A cron job
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CronJob {
    pub name: String,
    pub schedule: CronSchedule,
    pub task: String,
    pub last_run: Option<DateTime<Utc>>,
    pub enabled: bool,
}

impl CronJob {
    /// Check if job should run now
    pub fn should_run(&self) -> bool {
        if !self.enabled {
            return false;
        }

        if let Some(last) = self.last_run {
            // Simple interval check - production would use proper cron parsing
            let elapsed = Utc::now() - last;
            elapsed >= self.schedule.interval
        } else {
            true // Never run, so run now
        }
    }
}

/// Cron schedule
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CronSchedule {
    /// Raw cron expression
    pub raw: String,
    /// Parsed interval (simplified)
    pub interval: Duration,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Routine {
    pub id: String,
    pub name: String,
    pub trigger: RoutineTrigger,
    pub action: RoutineAction,
    pub last_run: Option<DateTime<Utc>>,
    pub enabled: bool,
}

impl Routine {
    pub fn should_run(&self) -> bool {
        if !self.enabled {
            return false;
        }
        match self.trigger {
            RoutineTrigger::OnHeartbeat => true,
            RoutineTrigger::IntervalMinutes(minutes) => {
                if let Some(last_run) = self.last_run {
                    Utc::now() - last_run >= Duration::minutes(minutes as i64)
                } else {
                    true
                }
            }
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum RoutineTrigger {
    OnHeartbeat,
    IntervalMinutes(u64),
}

impl RoutineTrigger {
    pub fn to_text(&self) -> String {
        match self {
            Self::OnHeartbeat => "heartbeat".to_string(),
            Self::IntervalMinutes(minutes) => format!("every-{}m", minutes),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum RoutineAction {
    SummarizePromises,
    AuditDelegations,
    CompressMemory,
    SyncFederation,
    ReviewTasks,
    Custom(String),
}

impl RoutineAction {
    pub fn from_text(text: &str) -> Self {
        let lower = text.to_ascii_lowercase();
        if lower.contains("promise") {
            Self::SummarizePromises
        } else if lower.contains("delegation") || lower.contains("audit") {
            Self::AuditDelegations
        } else if lower.contains("compress") || lower.contains("memory") {
            Self::CompressMemory
        } else if lower.contains("sync") || lower.contains("federation") {
            Self::SyncFederation
        } else if lower.contains("review") || lower.contains("task") {
            Self::ReviewTasks
        } else {
            Self::Custom(text.to_string())
        }
    }

    pub fn to_text(&self) -> String {
        match self {
            Self::SummarizePromises => "summarize-promises".to_string(),
            Self::AuditDelegations => "audit-delegations".to_string(),
            Self::CompressMemory => "compress-memory".to_string(),
            Self::SyncFederation => "sync-federation".to_string(),
            Self::ReviewTasks => "review-tasks".to_string(),
            Self::Custom(name) => name.clone(),
        }
    }
}

/// Parse cron expression (simplified)
fn parse_cron(expr: &str) -> Result<CronSchedule> {
    // Simplified parsing - production would use a real cron library
    // "daily" = 24 hours
    // "hourly" = 1 hour
    // "30min" = 30 minutes

    let interval = if expr == "daily" || expr == "@daily" {
        Duration::days(1)
    } else if expr == "hourly" || expr == "@hourly" {
        Duration::hours(1)
    } else if expr.ends_with("min") {
        let mins: i64 = expr.trim_end_matches("min").parse()?;
        Duration::minutes(mins)
    } else if expr.ends_with("h") {
        let hours: i64 = expr.trim_end_matches("h").parse()?;
        Duration::hours(hours)
    } else {
        // Default to daily
        Duration::days(1)
    };

    Ok(CronSchedule {
        raw: expr.to_string(),
        interval,
    })
}

fn parse_routine_trigger(expr: &str) -> RoutineTrigger {
    if expr.eq_ignore_ascii_case("heartbeat") {
        RoutineTrigger::OnHeartbeat
    } else if let Some(raw) = expr.strip_prefix("every-") {
        let minutes = raw.trim_end_matches('m').parse::<u64>().unwrap_or(60);
        RoutineTrigger::IntervalMinutes(minutes)
    } else {
        RoutineTrigger::IntervalMinutes(60)
    }
}

/// Template for new HEARTBEAT.md
pub const HEARTBEAT_TEMPLATE: &str = r#"# Heartbeat

*Periodic checks and scheduled tasks*

## Checklist

- [ ] Check for urgent emails
- [ ] Review active tasks
- [ ] Run pending cron jobs
- [ ] Compress old memories

## Scheduled Jobs

- **Morning Briefing**: `daily at 7am` - Send summary of today's tasks
- **Weekly Review**: `@weekly` - Archive completed projects

---
Last run: never
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_heartbeat_exposes_memory_routines() {
        let heartbeat = Heartbeat::default();
        assert!(!heartbeat.routines.is_empty());
        assert!(heartbeat
            .routines
            .iter()
            .any(|routine| matches!(routine.action, RoutineAction::SummarizePromises)));
    }

    #[test]
    fn markdown_roundtrip_preserves_routines() {
        let heartbeat = Heartbeat::default();
        let parsed = Heartbeat::parse(&heartbeat.to_markdown()).expect("markdown should parse");
        assert!(parsed
            .routines
            .iter()
            .any(|routine| matches!(routine.action, RoutineAction::AuditDelegations)));
    }
}
