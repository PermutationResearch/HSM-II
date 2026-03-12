//! Browser Automation Tools via Browserbase
//!
//! Full browser automation: navigation, clicking, form filling, screenshots,
//! JavaScript execution, session management, and more.

use anyhow::{anyhow, Result};
use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;
use tracing::{error, info, warn};

use super::{Tool, ToolOutput, object_schema};

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
        
        let response = self.client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({
                "projectId": std::env::var("BROWSERBASE_PROJECT_ID").ok(),
            }))
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
        
        let response = self.client
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
        
        let response = self.client
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
        "Navigate browser to a URL. Creates a new browser session if needed."
    }
    
    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            ("url", "The URL to navigate to", true),
            ("session_id", "Existing session ID (optional)", false),
            ("wait_until", "When to consider navigation complete: load, domcontentloaded, networkidle", false),
        ])
    }
    
    async fn execute(&self, params: Value) -> ToolOutput {
        let url = params.get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        
        if url.is_empty() {
            return ToolOutput::error("URL parameter is required");
        }
        
        let Some(client) = &self.client else {
            return ToolOutput::error("BROWSERBASE_API_KEY not configured");
        };
        
        // Use existing session or create new one
        let session_id = if let Some(id) = params.get("session_id").and_then(|v| v.as_str()) {
            id.to_string()
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
        
        // Navigate to URL
        let wait_until = params.get("wait_until")
            .and_then(|v| v.as_str())
            .unwrap_or("load");
        
        let result = client.execute_cdp(
            &session_id,
            "Page.navigate",
            serde_json::json!({
                "url": url,
            }),
        ).await;
        
        match result {
            Ok(_) => {
                // Wait for page load
                if let Err(e) = client.execute_cdp(
                    &session_id,
                    "Page.lifecycleEvent",
                    serde_json::json!({
                        "frameId": "main",
                        "waitUntil": wait_until,
                        "timeout": 30000,
                    }),
                ).await {
                    warn!("Wait condition not met: {}", e);
                }
                
                // Get page title
                let title_result = client.execute_cdp(
                    &session_id,
                    "Runtime.evaluate",
                    serde_json::json!({
                        "expression": "document.title",
                    }),
                ).await;
                
                let title = title_result
                    .ok()
                    .and_then(|r| r.get("result").cloned())
                    .and_then(|r| r.get("value").cloned())
                    .and_then(|v| v.as_str().map(|s| s.to_string()))
                    .unwrap_or_else(|| "Unknown".to_string());
                
                ToolOutput::success(format!("Navigated to: {} (Title: {})", url, title))
                    .with_metadata(serde_json::json!({
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
        "Click on an element by CSS selector or text content."
    }
    
    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            ("session_id", "Browser session ID", true),
            ("selector", "CSS selector for the element", false),
            ("text", "Text content to find and click (if no selector)", false),
        ])
    }
    
    async fn execute(&self, params: Value) -> ToolOutput {
        let session_id = params.get("session_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        
        if session_id.is_empty() {
            return ToolOutput::error("session_id is required");
        }
        
        let Some(client) = &self.client else {
            return ToolOutput::error("BROWSERBASE_API_KEY not configured");
        };
        
        // Build JavaScript to find and click element
        let js = if let Some(selector) = params.get("selector").and_then(|v| v.as_str()) {
            format!(
                "(function() {{
                    const el = document.querySelector('{}');
                    if (!el) return {{success: false, error: 'Element not found'}};
                    el.click();
                    return {{success: true, element: el.tagName}};
                }})()",
                selector.replace("'", "\\'")
            )
        } else if let Some(text) = params.get("text").and_then(|v| v.as_str()) {
            format!(
                "(function() {{
                    const xpath = \"//button[contains(text(), '{}')] | //a[contains(text(), '{}')] | //*[contains(text(), '{}')]\";
                    const result = document.evaluate(xpath, document, null, XPathResult.FIRST_ORDERED_NODE_TYPE, null);
                    const el = result.singleNodeValue;
                    if (!el) return {{success: false, error: 'Element with text not found'}};
                    el.click();
                    return {{success: true, element: el.tagName}};
                }})()",
                text.replace("'", "\\'"),
                text.replace("'", "\\'"),
                text.replace("'", "\\'")
            )
        } else {
            return ToolOutput::error("Either selector or text parameter is required");
        };
        
        let result = client.execute_cdp(
            session_id,
            "Runtime.evaluate",
            serde_json::json!({
                "expression": js,
                "returnByValue": true,
            }),
        ).await;
        
        match result {
            Ok(response) => {
                let result_value = response.get("result")
                    .and_then(|r| r.get("value"))
                    .cloned()
                    .unwrap_or_default();
                
                let success = result_value.get("success")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                
                if success {
                    let element = result_value.get("element")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Unknown");
                    ToolOutput::success(format!("Clicked on {} element", element))
                } else {
                    let error = result_value.get("error")
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
        "Type text into an input field (form filling)."
    }
    
    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            ("session_id", "Browser session ID", true),
            ("selector", "CSS selector for input field", true),
            ("text", "Text to type", true),
            ("clear_first", "Clear existing text first (default: true)", false),
        ])
    }
    
    async fn execute(&self, params: Value) -> ToolOutput {
        let session_id = params.get("session_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let selector = params.get("selector")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let text = params.get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        
        if session_id.is_empty() || selector.is_empty() {
            return ToolOutput::error("session_id and selector are required");
        }
        
        let Some(client) = &self.client else {
            return ToolOutput::error("BROWSERBASE_API_KEY not configured");
        };
        
        let clear_first = params.get("clear_first")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        
        let js = format!(
            "(function() {{
                const el = document.querySelector('{}');
                if (!el) return {{success: false, error: 'Element not found'}};
                if (!['INPUT', 'TEXTAREA', 'SELECT'].includes(el.tagName)) {{
                    return {{success: false, error: 'Element is not an input field'}};
                }}
                {}
                el.value = '{}';
                el.dispatchEvent(new Event('input', {{ bubbles: true }}));
                el.dispatchEvent(new Event('change', {{ bubbles: true }}));
                return {{success: true, tag: el.tagName}};
            }})()",
            selector.replace("'", "\\'"),
            if clear_first { "el.value = '';" } else { "" },
            text.replace("'", "\\'")
        );
        
        let result = client.execute_cdp(
            session_id,
            "Runtime.evaluate",
            serde_json::json!({
                "expression": js,
                "returnByValue": true,
            }),
        ).await;
        
        match result {
            Ok(response) => {
                let result_value = response.get("result")
                    .and_then(|r| r.get("value"))
                    .cloned()
                    .unwrap_or_default();
                
                let success = result_value.get("success")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                
                if success {
                    ToolOutput::success(format!("Typed '{}' into {}", text, selector))
                } else {
                    let error = result_value.get("error")
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
        "Take a screenshot of the current page. Returns base64 encoded image."
    }
    
    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            ("session_id", "Browser session ID", true),
            ("selector", "CSS selector to screenshot specific element (optional)", false),
            ("full_page", "Screenshot full page or just viewport (default: false)", false),
        ])
    }
    
    async fn execute(&self, params: Value) -> ToolOutput {
        let session_id = params.get("session_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        
        if session_id.is_empty() {
            return ToolOutput::error("session_id is required");
        }
        
        let Some(client) = &self.client else {
            return ToolOutput::error("BROWSERBASE_API_KEY not configured");
        };
        
        let full_page = params.get("full_page")
            .and_then(|v| v.as_bool())
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
        
        let result = client.execute_cdp(
            session_id,
            "Page.captureScreenshot",
            capture_params,
        ).await;
        
        match result {
            Ok(response) => {
                let data = response.get("data")
                    .and_then(|d| d.as_str())
                    .map(|s| s.to_string());
                
                if let Some(base64_data) = data {
                    ToolOutput::success(format!("Screenshot captured ({} bytes)", base64_data.len()))
                        .with_metadata(serde_json::json!({
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
        "Get text content from the page or a specific element."
    }
    
    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            ("session_id", "Browser session ID", true),
            ("selector", "CSS selector (optional, gets full page text if omitted)", false),
        ])
    }
    
    async fn execute(&self, params: Value) -> ToolOutput {
        let session_id = params.get("session_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        
        if session_id.is_empty() {
            return ToolOutput::error("session_id is required");
        }
        
        let Some(client) = &self.client else {
            return ToolOutput::error("BROWSERBASE_API_KEY not configured");
        };
        
        let js = if let Some(selector) = params.get("selector").and_then(|v| v.as_str()) {
            format!(
                "document.querySelector('{}')?.innerText || ''",
                selector.replace("'", "\\'")
            )
        } else {
            "document.body.innerText".to_string()
        };
        
        let result = client.execute_cdp(
            session_id,
            "Runtime.evaluate",
            serde_json::json!({
                "expression": js,
                "returnByValue": true,
            }),
        ).await;
        
        match result {
            Ok(response) => {
                let text = response.get("result")
                    .and_then(|r| r.get("value"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                
                let truncated = if text.len() > 5000 {
                    format!("{}...\n[Truncated, total: {} chars]", &text[..5000], text.len())
                } else {
                    text
                };
                
                ToolOutput::success(truncated)
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
        object_schema(vec![
            ("session_id", "Browser session ID to close", true),
        ])
    }
    
    async fn execute(&self, params: Value) -> ToolOutput {
        let session_id = params.get("session_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        
        if session_id.is_empty() {
            return ToolOutput::error("session_id is required");
        }
        
        let Some(client) = &self.client else {
            return ToolOutput::error("BROWSERBASE_API_KEY not configured");
        };
        
        match client.close_session(session_id).await {
            Ok(_) => ToolOutput::success(format!("Session {} closed", session_id)),
            Err(e) => ToolOutput::error(format!("Failed to close session: {}", e)),
        }
    }
}

impl Default for BrowserCloseTool {
    fn default() -> Self {
        Self::new()
    }
}
