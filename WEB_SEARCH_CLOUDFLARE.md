# Web Search with Cloudflare Browser Rendering

The web search tool now supports Cloudflare's **Browser Rendering API** (`/crawl` endpoint) as the preferred backend.

## Cloudflare Crawl Endpoint

Cloudflare's Browser Rendering API provides:
- **Full page crawling** with headless browser rendering
- **Multiple output formats**: HTML, Markdown, structured JSON
- **Respects robots.txt** and AI Crawl Control
- **Incremental crawling** with `modifiedSince` support
- **Async job processing** (poll for results)

## Setup

Set these environment variables:

```bash
export CF_ACCOUNT_ID=your_account_id
export CF_API_TOKEN=your_api_token
```

Get your credentials from:
- Account ID: Cloudflare Dashboard → right sidebar
- API Token: Cloudflare Dashboard → My Profile → API Tokens

## Usage

### Direct URL Crawl

```rust
let call = ToolCall {
    name: "web_search".to_string(),
    parameters: json!({
        "query": "https://blog.cloudflare.com/",
        "num_results": 5
    }),
    call_id: "1".to_string(),
};
```

### Search-to-Crawl (Auto)

If you provide a search query (not a URL), the tool will:
1. Search with DuckDuckGo to find relevant URLs
2. Crawl the top result with Cloudflare

```rust
let call = ToolCall {
    name: "web_search".to_string(),
    parameters: json!({
        "query": "Rust programming language tutorials",
        "num_results": 3
    }),
    call_id: "2".to_string(),
};
```

## Backend Priority

The tool automatically selects the best available backend:

1. **Cloudflare Crawl** (if `CF_ACCOUNT_ID` + `CF_API_TOKEN` set)
2. **Brave Search** (if `BRAVE_API_KEY` set)
3. **Serper** (if `SERPER_API_KEY` set)
4. **DuckDuckGo** (fallback, no key required)

## Response Format

Cloudflare crawl returns:

```json
{
  "success": true,
  "result": {
    "pages": [
      {
        "url": "https://example.com/page",
        "title": "Page Title",
        "markdown": "# Heading\nContent...",
        "structured": { /* extracted data */ }
      }
    ]
  }
}
```

## API Reference

- Endpoint: `https://api.cloudflare.com/client/v4/accounts/{account_id}/browser-rendering/crawl`
- Docs: https://developers.cloudflare.com/browser-rendering/
- Changelog: https://developers.cloudflare.com/changelog/post/2026-03-10-br-crawl-endpoint/

## Fallback Behavior

If Cloudflare crawl fails:
1. Error is logged
2. Tool returns error message
3. User can retry with explicit URL or different backend

## Comparison with Other Backends

| Feature | Cloudflare | Brave | Serper | DuckDuckGo |
|---------|------------|-------|--------|------------|
| Renders JS | ✅ | ❌ | ❌ | ❌ |
| Returns Markdown | ✅ | ❌ | ❌ | ❌ |
| Structured data | ✅ | ❌ | ❌ | ❌ |
| Respects robots.txt | ✅ | N/A | N/A | ✅ |
| Free tier | ✅ | Limited | Paid | ✅ |
| API Key required | ✅ | ✅ | ✅ | ❌ |
