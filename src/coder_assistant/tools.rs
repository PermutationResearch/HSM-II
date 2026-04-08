//! Tool Executor for Coder Assistant
//!
//! Executes tools with proper error handling and timeouts

use super::schemas::{ToolProviderKind, ToolProviderMetadata, ToolProviderRuntime, WasmCapability};
use super::*;
use crate::agent::Role;
use crate::council::CouncilMember;
use crate::harness::{ApprovalOutcome, ApprovalService};
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
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time::timeout;

/// Tool execution context
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
            timeout_ms: 60000,
            execution_policy: ToolExecutionPolicy::default(),
        }
    }
}

/// Tool execution result
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolResult {
    pub success: bool,
    pub output: String,
    pub error: Option<String>,
    pub execution_time_ms: u64,
}

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
            secret_env_patterns: vec![
                "KEY".into(),
                "TOKEN".into(),
                "SECRET".into(),
                "PASSWORD".into(),
            ],
            deny_secret_echo: true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NetworkBoundary {
    pub allowed_hosts: Vec<String>,
    pub block_network_for_bash: bool,
}

impl Default for NetworkBoundary {
    fn default() -> Self {
        Self {
            allowed_hosts: Vec::new(),
            // Safety baseline: deny network by default unless explicitly enabled.
            block_network_for_bash: true,
        }
    }
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
            suspicious_markers: vec![
                "BEGIN PRIVATE KEY".into(),
                "BEGIN OPENSSH PRIVATE KEY".into(),
                "ghp_".into(),
                "sk-".into(),
                "xoxb-".into(),
            ],
            max_output_chars: 10000,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolExecutionPolicy {
    pub sandbox_mode: SandboxMode,
    pub allowed_tools: Vec<String>,
    pub secret_boundary: SecretBoundary,
    pub network_boundary: NetworkBoundary,
    pub exfiltration_policy: ExfiltrationPolicy,
    pub max_write_bytes: usize,
    pub max_edit_bytes: usize,
}

impl Default for ToolExecutionPolicy {
    fn default() -> Self {
        Self {
            sandbox_mode: SandboxMode::WorkspaceWrite,
            allowed_tools: Vec::new(),
            secret_boundary: SecretBoundary::default(),
            network_boundary: NetworkBoundary::default(),
            exfiltration_policy: ExfiltrationPolicy::default(),
            max_write_bytes: 256 * 1024,
            max_edit_bytes: 256 * 1024,
        }
    }
}

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

/// Tool executor
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

    /// Execute a tool by name with JSON arguments
    pub async fn execute(
        &self,
        tool_name: &str,
        args: &serde_json::Value,
    ) -> Result<String, ToolError> {
        self.execute_with_provider(None, tool_name, args).await
    }

    pub async fn execute_with_provider(
        &self,
        provider: Option<&ToolProviderMetadata>,
        tool_name: &str,
        args: &serde_json::Value,
    ) -> Result<String, ToolError> {
        let start_time = std::time::Instant::now();
        let started_at = unix_now();

        if let Err(err) = self.enforce_tool_policy(tool_name, args) {
            self.record_audit(
                tool_name,
                args,
                started_at,
                false,
                true,
                format!("blocked(policy): {}", err),
                None,
                false,
                Vec::new(),
                None,
            );
            return Err(err);
        }

        if let Err(err) = self.enforce_ouroboros_gate(tool_name, args) {
            self.record_audit(
                tool_name,
                args,
                started_at,
                false,
                true,
                format!("blocked(policy): {}", err),
                None,
                false,
                Vec::new(),
                None,
            );
            return Err(err);
        }

        if let Err(err) = self.enforce_human_approval(tool_name, args, provider) {
            self.record_audit(
                tool_name,
                args,
                started_at,
                false,
                true,
                format!("blocked(approval): {}", err),
                None,
                false,
                Vec::new(),
                None,
            );
            return Err(err);
        }

        let (result, native_enforcement) = match provider {
            Some(provider) if provider.kind != ToolProviderKind::Builtin => {
                match self.execute_external_tool(provider, tool_name, args).await {
                    Ok(output) => (Ok(output), Some(format!("provider:{}", provider.id))),
                    Err(err) => (Err(err), Some(format!("provider:{}", provider.id))),
                }
            }
            _ => self.execute_builtin_tool(tool_name, args).await,
        };

        let _execution_time_ms = start_time.elapsed().as_millis() as u64;

        match result {
            Ok(output) => match self.sanitize_output(&output) {
                Ok((sanitized, redaction_applied, leak_flags)) => {
                    self.record_audit(
                        tool_name,
                        args,
                        started_at,
                        true,
                        false,
                        format!("tool `{tool_name}` executed successfully"),
                        Some(Self::preview(&sanitized)),
                        redaction_applied,
                        leak_flags,
                        native_enforcement.clone(),
                    );
                    Ok(sanitized)
                }
                Err(err) => {
                    self.record_audit(
                        tool_name,
                        args,
                        started_at,
                        false,
                        true,
                        format!("blocked(policy): {}", err),
                        None,
                        false,
                        Vec::new(),
                        native_enforcement.clone(),
                    );
                    Err(err)
                }
            },
            Err(err) => {
                self.record_audit(
                    tool_name,
                    args,
                    started_at,
                    false,
                    matches!(err, ToolError::SecurityViolation(_)),
                    format!("blocked(policy): {}", err),
                    None,
                    false,
                    Vec::new(),
                    native_enforcement,
                );
                Err(err)
            }
        }
    }

