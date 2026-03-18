//! External tool providers: MCP (HTTP JSON-RPC) and WASM execution.

use super::schemas::{ToolProviderKind, ToolProviderMetadata, ToolProviderRuntime, WasmCapability};
use super::tool_executor::{ToolContext, ToolError};
use super::security_policy::enforce_endpoint_allowed;
use super::sandbox::is_path_allowed;
use serde_json::json;
use std::time::Duration;
use tokio::time::timeout;

/// Route an external tool call to the appropriate provider runtime.
pub async fn execute_external_tool(
    context: &ToolContext,
    provider: &ToolProviderMetadata,
    tool_name: &str,
    args: &serde_json::Value,
) -> Result<String, ToolError> {
    match provider.runtime.as_ref() {
        Some(ToolProviderRuntime::Http) | None if provider.kind == ToolProviderKind::Mcp => {
            execute_mcp_tool(context, provider, tool_name, args).await
        }
        Some(ToolProviderRuntime::Wasm {
            module_path,
            entrypoint,
            capabilities,
        }) => {
            execute_wasm_tool(context, tool_name, args, module_path, entrypoint, capabilities)
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

// ── MCP over HTTP ────────────────────────────────────────────────────

async fn execute_mcp_tool(
    context: &ToolContext,
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
    enforce_endpoint_allowed(context, endpoint)?;

    let now = super::tool_executor::unix_now();
    let client = reqwest::Client::new();
    let request_body = json!({
        "jsonrpc": "2.0",
        "id": format!("tool-{}-{}", tool_name, now),
        "method": "tools/call",
        "params": {
            "name": tool_name,
            "arguments": args,
        }
    });

    let response = timeout(
        Duration::from_millis(context.timeout_ms),
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

// ── WASM ─────────────────────────────────────────────────────────────

async fn execute_wasm_tool(
    context: &ToolContext,
    tool_name: &str,
    args: &serde_json::Value,
    module_path: &str,
    entrypoint: &str,
    capabilities: &[WasmCapability],
) -> Result<String, ToolError> {
    let module_path = if std::path::Path::new(module_path).is_absolute() {
        std::path::PathBuf::from(module_path)
    } else {
        context.cwd.join(module_path)
    };
    if !is_path_allowed(context, &module_path) {
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

    let injected_env: std::collections::HashMap<String, String> = context
        .execution_policy
        .secret_boundary
        .injected_env_keys
        .iter()
        .filter_map(|key: &String| {
            context
                .env_vars
                .get(key)
                .cloned()
                .map(|value| (key.clone(), value))
        })
        .collect();

    let request = json!({
        "tool": tool_name,
        "arguments": args,
        "cwd": context.cwd,
        "timeout_ms": context.timeout_ms,
        "env": injected_env,
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

// ── Response Decoding ────────────────────────────────────────────────

fn decode_external_tool_response(body: &str) -> Result<String, ToolError> {
    let parsed = match serde_json::from_str::<serde_json::Value>(body) {
        Ok(value) => value,
        Err(_) => return Ok(body.to_string()),
    };

    if let Some(error) = parsed.get("error") {
        return Err(ToolError::CommandFailed {
            exit_code: -1,
            stderr: value_to_string(error),
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
                stderr: value_to_string(result),
            });
        }
        return decode_result_value(result);
    }

    decode_result_value(&parsed)
}

fn decode_result_value(value: &serde_json::Value) -> Result<String, ToolError> {
    if let Some(error) = value.get("error") {
        return Err(ToolError::CommandFailed {
            exit_code: -1,
            stderr: value_to_string(error),
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
        return Ok(value_to_string(structured));
    }
    if let Some(text) = value.as_str() {
        return Ok(text.to_string());
    }
    Ok(value_to_string(value))
}

fn value_to_string(value: &serde_json::Value) -> String {
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
