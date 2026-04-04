//! API Tools - HTTP clients, webhooks, and API integrations

use reqwest::{Client, Method};
use serde_json::Value;
use std::collections::HashMap;

use super::{object_schema, Tool, ToolOutput};

// ============================================================================
// HTTP Request Tool
// ============================================================================

pub struct HttpRequestTool {
    client: Client,
}

impl HttpRequestTool {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(60))
                .build()
                .expect("Failed to create HTTP client"),
        }
    }
}

#[async_trait::async_trait]
impl Tool for HttpRequestTool {
    fn name(&self) -> &str {
        "http_request"
    }

    fn description(&self) -> &str {
        "Make an HTTP request to any API endpoint. Supports GET, POST, PUT, DELETE, PATCH."
    }

    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            ("url", "URL to request", true),
            (
                "method",
                "HTTP method: GET, POST, PUT, DELETE, PATCH (default: GET)",
                false,
            ),
            ("headers", "JSON object of headers to send", false),
            ("body", "Request body (for POST/PUT/PATCH)", false),
            ("params", "Query parameters as JSON object", false),
        ])
    }

    async fn execute(&self, params: Value) -> ToolOutput {
        let url = params.get("url").and_then(|v| v.as_str()).unwrap_or("");
        if url.is_empty() {
            return ToolOutput::error("URL is required");
        }

        let method_str = params
            .get("method")
            .and_then(|v| v.as_str())
            .unwrap_or("GET");
        let method = match method_str.to_uppercase().as_str() {
            "GET" => Method::GET,
            "POST" => Method::POST,
            "PUT" => Method::PUT,
            "DELETE" => Method::DELETE,
            "PATCH" => Method::PATCH,
            "HEAD" => Method::HEAD,
            "OPTIONS" => Method::OPTIONS,
            _ => Method::GET,
        };

        let mut request = self.client.request(method, url);

        // Add headers
        if let Some(headers) = params.get("headers").and_then(|v| v.as_object()) {
            for (key, value) in headers {
                if let Some(val_str) = value.as_str() {
                    request = request.header(key, val_str);
                }
            }
        }

        // Add query params
        if let Some(query) = params.get("params").and_then(|v| v.as_object()) {
            let query_map: HashMap<String, String> = query
                .iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect();
            request = request.query(&query_map);
        }

        // Add body
        if let Some(body) = params.get("body") {
            request = request.json(body);
        }

        match request.send().await {
            Ok(response) => {
                let status = response.status();
                let headers: HashMap<String, String> = response
                    .headers()
                    .iter()
                    .filter_map(|(k, v)| {
                        v.to_str().ok().map(|val| (k.to_string(), val.to_string()))
                    })
                    .collect();

                match response.text().await {
                    Ok(body) => {
                        // Try to parse as JSON
                        let parsed_body: Value = serde_json::from_str(&body)
                            .unwrap_or_else(|_| Value::String(body.clone()));

                        let output = if status.is_success() {
                            ToolOutput::success(format!(
                                "HTTP {} {}",
                                status.as_u16(),
                                status.canonical_reason().unwrap_or("")
                            ))
                        } else {
                            ToolOutput::error(format!(
                                "HTTP {} {}",
                                status.as_u16(),
                                status.canonical_reason().unwrap_or("")
                            ))
                        };

                        output.with_metadata(serde_json::json!({
                            "status": status.as_u16(),
                            "status_text": status.canonical_reason(),
                            "headers": headers,
                            "body": parsed_body,
                        }))
                    }
                    Err(e) => ToolOutput::error(format!("Failed to read response body: {}", e)),
                }
            }
            Err(e) => ToolOutput::error(format!("HTTP request failed: {}", e)),
        }
    }
}

impl Default for HttpRequestTool {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Webhook Send Tool
// ============================================================================

pub struct WebhookSendTool {
    client: Client,
}

impl WebhookSendTool {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("Failed to create HTTP client"),
        }
    }
}

