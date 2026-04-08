//! Browser Automation Tools via Browserbase
//!
//! Full browser automation: navigation, clicking, form filling, screenshots,
//! waits, session management, and structured JSON for reliable agent loops.
//!
//! **Session flow (Hermes-style):** `browser_navigate` creates or reuses a session and returns
//! `session_id` in metadata and JSON. Subsequent `browser_*` calls may **omit** `session_id` to reuse
//! the last session started in this process (single-flight agent). Prefer passing `session_id`
//! explicitly when running parallel agents.

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;
use tracing::{error, info, warn};

use super::{object_schema, Tool, ToolOutput};

// ── Last session (reuse when models omit session_id) ───────────────────────

static LAST_BROWSER_SESSION: OnceLock<Mutex<Option<String>>> = OnceLock::new();

fn last_session_cell() -> &'static Mutex<Option<String>> {
    LAST_BROWSER_SESSION.get_or_init(|| Mutex::new(None))
}

fn remember_browser_session(id: impl Into<String>) {
    let id = id.into();
    if let Ok(mut g) = last_session_cell().lock() {
        *g = Some(id);
    }
}

fn take_browser_session_if_matches(session_id: &str) {
    if let Ok(mut g) = last_session_cell().lock() {
        if g.as_deref() == Some(session_id) {
            *g = None;
        }
    }
}

fn resolve_browser_session(params: &Value) -> Result<String, ToolOutput> {
    let from_param = params
        .get("session_id")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if let Some(s) = from_param {
        return Ok(s.to_string());
    }
    if let Ok(g) = last_session_cell().lock() {
        if let Some(ref id) = *g {
            return Ok(id.clone());
        }
    }
    Err(ToolOutput::error(
        "session_id is required (or run browser_navigate first to set the default session)",
    ))
}

/// Browserbase wraps CDP; unwrap nested `result.value` shapes.
fn bb_eval_return_value(response: &Value) -> Option<Value> {
    response
        .pointer("/result/value")
        .cloned()
        .or_else(|| response.pointer("/result/result/value").cloned())
}

fn bb_screenshot_data(response: &Value) -> Option<String> {
    response
        .get("data")
        .and_then(|d| d.as_str())
        .map(|s| s.to_string())
        .or_else(|| {
            response
                .pointer("/result/data")
                .and_then(|d| d.as_str())
                .map(|s| s.to_string())
        })
        .or_else(|| {
            response
                .pointer("/result/result/data")
                .and_then(|d| d.as_str())
                .map(|s| s.to_string())
        })
}

async fn wait_document_ready(client: &BrowserbaseClient, session_id: &str, max_wait: Duration) {
    let deadline = Instant::now() + max_wait;
    while Instant::now() < deadline {
        let Ok(resp) = client
            .execute_cdp(
                session_id,
                "Runtime.evaluate",
                serde_json::json!({
                    "expression": "document.readyState",
                    "returnByValue": true,
                }),
            )
            .await
        else {
            tokio::time::sleep(Duration::from_millis(120)).await;
            continue;
        };
        let ready = bb_eval_return_value(&resp)
            .and_then(|v| v.as_str().map(|s| s == "complete"))
            .unwrap_or(false);
        if ready {
            break;
        }
        tokio::time::sleep(Duration::from_millis(120)).await;
    }
}

fn json_string_for_js(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string())
}

/// Browserbase API client
pub struct BrowserbaseClient {
    client: Client,
    api_key: String,
    base_url: String,
}