    fn enforce_human_approval(
        &self,
        tool_name: &str,
        args: &serde_json::Value,
        provider: Option<&ToolProviderMetadata>,
    ) -> Result<(), ToolError> {
        let high_risk_local = matches!(tool_name, "bash" | "write" | "edit");
        let external_provider = provider.is_some_and(|p| p.kind != ToolProviderKind::Builtin);
        if !(high_risk_local || external_provider) {
            return Ok(());
        }
        let provider_id = provider.map(|p| p.id.as_str()).unwrap_or("builtin");
        let key = format!("tool:{}@{}", tool_name, provider_id);
        let summary = format!(
            "tool={} provider={} args={}",
            tool_name,
            provider_id,
            Self::preview(&args.to_string())
        );
        let svc = ApprovalService::from_env();
        match svc.evaluate_or_queue(&key, &summary) {
            Ok(ApprovalOutcome::Allow) => Ok(()),
            Ok(ApprovalOutcome::Deny) => Err(ToolError::SecurityViolation(format!(
                "approval denied for {}",
                key
            ))),
            Err(e) => Err(ToolError::SecurityViolation(e.to_string())),
        }
    }

    async fn execute_builtin_tool(
        &self,
        tool_name: &str,
        args: &serde_json::Value,
    ) -> (Result<String, ToolError>, Option<String>) {
        match tool_name {
            "read" => (self.execute_read(args).await, None),
            "write" => (self.execute_write(args).await, None),
            "edit" => (self.execute_edit(args).await, None),
            "bash" => self.execute_bash(args).await,
            "grep" => (self.execute_grep(args).await, None),
            "find" => (self.execute_find(args).await, None),
            "ls" => (self.execute_ls(args).await, None),
            _ => (Err(ToolError::UnknownTool(tool_name.to_string())), None),
        }
    }

