//! Security policy enforcement for Coder Assistant tool execution.
//!
//! Handles:
//! - Tool allowlist enforcement
//! - Secret boundary detection (prevent leaking env vars)
//! - Network boundary enforcement (host allowlists)
//! - Ouroboros/HSM-II governance gate
//! - Output sanitization and exfiltration detection
//! - Audit logging

use super::*;
use crate::agent::Role;
use crate::config::{limits, security};
use crate::council::CouncilMember;
use crate::ouroboros_compat::phase1_policy::{
    ConstitutionConfig, PolicyContext, PolicyDecision, PolicyEngine, ReleaseState,
};
use crate::ouroboros_compat::phase2_risk_gate::{RiskGate, RiskGateConfig};
use crate::ouroboros_compat::phase3_council_bridge::{CouncilBridge, CouncilBridgeConfig};
use crate::ouroboros_compat::phase4_evidence_contract::{
    EvidenceBundle, EvidenceContract, EvidenceRequirements,
};
use crate::ouroboros_compat::phase5_ops_memory::{
    evaluate_runtime_slos, RuntimeSnapshot, RuntimeThresholds,
};
use crate::ouroboros_compat::{ActionKind as OuroActionKind, ProposedAction};
use crate::ToolCallRecord;
use serde_json::json;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use super::tool_executor::{unix_now, ToolContext, ToolError};

// ── Audit Types ──────────────────────────────────────────────────────

/// Immutable record of a single tool execution for compliance and debugging.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolExecutionAudit {
    pub id: String,
    pub tool_name: String,
    pub success: bool,
    pub blocked: bool,
    pub authority: String,
    pub task_key: Option<String>,
    pub sandbox_mode: SandboxMode,
    pub arguments: serde_json::Value,
    pub summary: String,
    pub output_preview: Option<String>,
    pub redaction_applied: bool,
    pub leak_flags: Vec<String>,
    pub native_enforcement: Option<String>,
    pub started_at: u64,
    pub completed_at: u64,
}

/// Builder for audit entries — replaces the 10-parameter positional call.
pub struct AuditEntry<'a> {
    tool_name: &'a str,
    args: &'a serde_json::Value,
    started_at: u64,
    success: bool,
    blocked: bool,
    summary: String,
    output_preview: Option<String>,
    redaction_applied: bool,
    leak_flags: Vec<String>,
    native_enforcement: Option<String>,
}

impl<'a> AuditEntry<'a> {
    pub fn for_tool(tool_name: &'a str, args: &'a serde_json::Value, started_at: u64) -> Self {
        Self {
            tool_name,
            args,
            started_at,
            success: false,
            blocked: false,
            summary: String::new(),
            output_preview: None,
            redaction_applied: false,
            leak_flags: Vec::new(),
            native_enforcement: None,
        }
    }

    pub fn succeeded(mut self, summary: String) -> Self {
        self.success = true;
        self.summary = summary;
        self
    }

    pub fn blocked_with_reason(mut self, reason: String) -> Self {
        self.blocked = true;
        self.summary = reason;
        self
    }

    pub fn failed(mut self, reason: String) -> Self {
        self.summary = reason;
        self
    }

    pub fn failed_security(mut self, reason: String) -> Self {
        self.blocked = true;
        self.summary = reason;
        self
    }

    pub fn with_preview(mut self, preview: String) -> Self {
        self.output_preview = Some(preview);
        self
    }

    pub fn with_redaction(mut self, applied: bool, flags: Vec<String>) -> Self {
        self.redaction_applied = applied;
        self.leak_flags = flags;
        self
    }

    pub fn with_enforcement(mut self, enforcement: Option<String>) -> Self {
        self.native_enforcement = enforcement;
        self
    }

