//! [Firecrawl](https://docs.firecrawl.dev/api-reference/v1-endpoint/scrape) scrape API — markdown/HTML extraction without running a local browser.
//!
//! **Cloud:** set `FIRECRAWL_API_KEY` (required for `api.firecrawl.dev`). Optional `FIRECRAWL_API_BASE`
//! if you use a custom cloud base.
//!
//! **Self-hosted** ([open-source](https://github.com/firecrawl/firecrawl)): set `FIRECRAWL_API_URL` to your
//! instance (e.g. `http://localhost:3002`). `/v1` is appended when missing. `FIRECRAWL_API_KEY` is optional
//! when not hitting the official cloud API.

use reqwest::Client;
use reqwest::RequestBuilder;
use serde_json::Value;
use tracing::{info, warn};

use super::{object_schema, Tool, ToolOutput};
use crate::tools::security::validate_outbound_url;

const DEFAULT_BASE: &str = "https://api.firecrawl.dev/v1";
const MAX_RESULT_CHARS: usize = 120_000;

/// Normalize origin to a base that includes `/v1` (matches Firecrawl OSS and cloud layout).
pub(crate) fn normalize_firecrawl_base(raw: &str) -> String {
    let u = raw.trim().trim_end_matches('/');
    if u.is_empty() {
        return DEFAULT_BASE.to_string();
    }
    if u.ends_with("/v1") || u.ends_with("/v2") {
        u.to_string()
    } else {
        format!("{}/v1", u)
    }
}

fn resolve_firecrawl_base() -> String {
    if let Ok(u) = std::env::var("FIRECRAWL_API_URL") {
        let t = u.trim();
        if !t.is_empty() {
            return normalize_firecrawl_base(t);
        }
    }
    if let Ok(b) = std::env::var("FIRECRAWL_API_BASE") {
        let t = b.trim();
        if !t.is_empty() {
            return normalize_firecrawl_base(t);
        }
    }
    DEFAULT_BASE.to_string()
}

/// Official cloud host expects a bearer key; self-hosted often has no auth.
fn firecrawl_cloud_requires_key(base: &str) -> bool {
    base.contains("api.firecrawl.dev")
}

pub struct FirecrawlScrapeTool {
    client: Client,
    api_key: Option<String>,
    base: String,
}

impl FirecrawlScrapeTool {
    pub fn new() -> Self {
        let base = resolve_firecrawl_base();
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .user_agent("HSM-II/0.1 (Firecrawl tool)")
                .build()
                .expect("reqwest client"),
            api_key: std::env::var("FIRECRAWL_API_KEY")
                .ok()
                .filter(|s| !s.trim().is_empty()),
            base,
        }
    }

    fn apply_auth(&self, mut req: RequestBuilder) -> RequestBuilder {
        if let Some(ref key) = self.api_key {
            req = req.header("Authorization", format!("Bearer {}", key));
        }
        req
    }

    fn parse_formats(params: &Value) -> Vec<String> {
        if let Some(arr) = params.get("formats").and_then(|v| v.as_array()) {
            let v: Vec<String> = arr
                .iter()
                .filter_map(|x| x.as_str().map(|s| s.trim().to_string()))
                .filter(|s| !s.is_empty())
                .collect();
            if !v.is_empty() {
                return v;
            }
        }
        if let Some(s) = params.get("formats").and_then(|v| v.as_str()) {
            let v: Vec<String> = s
                .split(',')
                .map(|x| x.trim().to_string())
                .filter(|x| !x.is_empty())
                .collect();
            if !v.is_empty() {
                return v;
            }
        }
        vec!["markdown".to_string()]
    }
}

#[async_trait::async_trait]
impl Tool for FirecrawlScrapeTool {
    fn name(&self) -> &str {
        "firecrawl_scrape"
    }

