//! Web Search Tool
//!
//! Provides web search capabilities using:
//! - Cloudflare Browser Rendering API (crawl endpoint) - recommended
//! - DuckDuckGo (fallback, no API key)
//! - Brave Search (API key required)
//! - Serper (API key required)

use anyhow::{anyhow, Result};
use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;
use tracing::{debug, error, info, warn};

use super::{object_schema, Tool, ToolOutput};

/// Web search tool
pub struct WebSearchTool {
    client: Client,
    backend: SearchBackend,
    cf_account_id: Option<String>,
    cf_api_token: Option<String>,
    api_key: Option<String>,
}

#[derive(Clone, Debug)]
enum SearchBackend {
    CloudflareCrawl,
    DuckDuckGo,
    Brave,
    Serper,
}

impl Default for SearchBackend {
    fn default() -> Self {
        // Prefer Cloudflare if credentials available
        if std::env::var("CF_ACCOUNT_ID").is_ok() && std::env::var("CF_API_TOKEN").is_ok() {
            SearchBackend::CloudflareCrawl
        } else {
            SearchBackend::DuckDuckGo
        }
    }
}

impl WebSearchTool {
    pub fn new() -> Self {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .user_agent("HSM-II/0.1.0 (Research Agent)")
            .build()
            .expect("Failed to create HTTP client");

        // Check for Cloudflare credentials first (preferred)
        let cf_account_id = std::env::var("CF_ACCOUNT_ID").ok();
        let cf_api_token = std::env::var("CF_API_TOKEN").ok();

        let (backend, api_key) = if cf_account_id.is_some() && cf_api_token.is_some() {
            info!("Using Cloudflare Browser Rendering crawl endpoint");
            (SearchBackend::CloudflareCrawl, None)
        } else if let Ok(key) = std::env::var("BRAVE_API_KEY") {
            info!("Using Brave Search backend");
            (SearchBackend::Brave, Some(key))
        } else if let Ok(key) = std::env::var("SERPER_API_KEY") {
            info!("Using Serper backend");
            (SearchBackend::Serper, Some(key))
        } else {
            info!("Using DuckDuckGo backend (no API key required)");
            (SearchBackend::DuckDuckGo, None)
        };

        Self {
            client,
            backend,
            cf_account_id,
            cf_api_token,
            api_key,
        }
    }