    pub fn record(self, context: &ToolContext, audit_log: &Arc<Mutex<Vec<ToolExecutionAudit>>>) {
        let authority = context
            .env_vars
            .get("OUROBOROS_ACTOR_ID")
            .cloned()
            .unwrap_or_else(|| "coder-agent".to_string());
        let task_key = context.env_vars.get("HSM_TASK_KEY").cloned();

        let audit = ToolExecutionAudit {
            id: format!("tool-audit-{}-{}", self.tool_name, unix_now()),
            tool_name: self.tool_name.to_string(),
            success: self.success,
            blocked: self.blocked,
            authority,
            task_key,
            sandbox_mode: context.execution_policy.sandbox_mode.clone(),
            arguments: self.args.clone(),
            summary: self.summary,
            output_preview: self.output_preview,
            redaction_applied: self.redaction_applied,
            leak_flags: self.leak_flags,
            native_enforcement: self.native_enforcement,
            started_at: self.started_at,
            completed_at: unix_now(),
        };
        audit_log
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(audit);
    }
}

// ── Policy Types ─────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SandboxMode {
    Observe,
    WorkspaceWrite,
    CapabilityWasm,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SecretBoundary {
    pub injected_env_keys: Vec<String>,
    pub secret_env_patterns: Vec<String>,
    pub deny_secret_echo: bool,
}