#[async_trait::async_trait]
impl Tool for WebhookSendTool {
    fn name(&self) -> &str {
        "webhook_send"
    }

    fn description(&self) -> &str {
        "Send a webhook payload to a URL. Supports Discord, Slack, and generic webhooks."
    }

    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            ("url", "Webhook URL", true),
            ("content", "Message content", true),
            ("username", "Override username (optional)", false),
            ("avatar_url", "Override avatar URL (optional)", false),
        ])
    }

    async fn execute(&self, params: Value) -> ToolOutput {
        let url = params.get("url").and_then(|v| v.as_str()).unwrap_or("");
        let content = params.get("content").and_then(|v| v.as_str()).unwrap_or("");

        if url.is_empty() || content.is_empty() {
            return ToolOutput::error("URL and content are required");
        }

        // Detect webhook type and format payload
        let payload = if url.contains("discord") || url.contains("slack") {
            let mut p = serde_json::json!({
                "content": content,
            });
            if let Some(username) = params.get("username").and_then(|v| v.as_str()) {
                p["username"] = Value::String(username.to_string());
            }
            if let Some(avatar) = params.get("avatar_url").and_then(|v| v.as_str()) {
                p["avatar_url"] = Value::String(avatar.to_string());
            }
            p
        } else {
            // Generic webhook
            serde_json::json!({
                "message": content,
                "timestamp": chrono::Utc::now().to_rfc3339(),
            })
        };

        match self.client.post(url).json(&payload).send().await {
            Ok(response) => {
                if response.status().is_success() {
                    ToolOutput::success("Webhook sent successfully".to_string())
                } else {
                    ToolOutput::error(format!("Webhook failed: HTTP {}", response.status()))
                }
            }
            Err(e) => ToolOutput::error(format!("Webhook failed: {}", e)),
        }
    }
}

impl Default for WebhookSendTool {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// JSON Parse Tool
// ============================================================================

pub struct JsonParseTool;

impl JsonParseTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl Tool for JsonParseTool {
    fn name(&self) -> &str {
        "json_parse"
    }

    fn description(&self) -> &str {
        "Parse and extract data from JSON using dot-notation path."
    }

    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            ("json", "JSON string or object to parse", true),
            (
                "path",
                "Dot-notation path to extract (e.g., 'data.users.0.name')",
                false,
            ),
        ])
    }

    async fn execute(&self, params: Value) -> ToolOutput {
        let json_input = params.get("json").cloned().unwrap_or(Value::Null);

        let value = if let Some(s) = json_input.as_str() {
            match serde_json::from_str::<Value>(s) {
                Ok(v) => v,
                Err(e) => return ToolOutput::error(format!("Invalid JSON: {}", e)),
            }
        } else {
            json_input
        };

        let result = if let Some(path) = params.get("path").and_then(|v| v.as_str()) {
            // Navigate path
            let parts: Vec<&str> = path.split('.').collect();
            let mut current = &value;
            for part in parts {
                if let Ok(index) = part.parse::<usize>() {
                    current = current.get(index).unwrap_or(&Value::Null);
                } else {
                    current = current.get(part).unwrap_or(&Value::Null);
                }
            }
            current.clone()
        } else {
            value
        };

        ToolOutput::success(format!("Extracted: {}", result)).with_metadata(serde_json::json!({
            "result": result,
        }))
    }
}

impl Default for JsonParseTool {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// JSON Validate Tool
// ============================================================================

pub struct JsonValidateTool;

impl JsonValidateTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl Tool for JsonValidateTool {
    fn name(&self) -> &str {
        "json_validate"
    }

