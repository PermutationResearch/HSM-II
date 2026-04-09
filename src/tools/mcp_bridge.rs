//! Register Hermes-style HTTP MCP tools on the personal-agent [`super::ToolRegistry`].
//!
//! Loads enabled plugin manifests (same as coder assistant), maps MCP providers to
//! JSON-RPC `tools/call` at the manifest endpoint. Optional runtime discovery via
//! `tools/list` when `HSM_PERSONAL_MCP_DISCOVER=1`.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use serde_json::{json, Map, Value};
use tracing::{info, warn};

use super::{Tool, ToolOutput, ToolRegistry};
use crate::tools::connector_runtime::{auth_header, enforce_policy};
use crate::coder_assistant::plugin_lifecycle::PluginManager;
use crate::coder_assistant::schemas::{
    ObjectSchema, ParameterType, ToolProviderKind, ToolProviderRuntime, ToolSchema,
};

fn unix_now() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn param_type_str(t: &ParameterType) -> &'static str {
    match t {
        ParameterType::String => "string",
        ParameterType::Integer => "integer",
        ParameterType::Number => "number",
        ParameterType::Boolean => "boolean",
        ParameterType::Array => "array",
        ParameterType::Object => "object",
    }
}

/// JSON Schema `parameters` object for [`Tool::parameters_schema`].
pub fn coder_tool_schema_to_parameters_json(schema: &ToolSchema) -> Value {
    let mut props = Map::new();
    for (k, ps) in &schema.parameters.properties {
        let mut prop = json!({
            "type": param_type_str(&ps.param_type),
            "description": ps.description,
        });
        if let Some(def) = &ps.default {
            prop.as_object_mut()
                .expect("object")
                .insert("default".to_string(), def.clone());
        }
        if let Some(en) = &ps.enum_values {
            prop.as_object_mut()
                .expect("object")
                .insert("enum".to_string(), json!(en));
        }
        props.insert(k.clone(), prop);
    }
    json!({
        "type": "object",
        "properties": props,
        "required": schema.required,
    })
}

fn decode_mcp_tool_result(body: &str) -> Result<String, String> {
    let parsed = match serde_json::from_str::<Value>(body) {
        Ok(v) => v,
        Err(_) => return Ok(body.to_string()),
    };

    if let Some(err) = parsed.get("error") {
        return Err(err.to_string());
    }

    let value = parsed.get("result").unwrap_or(&parsed);

    if value
        .get("isError")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        return Err(value.to_string());
    }

    if let Some(out) = value.get("output").and_then(|v| v.as_str()) {
        return Ok(out.to_string());
    }
    if let Some(content) = value.get("content") {
        if let Some(text) = content.as_str() {
            return Ok(text.to_string());
        }
        if let Some(items) = content.as_array() {
            let parts: Vec<String> = items
                .iter()
                .filter_map(|it| it.get("text").and_then(|t| t.as_str()))
                .map(|s| s.to_string())
                .collect();
            if !parts.is_empty() {
                return Ok(parts.join("\n"));
            }
        }
    }
    if let Some(sc) = value.get("structuredContent") {
        return Ok(serde_json::to_string_pretty(sc).unwrap_or_else(|_| sc.to_string()));
    }

    match value {
        Value::String(s) => Ok(s.clone()),
        _ => Ok(serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())),
    }
}

/// Single MCP tool backed by HTTP JSON-RPC `tools/call`.
pub struct McpHttpTool {
    endpoint: String,
    connector_ref: Option<String>,
    tool_name: String,
    description: String,
    parameters_schema: Value,
}

impl McpHttpTool {
    pub fn new(endpoint: impl Into<String>, schema: &ToolSchema) -> Self {
        Self {
            endpoint: endpoint.into(),
            connector_ref: None,
            tool_name: schema.name.clone(),
            description: schema.description.clone(),
            parameters_schema: coder_tool_schema_to_parameters_json(schema),
        }
    }