impl Default for SecretBoundary {
    fn default() -> Self {
        Self {
            injected_env_keys: Vec::new(),
            secret_env_patterns: security::SECRET_ENV_PATTERNS
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
            deny_secret_echo: true,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct NetworkBoundary {
    pub allowed_hosts: Vec<String>,
    pub block_network_for_bash: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExfiltrationPolicy {
    pub enabled: bool,
    pub redact_env_secrets: bool,
    pub suspicious_markers: Vec<String>,
    pub max_output_chars: usize,
}

impl Default for ExfiltrationPolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            redact_env_secrets: true,
            suspicious_markers: security::SUSPICIOUS_MARKERS
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
            max_output_chars: limits::MAX_OUTPUT_CHARS,
        }
    }
}

// ── Policy Enforcement ───────────────────────────────────────────────

/// Check that a tool call is permitted by the execution policy.
pub fn enforce_tool_policy(
    context: &ToolContext,
    tool_name: &str,
    args: &serde_json::Value,
) -> Result<(), ToolError> {
    let policy = &context.execution_policy;
    if !policy.allowed_tools.is_empty()
        && !policy.allowed_tools.iter().any(|name| name == tool_name)
    {
        return Err(ToolError::SecurityViolation(format!(
            "Tool `{}` is not allowed by execution policy",
            tool_name
        )));
    }

    enforce_secret_boundary(context, tool_name, args)?;

    match tool_name {
        "write" => {
            let content = args
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            if content.len() > policy.max_write_bytes {
                return Err(ToolError::SecurityViolation(format!(
                    "write payload exceeds {} bytes",
                    policy.max_write_bytes
                )));
            }
        }
        "edit" => {
            let old_len = args
                .get("oldText")
                .and_then(|v| v.as_str())
                .map(|v| v.len())
                .unwrap_or_default();
            let new_len = args
                .get("newText")
                .and_then(|v| v.as_str())
                .map(|v| v.len())
                .unwrap_or_default();
            if old_len.max(new_len) > policy.max_edit_bytes {
                return Err(ToolError::SecurityViolation(format!(
                    "edit payload exceeds {} bytes",
                    policy.max_edit_bytes
                )));
            }
        }
        "bash" => {
            if matches!(policy.sandbox_mode, SandboxMode::CapabilityWasm) {
                return Err(ToolError::SecurityViolation(
                    "bash is disabled in capability_wasm mode; use a registered MCP/plugin tool instead"
                        .to_string(),
                ));
            }
            let command = args
                .get("command")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            enforce_network_boundary(context, command)?;
        }
        _ => {}
    }

    Ok(())
}

fn enforce_secret_boundary(
    context: &ToolContext,
    tool_name: &str,
    args: &serde_json::Value,
) -> Result<(), ToolError> {
    let arg_text = args.to_string();
    let mut leaked_keys = Vec::new();
    let configured_keys = secret_env_keys(context);
    for key in configured_keys {
        if let Some(value) = context.env_vars.get(&key) {
            if !value.is_empty() && arg_text.contains(value) {
                leaked_keys.push(key);
            }
        }
    }

    if leaked_keys.is_empty() {
        return Ok(());
    }

    Err(ToolError::SecurityViolation(format!(
        "secret boundary blocked `{}` because arguments contained protected values: {}",
        tool_name,
        leaked_keys.join(", ")
    )))
}

pub fn enforce_network_boundary(context: &ToolContext, command: &str) -> Result<(), ToolError> {
    let boundary = &context.execution_policy.network_boundary;
    let hosts = extract_network_hosts(command);
    if hosts.is_empty() {
        return Ok(());
    }

    if boundary.block_network_for_bash {
        return Err(ToolError::SecurityViolation(
            "network egress is disabled for bash tool".to_string(),
        ));
    }

    if boundary.allowed_hosts.is_empty() {
        return Ok(());
    }

    let denied: Vec<String> = hosts
        .into_iter()
        .filter(|host| !host_allowed(host, &boundary.allowed_hosts))
        .collect();
    if denied.is_empty() {
        Ok(())
    } else {
        Err(ToolError::SecurityViolation(format!(
            "network boundary blocked non-allowlisted hosts: {}",
            denied.join(", ")
        )))
    }
}

pub fn enforce_endpoint_allowed(context: &ToolContext, endpoint: &str) -> Result<(), ToolError> {
    let host = parse_host_from_token(endpoint).ok_or_else(|| {
        ToolError::InvalidArguments(format!("invalid provider endpoint: {}", endpoint))
    })?;
    let boundary = &context.execution_policy.network_boundary;

    if boundary.block_network_for_bash {
        return Err(ToolError::SecurityViolation(
            "network egress is disabled for tool providers".to_string(),
        ));
    }

    if boundary.allowed_hosts.is_empty() || host_allowed(&host, &boundary.allowed_hosts) {
        Ok(())
    } else {
        Err(ToolError::SecurityViolation(format!(
            "network boundary blocked non-allowlisted provider host: {}",
            host
        )))
    }
}

// ── Ouroboros Gate ────────────────────────────────────────────────────

/// Evaluate the Ouroboros/HSM-II governance gate for a proposed tool action.
pub fn enforce_ouroboros_gate(
    context: &ToolContext,
    tool_name: &str,
    args: &serde_json::Value,
) -> Result<(), ToolError> {
    let Some(action) = build_proposed_action(context, tool_name, args) else {
        return Ok(());
    };

    let actor_id = context
        .env_vars
        .get("OUROBOROS_ACTOR_ID")
        .cloned()
        .unwrap_or_else(|| "coder-agent".to_string());
    let release_state = ReleaseState {
        version: context.env_vars.get("OUROBOROS_VERSION").cloned(),
        git_tag: context.env_vars.get("OUROBOROS_GIT_TAG").cloned(),
        readme_version: context.env_vars.get("OUROBOROS_README_VERSION").cloned(),
    };

    let policy = PolicyEngine::new(ConstitutionConfig::default()).evaluate(
        &action,
        &PolicyContext {
            requested_by: actor_id,
            release_state,
        },
    );
    let risk = RiskGate::new(RiskGateConfig::default()).assess(&action, &policy);
    let members = default_council_members();
    let bridge = CouncilBridge::new(CouncilBridgeConfig::default());
    let plan = bridge.plan(&action, &risk, &members);

    let now_str = unix_now().to_string();
    let evidence = EvidenceBundle {
        investigation_session_id: Some(format!("tool-{}-{}", tool_name, unix_now())),
        tool_calls: vec![ToolCallRecord {
            id: format!("gate-{}-{}", tool_name, unix_now()),
            tool_name: tool_name.to_string(),
            arguments: args.clone(),
            result_value: None,
            error_message: None,
            started_at: now_str.clone(),
            completed_at: now_str,
        }],
        evidence_chain_count: 1,
        claim_count: 1,
        evidence_count: 1,
        coverage: 1.0,
    };
    let evidence_validation =
        EvidenceContract::new(EvidenceRequirements::default()).validate(&evidence);

    let coherence = context
        .env_vars
        .get("HSM_COHERENCE")
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(crate::config::thresholds::COHERENCE_DEFAULT);
    let stability = context
        .env_vars
        .get("HSM_STABILITY")
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(crate::config::thresholds::STABILITY_DEFAULT);
    let mean_trust = context
        .env_vars
        .get("HSM_MEAN_TRUST")
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(crate::config::thresholds::MEAN_TRUST_DEFAULT);
    let council_confidence = context
        .env_vars
        .get("HSM_COUNCIL_CONFIDENCE")
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(crate::config::thresholds::COUNCIL_CONFIDENCE);

    let slo = evaluate_runtime_slos(
        &RuntimeSnapshot {
            coherence,
            stability,
            mean_trust,
            council_confidence: Some(council_confidence),
            evidence_coverage: Some(evidence.coverage),
        },
        &RuntimeThresholds::default(),
    );

    let policy_allows_execution = !matches!(policy.decision, PolicyDecision::Deny);
    let approved = if risk.council_required {
        bridge.should_approve(
            council_confidence,
            evidence.coverage,
            policy_allows_execution && evidence_validation.ok && slo.healthy,
        )
    } else {
        policy_allows_execution
    };

    if approved {
        return Ok(());
    }

    let mode = plan
        .mode_report
        .as_ref()
        .map(|m| format!("{:?}", m.selected_mode))
        .unwrap_or_else(|| "none".to_string());
    let denial = json!({
        "policy": format!("{:?}", policy.decision),
        "risk_level": format!("{:?}", risk.level),
        "risk_score": risk.score,
        "council_required": risk.council_required,
        "council_mode": mode,
        "policy_reasons": policy.reasons,
        "risk_reasons": risk.reasons,
        "evidence_reasons": evidence_validation.reasons,
        "slo_failures": slo.failed_checks
    });
    Err(ToolError::SecurityViolation(format!(
        "Ouroboros/HSM-II gate blocked {}: {}",
        tool_name, denial
    )))
}

fn build_proposed_action(
    _context: &ToolContext,
    tool_name: &str,
    args: &serde_json::Value,
) -> Option<ProposedAction> {
    if tool_name != "write" && tool_name != "edit" && tool_name != "bash" {
        return None;
    }

    let target_path = args
        .get("path")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let kind = match tool_name {
        "write" | "edit" => OuroActionKind::ExternalWrite,
        "bash" => {
            let cmd = args
                .get("command")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_lowercase();
            if cmd.contains("git commit")
                || cmd.contains("git tag")
                || cmd.contains("git push")
                || cmd.contains("release")
            {
                OuroActionKind::SelfModification
            } else {
                OuroActionKind::ExternalWrite
            }
        }
        _ => OuroActionKind::ReadOnly,
    };

    let mut metadata = std::collections::HashMap::new();
    if tool_name == "bash" {
        metadata.insert("touches_external_system".to_string(), "true".to_string());
    }

    Some(ProposedAction {
        id: format!("tool-{}-{}", tool_name, unix_now()),
        title: format!("Tool call: {}", tool_name),
        description: format!("Coder assistant tool call `{}`", tool_name),
        actor_id: "coder-agent".to_string(),
        kind,
        target_path,
        target_peer: None,
        metadata,
    })
}

// ── Output Sanitization ──────────────────────────────────────────────

/// Sanitize tool output: truncate, redact secrets, flag suspicious content.
pub fn sanitize_output(
    context: &ToolContext,
    output: &str,
) -> Result<(String, bool, Vec<String>), ToolError> {
    let policy = &context.execution_policy.exfiltration_policy;
    let mut sanitized = if output.len() > policy.max_output_chars {
        format!(
            "{}... [truncated, total: {} chars]",
            &output[..policy.max_output_chars],
            output.len()
        )
    } else {
        output.to_string()
    };
    if !policy.enabled {
        return Ok((sanitized, false, Vec::new()));
    }

    let mut flags = Vec::new();
    let mut redaction_applied = false;
    for marker in &policy.suspicious_markers {
        if sanitized.contains(marker) {
            flags.push(format!("marker:{marker}"));
        }
    }

    if policy.redact_env_secrets {
        for key in secret_env_keys(context) {
            if let Some(value) = context.env_vars.get(&key) {
                if !value.is_empty() && sanitized.contains(value) {
                    sanitized = sanitized.replace(value, &format!("[REDACTED:{key}]"));
                    redaction_applied = true;
                    flags.push(format!("env:{key}"));
                }
            }
        }
    }

    if !flags.is_empty() && context.execution_policy.secret_boundary.deny_secret_echo {
        return Err(ToolError::SecurityViolation(format!(
            "tool output blocked by exfiltration policy: {}",
            flags.join(", ")
        )));
    }

    Ok((sanitized, redaction_applied, flags))
}

// ── Helpers ──────────────────────────────────────────────────────────

pub fn secret_env_keys(context: &ToolContext) -> Vec<String> {
    let boundary = &context.execution_policy.secret_boundary;
    let mut keys: HashSet<String> = HashSet::new();
    for key in context.env_vars.keys() {
        let upper = key.to_ascii_uppercase();
        if boundary
            .secret_env_patterns
            .iter()
            .any(|pattern: &String| upper.contains(&pattern.to_ascii_uppercase()))
        {
            keys.insert(key.clone());
        }
    }
    for key in &boundary.injected_env_keys {
        let upper = key.to_ascii_uppercase();
        if boundary
            .secret_env_patterns
            .iter()
            .any(|pattern: &String| upper.contains(&pattern.to_ascii_uppercase()))
        {
            keys.insert(key.clone());
        }
    }
    keys.into_iter().collect()
}

pub fn preview(value: &str) -> String {
    let preview: String = value.chars().take(limits::PREVIEW_CHARS).collect();
    if value.chars().count() > limits::PREVIEW_CHARS {
        format!("{preview}...")
    } else {
        preview
    }
}

pub fn extract_network_hosts(command: &str) -> Vec<String> {
    command
        .split_whitespace()
        .filter_map(parse_host_from_token)
        .collect()
}

pub fn parse_host_from_token(token: &str) -> Option<String> {
    let trimmed = token.trim_matches(|c: char| matches!(c, '"' | '\'' | '(' | ')' | ',' | ';'));
    let without_scheme = if let Some(value) = trimmed.strip_prefix("https://") {
        value
    } else if let Some(value) = trimmed.strip_prefix("http://") {
        value
    } else {
        return None;
    };
    let host = without_scheme
        .split('/')
        .next()
        .unwrap_or_default()
        .split('@')
        .next_back()
        .unwrap_or_default()
        .split(':')
        .next()
        .unwrap_or_default();
    (!host.is_empty()).then(|| host.to_string())
}

pub fn host_allowed(host: &str, allowlist: &[String]) -> bool {
    allowlist.iter().any(|allowed| {
        if let Some(suffix) = allowed.strip_prefix("*.") {
            host == suffix || host.ends_with(&format!(".{}", suffix))
        } else {
            host == allowed
        }
    })
}

fn default_council_members() -> Vec<CouncilMember> {
    vec![
        CouncilMember {
            agent_id: 1,
            role: Role::Architect,
            expertise_score: 0.9,
            participation_weight: 1.0,
        },
        CouncilMember {
            agent_id: 2,
            role: Role::Critic,
            expertise_score: 0.9,
            participation_weight: 1.0,
        },
        CouncilMember {
            agent_id: 3,
            role: Role::Explorer,
            expertise_score: 0.8,
            participation_weight: 1.0,
        },
        CouncilMember {
            agent_id: 4,
            role: Role::Chronicler,
            expertise_score: 0.8,
            participation_weight: 1.0,
        },
    ]
}