    fn description(&self) -> &str {
        "Scrape a URL with Firecrawl (cloud or self-hosted via FIRECRAWL_API_URL). Returns JSON with markdown. Cloud needs FIRECRAWL_API_KEY; self-hosted often does not."
    }

    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            ("url", "Absolute https URL to scrape", true),
            (
                "formats",
                "Comma-separated or omit for markdown only — e.g. markdown,html,links",
                false,
            ),
        ])
    }

    async fn execute(&self, params: Value) -> ToolOutput {
        if firecrawl_cloud_requires_key(&self.base) && self.api_key.is_none() {
            return ToolOutput::error(
                "FIRECRAWL_API_KEY is required for api.firecrawl.dev. For self-hosted Firecrawl, set FIRECRAWL_API_URL (e.g. http://localhost:3002); key is optional.",
            );
        }

        let url = params
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        if url.is_empty() {
            return ToolOutput::error("url is required");
        }
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return ToolOutput::error("url must start with http:// or https://");
        }
        if let Err(e) = validate_outbound_url(url) {
            return ToolOutput::error(format!("Blocked by SSRF guard: {e}"));
        }

        let formats = Self::parse_formats(&params);
        let body = serde_json::json!({
            "url": url,
            "formats": formats,
        });

        let endpoint = format!("{}/scrape", self.base);
        info!(base = %self.base, %url, "Firecrawl scrape");

        let req = self
            .client
            .post(&endpoint)
            .header("Content-Type", "application/json")
            .json(&body);
        let resp = match self.apply_auth(req).send().await {
            Ok(r) => r,
            Err(e) => return ToolOutput::error(format!("Firecrawl request failed: {}", e)),
        };

        let status = resp.status();
        let text = match resp.text().await {
            Ok(t) => t,
            Err(e) => return ToolOutput::error(format!("Firecrawl read body: {}", e)),
        };

        let parsed: Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(_) => {
                return ToolOutput::error(format!(
                    "Firecrawl non-JSON response (HTTP {}): {}",
                    status.as_u16(),
                    text.chars().take(500).collect::<String>()
                ));
            }
        };

        if !status.is_success() {
            let err = parsed
                .get("error")
                .and_then(|e| e.as_str())
                .unwrap_or(&text);
            return ToolOutput::error(format!("Firecrawl HTTP {}: {}", status.as_u16(), err));
        }

        let success = parsed
            .get("success")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !success {
            let err = parsed
                .get("error")
                .and_then(|e| e.as_str())
                .or_else(|| parsed.get("message").and_then(|m| m.as_str()))
                .unwrap_or("Firecrawl success=false");
            warn!("Firecrawl scrape failed: {}", err);
            return ToolOutput::error(err.to_string());
        }

        let mut data = parsed.get("data").cloned().unwrap_or(Value::Null);
        if let Some(d) = data.as_object_mut() {
            for key in ["markdown", "html", "rawHtml"] {
                if let Some(Value::String(st)) = d.get_mut(key) {
                    if st.chars().count() > 60_000 {
                        *st =
                            st.chars().take(60_000).collect::<String>() + "… [truncated by HSM-II]";
                    }
                }
            }
        }

        let mut out = serde_json::json!({
            "url": url,
            "source": "firecrawl",
            "data": data.clone(),
        });

        if let Some(md) = data.get("markdown").and_then(|v| v.as_str()) {
            out["markdown"] = Value::String(md.to_string());
        }

        let mut s = serde_json::to_string_pretty(&out).unwrap_or_else(|_| "{}".to_string());
        if s.len() > MAX_RESULT_CHARS {
            if let Some(obj) = out.as_object_mut() {
                obj.remove("data");
                if let Some(Value::String(md)) = obj.get_mut("markdown") {
                    let take = MAX_RESULT_CHARS.saturating_sub(500);
                    *md = md.chars().take(take).collect::<String>() + "… [truncated]";
                }
            }
            s = serde_json::to_string_pretty(&out).unwrap_or_else(|_| "{}".to_string());
        }

        ToolOutput::success(s).with_metadata(serde_json::json!({
            "url": url,
            "provider": "firecrawl",
            "firecrawl_base": self.base,
        }))
    }
}

impl Default for FirecrawlScrapeTool {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_default() {
        let p = serde_json::json!({});
        assert_eq!(FirecrawlScrapeTool::parse_formats(&p), vec!["markdown"]);
    }

    #[test]
    fn formats_csv() {
        let p = serde_json::json!({"formats": "markdown, links"});
        let f = FirecrawlScrapeTool::parse_formats(&p);
        assert!(f.contains(&"markdown".to_string()));
        assert!(f.contains(&"links".to_string()));
    }

    #[test]
    fn normalize_appends_v1() {
        assert_eq!(
            normalize_firecrawl_base("http://localhost:3002"),
            "http://localhost:3002/v1"
        );
    }

    #[test]
    fn normalize_preserves_v1_suffix() {
        assert_eq!(
            normalize_firecrawl_base("http://127.0.0.1:3002/v1"),
            "http://127.0.0.1:3002/v1"
        );
    }
}