impl BrowserbaseClient {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .expect("Failed to create HTTP client"),
            api_key: api_key.into(),
            base_url: "https://www.browserbase.com/v1".to_string(),
        }
    }

    async fn create_session(&self) -> Result<BrowserSession> {
        let url = format!("{}/sessions", self.base_url);

        let mut body_map: serde_json::Map<String, Value> = serde_json::Map::new();
        body_map.insert(
            "projectId".to_string(),
            serde_json::to_value(std::env::var("BROWSERBASE_PROJECT_ID").ok())
                .unwrap_or(Value::Null),
        );

        let mut browser_settings = serde_json::Map::new();
        if std::env::var("BROWSERBASE_SOLVE_CAPTCHAS").ok().as_deref() == Some("1") {
            browser_settings.insert("solveCaptchas".to_string(), Value::Bool(true));
        }
        if let Ok(vp) = std::env::var("BROWSERBASE_VIEWPORT") {
            let parts: Vec<&str> = vp.split('x').collect();
            if parts.len() == 2 {
                let w: u32 = parts[0].parse().unwrap_or(1280);
                let h: u32 = parts[1].parse().unwrap_or(720);
                browser_settings.insert(
                    "viewport".to_string(),
                    serde_json::json!({ "width": w, "height": h }),
                );
            }
        }
        if !browser_settings.is_empty() {
            body_map.insert(
                "browserSettings".to_string(),
                Value::Object(browser_settings),
            );
        }

        let mut body = Value::Object(body_map);
        if let Ok(extra) = std::env::var("BROWSERBASE_SESSION_JSON") {
            if let Ok(v) = serde_json::from_str::<Value>(&extra) {
                if let (Some(bo), Some(eo)) = (body.as_object_mut(), v.as_object()) {
                    for (k, val) in eo {
                        if k == "browserSettings" {
                            if let (Some(lhs), Some(rhs)) = (
                                bo.get_mut("browserSettings")
                                    .and_then(|x| x.as_object_mut()),
                                val.as_object(),
                            ) {
                                for (ik, iv) in rhs {
                                    lhs.insert(ik.clone(), iv.clone());
                                }
                            } else {
                                bo.insert(k.clone(), val.clone());
                            }
                        } else {
                            bo.insert(k.clone(), val.clone());
                        }
                    }
                }
            }
        }

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow!("Failed to create browser session: {}", error_text));
        }

        let session: BrowserSession = response.json().await?;
        Ok(session)
    }

    async fn execute_cdp(&self, session_id: &str, method: &str, params: Value) -> Result<Value> {
        let url = format!("{}/sessions/{}/cdp", self.base_url, session_id);

        let body = serde_json::json!({
            "method": method,
            "params": params,
        });

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow!("CDP command failed: {}", error_text));
        }

        let result: Value = response.json().await?;
        Ok(result)
    }

    async fn close_session(&self, session_id: &str) -> Result<()> {
        let url = format!("{}/sessions/{}", self.base_url, session_id);

        let response = self
            .client
            .delete(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            warn!("Failed to close browser session: {}", error_text);
        }

        Ok(())
    }
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct BrowserSession {
    id: String,
    ws_url: String,
}

// ============================================================================
// Browser Navigate Tool
// ============================================================================

pub struct BrowserNavigateTool {
    client: Option<BrowserbaseClient>,
}

impl BrowserNavigateTool {
    pub fn new() -> Self {
        let client = std::env::var("BROWSERBASE_API_KEY")
            .ok()
            .map(|key| BrowserbaseClient::new(key));
        Self { client }
    }
}

#[async_trait::async_trait]
impl Tool for BrowserNavigateTool {
    fn name(&self) -> &str {
        "browser_navigate"
    }

