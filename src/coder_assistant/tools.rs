//! Tool Executor for Coder Assistant — re-exports from focused sub-modules.
//!
//! The implementation is split across:
//! - `tool_executor`       — core dispatch, ToolError, ToolContext
//! - `security_policy`     — policy enforcement, audit, boundaries
//! - `builtin_tools`       — read/write/edit/bash/grep/find/ls
//! - `external_providers`  — MCP (HTTP) and WASM execution
//! - `sandbox`             — macOS sandbox, environment curation

// Re-export everything that was previously public from this file
// so that existing `use crate::coder_assistant::tools::*` keeps working.

pub use super::tool_executor::{CoderTool, ToolContext, ToolError, ToolExecutor, ToolResult};
pub use super::security_policy::{
    AuditEntry, ExfiltrationPolicy, NetworkBoundary, SandboxMode, SecretBoundary,
    ToolExecutionAudit,
};

// ToolExecutionPolicy needs to stay here for backward compat since
// it was originally defined in this file and is re-exported by mod.rs.
use crate::config::limits;
use serde::{Deserialize, Serialize};


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
            max_write_bytes: limits::MAX_WRITE_BYTES,
            max_edit_bytes: limits::MAX_EDIT_BYTES,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coder_assistant::schemas::{ObjectSchema, ToolRegistry, ToolSchema, WasmCapability};
    use axum::{extract::Json, routing::post, Router};
    use serde_json::json;
    use std::collections::HashMap;

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
        let mut context = ToolContext::default();
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

        let executor = ToolExecutor::new();
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
