//! Browser Use provider bridge (cloud/local API wrapper).
//!
//! This offers a first-class provider abstraction alongside Browserbase tools:
//! - set `BROWSER_USE_API_KEY`
//! - optional `BROWSER_USE_API_BASE` (defaults to `https://api.browser-use.com/v1`)
//!
//! The tool executes a high-level task and returns provider JSON.

use reqwest::Client;
use serde_json::Value;

use super::{object_schema, Tool, ToolOutput};
use crate::tools::security::validate_outbound_url;

fn browser_use_base() -> String {
    std::env::var("BROWSER_USE_API_BASE")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "https://api.browser-use.com/v1".to_string())
        .trim_end_matches('/')
        .to_string()
}

pub struct BrowserUseRunTool {
    client: Client,
    api_key: Option<String>,
    base: String,
}

impl BrowserUseRunTool {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(180))
                .user_agent("HSM-II/0.1 (BrowserUse tool)")
                .build()
                .expect("reqwest client"),
            api_key: std::env::var("BROWSER_USE_API_KEY")
                .ok()
                .filter(|s| !s.trim().is_empty()),
            base: browser_use_base(),
        }
    }
}

#[async_trait::async_trait]
impl Tool for BrowserUseRunTool {
    fn name(&self) -> &str {
        "browser_use_run"
    }

    fn description(&self) -> &str {
        "Run a high-level browser automation task via Browser Use provider. Needs BROWSER_USE_API_KEY."
    }

    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            ("task", "Natural-language browser task for Browser Use", true),
            (
                "start_url",
                "Optional URL where the session should begin",
                false,
            ),
            (
                "session_id",
                "Optional provider session id to continue an existing run",
                false,
            ),
        ])
    }

    async fn execute(&self, params: Value) -> ToolOutput {
        let Some(api_key) = self.api_key.as_ref() else {
            return ToolOutput::error("BROWSER_USE_API_KEY not configured");
        };
        let task = params
            .get("task")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        if task.is_empty() {
            return ToolOutput::error("task is required");
        }

        let endpoint = format!("{}/tasks/run", self.base);
        let mut body = serde_json::json!({ "task": task });
        if let Some(url) = params.get("start_url").and_then(|v| v.as_str()) {
            let u = url.trim();
            if !u.is_empty() {
                if let Err(e) = validate_outbound_url(u) {
                    return ToolOutput::error(format!("Blocked by SSRF guard: {e}"));
                }
                body["start_url"] = Value::String(u.to_string());
            }
        }
        if let Some(session_id) = params.get("session_id").and_then(|v| v.as_str()) {
            let s = session_id.trim();
            if !s.is_empty() {
                body["session_id"] = Value::String(s.to_string());
            }
        }

        let resp = match self
            .client
            .post(&endpoint)
            .header("Authorization", format!("Bearer {api_key}"))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => return ToolOutput::error(format!("Browser Use request failed: {e}")),
        };
        let status = resp.status();
        let text = match resp.text().await {
            Ok(t) => t,
            Err(e) => return ToolOutput::error(format!("Browser Use read body failed: {e}")),
        };
        let parsed: Value = serde_json::from_str(&text).unwrap_or_else(|_| {
            serde_json::json!({
                "raw": text,
            })
        });
        if !status.is_success() {
            return ToolOutput::error(format!(
                "Browser Use HTTP {}: {}",
                status.as_u16(),
                parsed
                    .get("error")
                    .and_then(|v| v.as_str())
                    .unwrap_or("request failed")
            ));
        }

        ToolOutput::success(
            serde_json::to_string_pretty(&parsed).unwrap_or_else(|_| "{}".to_string()),
        )
        .with_metadata(serde_json::json!({
            "provider": "browser_use",
            "base": self.base,
        }))
    }
}

impl Default for BrowserUseRunTool {
    fn default() -> Self {
        Self::new()
    }
}