    fn description(&self) -> &str {
        "Navigate to a URL via Browserbase. Creates a session if session_id is omitted; stores it as the default for subsequent browser_* tools. Returns structured JSON with session_id."
    }

    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            ("url", "The URL to navigate to", true),
            (
                "session_id",
                "Reuse an existing Browserbase session (optional). Omit to create a new session.",
                false,
            ),
            (
                "wait_ms",
                "Max milliseconds to wait for document.readyState complete after navigation (default 30000)",
                false,
            ),
        ])
    }

    async fn execute(&self, params: Value) -> ToolOutput {
        let url = params.get("url").and_then(|v| v.as_str()).unwrap_or("");

        if url.is_empty() {
            return ToolOutput::error("URL parameter is required");
        }

        let Some(client) = &self.client else {
            return ToolOutput::error("BROWSERBASE_API_KEY not configured");
        };

        let session_id = if let Some(id) = params.get("session_id").and_then(|v| v.as_str()) {
            let t = id.trim();
            if t.is_empty() {
                match client.create_session().await {
                    Ok(session) => {
                        info!("Created browser session: {}", session.id);
                        session.id
                    }
                    Err(e) => {
                        error!("Failed to create browser session: {}", e);
                        return ToolOutput::error(format!("Session creation failed: {}", e));
                    }
                }
            } else {
                t.to_string()
            }
        } else {
            match client.create_session().await {
                Ok(session) => {
                    info!("Created browser session: {}", session.id);
                    session.id
                }
                Err(e) => {
                    error!("Failed to create browser session: {}", e);
                    return ToolOutput::error(format!("Session creation failed: {}", e));
                }
            }
        };

        let wait_ms = params
            .get("wait_ms")
            .and_then(|v| v.as_u64())
            .or_else(|| params.get("wait_ms").and_then(|v| v.as_str()?.parse().ok()))
            .unwrap_or(30_000)
            .clamp(1_000, 120_000);

        let result = client
            .execute_cdp(
                &session_id,
                "Page.navigate",
                serde_json::json!({
                    "url": url,
                }),
            )
            .await;

        match result {
            Ok(_) => {
                wait_document_ready(client, &session_id, Duration::from_millis(wait_ms)).await;

                let title_result = client
                    .execute_cdp(
                        &session_id,
                        "Runtime.evaluate",
                        serde_json::json!({
                            "expression": "document.title",
                            "returnByValue": true,
                        }),
                    )
                    .await;

                let title = title_result
                    .ok()
                    .as_ref()
                    .and_then(|r| bb_eval_return_value(r))
                    .and_then(|v| v.as_str().map(|s| s.to_string()))
                    .unwrap_or_else(|| "Unknown".to_string());

                remember_browser_session(&session_id);

                let payload = serde_json::json!({
                    "ok": true,
                    "session_id": session_id,
                    "url": url,
                    "title": title,
                    "hint": "Reuse session_id for browser_click, browser_type, browser_get_text, browser_screenshot, browser_wait, browser_close — or omit session_id to use this session by default."
                });

                ToolOutput::success(payload.to_string()).with_metadata(serde_json::json!({
                    "session_id": session_id,
                    "url": url,
                    "title": title,
                }))
            }
            Err(e) => {
                error!("Navigation failed: {}", e);
                ToolOutput::error(format!("Navigation failed: {}", e))
            }
        }
    }
}

impl Default for BrowserNavigateTool {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Browser Click Tool
// ============================================================================

pub struct BrowserClickTool {
    client: Option<BrowserbaseClient>,
}

impl BrowserClickTool {
    pub fn new() -> Self {
        let client = std::env::var("BROWSERBASE_API_KEY")
            .ok()
            .map(|key| BrowserbaseClient::new(key));
        Self { client }
    }
}

#[async_trait::async_trait]
impl Tool for BrowserClickTool {
    fn name(&self) -> &str {
        "browser_click"
    }

    fn description(&self) -> &str {
        "Click an element by CSS selector or visible text. session_id optional if browser_navigate ran in this process."
    }

    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            (
                "session_id",
                "Browserbase session ID (optional if default session set)",
                false,
            ),
            ("selector", "CSS selector for the element", false),
            ("text", "Text to find and click (if no selector)", false),
        ])
    }

    async fn execute(&self, params: Value) -> ToolOutput {
        let session_id = match resolve_browser_session(&params) {
            Ok(s) => s,
            Err(out) => return out,
        };

        let Some(client) = &self.client else {
            return ToolOutput::error("BROWSERBASE_API_KEY not configured");
        };

        let js = if let Some(selector) = params.get("selector").and_then(|v| v.as_str()) {
            if selector.trim().is_empty() {
                return ToolOutput::error("selector must not be empty");
            }
            let q = json_string_for_js(selector);
            format!(
                "(function() {{
                    const el = document.querySelector({q});
                    if (!el) return {{success: false, error: 'Element not found'}};
                    el.click();
                    return {{success: true, element: el.tagName}};
                }})()",
                q = q
            )
        } else if let Some(text) = params.get("text").and_then(|v| v.as_str()) {
            if text.trim().is_empty() {
                return ToolOutput::error("text must not be empty");
            }
            let lit = json_string_for_js(text);
            format!(
                r#"(function() {{
                    const needle = {lit};
                    const snap = document.evaluate(
                        "//*[not(self::script)][not(self::style)][not(self::noscript)]",
                        document,
                        null,
                        XPathResult.ORDERED_NODE_SNAPSHOT_TYPE,
                        null
                    );
                    for (let i = 0; i < snap.snapshotLength; i++) {{
                        const el = snap.snapshotItem(i);
                        if (!el || !el.textContent) continue;
                        if (el.textContent.includes(needle)) {{
                            el.click();
                            return {{ success: true, element: el.tagName }};
                        }}
                    }}
                    return {{ success: false, error: 'Element with text not found' }};
                }})()"#,
                lit = lit
            )
        } else {
            return ToolOutput::error("Either selector or text parameter is required");
        };

        let result = client
            .execute_cdp(
                &session_id,
                "Runtime.evaluate",
                serde_json::json!({
                    "expression": js,
                    "returnByValue": true,
                }),
            )
            .await;

        match result {
            Ok(response) => {
                let result_value = bb_eval_return_value(&response).unwrap_or(Value::Null);

                let success = result_value
                    .get("success")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                if success {
                    let element = result_value
                        .get("element")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Unknown");
                    let out = serde_json::json!({
                        "ok": true,
                        "session_id": session_id,
                        "clicked": element
                    });
                    ToolOutput::success(out.to_string()).with_metadata(serde_json::json!({
                        "session_id": session_id,
                    }))
                } else {
                    let error = result_value
                        .get("error")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Unknown error");
                    ToolOutput::error(format!("Click failed: {}", error))
                }
            }
            Err(e) => ToolOutput::error(format!("Click failed: {}", e)),
        }
    }
}