    fn description(&self) -> &str {
        "Validate JSON against a JSON Schema."
    }

    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            ("json", "JSON to validate", true),
            ("schema", "JSON Schema to validate against", true),
        ])
    }

    async fn execute(&self, params: Value) -> ToolOutput {
        // Basic validation - just check both are valid JSON
        let json_value = params.get("json").cloned().unwrap_or(Value::Null);
        let schema_value = params.get("schema").cloned().unwrap_or(Value::Null);

        if json_value.is_null() {
            return ToolOutput::error("Invalid or missing JSON");
        }
        if schema_value.is_null() {
            return ToolOutput::error("Invalid or missing schema");
        }

        // Check if required fields exist (basic schema validation)
        let mut errors = Vec::new();

        if let Some(required) = schema_value.get("required").and_then(|v| v.as_array()) {
            for req in required {
                if let Some(field) = req.as_str() {
                    if json_value.get(field).is_none() {
                        errors.push(format!("Missing required field: {}", field));
                    }
                }
            }
        }

        // Check types
        if let Some(properties) = schema_value.get("properties").and_then(|v| v.as_object()) {
            for (prop, schema_def) in properties {
                if let Some(value) = json_value.get(prop) {
                    if let Some(expected_type) = schema_def.get("type").and_then(|v| v.as_str()) {
                        let valid = match expected_type {
                            "string" => value.is_string(),
                            "number" => value.is_number(),
                            "boolean" => value.is_boolean(),
                            "array" => value.is_array(),
                            "object" => value.is_object(),
                            "null" => value.is_null(),
                            _ => true,
                        };
                        if !valid {
                            errors.push(format!(
                                "Field '{}' should be {} but is {:?}",
                                prop, expected_type, value
                            ));
                        }
                    }
                }
            }
        }

        if errors.is_empty() {
            ToolOutput::success("JSON is valid against schema".to_string())
        } else {
            ToolOutput::error(format!("Validation errors:\n{}", errors.join("\n")))
        }
    }
}

impl Default for JsonValidateTool {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Base64 Encode/Decode Tool
// ============================================================================

pub struct Base64Tool;

impl Base64Tool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl Tool for Base64Tool {
    fn name(&self) -> &str {
        "base64"
    }

    fn description(&self) -> &str {
        "Encode or decode Base64 strings."
    }

    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            ("operation", "encode or decode", true),
            ("data", "String to encode or base64 to decode", true),
            ("url_safe", "Use URL-safe base64 (default: false)", false),
        ])
    }

    async fn execute(&self, params: Value) -> ToolOutput {
        use base64::{engine::general_purpose, Engine};

        let operation = params
            .get("operation")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let data = params.get("data").and_then(|v| v.as_str()).unwrap_or("");
        let url_safe = params
            .get("url_safe")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        match operation {
            "encode" => {
                let encoded = if url_safe {
                    general_purpose::URL_SAFE_NO_PAD.encode(data)
                } else {
                    general_purpose::STANDARD.encode(data)
                };
                ToolOutput::success(encoded)
            }
            "decode" => {
                let result = if url_safe {
                    general_purpose::URL_SAFE_NO_PAD.decode(data)
                } else {
                    general_purpose::STANDARD.decode(data)
                };

                match result {
                    Ok(bytes) => match String::from_utf8(bytes) {
                        Ok(decoded) => ToolOutput::success(decoded),
                        Err(e) => {
                            ToolOutput::success(format!("Decoded bytes (not valid UTF-8): {}", e))
                        }
                    },
                    Err(e) => ToolOutput::error(format!("Decode failed: {}", e)),
                }
            }
            _ => ToolOutput::error("Operation must be 'encode' or 'decode'"),
        }
    }
}

impl Default for Base64Tool {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// URL Parse/Build Tool
// ============================================================================

pub struct UrlTool;

impl UrlTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl Tool for UrlTool {
    fn name(&self) -> &str {
        "url"
    }