    /// Crawl a URL using Cloudflare Browser Rendering API
    /// https://developers.cloudflare.com/browser-rendering/
    async fn crawl_cloudflare(&self, url: &str, _num_results: usize) -> Result<Vec<SearchResult>> {
        let account_id = self
            .cf_account_id
            .as_ref()
            .ok_or_else(|| anyhow!("CF_ACCOUNT_ID not configured"))?;
        let api_token = self
            .cf_api_token
            .as_ref()
            .ok_or_else(|| anyhow!("CF_API_TOKEN not configured"))?;

        // Step 1: Initiate crawl
        let crawl_url = format!(
            "https://api.cloudflare.com/client/v4/accounts/{}/browser-rendering/crawl",
            account_id
        );

        let body = serde_json::json!({
            "url": url,
            "limit": _num_results.max(1).min(10),
            "render": true,
            "output": ["markdown", "structured"],
        });

        debug!("Initiating Cloudflare crawl for: {}", url);

        let response = self
            .client
            .post(&crawl_url)
            .header("Authorization", format!("Bearer {}", api_token))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow!(
                "Cloudflare crawl initiation failed: {}",
                error_text
            ));
        }

        let crawl_response: CloudflareCrawlResponse = response.json().await?;

        if !crawl_response.success {
            return Err(anyhow!(
                "Cloudflare crawl failed: {:?}",
                crawl_response.errors
            ));
        }

        let job_id = crawl_response.result.id;
        debug!("Crawl job initiated: {}", job_id);

        // Step 2: Poll for results (with timeout)
        let status_url = format!(
            "https://api.cloudflare.com/client/v4/accounts/{}/browser-rendering/crawl/{}",
            account_id, job_id
        );

        let max_retries = 30; // 30 seconds max wait
        for attempt in 0..max_retries {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;

            let status_response = self
                .client
                .get(&status_url)
                .header("Authorization", format!("Bearer {}", api_token))
                .send()
                .await?;

            if !status_response.status().is_success() {
                continue;
            }

            let status: CloudflareCrawlStatus = status_response.json().await?;

            if !status.success {
                return Err(anyhow!("Crawl status check failed"));
            }

            match status.result.status.as_str() {
                "completed" => {
                    // Extract results from crawled pages
                    let mut results = Vec::new();

                    for page in status.result.pages.unwrap_or_default() {
                        let snippet = page
                            .markdown
                            .as_ref()
                            .map(|md| {
                                let preview = md.chars().take(500).collect::<String>();
                                if md.len() > 500 {
                                    format!("{}...", preview)
                                } else {
                                    preview
                                }
                            })
                            .unwrap_or_else(|| page.title.clone());

                        results.push(SearchResult {
                            title: page.title,
                            url: page.url,
                            snippet,
                        });
                    }

                    return Ok(results);
                }
                "failed" => {
                    return Err(anyhow!("Crawl job failed"));
                }
                _ => {
                    debug!(
                        "Crawl job status: {} (attempt {})",
                        status.result.status, attempt
                    );
                }
            }
        }

        Err(anyhow!("Crawl timed out after {} seconds", max_retries))
    }

    /// Search with DuckDuckGo (HTML scraping)
    async fn search_duckduckgo(
        &self,
        query: &str,
        num_results: usize,
    ) -> Result<Vec<SearchResult>> {
        // DuckDuckGo Lite HTML interface
        let url = format!(
            "https://duckduckgo.com/html/?q={}",
            urlencoding::encode(query)
        );

        let response = self.client.get(&url).send().await?;
        let html = response.text().await?;

        // Parse results from HTML
        let results = parse_duckduckgo_html(&html, num_results)?;
        Ok(results)
    }

    /// Search with Brave API
    async fn search_brave(&self, query: &str, num_results: usize) -> Result<Vec<SearchResult>> {
        let api_key = self
            .api_key
            .as_ref()
            .ok_or_else(|| anyhow!("Brave API key not configured"))?;

        let url = "https://api.search.brave.com/res/v1/web/search";

        let response = self
            .client
            .get(url)
            .header("X-Subscription-Token", api_key)
            .query(&[("q", query), ("count", &num_results.to_string())])
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(anyhow!("Brave API error: {}", response.status()));
        }

        let data: BraveResponse = response.json().await?;
        let results = data
            .web
            .results
            .into_iter()
            .map(|r| SearchResult {
                title: r.title,
                url: r.url,
                snippet: r.description,
            })
            .collect();

        Ok(results)
    }

    /// Search with Serper API (Google results)
    async fn search_serper(&self, query: &str, num_results: usize) -> Result<Vec<SearchResult>> {
        let api_key = self
            .api_key
            .as_ref()
            .ok_or_else(|| anyhow!("Serper API key not configured"))?;

        let url = "https://google.serper.dev/search";

        let body = serde_json::json!({
            "q": query,
            "num": num_results,
        });

        let response = self
            .client
            .post(url)
            .header("X-API-KEY", api_key)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(anyhow!("Serper API error: {}", response.status()));
        }

        let data: SerperResponse = response.json().await?;
        let results = data
            .organic
            .into_iter()
            .map(|r| SearchResult {
                title: r.title,
                url: r.link,
                snippet: r.snippet.unwrap_or_default(),
            })
            .collect();

        Ok(results)
    }

    /// Format search results as readable text
    fn format_results(&self, results: &[SearchResult]) -> String {
        if results.is_empty() {
            return "No results found.".to_string();
        }

        let mut output = format!("Found {} results:\n\n", results.len());

        for (i, result) in results.iter().enumerate() {
            output.push_str(&format!(
                "{}. {}\n   URL: {}\n   {}\n\n",
                i + 1,
                result.title,
                result.url,
                result.snippet
            ));
        }

        output
    }
}

#[async_trait::async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "web_search"
    }

    fn description(&self) -> &str {
        "Search the web or crawl a website for information. Returns results with titles, URLs, and content snippets."
    }

    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            ("query", "The search query or URL to crawl", true),
            (
                "num_results",
                "Number of results to return (1-10, default 5)",
                false,
            ),
        ])
    }

    async fn execute(&self, params: Value) -> ToolOutput {
        let query = params.get("query").and_then(|v| v.as_str()).unwrap_or("");

        if query.is_empty() {
            return ToolOutput::error("Query parameter is required");
        }

        let num_results: usize = params
            .get("num_results")
            .and_then(|v| v.as_u64())
            .unwrap_or(5)
            .clamp(1, 10) as usize;

        debug!("Searching for: {} ({} results)", query, num_results);

        let results = match self.backend {
            SearchBackend::CloudflareCrawl => {
                // For Cloudflare, query should be a URL to crawl
                let url = if query.starts_with("http") {
                    query.to_string()
                } else {
                    // Try to search first with DuckDuckGo, then crawl top result
                    match self.search_duckduckgo(query, 1).await {
                        Ok(r) if !r.is_empty() => r[0].url.clone(),
                        _ => return ToolOutput::error(
                            "Cloudflare crawl requires a URL. Please provide a full URL starting with http:// or https://"
                        ),
                    }
                };

                match self.crawl_cloudflare(&url, num_results).await {
                    Ok(r) => r,
                    Err(e) => {
                        error!("Cloudflare crawl failed: {}", e);
                        return ToolOutput::error(format!("Crawl failed: {}", e));
                    }
                }
            }
            SearchBackend::DuckDuckGo => match self.search_duckduckgo(query, num_results).await {
                Ok(r) => r,
                Err(e) => {
                    error!("DuckDuckGo search failed: {}", e);
                    return ToolOutput::error(format!("Search failed: {}", e));
                }
            },
            SearchBackend::Brave => match self.search_brave(query, num_results).await {
                Ok(r) => r,
                Err(e) => {
                    error!("Brave search failed: {}", e);
                    return ToolOutput::error(format!("Search failed: {}", e));
                }
            },
            SearchBackend::Serper => match self.search_serper(query, num_results).await {
                Ok(r) => r,
                Err(e) => {
                    error!("Serper search failed: {}", e);
                    return ToolOutput::error(format!("Search failed: {}", e));
                }
            },
        };

        let formatted = self.format_results(&results);
        let metadata = serde_json::json!({
            "backend": format!("{:?}", self.backend),
            "result_count": results.len(),
        });

        ToolOutput::success(formatted).with_metadata(metadata)
    }
}