    fn enforce_tool_policy(
        &self,
        tool_name: &str,
        args: &serde_json::Value,
    ) -> Result<(), ToolError> {
        let policy = &self.context.execution_policy;
        if !policy.allowed_tools.is_empty()
            && !policy.allowed_tools.iter().any(|name| name == tool_name)
        {
            return Err(ToolError::SecurityViolation(format!(
                "Tool `{}` is not allowed by execution policy",
                tool_name
            )));
        }

        self.enforce_secret_boundary(tool_name, args)?;

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
                self.enforce_network_boundary(command)?;
            }
            _ => {}
        }

        Ok(())
    }

    fn enforce_ouroboros_gate(
        &self,
        tool_name: &str,
        args: &serde_json::Value,
    ) -> Result<(), ToolError> {
        let Some(action) = self.build_proposed_action(tool_name, args) else {
            return Ok(());
        };

        let actor_id = self
            .context
            .env_vars
            .get("OUROBOROS_ACTOR_ID")
            .cloned()
            .unwrap_or_else(|| "coder-agent".to_string());
        let release_state = ReleaseState {
            version: self.context.env_vars.get("OUROBOROS_VERSION").cloned(),
            git_tag: self.context.env_vars.get("OUROBOROS_GIT_TAG").cloned(),
            readme_version: self
                .context
                .env_vars
                .get("OUROBOROS_README_VERSION")
                .cloned(),
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

        let coherence = self
            .context
            .env_vars
            .get("HSM_COHERENCE")
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(0.72);
        let stability = self
            .context
            .env_vars
            .get("HSM_STABILITY")
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(0.30);
        let mean_trust = self
            .context
            .env_vars
            .get("HSM_MEAN_TRUST")
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(0.70);
        let council_confidence = self
            .context
            .env_vars
            .get("HSM_COUNCIL_CONFIDENCE")
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(0.70);

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
        &self,
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

    fn enforce_secret_boundary(
        &self,
        tool_name: &str,
        args: &serde_json::Value,
    ) -> Result<(), ToolError> {
        let arg_text = args.to_string();
        let mut leaked_keys = Vec::new();
        let configured_keys = self.secret_env_keys();
        for key in configured_keys {
            if let Some(value) = self.context.env_vars.get(&key) {
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

    fn enforce_network_boundary(&self, command: &str) -> Result<(), ToolError> {
        let boundary = &self.context.execution_policy.network_boundary;
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

    async fn execute_external_tool(
        &self,
        provider: &ToolProviderMetadata,
        tool_name: &str,
        args: &serde_json::Value,
    ) -> Result<String, ToolError> {
        match provider.runtime.as_ref() {
            Some(ToolProviderRuntime::Http) | None if provider.kind == ToolProviderKind::Mcp => {
                self.execute_mcp_tool(provider, tool_name, args).await
            }
            Some(ToolProviderRuntime::Wasm {
                module_path,
                entrypoint,
                capabilities,
            }) => {
                self.execute_wasm_tool(tool_name, args, module_path, entrypoint, capabilities)
                    .await
            }
            Some(ToolProviderRuntime::Http) => Err(ToolError::InvalidArguments(format!(
                "provider `{}` uses unsupported http runtime for non-MCP tools",
                provider.id
            ))),
            None => Err(ToolError::InvalidArguments(format!(
                "provider `{}` has no live runtime configured",
                provider.id
            ))),
        }
    }

    async fn execute_mcp_tool(
        &self,
        provider: &ToolProviderMetadata,
        tool_name: &str,
        args: &serde_json::Value,
    ) -> Result<String, ToolError> {
        let endpoint = provider.endpoint.as_deref().ok_or_else(|| {
            ToolError::InvalidArguments(format!(
                "provider `{}` is missing an endpoint",
                provider.id
            ))
        })?;
        self.enforce_endpoint_allowed(endpoint)?;

        let client = reqwest::Client::new();
        let request_body = json!({
            "jsonrpc": "2.0",
            "id": format!("tool-{}-{}", tool_name, unix_now()),
            "method": "tools/call",
            "params": {
                "name": tool_name,
                "arguments": args,
            }
        });

        let response = timeout(
            Duration::from_millis(self.context.timeout_ms),
            client.post(endpoint).json(&request_body).send(),
        )
        .await
        .map_err(|_| ToolError::Timeout)?
        .map_err(|e| ToolError::IoError(format!("MCP request failed: {}", e)))?;

        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| ToolError::IoError(format!("MCP response read failed: {}", e)))?;

        if !status.is_success() {
            return Err(ToolError::CommandFailed {
                exit_code: i32::from(status.as_u16()),
                stderr: body,
            });
        }

        decode_external_tool_response(&body)
    }

    async fn execute_wasm_tool(
        &self,
        tool_name: &str,
        args: &serde_json::Value,
        module_path: &str,
        entrypoint: &str,
        capabilities: &[WasmCapability],
    ) -> Result<String, ToolError> {
        let module_path = if std::path::Path::new(module_path).is_absolute() {
            std::path::PathBuf::from(module_path)
        } else {
            self.context.cwd.join(module_path)
        };
        if !self.is_path_allowed(&module_path) {
            return Err(ToolError::SecurityViolation(
                "WASM module path outside project directory".to_string(),
            ));
        }

        let engine = wasmtime::Engine::default();
        let module = wasmtime::Module::from_file(&engine, &module_path)
            .map_err(|e| ToolError::IoError(format!("Cannot load wasm module: {}", e)))?;
        let mut store = wasmtime::Store::new(&engine, ());
        let instance = wasmtime::Instance::new(&mut store, &module, &[])
            .map_err(|e| ToolError::IoError(format!("Cannot instantiate wasm module: {}", e)))?;
        let memory = instance.get_memory(&mut store, "memory").ok_or_else(|| {
            ToolError::ValidationError("wasm module must export memory".to_string())
        })?;
        let alloc = instance
            .get_typed_func::<i32, i32>(&mut store, "alloc")
            .map_err(|e| ToolError::ValidationError(format!("wasm alloc export missing: {}", e)))?;
        let run = instance
            .get_typed_func::<(i32, i32), i64>(&mut store, entrypoint)
            .map_err(|e| {
                ToolError::ValidationError(format!(
                    "wasm entrypoint `{}` missing or invalid: {}",
                    entrypoint, e
                ))
            })?;

        let request = json!({
            "tool": tool_name,
            "arguments": args,
            "cwd": self.context.cwd,
            "timeout_ms": self.context.timeout_ms,
            "env": self.injected_env_vars(),
            "capabilities": capabilities.iter().map(wasm_capability_label).collect::<Vec<_>>(),
        })
        .to_string();

        let request_len = i32::try_from(request.len())
            .map_err(|_| ToolError::InvalidArguments("wasm request too large".to_string()))?;
        let request_ptr = alloc
            .call(&mut store, request_len)
            .map_err(|e| ToolError::IoError(format!("wasm alloc failed: {}", e)))?;
        memory
            .write(&mut store, request_ptr as usize, request.as_bytes())
            .map_err(|e| ToolError::IoError(format!("wasm memory write failed: {}", e)))?;

        let packed = run
            .call(&mut store, (request_ptr, request_len))
            .map_err(|e| ToolError::IoError(format!("wasm execution failed: {}", e)))?;
        let (response_ptr, response_len) = unpack_wasm_ptr_len(packed)?;
        let mut bytes = vec![0u8; response_len];
        memory
            .read(&store, response_ptr, &mut bytes)
            .map_err(|e| ToolError::IoError(format!("wasm memory read failed: {}", e)))?;
        let body = String::from_utf8(bytes).map_err(|e| {
            ToolError::ValidationError(format!("wasm returned invalid utf-8: {}", e))
        })?;

        decode_external_tool_response(&body)
    }

    fn enforce_endpoint_allowed(&self, endpoint: &str) -> Result<(), ToolError> {
        let host = parse_host_from_token(endpoint).ok_or_else(|| {
            ToolError::InvalidArguments(format!("invalid provider endpoint: {}", endpoint))
        })?;
        let boundary = &self.context.execution_policy.network_boundary;

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

    fn injected_env_vars(&self) -> std::collections::HashMap<String, String> {
        self.context
            .execution_policy
            .secret_boundary
            .injected_env_keys
            .iter()
            .filter_map(|key| {
                self.context
                    .env_vars
                    .get(key)
                    .cloned()
                    .map(|value| (key.clone(), value))
            })
            .collect()
    }

    fn sanitize_output(&self, output: &str) -> Result<(String, bool, Vec<String>), ToolError> {
        let policy = &self.context.execution_policy.exfiltration_policy;
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
            for key in self.secret_env_keys() {
                if let Some(value) = self.context.env_vars.get(&key) {
                    if !value.is_empty() && sanitized.contains(value) {
                        sanitized = sanitized.replace(value, &format!("[REDACTED:{key}]"));
                        redaction_applied = true;
                        flags.push(format!("env:{key}"));
                    }
                }
            }
        }

        if !flags.is_empty()
            && self
                .context
                .execution_policy
                .secret_boundary
                .deny_secret_echo
        {
            return Err(ToolError::SecurityViolation(format!(
                "tool output blocked by exfiltration policy: {}",
                flags.join(", ")
            )));
        }

        Ok((sanitized, redaction_applied, flags))
    }

    fn secret_env_keys(&self) -> Vec<String> {
        let boundary = &self.context.execution_policy.secret_boundary;
        let mut keys: HashSet<String> = HashSet::new();
        for key in self.context.env_vars.keys() {
            let upper = key.to_ascii_uppercase();
            if boundary
                .secret_env_patterns
                .iter()
                .any(|pattern| upper.contains(&pattern.to_ascii_uppercase()))
            {
                keys.insert(key.clone());
            }
        }
        for key in &boundary.injected_env_keys {
            let upper = key.to_ascii_uppercase();
            if boundary
                .secret_env_patterns
                .iter()
                .any(|pattern| upper.contains(&pattern.to_ascii_uppercase()))
            {
                keys.insert(key.clone());
            }
        }
        keys.into_iter().collect()
    }

    fn record_audit(
        &self,
        tool_name: &str,
        args: &serde_json::Value,
        started_at: u64,
        success: bool,
        blocked: bool,
        summary: String,
        output_preview: Option<String>,
        redaction_applied: bool,
        leak_flags: Vec<String>,
        native_enforcement: Option<String>,
    ) {
        let authority = self
            .context
            .env_vars
            .get("OUROBOROS_ACTOR_ID")
            .cloned()
            .unwrap_or_else(|| "coder-agent".to_string());
        let task_key = self.context.env_vars.get("HSM_TASK_KEY").cloned();
        let audit = ToolExecutionAudit {
            id: format!("tool-audit-{}-{}", tool_name, unix_now()),
            tool_name: tool_name.to_string(),
            success,
            blocked,
            authority,
            task_key,
            sandbox_mode: self.context.execution_policy.sandbox_mode.clone(),
            arguments: args.clone(),
            summary,
            output_preview,
            redaction_applied,
            leak_flags,
            native_enforcement,
            started_at,
            completed_at: unix_now(),
        };
        self.audit_log
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(audit);
    }

    fn preview(value: &str) -> String {
        let preview: String = value.chars().take(240).collect();
        if value.chars().count() > 240 {
            format!("{preview}...")
        } else {
            preview
        }
    }

    async fn execute_bash_with_runtime(
        &self,
        command: &str,
        timeout_secs: u64,
    ) -> Result<(std::process::Output, Option<String>), ToolError> {
        if self.should_use_native_macos_sandbox() {
            match self
                .execute_bash_with_macos_sandbox(command, timeout_secs)
                .await
            {
                Ok(output) => {
                    if is_nested_sandbox_rejection(&output) {
                        let fallback = self.execute_bash_unsandboxed(command, timeout_secs).await?;
                        return Ok((
                            fallback,
                            Some("sandbox-exec:fallback-nested-sandbox".to_string()),
                        ));
                    }
                    return Ok((output, Some("sandbox-exec:file-sandbox".to_string())));
                }
                Err(ToolError::IoError(message))
                    if message.contains("sandbox_apply: Operation not permitted") =>
                {
                    let output = self.execute_bash_unsandboxed(command, timeout_secs).await?;
                    return Ok((
                        output,
                        Some("sandbox-exec:fallback-nested-sandbox".to_string()),
                    ));
                }
                Err(err) => return Err(err),
            }
        }

        let output = self.execute_bash_unsandboxed(command, timeout_secs).await?;
        Ok((output, Some("direct-process".to_string())))
    }

    async fn execute_bash_unsandboxed(
        &self,
        command: &str,
        timeout_secs: u64,
    ) -> Result<std::process::Output, ToolError> {
        let mut process = tokio::process::Command::new("/bin/bash");
        process
            .arg("--noprofile")
            .arg("--norc")
            .arg("-c")
            .arg(command)
            .current_dir(&self.context.cwd);
        self.apply_curated_environment(&mut process);
        timeout(Duration::from_secs(timeout_secs), process.output())
            .await
            .map_err(|_| ToolError::Timeout)?
            .map_err(|e| ToolError::IoError(format!("Failed to execute: {}", e)))
    }

    async fn execute_bash_with_macos_sandbox(
        &self,
        command: &str,
        timeout_secs: u64,
    ) -> Result<std::process::Output, ToolError> {
        let profile = build_macos_sandbox_profile(
            &self.context.cwd,
            &self.context.execution_policy.network_boundary,
        );
        let mut process = tokio::process::Command::new("/usr/bin/sandbox-exec");
        process
            .arg("-p")
            .arg(profile)
            .arg("/bin/bash")
            .arg("--noprofile")
            .arg("--norc")
            .arg("-c")
            .arg(command)
            .current_dir(&self.context.cwd);
        self.apply_curated_environment(&mut process);
        timeout(Duration::from_secs(timeout_secs), process.output())
            .await
            .map_err(|_| ToolError::Timeout)?
            .map_err(|e| ToolError::IoError(format!("Failed to execute in native sandbox: {}", e)))
    }

    fn apply_curated_environment(&self, process: &mut tokio::process::Command) {
        process.env_clear();
        for (key, value) in curated_passthrough_env() {
            process.env(key, value);
        }
        for key in &self
            .context
            .execution_policy
            .secret_boundary
            .injected_env_keys
        {
            if let Some(value) = self.context.env_vars.get(key) {
                process.env(key, value);
            }
        }
    }

    fn should_use_native_macos_sandbox(&self) -> bool {
        cfg!(target_os = "macos")
            && matches!(
                self.context.execution_policy.sandbox_mode,
                SandboxMode::WorkspaceWrite
            )
            && std::path::Path::new("/usr/bin/sandbox-exec").exists()
    }

    /// Read file contents
    async fn execute_read(&self, args: &serde_json::Value) -> Result<String, ToolError> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or(ToolError::MissingArgument("path".to_string()))?;

        let offset = args.get("offset").and_then(|v| v.as_u64()).unwrap_or(1) as usize;

        let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(2000) as usize;

        let full_path = self.context.cwd.join(path);

        // Security check
        if !self.is_path_allowed(&full_path) {
            return Err(ToolError::SecurityViolation(
                "Path outside project directory".to_string(),
            ));
        }

        let content = tokio::fs::read_to_string(&full_path)
            .await
            .map_err(|e| ToolError::IoError(format!("Cannot read file: {}", e)))?;

        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len();

        let start_idx = if offset > 0 { offset - 1 } else { 0 }.min(total_lines);
        let end_idx = (start_idx + limit).min(total_lines);

        let selected: Vec<&str> = lines[start_idx..end_idx].to_vec();
        let result = selected.join("\n");

        if start_idx > 0 || end_idx < total_lines {
            Ok(format!(
                "{}\n\n[Lines {}-{} of {}]",
                result,
                start_idx + 1,
                end_idx,
                total_lines
            ))
        } else {
            Ok(result)
        }
    }

    /// Write file contents
    async fn execute_write(&self, args: &serde_json::Value) -> Result<String, ToolError> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or(ToolError::MissingArgument("path".to_string()))?;

        let content = args
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or(ToolError::MissingArgument("content".to_string()))?;

        let full_path = self.context.cwd.join(path);

        if !self.is_path_allowed(&full_path) {
            return Err(ToolError::SecurityViolation(
                "Path outside project directory".to_string(),
            ));
        }

        // Create parent directories
        if let Some(parent) = full_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| ToolError::IoError(format!("Cannot create directories: {}", e)))?;
        }

        tokio::fs::write(&full_path, content)
            .await
            .map_err(|e| ToolError::IoError(format!("Cannot write file: {}", e)))?;

        Ok(format!(
            "Wrote {} bytes to {}",
            content.len(),
            full_path.display()
        ))
    }

    /// Edit file (search and replace)
    async fn execute_edit(&self, args: &serde_json::Value) -> Result<String, ToolError> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or(ToolError::MissingArgument("path".to_string()))?;

        let old_text = args
            .get("oldText")
            .and_then(|v| v.as_str())
            .ok_or(ToolError::MissingArgument("oldText".to_string()))?;

        let new_text = args
            .get("newText")
            .and_then(|v| v.as_str())
            .ok_or(ToolError::MissingArgument("newText".to_string()))?;

        let full_path = self.context.cwd.join(path);

        if !self.is_path_allowed(&full_path) {
            return Err(ToolError::SecurityViolation(
                "Path outside project directory".to_string(),
            ));
        }

        let content = tokio::fs::read_to_string(&full_path)
            .await
            .map_err(|e| ToolError::IoError(format!("Cannot read file: {}", e)))?;

        if !content.contains(old_text) {
            return Err(ToolError::ValidationError(format!(
                "oldText not found in file (looking for {} chars)",
                old_text.len()
            )));
        }

        let new_content = content.replacen(old_text, new_text, 1);

        tokio::fs::write(&full_path, new_content)
            .await
            .map_err(|e| ToolError::IoError(format!("Cannot write file: {}", e)))?;

        Ok(format!(
            "Replaced {} chars with {} chars in {}",
            old_text.len(),
            new_text.len(),
            full_path.display()
        ))
    }

    /// Execute bash command
    async fn execute_bash(
        &self,
        args: &serde_json::Value,
    ) -> (Result<String, ToolError>, Option<String>) {
        let command = args
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or(ToolError::MissingArgument("command".to_string()));
        let command = match command {
            Ok(command) => command,
            Err(err) => return (Err(err), None),
        };

        let timeout_secs = args
            .get("timeout")
            .and_then(|v| v.as_u64())
            .unwrap_or(self.context.timeout_ms / 1000);

        // Security: block dangerous commands
        let blocked = [
            "rm -rf /",
            "rm -rf ~",
            "> /dev/sda",
            "mkfs",
            "dd if=/dev/zero",
        ];
        for dangerous in &blocked {
            if command.contains(dangerous) {
                return (
                    Err(ToolError::SecurityViolation(format!(
                        "Blocked dangerous command: {}",
                        dangerous
                    ))),
                    None,
                );
            }
        }

        let (output, native_enforcement) =
            match self.execute_bash_with_runtime(command, timeout_secs).await {
                Ok(value) => value,
                Err(err) => return (Err(err), None),
            };

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        let result = if stderr.is_empty() {
            stdout.to_string()
        } else if stdout.is_empty() {
            stderr.to_string()
        } else {
            format!("stdout:\n{}\n\nstderr:\n{}", stdout, stderr)
        };

        // Truncate if too long
        let truncated = if result.len() > 10000 {
            format!(
                "{}... [truncated, total: {} chars]",
                &result[..10000],
                result.len()
            )
        } else {
            result
        };

        if output.status.success() {
            (Ok(truncated), native_enforcement)
        } else {
            (
                Err(ToolError::CommandFailed {
                    exit_code: output.status.code().unwrap_or(-1),
                    stderr: truncated,
                }),
                native_enforcement,
            )
        }
    }

    /// Grep for pattern in files
    async fn execute_grep(&self, args: &serde_json::Value) -> Result<String, ToolError> {
        let pattern = args
            .get("pattern")
            .and_then(|v| v.as_str())
            .ok_or(ToolError::MissingArgument("pattern".to_string()))?;

        let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");

        let full_path = self.context.cwd.join(path);

        if !self.is_path_allowed(&full_path) {
            return Err(ToolError::SecurityViolation(
                "Path outside project directory".to_string(),
            ));
        }

        // Use ripgrep if available, otherwise grep
        let output = Command::new("rg")
            .args(&["-n", "--max-count", "100", pattern])
            .current_dir(&full_path)
            .output();

        let result = match output {
            Ok(output) => String::from_utf8_lossy(&output.stdout).to_string(),
            Err(_) => {
                // Fallback to grep
                let output = Command::new("grep")
                    .args(&["-rn", "--max-count=100", pattern, "."])
                    .current_dir(&full_path)
                    .output()
                    .map_err(|e| ToolError::IoError(format!("grep failed: {}", e)))?;
                String::from_utf8_lossy(&output.stdout).to_string()
            }
        };

        if result.is_empty() {
            Ok(format!("No matches found for '{}'", pattern))
        } else {
            Ok(result)
        }
    }

    /// Find files by pattern
    async fn execute_find(&self, args: &serde_json::Value) -> Result<String, ToolError> {
        let pattern = args
            .get("pattern")
            .and_then(|v| v.as_str())
            .ok_or(ToolError::MissingArgument("pattern".to_string()))?;

        let output = Command::new("find")
            .args(&[".", "-name", pattern, "-type", "f", "-maxdepth", "5"])
            .current_dir(&self.context.cwd)
            .output()
            .map_err(|e| ToolError::IoError(format!("find failed: {}", e)))?;

        let result = String::from_utf8_lossy(&output.stdout).to_string();

        if result.is_empty() {
            Ok(format!("No files found matching '{}'", pattern))
        } else {
            Ok(result)
        }
    }

    /// List directory contents
    async fn execute_ls(&self, args: &serde_json::Value) -> Result<String, ToolError> {
        let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");

        let full_path = self.context.cwd.join(path);

        if !self.is_path_allowed(&full_path) {
            return Err(ToolError::SecurityViolation(
                "Path outside project directory".to_string(),
            ));
        }

        let mut entries = tokio::fs::read_dir(&full_path)
            .await
            .map_err(|e| ToolError::IoError(format!("Cannot read directory: {}", e)))?;

        let mut dirs = Vec::new();
        let mut files = Vec::new();

        while let Some(entry) = entries.next_entry().await? {
            let name = entry.file_name().to_string_lossy().to_string();
            let metadata = entry.metadata().await?;

            if metadata.is_dir() {
                dirs.push(format!("{}/", name));
            } else {
                files.push(name);
            }
        }

        dirs.sort();
        files.sort();

        let mut output = format!("Directory: {}\n\n", full_path.display());

        if !dirs.is_empty() {
            output.push_str(&format!(
                "Directories ({}):\n  {}\n\n",
                dirs.len(),
                dirs.join("\n  ")
            ));
        }

        if !files.is_empty() {
            output.push_str(&format!(
                "Files ({}):\n  {}",
                files.len(),
                files.join("\n  ")
            ));
        }

        Ok(output)
    }

    /// Check if path is within allowed directory
    fn is_path_allowed(&self, path: &std::path::Path) -> bool {
        // Resolve to absolute path
        let abs_path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.context.cwd.join(path)
        };

        // Check if within cwd
        abs_path.starts_with(&self.context.cwd)
    }
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn decode_external_tool_response(body: &str) -> Result<String, ToolError> {
    let parsed = match serde_json::from_str::<serde_json::Value>(body) {
        Ok(value) => value,
        Err(_) => return Ok(body.to_string()),
    };

    if let Some(error) = parsed.get("error") {
        return Err(ToolError::CommandFailed {
            exit_code: -1,
            stderr: external_value_to_string(error),
        });
    }

    if let Some(result) = parsed.get("result") {
        if result
            .get("isError")
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
        {
            return Err(ToolError::CommandFailed {
                exit_code: -1,
                stderr: external_value_to_string(result),
            });
        }
        return decode_external_result(result);
    }

    decode_external_result(&parsed)
}