    fn description(&self) -> &str {
        "Parse URLs into components or build URLs from components."
    }

    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            ("url", "URL to parse", false),
            ("build", "Build URL from components (JSON object)", false),
            ("add_params", "Query params to add to parsed URL", false),
        ])
    }

    async fn execute(&self, params: Value) -> ToolOutput {
        // Parse URL
        if let Some(url_str) = params.get("url").and_then(|v| v.as_str()) {
            match url::Url::parse(url_str) {
                Ok(url) => {
                    let query_pairs: std::collections::HashMap<String, String> = url
                        .query_pairs()
                        .map(|(k, v)| (k.to_string(), v.to_string()))
                        .collect();

                    let result = serde_json::json!({
                        "scheme": url.scheme(),
                        "host": url.host_str(),
                        "port": url.port(),
                        "path": url.path(),
                        "query": url.query(),
                        "fragment": url.fragment(),
                        "query_params": query_pairs,
                    });

                    ToolOutput::success(format!("Parsed: {}", url)).with_metadata(result)
                }
                Err(e) => ToolOutput::error(format!("Failed to parse URL: {}", e)),
            }
        } else if let Some(build) = params.get("build").and_then(|v| v.as_object()) {
            let mut url = String::new();

            if let Some(scheme) = build.get("scheme").and_then(|v| v.as_str()) {
                url.push_str(scheme);
                url.push_str("://");
            }

            if let Some(host) = build.get("host").and_then(|v| v.as_str()) {
                url.push_str(host);
            }

            if let Some(port) = build.get("port").and_then(|v| v.as_u64()) {
                url.push(':');
                url.push_str(&port.to_string());
            }

            if let Some(path) = build.get("path").and_then(|v| v.as_str()) {
                if !path.starts_with('/') {
                    url.push('/');
                }
                url.push_str(path);
            }

            if let Some(params) = build.get("params").and_then(|v| v.as_object()) {
                url.push('?');
                let pairs: Vec<String> = params
                    .iter()
                    .filter_map(|(k, v)| v.as_str().map(|val| format!("{}={}", k, val)))
                    .collect();
                url.push_str(&pairs.join("&"));
            }

            ToolOutput::success(url)
        } else {
            ToolOutput::error("Either 'url' to parse or 'build' object required")
        }
    }
}

impl Default for UrlTool {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Markdown to HTML Tool
// ============================================================================

pub struct MarkdownTool;

impl MarkdownTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl Tool for MarkdownTool {
    fn name(&self) -> &str {
        "markdown"
    }

    fn description(&self) -> &str {
        "Convert Markdown to HTML or strip Markdown formatting."
    }

    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            ("operation", "to_html or strip (default: to_html)", false),
            ("text", "Markdown text to process", true),
        ])
    }

    async fn execute(&self, params: Value) -> ToolOutput {
        let text = params.get("text").and_then(|v| v.as_str()).unwrap_or("");
        let operation = params
            .get("operation")
            .and_then(|v| v.as_str())
            .unwrap_or("to_html");

        match operation {
            "to_html" => {
                // Simple markdown to HTML conversion
                let mut html = text.to_string();

                // Headers
                html = html.replace("# ", "<h1>").replace("\n# ", "\n<h1>");
                html = html.replace("## ", "<h2>").replace("\n## ", "\n<h2>");
                html = html.replace("### ", "<h3>").replace("\n### ", "\n<h3>");
                html = html.replace("#### ", "<h4>").replace("\n#### ", "\n<h4>");

                // Bold and italic
                html = html.replace("**", "<strong>").replace("__", "<strong>");
                html = html.replace("*", "<em>").replace("_", "<em>");

                // Links - simple regex-like replacement
                // This is a simplified version; a real implementation would use a proper parser
                html = format!(
                    "<p>{}</p>",
                    html.replace("\n\n", "</p><p>").replace("\n", "<br>")
                );

                ToolOutput::success(html)
            }
            "strip" => {
                // Strip markdown syntax
                let mut plain = text.to_string();
                plain = plain.replace("**", "").replace("__", "");
                plain = plain.replace("*", "").replace("_", "");
                plain = plain
                    .replace("# ", "")
                    .replace("## ", "")
                    .replace("### ", "");
                plain = plain.replace("`", "").replace("```", "");
                ToolOutput::success(plain)
            }
            _ => ToolOutput::error("Operation must be 'to_html' or 'strip'"),
        }
    }
}