    pub fn from_discovered(
        endpoint: impl Into<String>,
        name: impl Into<String>,
        description: impl Into<String>,
        input_schema: Value,
    ) -> Self {
        let mut params = input_schema;
        if !params.is_object() || params.get("type").is_none() {
            params = json!({
                "type": "object",
                "properties": {},
                "required": []
            });
        }
        Self {
            endpoint: endpoint.into(),
            connector_ref: None,
            tool_name: name.into(),
            description: description.into(),
            parameters_schema: params,
        }
    }

    pub fn with_connector_ref(mut self, connector_ref: Option<String>) -> Self {
        self.connector_ref = connector_ref
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty());
        self
    }
}

#[async_trait]
impl Tool for McpHttpTool {
    fn name(&self) -> &str {
        &self.tool_name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn parameters_schema(&self) -> Value {
        self.parameters_schema.clone()
    }

    async fn execute(&self, params: Value) -> ToolOutput {
        let client = match reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
        {
            Ok(c) => c,
            Err(e) => return ToolOutput::error(format!("MCP HTTP client: {e}")),
        };

        let request_body = json!({
            "jsonrpc": "2.0",
            "id": format!("mcp-{}-{}", self.tool_name, unix_now()),
            "method": "tools/call",
            "params": {
                "name": self.tool_name,
                "arguments": params,
            }
        });
        if let Some(ref connector_ref) = self.connector_ref {
            let host = reqwest::Url::parse(&self.endpoint)
                .ok()
                .and_then(|u| u.host_str().map(|s| s.to_string()))
                .unwrap_or_default();
            if let Err(e) = enforce_policy(connector_ref, "POST", &host) {
                return ToolOutput::error(format!("Connector policy blocked MCP call: {e}"));
            }
        }
        let mut req = client.post(&self.endpoint).json(&request_body);
        if let Some(ref connector_ref) = self.connector_ref {
            if let Some((name, value)) = auth_header(connector_ref) {
                req = req.header(name, value);
            }
        }
        let resp = match req.send().await {
            Ok(r) => r,
            Err(e) => return ToolOutput::error(format!("MCP request failed: {e}")),
        };

        let status = resp.status();
        let body = match resp.text().await {
            Ok(b) => b,
            Err(e) => return ToolOutput::error(format!("MCP body read failed: {e}")),
        };

        if !status.is_success() {
            return ToolOutput::error(format!("MCP HTTP {}: {}", status.as_u16(), body));
        }

        match decode_mcp_tool_result(&body) {
            Ok(s) => ToolOutput::success(s).with_metadata(json!({ "provider": "mcp_http" })),
            Err(e) => ToolOutput::error(e),
        }
    }
}

fn mcp_endpoint(
    manifest: &crate::coder_assistant::plugin_lifecycle::PluginManifest,
) -> Option<String> {
    let p = &manifest.provider;
    if p.kind != ToolProviderKind::Mcp {
        return None;
    }
    match p.runtime.as_ref() {
        Some(ToolProviderRuntime::Http) | None => {}
        _ => return None,
    }
    p.endpoint.clone().filter(|e| !e.trim().is_empty())
}

async fn discover_tools_list(endpoint: &str) -> Option<Vec<Value>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .ok()?;
    let body = json!({
        "jsonrpc": "2.0",
        "id": format!("list-{}", unix_now()),
        "method": "tools/list",
        "params": filter_discover_params(),
    });
    let resp = client.post(endpoint).json(&body).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let text = resp.text().await.ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    let tools = v
        .get("result")
        .and_then(|r| r.get("tools"))
        .and_then(|t| t.as_array())
        .cloned()?;
    Some(tools)
}

/// Some MCP servers expect `params: {}`; others accept omitted. Send empty object.
fn filter_discover_params() -> Value {
    json!({})
}