fn decode_external_result(value: &serde_json::Value) -> Result<String, ToolError> {
    if let Some(error) = value.get("error") {
        return Err(ToolError::CommandFailed {
            exit_code: -1,
            stderr: external_value_to_string(error),
        });
    }
    if let Some(output) = value.get("output").and_then(|v| v.as_str()) {
        return Ok(output.to_string());
    }
    if let Some(content) = value.get("content") {
        if let Some(text) = content.as_str() {
            return Ok(text.to_string());
        }
        if let Some(items) = content.as_array() {
            let text_parts: Vec<String> = items
                .iter()
                .filter_map(|item| item.get("text").and_then(|text| text.as_str()))
                .map(|text| text.to_string())
                .collect();
            if !text_parts.is_empty() {
                return Ok(text_parts.join("\n"));
            }
        }
    }
    if let Some(structured) = value.get("structuredContent") {
        return Ok(external_value_to_string(structured));
    }
    if let Some(text) = value.as_str() {
        return Ok(text.to_string());
    }
    Ok(external_value_to_string(value))
}

fn external_value_to_string(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.clone(),
        _ => serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string()),
    }
}

fn unpack_wasm_ptr_len(packed: i64) -> Result<(usize, usize), ToolError> {
    let packed = packed as u64;
    let ptr = (packed & 0xFFFF_FFFF) as usize;
    let len = (packed >> 32) as usize;
    if len == 0 {
        return Ok((ptr, len));
    }
    ptr.checked_add(len)
        .map(|_| (ptr, len))
        .ok_or_else(|| ToolError::ValidationError("wasm response pointer overflow".to_string()))
}