impl Default for MarkdownTool {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// CSV Tools
// ============================================================================

pub struct CsvParseTool;

impl CsvParseTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl Tool for CsvParseTool {
    fn name(&self) -> &str {
        "csv_parse"
    }

    fn description(&self) -> &str {
        "Parse CSV data into JSON."
    }

    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            ("data", "CSV string to parse", true),
            (
                "headers",
                "Whether first row is headers (default: true)",
                false,
            ),
            ("delimiter", "Field delimiter (default: comma)", false),
        ])
    }

    async fn execute(&self, params: Value) -> ToolOutput {
        let data = params.get("data").and_then(|v| v.as_str()).unwrap_or("");
        let has_headers = params
            .get("headers")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let delimiter = params
            .get("delimiter")
            .and_then(|v| v.as_str())
            .unwrap_or(",");

        let delim = delimiter.chars().next().unwrap_or(',');
        let lines: Vec<&str> = data.lines().collect();

        if lines.is_empty() {
            return ToolOutput::error("Empty CSV data");
        }

        let headers: Vec<String> = if has_headers {
            lines[0]
                .split(delim)
                .map(|s| s.trim().to_string())
                .collect()
        } else {
            (0..lines[0].split(delim).count())
                .map(|i| format!("column_{}", i))
                .collect()
        };

        let start_row = if has_headers { 1 } else { 0 };
        let mut records = Vec::new();

        for line in &lines[start_row..] {
            let values: Vec<&str> = line.split(delim).collect();
            let mut record = serde_json::Map::new();
            for (i, header) in headers.iter().enumerate() {
                let value = values.get(i).map(|s| s.trim()).unwrap_or("");
                record.insert(header.clone(), Value::String(value.to_string()));
            }
            records.push(Value::Object(record));
        }

        ToolOutput::success(format!("Parsed {} records", records.len())).with_metadata(
            serde_json::json!({
                "headers": headers,
                "records": records,
            }),
        )
    }
}

impl Default for CsvParseTool {
    fn default() -> Self {
        Self::new()
    }
}

pub struct CsvGenerateTool;

impl CsvGenerateTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl Tool for CsvGenerateTool {
    fn name(&self) -> &str {
        "csv_generate"
    }

    fn description(&self) -> &str {
        "Generate CSV from JSON array."
    }

    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            ("data", "JSON array of objects to convert to CSV", true),
            (
                "headers",
                "Column headers (optional, auto-detected if not provided)",
                false,
            ),
            ("delimiter", "Field delimiter (default: comma)", false),
        ])
    }

    async fn execute(&self, params: Value) -> ToolOutput {
        let data = params
            .get("data")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let delimiter = params
            .get("delimiter")
            .and_then(|v| v.as_str())
            .unwrap_or(",");

        if data.is_empty() {
            return ToolOutput::error("Empty data array");
        }

        let headers: Vec<String> = if let Some(h) = params.get("headers").and_then(|v| v.as_array())
        {
            h.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        } else {
            // Auto-detect from first record
            data[0]
                .as_object()
                .map(|m| m.keys().cloned().collect())
                .unwrap_or_default()
        };

        let mut csv = headers.join(delimiter);
        csv.push('\n');

        for record in &data {
            let values: Vec<String> = headers
                .iter()
                .map(|h| {
                    record
                        .get(h)
                        .and_then(|v| v.as_str().map(|s| s.to_string()))
                        .unwrap_or_default()
                })
                .collect();
            csv.push_str(&values.join(delimiter));
            csv.push('\n');
        }

        ToolOutput::success(csv)
    }
}

impl Default for CsvGenerateTool {
    fn default() -> Self {
        Self::new()
    }
}