fn discovered_tool_to_mcp_tool(endpoint: &str, tool: &Value) -> Option<Arc<dyn Tool>> {
    let name = tool.get("name")?.as_str()?.to_string();
    let description = tool
        .get("description")
        .and_then(|d| d.as_str())
        .unwrap_or("MCP tool (discovered)")
        .to_string();
    let input = tool
        .get("inputSchema")
        .cloned()
        .or_else(|| tool.get("parameters").cloned())
        .unwrap_or_else(|| {
            json!({
                "type": "object",
                "properties": {},
                "required": []
            })
        });
    Some(Arc::new(McpHttpTool::from_discovered(
        endpoint,
        name,
        description,
        input,
    )))
}

fn blank_object_schema() -> ObjectSchema {
    ObjectSchema {
        schema_type: "object".to_string(),
        properties: HashMap::new(),
    }
}

/// Fallback schema when `tools/list` returns tools without `inputSchema`.
fn minimal_schema(name: &str, description: &str) -> ToolSchema {
    ToolSchema {
        name: name.to_string(),
        description: description.to_string(),
        parameters: blank_object_schema(),
        required: vec![],
    }
}

/// Register MCP tools from enabled plugin manifests; optionally merge `tools/list` discovery.
pub async fn register_personal_mcp_tools(registry: &mut ToolRegistry) {
    let discover = std::env::var("HSM_PERSONAL_MCP_DISCOVER")
        .map(|v| {
            let s = v.trim();
            s == "1" || s.eq_ignore_ascii_case("true") || s.eq_ignore_ascii_case("yes")
        })
        .unwrap_or(false);

    let pm = PluginManager::from_env();
    let manifests = match pm.list_manifests() {
        Ok(m) => m,
        Err(e) => {
            warn!(target: "hsm_personal_mcp", "plugin manifests: {}", e);
            return;
        }
    };

    let mut registered_from_manifest = 0u32;
    let mut registered_from_discover = 0u32;

    for manifest in manifests {
        if !manifest.enabled {
            continue;
        }
        let Some(endpoint) = mcp_endpoint(&manifest) else {
            continue;
        };

        for tool in &manifest.tools {
            if registry.has(&tool.name) {
                warn!(
                    target: "hsm_personal_mcp",
                    plugin = %manifest.id,
                    tool = %tool.name,
                    "skipping MCP tool: name collides with native tool"
                );
                continue;
            }
            registry.register(Arc::new(
                McpHttpTool::new(&endpoint, tool).with_connector_ref(Some(manifest.provider.id.clone())),
            ));
            registered_from_manifest += 1;
        }

        if discover {
            if let Some(listed) = discover_tools_list(&endpoint).await {
                for item in listed {
                    let name = match item.get("name").and_then(|n| n.as_str()) {
                        Some(n) => n.to_string(),
                        None => continue,
                    };
                    if registry.has(&name) {
                        continue;
                    }
                    if let Some(t) = discovered_tool_to_mcp_tool(&endpoint, &item).map(|tool| {
                        Arc::new(
                            McpHttpTool::from_discovered(
                                &endpoint,
                                tool.name().to_string(),
                                tool.description().to_string(),
                                tool.parameters_schema(),
                            )
                            .with_connector_ref(Some(manifest.provider.id.clone())),
                        ) as Arc<dyn Tool>
                    }) {
                        registry.register(t);
                        registered_from_discover += 1;
                    } else {
                        let desc = item
                            .get("description")
                            .and_then(|d| d.as_str())
                            .unwrap_or("");
                        let schema = minimal_schema(&name, desc);
                        registry.register(Arc::new(
                            McpHttpTool::new(&endpoint, &schema)
                                .with_connector_ref(Some(manifest.provider.id.clone())),
                        ));
                        registered_from_discover += 1;
                    }
                }
            } else if manifest.tools.is_empty() {
                warn!(
                    target: "hsm_personal_mcp",
                    plugin = %manifest.id,
                    endpoint = %endpoint,
                    "tools/list discovery failed or returned empty; no static tools in manifest"
                );
            }
        }
    }

    if registered_from_manifest > 0 || registered_from_discover > 0 {
        info!(
            target: "hsm_personal_mcp",
            from_manifest = registered_from_manifest,
            from_discover = registered_from_discover,
            discover,
            "registered MCP tools on personal agent registry"
        );
    }
}