impl Default for BrowserClickTool {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Browser Type Tool (Fill Form)
// ============================================================================

pub struct BrowserTypeTool {
    client: Option<BrowserbaseClient>,
}

impl BrowserTypeTool {
    pub fn new() -> Self {
        let client = std::env::var("BROWSERBASE_API_KEY")
            .ok()
            .map(|key| BrowserbaseClient::new(key));
        Self { client }
    }
}

#[async_trait::async_trait]
impl Tool for BrowserTypeTool {
    fn name(&self) -> &str {
        "browser_type"
    }

    fn description(&self) -> &str {
        "Type into an input/textarea/select. session_id optional if a default session exists."
    }

    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            (
                "session_id",
                "Browserbase session (optional if default session set)",
                false,
            ),
            ("selector", "CSS selector for input field", true),
            ("text", "Text to type", true),
            (
                "clear_first",
                "Clear first: true/false (default true)",
                false,
            ),
        ])
    }

    async fn execute(&self, params: Value) -> ToolOutput {
        let session_id = match resolve_browser_session(&params) {
            Ok(s) => s,
            Err(out) => return out,
        };
        let selector = params
            .get("selector")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let text = params.get("text").and_then(|v| v.as_str()).unwrap_or("");

        if selector.is_empty() {
            return ToolOutput::error("selector is required");
        }

        let Some(client) = &self.client else {
            return ToolOutput::error("BROWSERBASE_API_KEY not configured");
        };

        let clear_first = params
            .get("clear_first")
            .and_then(|v| {
                v.as_bool().or_else(|| {
                    v.as_str()
                        .map(|s| matches!(s.to_lowercase().as_str(), "true" | "1" | "yes"))
                })
            })
            .unwrap_or(true);

        let sel_q = json_string_for_js(selector);
        let val_q = json_string_for_js(text);
        let clear_js = if clear_first { "el.value = '';" } else { "" };
        let js = format!(
            r#"(function() {{
                const el = document.querySelector({sel_q});
                if (!el) return {{success: false, error: 'Element not found'}};
                if (!['INPUT', 'TEXTAREA', 'SELECT'].includes(el.tagName)) {{
                    return {{success: false, error: 'Element is not an input field'}};
                }}
                {clear_js}
                el.value = {val_q};
                el.dispatchEvent(new Event('input', {{ bubbles: true }}));
                el.dispatchEvent(new Event('change', {{ bubbles: true }}));
                return {{success: true, tag: el.tagName}};
            }})()"#,
            sel_q = sel_q,
            clear_js = clear_js,
            val_q = val_q
        );

        let result = client
            .execute_cdp(
                &session_id,
                "Runtime.evaluate",
                serde_json::json!({
                    "expression": js,
                    "returnByValue": true,
                }),
            )
            .await;

        match result {
            Ok(response) => {
                let result_value = bb_eval_return_value(&response).unwrap_or(Value::Null);

                let success = result_value
                    .get("success")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                if success {
                    let out = serde_json::json!({
                        "ok": true,
                        "session_id": session_id,
                        "selector": selector,
                        "typed_chars": text.chars().count(),
                    });
                    ToolOutput::success(out.to_string())
                } else {
                    let error = result_value
                        .get("error")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Unknown error");
                    ToolOutput::error(format!("Type failed: {}", error))
                }
            }
            Err(e) => ToolOutput::error(format!("Type failed: {}", e)),
        }
    }
}