impl Default for WebSearchTool {
    fn default() -> Self {
        Self::new()
    }
}

/// Single search result
#[derive(Clone, Debug)]
struct SearchResult {
    title: String,
    url: String,
    snippet: String,
}

/// Parse DuckDuckGo HTML results
fn parse_duckduckgo_html(html: &str, max_results: usize) -> Result<Vec<SearchResult>> {
    use regex::Regex;

    let mut results = Vec::new();

    let result_regex =
        Regex::new(r#"<a[^>]*class="result__a"[^>]*href="([^"]+)"[^>]*>([^<]+)</a>"#).ok();

    let snippet_regex = Regex::new(r#"<a[^>]*class="result__snippet"[^>]*>([^<]+)</a>"#).ok();

    if let (Some(title_re), Some(snippet_re)) = (result_regex, snippet_regex) {
        let titles: Vec<(String, String)> = title_re
            .captures_iter(html)
            .filter_map(|cap| {
                let url = cap.get(1)?.as_str().to_string();
                let title = cap.get(2)?.as_str().to_string();
                Some((url, title))
            })
            .collect();

        let snippets: Vec<String> = snippet_re
            .captures_iter(html)
            .filter_map(|cap| cap.get(1).map(|m| m.as_str().to_string()))
            .collect();

        for (i, (url, title)) in titles.iter().enumerate().take(max_results) {
            let snippet = snippets.get(i).cloned().unwrap_or_default();
            results.push(SearchResult {
                title: title.clone(),
                url: url.clone(),
                snippet,
            });
        }
    }

    if results.is_empty() {
        warn!("Regex parsing failed, trying fallback extraction");
        results.push(SearchResult {
            title: "Search results available".to_string(),
            url: "https://duckduckgo.com".to_string(),
            snippet: format!(
                "Please visit DuckDuckGo directly to search for: {}",
                html.split("q=")
                    .nth(1)
                    .unwrap_or("")
                    .split('&')
                    .next()
                    .unwrap_or("")
            )
            .replace("+", " "),
        });
    }

    Ok(results)
}

// Cloudflare API Response types

#[derive(Debug, Deserialize)]
struct CloudflareCrawlResponse {
    success: bool,
    errors: Option<Vec<CloudflareError>>,
    result: CloudflareCrawlJob,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct CloudflareError {
    code: i32,
    message: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct CloudflareCrawlJob {
    id: String,
    status: String,
}

#[derive(Debug, Deserialize)]
struct CloudflareCrawlStatus {
    success: bool,
    result: CloudflareCrawlResult,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct CloudflareCrawlResult {
    id: String,
    status: String,
    pages: Option<Vec<CloudflarePage>>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct CloudflarePage {
    url: String,
    title: String,
    markdown: Option<String>,
    html: Option<String>,
    structured: Option<Value>,
}

// Other API Response types

#[derive(Debug, Deserialize)]
struct BraveResponse {
    web: BraveWebResults,
}

#[derive(Debug, Deserialize)]
struct BraveWebResults {
    results: Vec<BraveResult>,
}

#[derive(Debug, Deserialize)]
struct BraveResult {
    title: String,
    url: String,
    description: String,
}

#[derive(Debug, Deserialize)]
struct SerperResponse {
    organic: Vec<SerperResult>,
}

#[derive(Debug, Deserialize)]
struct SerperResult {
    title: String,
    link: String,
    snippet: Option<String>,
}
