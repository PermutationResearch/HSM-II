//! Core tool executor — dispatches to builtin or external providers,
//! enforces policies, sanitizes output, and records audit entries.

use super::schemas::{ToolProviderKind, ToolProviderMetadata};
use super::security_policy::{self, AuditEntry, ToolExecutionAudit};
use super::tools::ToolExecutionPolicy;
use crate::config::timeouts;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

// ── Context & Error Types ────────────────────────────────────────────

/// Runtime context for tool execution.
#[derive(Clone, Debug)]
pub struct ToolContext {
    pub cwd: std::path::PathBuf,
    pub env_vars: std::collections::HashMap<String, String>,
    pub timeout_ms: u64,
    pub execution_policy: ToolExecutionPolicy,
}

impl Default for ToolContext {
    fn default() -> Self {
        Self {
            cwd: std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
            env_vars: std::collections::HashMap::new(),
            timeout_ms: timeouts::TOOL_EXECUTION_MS,
            execution_policy: ToolExecutionPolicy::default(),
        }
    }
}

/// Tool execution result (success payload).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolResult {
    pub success: bool,
    pub output: String,
    pub error: Option<String>,
    pub execution_time_ms: u64,
}

/// All possible tool error variants.
#[derive(Debug, Clone)]
pub enum ToolError {
    UnknownTool(String),
    MissingArgument(String),
    InvalidArguments(String),
    IoError(String),
    SecurityViolation(String),
    ValidationError(String),
    Timeout,
    CommandFailed { exit_code: i32, stderr: String },
}

impl std::fmt::Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ToolError::UnknownTool(name) => write!(f, "Unknown tool: {}", name),
            ToolError::MissingArgument(name) => write!(f, "Missing required argument: {}", name),
            ToolError::InvalidArguments(msg) => write!(f, "Invalid arguments: {}", msg),
            ToolError::IoError(msg) => write!(f, "IO error: {}", msg),
            ToolError::SecurityViolation(msg) => write!(f, "Security violation: {}", msg),
            ToolError::ValidationError(msg) => write!(f, "Validation error: {}", msg),
            ToolError::Timeout => write!(f, "Tool execution timed out"),
            ToolError::CommandFailed { exit_code, stderr } => {
                write!(f, "Command failed with exit code {}: {}", exit_code, stderr)
            }
        }
    }
}

impl std::error::Error for ToolError {}

impl From<std::io::Error> for ToolError {
    fn from(e: std::io::Error) -> Self {
        ToolError::IoError(e.to_string())
    }
}

// ── Executor ─────────────────────────────────────────────────────────

/// Orchestrates tool execution with policy enforcement and audit logging.
pub struct ToolExecutor {
    context: ToolContext,
    audit_log: Arc<Mutex<Vec<ToolExecutionAudit>>>,
}

impl ToolExecutor {
    pub fn new() -> Self {
        Self {
            context: ToolContext::default(),
            audit_log: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn with_context(context: ToolContext) -> Self {
        Self {
            context,
            audit_log: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn audit_log(&self) -> Vec<ToolExecutionAudit> {
        self.audit_log
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Execute a builtin tool by name.
    pub async fn execute(
        &self,
        tool_name: &str,
        args: &serde_json::Value,
    ) -> Result<String, ToolError> {
        self.execute_with_provider(None, tool_name, args).await
    }

    /// Execute a tool, optionally routing through an external provider.
    pub async fn execute_with_provider(
        &self,
        provider: Option<&ToolProviderMetadata>,
        tool_name: &str,
        args: &serde_json::Value,
    ) -> Result<String, ToolError> {
        let started_at = unix_now();

        // ── Policy gates ─────────────────────────────────────────
        if let Err(err) = security_policy::enforce_tool_policy(&self.context, tool_name, args) {
            AuditEntry::for_tool(tool_name, args, started_at)
                .blocked_with_reason(err.to_string())
                .record(&self.context, &self.audit_log);
            return Err(err);
        }

        if let Err(err) = security_policy::enforce_ouroboros_gate(&self.context, tool_name, args) {
            AuditEntry::for_tool(tool_name, args, started_at)
                .blocked_with_reason(err.to_string())
                .record(&self.context, &self.audit_log);
            return Err(err);
        }

        // ── Dispatch ─────────────────────────────────────────────
        let (result, native_enforcement) = match provider {
            Some(provider) if provider.kind != ToolProviderKind::Builtin => {
                match super::external_providers::execute_external_tool(
                    &self.context,
                    provider,
                    tool_name,
                    args,
                )
                .await
                {
                    Ok(output) => (Ok(output), Some(format!("provider:{}", provider.id))),
                    Err(err) => (Err(err), Some(format!("provider:{}", provider.id))),
                }
            }
            _ => super::builtin_tools::execute_builtin_tool(&self.context, tool_name, args).await,
        };

        // ── Sanitize & audit ─────────────────────────────────────
        match result {
            Ok(output) => match security_policy::sanitize_output(&self.context, &output) {
                Ok((sanitized, redaction_applied, leak_flags)) => {
                    AuditEntry::for_tool(tool_name, args, started_at)
                        .succeeded(format!("tool `{tool_name}` executed successfully"))
                        .with_preview(security_policy::preview(&sanitized))
                        .with_redaction(redaction_applied, leak_flags)
                        .with_enforcement(native_enforcement)
                        .record(&self.context, &self.audit_log);
                    Ok(sanitized)
                }
                Err(err) => {
                    AuditEntry::for_tool(tool_name, args, started_at)
                        .blocked_with_reason(err.to_string())
                        .with_enforcement(native_enforcement)
                        .record(&self.context, &self.audit_log);
                    Err(err)
                }
            },
            Err(err) => {
                let is_security = matches!(err, ToolError::SecurityViolation(_));
                let mut entry = AuditEntry::for_tool(tool_name, args, started_at)
                    .with_enforcement(native_enforcement);
                entry = if is_security {
                    entry.failed_security(err.to_string())
                } else {
                    entry.failed(err.to_string())
                };
                entry.record(&self.context, &self.audit_log);
                Err(err)
            }
        }
    }
}

impl Default for ToolExecutor {
    fn default() -> Self {
        Self::new()
    }
}

// ── Trait for custom tools ───────────────────────────────────────────

#[async_trait::async_trait]
pub trait CoderTool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn schema(&self) -> super::schemas::ToolSchema;
    async fn execute(&self, args: &serde_json::Value) -> Result<String, ToolError>;
}

// ── Helpers ──────────────────────────────────────────────────────────

pub fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