impl Default for BrowserTypeTool {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Browser Screenshot Tool
// ============================================================================

pub struct BrowserScreenshotTool {
    client: Option<BrowserbaseClient>,
}

impl BrowserScreenshotTool {
    pub fn new() -> Self {
        let client = std::env::var("BROWSERBASE_API_KEY")
            .ok()
            .map(|key| BrowserbaseClient::new(key));
        Self { client }
    }
}

#[async_trait::async_trait]
impl Tool for BrowserScreenshotTool {
    fn name(&self) -> &str {
        "browser_screenshot"
    }

    fn description(&self) -> &str {
        "PNG screenshot (viewport or full page). Returns JSON summary; base64 in metadata for vision models. session_id optional if default session set."
    }

    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            (
                "session_id",
                "Browserbase session (optional if default session set)",
                false,
            ),
            (
                "selector",
                "CSS selector to screenshot specific element (optional)",
                false,
            ),
            (
                "full_page",
                "true for full page, false for viewport (default false)",
                false,
            ),
        ])
    }

    async fn execute(&self, params: Value) -> ToolOutput {
        let session_id = match resolve_browser_session(&params) {
            Ok(s) => s,
            Err(out) => return out,
        };

        let Some(client) = &self.client else {
            return ToolOutput::error("BROWSERBASE_API_KEY not configured");
        };

        let full_page = params
            .get("full_page")
            .and_then(|v| {
                v.as_bool().or_else(|| {
                    v.as_str()
                        .map(|s| matches!(s.to_lowercase().as_str(), "true" | "1" | "yes"))
                })
            })
            .unwrap_or(false);

        let capture_params = if full_page {
            serde_json::json!({
                "format": "png",
                "fromSurface": true,
                "captureBeyondViewport": true,
            })
        } else {
            serde_json::json!({
                "format": "png",
                "fromSurface": true,
            })
        };

        let result = if let Some(sel) = params.get("selector").and_then(|v| v.as_str()) {
            if sel.trim().is_empty() {
                client
                    .execute_cdp(&session_id, "Page.captureScreenshot", capture_params)
                    .await
            } else {
                let clip_js = format!(
                    r#"(function() {{
                        const el = document.querySelector({q});
                        if (!el) return null;
                        const r = el.getBoundingClientRect();
                        return {{
                            x: Math.max(0, Math.floor(r.x)),
                            y: Math.max(0, Math.floor(r.y)),
                            width: Math.max(1, Math.ceil(r.width)),
                            height: Math.max(1, Math.ceil(r.height)),
                            scale: 1
                        }};
                    }})()"#,
                    q = json_string_for_js(sel)
                );
                let clip_resp = client
                    .execute_cdp(
                        &session_id,
                        "Runtime.evaluate",
                        serde_json::json!({
                            "expression": clip_js,
                            "returnByValue": true,
                        }),
                    )
                    .await;
                let clip = clip_resp
                    .ok()
                    .as_ref()
                    .and_then(|r| bb_eval_return_value(r))
                    .filter(|v| !v.is_null());
                let mut cap = capture_params.clone();
                if let Some(c) = clip {
                    if let Some(obj) = cap.as_object_mut() {
                        obj.insert("clip".to_string(), c);
                    }
                }
                client
                    .execute_cdp(&session_id, "Page.captureScreenshot", cap)
                    .await
            }
        } else {
            client
                .execute_cdp(&session_id, "Page.captureScreenshot", capture_params)
                .await
        };

        match result {
            Ok(response) => {
                let data = bb_screenshot_data(&response);

                if let Some(base64_data) = data {
                    let summary = serde_json::json!({
                        "ok": true,
                        "session_id": session_id,
                        "format": "png",
                        "base64_len": base64_data.len(),
                        "full_page": full_page,
                    });
                    ToolOutput::success(summary.to_string()).with_metadata(serde_json::json!({
                        "session_id": session_id,
                        "format": "png",
                        "base64_data": base64_data,
                    }))
                } else {
                    ToolOutput::error("Screenshot data not found in response")
                }
            }
            Err(e) => ToolOutput::error(format!("Screenshot failed: {}", e)),
        }
    }
}