fn wasm_capability_label(capability: &WasmCapability) -> &'static str {
    match capability {
        WasmCapability::ReadWorkspace => "read_workspace",
        WasmCapability::WriteWorkspace => "write_workspace",
    }
}

fn extract_network_hosts(command: &str) -> Vec<String> {
    command
        .split_whitespace()
        .filter_map(parse_host_from_token)
        .collect()
}

fn parse_host_from_token(token: &str) -> Option<String> {
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

fn host_allowed(host: &str, allowlist: &[String]) -> bool {
    allowlist.iter().any(|allowed| {
        if let Some(suffix) = allowed.strip_prefix("*.") {
            host == suffix || host.ends_with(&format!(".{}", suffix))
        } else {
            host == allowed
        }
    })
}

fn build_macos_sandbox_profile(
    cwd: &std::path::Path,
    network_boundary: &NetworkBoundary,
) -> String {
    let mut rules = vec![
        "(version 1)".to_string(),
        "(deny default)".to_string(),
        "(allow process*)".to_string(),
        "(allow sysctl-read)".to_string(),
        "(allow file-read*)".to_string(),
        format!(
            "(allow file-write* (subpath \"{}\"))",
            escape_sandbox_path(cwd)
        ),
    ];

    if let Ok(tmpdir) = std::env::var("TMPDIR") {
        rules.push(format!(
            "(allow file-write* (subpath \"{}\"))",
            escape_sandbox_path(std::path::Path::new(&tmpdir))
        ));
    }
    rules.push("(allow file-write* (subpath \"/private/tmp\"))".to_string());

    if network_boundary.block_network_for_bash || network_boundary.allowed_hosts.is_empty() {
        rules.push("(deny network*)".to_string());
    } else {
        rules.push("(allow network*)".to_string());
    }

    rules.join(" ")
}

fn escape_sandbox_path(path: &std::path::Path) -> String {
    path.to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
}

fn curated_passthrough_env() -> Vec<(String, String)> {
    let keys = [
        "PATH", "HOME", "TMPDIR", "USER", "LOGNAME", "LANG", "LC_ALL",
    ];
    keys.iter()
        .filter_map(|key| {
            std::env::var(key)
                .ok()
                .map(|value| ((*key).to_string(), value))
        })
        .collect()
}

fn is_nested_sandbox_rejection(output: &std::process::Output) -> bool {
    output.status.code() == Some(71)
        && String::from_utf8_lossy(&output.stderr)
            .contains("sandbox_apply: Operation not permitted")
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

impl Default for ToolExecutor {
    fn default() -> Self {
        Self::new()
    }
}

/// Tool error types
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

/// Trait for custom tools
#[async_trait::async_trait]
pub trait CoderTool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn schema(&self) -> super::schemas::ToolSchema;
    async fn execute(&self, args: &serde_json::Value) -> Result<String, ToolError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coder_assistant::schemas::{ObjectSchema, ToolRegistry, ToolSchema};
    use axum::{extract::Json, routing::post, Router};
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::OnceLock;
    use tokio::sync::Mutex;

    /// `HSM_APPROVAL_*` is process-global; serialize tests that rewrite it.
    static APPROVAL_ENV_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();

    fn approval_env_lock() -> &'static Mutex<()> {
        APPROVAL_ENV_MUTEX.get_or_init(|| Mutex::new(()))
    }

    /// Restores a previous env value (or unsets) on drop.
    struct EnvSet {
        key: String,
        prev: Option<String>,
    }

    impl EnvSet {
        fn new(key: &str, value: &str) -> Self {
            let prev = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self {
                key: key.to_string(),
                prev,
            }
        }
    }

    impl Drop for EnvSet {
        fn drop(&mut self) {
            match &self.prev {
                Some(v) => std::env::set_var(&self.key, v),
                None => std::env::remove_var(&self.key),
            }
        }
    }

    /// `ApprovalService` defaults to interactive approvals; unit tests need explicit allow rules.
    struct ApprovalAllowGuard {
        _dir: tempfile::TempDir,
        _store_path: EnvSet,
        _interactive: EnvSet,
    }

    fn approval_allow_keys(keys: &[&str]) -> ApprovalAllowGuard {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("approvals.json");
        let rules: Vec<serde_json::Value> = keys
            .iter()
            .map(|k| {
                json!({
                    "key": k,
                    "outcome": "allow",
                    "scope": "test",
                    "actor": "unit",
                    "updated_unix": 0
                })
            })
            .collect();
        let doc = json!({ "rules": rules, "pending": [] });
        std::fs::write(
            &path,
            serde_json::to_vec_pretty(&doc).expect("serialize approvals"),
        )
        .expect("write approvals");
        let store_path = path.to_str().expect("utf8 path").to_string();
        ApprovalAllowGuard {
            _dir: dir,
            _store_path: EnvSet::new("HSM_APPROVAL_STORE", &store_path),
            _interactive: EnvSet::new("HSM_APPROVAL_INTERACTIVE", "0"),
        }
    }

    #[tokio::test]
    async fn blocks_secret_echo_and_records_audit() {
        let mut context = ToolContext::default();
        context
            .env_vars
            .insert("API_TOKEN".into(), "super-secret-value".into());
        context
            .execution_policy
            .secret_boundary
            .injected_env_keys
            .push("API_TOKEN".into());
        let executor = ToolExecutor::with_context(context);

        let result = executor
            .execute("bash", &json!({ "command": "echo super-secret-value" }))
            .await;

        assert!(matches!(result, Err(ToolError::SecurityViolation(_))));
        let audits = executor.audit_log();
        assert_eq!(audits.len(), 1);
        assert!(audits[0].blocked);
        assert!(audits[0].summary.contains("secret boundary"));
    }

    #[tokio::test]
    async fn blocks_non_allowlisted_network_hosts() {
        let mut context = ToolContext::default();
        context
            .execution_policy
            .network_boundary
            .block_network_for_bash = false;
        context.execution_policy.network_boundary.allowed_hosts = vec!["api.example.com".into()];
        let executor = ToolExecutor::with_context(context);

        let result = executor
            .execute(
                "bash",
                &json!({ "command": "curl https://evil.example.net/data" }),
            )
            .await;

        assert!(matches!(result, Err(ToolError::SecurityViolation(_))));
        let audits = executor.audit_log();
        assert_eq!(audits.len(), 1);
        assert!(audits[0].blocked);
        assert!(audits[0].summary.contains("non-allowlisted hosts"));
    }

    #[tokio::test]
    async fn bash_receives_only_injected_environment_variables() {
        let _env = approval_env_lock().lock().await;
        let _appr = approval_allow_keys(&["tool:bash@builtin"]);
        let mut context = ToolContext::default();
        context
            .execution_policy
            .network_boundary
            .block_network_for_bash = false;
        context
            .env_vars
            .insert("API_TOKEN".into(), "super-secret-value".into());
        context
            .env_vars
            .insert("PUBLIC_NAME".into(), "agent-local".into());
        context
            .execution_policy
            .secret_boundary
            .injected_env_keys
            .push("PUBLIC_NAME".into());
        let executor = ToolExecutor::with_context(context);

        let hidden = executor
            .execute(
                "bash",
                &json!({ "command": "printf '%s' \"${API_TOKEN-}\"" }),
            )
            .await
            .expect("bash should execute");
        let exposed = executor
            .execute(
                "bash",
                &json!({ "command": "printf '%s' \"${PUBLIC_NAME-}\"" }),
            )
            .await
            .expect("bash should execute");

        assert_eq!(hidden.trim(), "");
        assert_eq!(exposed.trim(), "agent-local");
    }

    #[tokio::test]
    async fn executes_mcp_provider_tools_over_http() {
        let _env = approval_env_lock().lock().await;
        let _appr = approval_allow_keys(&["tool:mail_search@mailbox-mcp"]);
        let app = Router::new().route(
            "/mcp",
            post(|Json(payload): Json<serde_json::Value>| async move {
                let query = payload["params"]["arguments"]["query"]
                    .as_str()
                    .unwrap_or("missing");
                Json(json!({
                    "jsonrpc": "2.0",
                    "id": payload["id"].clone(),
                    "result": {
                        "content": [
                            {
                                "type": "text",
                                "text": format!("mail: {}", query),
                            }
                        ]
                    }
                }))
            }),
        );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test listener");
        let addr = listener.local_addr().expect("listener addr");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve mcp app");
        });

        let mut registry = ToolRegistry::new();
        registry.register_mcp_provider(
            "mailbox-mcp",
            format!("http://{}/mcp", addr),
            vec!["mail".into()],
        );
        registry
            .register_external_tool(tool_schema("mail_search"), "mailbox-mcp")
            .expect("register external tool");
        let provider = registry
            .provider_for("mail_search")
            .cloned()
            .expect("provider should exist");

        let mut context = ToolContext::default();
        context
            .execution_policy
            .network_boundary
            .block_network_for_bash = false;
        context.execution_policy.network_boundary.allowed_hosts = vec!["127.0.0.1".into()];
        let executor = ToolExecutor::with_context(context);
        let output = executor
            .execute_with_provider(
                Some(&provider),
                "mail_search",
                &json!({ "query": "latest status" }),
            )
            .await
            .expect("mcp tool should execute");

        assert_eq!(output, "mail: latest status");
        server.abort();
    }

    #[tokio::test]
    async fn executes_wasm_provider_tools_in_capability_wasm_mode() {
        let _env = approval_env_lock().lock().await;
        let _appr = approval_allow_keys(&["tool:wasm_transform@wasm-plugin"]);
        let workspace = std::env::temp_dir().join(format!("hsm-tools-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&workspace).expect("create workspace");
        let wasm_path = workspace.join("fixture.wasm");
        let wasm_bytes = wat::parse_str(
            r#"
            (module
              (memory (export "memory") 1)
              (global $heap (mut i32) (i32.const 1024))
              (data (i32.const 2048) "{\"output\":\"wasm ok\"}")
              (func (export "alloc") (param $len i32) (result i32)
                (local $ptr i32)
                global.get $heap
                local.set $ptr
                global.get $heap
                local.get $len
                i32.add
                global.set $heap
                local.get $ptr)
              (func (export "run") (param $ptr i32) (param $len i32) (result i64)
                i64.const 20
                i64.const 32
                i64.shl
                i64.const 2048
                i64.or))
            "#,
        )
        .expect("compile wat");
        std::fs::write(&wasm_path, wasm_bytes).expect("write wasm fixture");

        let mut registry = ToolRegistry::new();
        registry.register_wasm_plugin_provider(
            "wasm-plugin",
            "fixture.wasm",
            vec!["transform".into()],
            vec![WasmCapability::ReadWorkspace],
        );
        registry
            .register_external_tool(tool_schema("wasm_transform"), "wasm-plugin")
            .expect("register wasm tool");
        let provider = registry
            .provider_for("wasm_transform")
            .cloned()
            .expect("provider should exist");

        let mut context = ToolContext::default();
        context.cwd = workspace.clone();
        context.execution_policy.sandbox_mode = SandboxMode::CapabilityWasm;
        let executor = ToolExecutor::with_context(context);

        let bash_result = executor
            .execute("bash", &json!({ "command": "echo blocked" }))
            .await;
        assert!(matches!(bash_result, Err(ToolError::SecurityViolation(_))));

        let output = executor
            .execute_with_provider(
                Some(&provider),
                "wasm_transform",
                &json!({ "prompt": "integrate" }),
            )
            .await
            .expect("wasm tool should execute");

        assert_eq!(output, "wasm ok");
        let _ = std::fs::remove_file(&wasm_path);
        let _ = std::fs::remove_dir(&workspace);
    }

    fn tool_schema(name: &str) -> ToolSchema {
        ToolSchema {
            name: name.to_string(),
            description: format!("external tool {}", name),
            parameters: ObjectSchema {
                schema_type: "object".to_string(),
                properties: HashMap::new(),
            },
            required: vec![],
        }
    }
}