impl Default for BrowserScreenshotTool {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Browser Get Text Tool
// ============================================================================

pub struct BrowserGetTextTool {
    client: Option<BrowserbaseClient>,
}

impl BrowserGetTextTool {
    pub fn new() -> Self {
        let client = std::env::var("BROWSERBASE_API_KEY")
            .ok()
            .map(|key| BrowserbaseClient::new(key));
        Self { client }
    }
}

#[async_trait::async_trait]
impl Tool for BrowserGetTextTool {
    fn name(&self) -> &str {
        "browser_get_text"
    }

    fn description(&self) -> &str {
        "Extract innerText from the page or a selector; returns JSON with text and char counts. session_id optional if default session set."
    }

    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            (
                "session_id",
                "Browserbase session (optional if default session set)",
                false,
            ),
            (
                "selector",
                "CSS selector (optional; full page innerText if omitted)",
                false,
            ),
            (
                "max_chars",
                "Max characters to return (default 12000, cap 50000)",
                false,
            ),
        ])
    }

    async fn execute(&self, params: Value) -> ToolOutput {
        let session_id = match resolve_browser_session(&params) {
            Ok(s) => s,
            Err(out) => return out,
        };

        let Some(client) = &self.client else {
            return ToolOutput::error("BROWSERBASE_API_KEY not configured");
        };

        let max_chars = params
            .get("max_chars")
            .and_then(|v| v.as_u64())
            .or_else(|| {
                params
                    .get("max_chars")
                    .and_then(|v| v.as_str()?.parse().ok())
            })
            .unwrap_or(12_000)
            .clamp(500, 50_000) as usize;

        let js = if let Some(selector) = params.get("selector").and_then(|v| v.as_str()) {
            if selector.trim().is_empty() {
                "document.body.innerText".to_string()
            } else {
                let q = json_string_for_js(selector);
                format!(
                    "(function() {{ const el = document.querySelector({q}); return el ? el.innerText : ''; }})()",
                    q = q
                )
            }
        } else {
            "document.body.innerText".to_string()
        };

        let result = client
            .execute_cdp(
                &session_id,
                "Runtime.evaluate",
                serde_json::json!({
                    "expression": js,
                    "returnByValue": true,
                }),
            )
            .await;

        match result {
            Ok(response) => {
                let text = bb_eval_return_value(&response)
                    .and_then(|v| v.as_str().map(|s| s.to_string()))
                    .unwrap_or_default();

                let total = text.len();
                let body = if text.len() > max_chars {
                    format!(
                        "{}\n\n[Truncated: showing {} of {} chars; increase max_chars or narrow selector]",
                        &text[..max_chars],
                        max_chars,
                        total
                    )
                } else {
                    text
                };

                let payload = serde_json::json!({
                    "ok": true,
                    "session_id": session_id,
                    "chars_total": total,
                    "text": body,
                });
                ToolOutput::success(payload.to_string())
            }
            Err(e) => ToolOutput::error(format!("Get text failed: {}", e)),
        }
    }
}

impl Default for BrowserGetTextTool {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Browser Wait Tool
// ============================================================================

pub struct BrowserWaitTool {
    client: Option<BrowserbaseClient>,
}

impl BrowserWaitTool {
    pub fn new() -> Self {
        let client = std::env::var("BROWSERBASE_API_KEY")
            .ok()
            .map(|key| BrowserbaseClient::new(key));
        Self { client }
    }
}

#[async_trait::async_trait]
impl Tool for BrowserWaitTool {
    fn name(&self) -> &str {
        "browser_wait"
    }

    fn description(&self) -> &str {
        "Wait for milliseconds and/or until a CSS selector appears (polls Runtime.evaluate). Use after navigation or before click on slow SPAs."
    }

    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            (
                "session_id",
                "Browserbase session (optional if default session set)",
                false,
            ),
            (
                "wait_ms",
                "Milliseconds to sleep first (default 0, max 60000)",
                false,
            ),
            (
                "selector",
                "If set, poll until this selector matches or timeout_ms",
                false,
            ),
            (
                "timeout_ms",
                "Max time to poll for selector (default 30000)",
                false,
            ),
            (
                "poll_ms",
                "Poll interval when waiting for selector (default 250)",
                false,
            ),
        ])
    }

    async fn execute(&self, params: Value) -> ToolOutput {
        let session_id = match resolve_browser_session(&params) {
            Ok(s) => s,
            Err(out) => return out,
        };

        let Some(client) = &self.client else {
            return ToolOutput::error("BROWSERBASE_API_KEY not configured");
        };

        let wait_ms = params
            .get("wait_ms")
            .and_then(|v| v.as_u64())
            .or_else(|| params.get("wait_ms").and_then(|v| v.as_str()?.parse().ok()))
            .unwrap_or(0)
            .min(60_000);

        if wait_ms > 0 {
            tokio::time::sleep(Duration::from_millis(wait_ms)).await;
        }

        let selector = params
            .get("selector")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty());

        if let Some(sel) = selector {
            let timeout_ms = params
                .get("timeout_ms")
                .and_then(|v| v.as_u64())
                .or_else(|| {
                    params
                        .get("timeout_ms")
                        .and_then(|v| v.as_str()?.parse().ok())
                })
                .unwrap_or(30_000)
                .min(120_000);
            let poll_ms = params
                .get("poll_ms")
                .and_then(|v| v.as_u64())
                .unwrap_or(250)
                .clamp(50, 5_000);

            let q = json_string_for_js(sel);
            let js = format!(
                "(function() {{ return !!document.querySelector({q}); }})()",
                q = q
            );

            let deadline = Instant::now() + Duration::from_millis(timeout_ms);
            let mut found = false;
            while Instant::now() < deadline {
                let Ok(resp) = client
                    .execute_cdp(
                        &session_id,
                        "Runtime.evaluate",
                        serde_json::json!({
                            "expression": js,
                            "returnByValue": true,
                        }),
                    )
                    .await
                else {
                    tokio::time::sleep(Duration::from_millis(poll_ms)).await;
                    continue;
                };
                found = bb_eval_return_value(&resp)
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                if found {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(poll_ms)).await;
            }

            let out = serde_json::json!({
                "ok": found,
                "session_id": session_id,
                "selector": sel,
                "found": found,
                "waited_ms": wait_ms,
            });
            if found {
                ToolOutput::success(out.to_string())
            } else {
                ToolOutput::error(format!(
                    "Timeout waiting for selector {} after {}ms",
                    sel, timeout_ms
                ))
                .with_metadata(out)
            }
        } else {
            let out = serde_json::json!({
                "ok": true,
                "session_id": session_id,
                "waited_ms": wait_ms,
            });
            ToolOutput::success(out.to_string())
        }
    }
}

impl Default for BrowserWaitTool {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Browser Close Session Tool
// ============================================================================

pub struct BrowserCloseTool {
    client: Option<BrowserbaseClient>,
}

impl BrowserCloseTool {
    pub fn new() -> Self {
        let client = std::env::var("BROWSERBASE_API_KEY")
            .ok()
            .map(|key| BrowserbaseClient::new(key));
        Self { client }
    }
}

#[async_trait::async_trait]
impl Tool for BrowserCloseTool {
    fn name(&self) -> &str {
        "browser_close"
    }

    fn description(&self) -> &str {
        "Close a browser session and release resources."
    }

    fn parameters_schema(&self) -> Value {
        object_schema(vec![(
            "session_id",
            "Session to close (optional if closing the default session)",
            false,
        )])
    }

    async fn execute(&self, params: Value) -> ToolOutput {
        let session_id = match resolve_browser_session(&params) {
            Ok(s) => s,
            Err(out) => return out,
        };

        let Some(client) = &self.client else {
            return ToolOutput::error("BROWSERBASE_API_KEY not configured");
        };

        match client.close_session(&session_id).await {
            Ok(_) => {
                take_browser_session_if_matches(&session_id);
                let out = serde_json::json!({
                    "ok": true,
                    "closed_session_id": session_id,
                });
                ToolOutput::success(out.to_string())
            }
            Err(e) => ToolOutput::error(format!("Failed to close session: {}", e)),
        }
    }
}

impl Default for BrowserCloseTool {
    fn default() -> Self {
        Self::new()
    }
}
