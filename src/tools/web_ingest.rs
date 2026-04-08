//! Optional ingestion of successful `web_search`, `browser_get_text`, or `firecrawl_scrape` into the world model.
//!
//! Env:
//! - `HSM_WEB_INGEST=1` — `record_experience` with extractive summary of tool output
//! - `HSM_WEB_INGEST_BELIEFS=1` — also `add_belief_with_extras` (low confidence, observation source)
//! - `HSM_WEB_INGEST_MAX_CHARS` — cap stored text (default 8000)
//! - `HSM_WEB_INGEST_BELIEF_MIN_CHARS` — min body length to add a belief (default 80)

use serde_json::Value;

use crate::hyper_stigmergy::{
    AddBeliefExtras, BeliefSource, ExperienceOutcome, HyperStigmergicMorphogenesis,
};

use super::ToolOutput;

fn env_truthy(key: &str) -> bool {
    match std::env::var(key) {
        Ok(s) => {
            let t = s.trim().to_lowercase();
            matches!(t.as_str(), "1" | "true" | "yes" | "on")
        }
        Err(_) => false,
    }
}

/// Master switch: persist web tool payloads into experiences (+ optional beliefs).
pub fn web_ingest_enabled() -> bool {
    env_truthy("HSM_WEB_INGEST")
}

pub fn web_ingest_beliefs_enabled() -> bool {
    env_truthy("HSM_WEB_INGEST_BELIEFS")
}

fn max_ingest_chars() -> usize {
    std::env::var("HSM_WEB_INGEST_MAX_CHARS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8000)
        .clamp(500, 100_000)
}

fn min_belief_body_chars() -> usize {
    std::env::var("HSM_WEB_INGEST_BELIEF_MIN_CHARS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(80)
        .clamp(20, 10_000)
}

fn truncate_body(s: &str, max_chars: usize) -> String {
    let t = s.trim();
    let n = t.chars().count();
    if n <= max_chars {
        return t.to_string();
    }
    let take = max_chars.saturating_sub(24);
    let head: String = t.chars().take(take).collect();
    format!("{head}… [truncated, {n} chars total]")
}

/// Plain text body for summarization / storage.
fn extract_ingest_body(tool_name: &str, result: &str) -> String {
    if tool_name == "browser_get_text" {
        if let Ok(v) = serde_json::from_str::<Value>(result) {
            if let Some(t) = v.get("text").and_then(|x| x.as_str()) {
                return t.to_string();
            }
        }
    }
    if tool_name == "firecrawl_scrape" {
        if let Ok(v) = serde_json::from_str::<Value>(result) {
            if let Some(m) = v.get("markdown").and_then(|x| x.as_str()) {
                return m.to_string();
            }
            if let Some(m) = v
                .get("data")
                .and_then(|d| d.get("markdown"))
                .and_then(|x| x.as_str())
            {
                return m.to_string();
            }
            return serde_json::to_string_pretty(&v).unwrap_or_else(|_| result.to_string());
        }
    }
    result.to_string()
}

fn build_context(tool_name: &str, params: &Value) -> String {
    match tool_name {
        "web_search" => {
            let q = params
                .get("query")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            format!("tool=web_search query={q} ingest=HSM_WEB_INGEST")
        }
        "browser_get_text" => {
            let sel = params
                .get("selector")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let sid = params
                .get("session_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            format!(
                "tool=browser_get_text selector={} session_id={} ingest=HSM_WEB_INGEST",
                sel.trim(),
                sid.trim()
            )
        }
        "firecrawl_scrape" => {
            let u = params
                .get("url")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            format!("tool=firecrawl_scrape url={u} ingest=HSM_WEB_INGEST")
        }
        _ => format!("tool={tool_name} ingest=HSM_WEB_INGEST"),
    }
}

/// After a successful web tool, record experience and optionally a low-confidence belief.
pub fn ingest_web_tool_success(
    world: &mut HyperStigmergicMorphogenesis,
    tool_name: &str,
    params: &Value,
    output: &ToolOutput,
) {
    if !web_ingest_enabled() {
        return;
    }
    if !output.success {
        return;
    }
    if tool_name != "web_search"
        && tool_name != "browser_get_text"
        && tool_name != "firecrawl_scrape"
    {
        return;
    }

    let raw = extract_ingest_body(tool_name, &output.result);
    let capped = truncate_body(&raw, max_ingest_chars());
    if capped.trim().len() < 20 {
        return;
    }

    let (l0, l1) = crate::memory::derive_hierarchy(&capped);
    if l0.trim().is_empty() {
        return;
    }

    let description = if l1.len() > 400 {
        format!("{l0}\n\n{}", truncate_body(&l1, 3500))
    } else {
        format!("{l0}\n\n{l1}")
    };
    let context = build_context(tool_name, params);

    world.record_experience(
        &description,
        &context,
        ExperienceOutcome::Positive {
            coherence_delta: 0.12,
        },
    );

    if !web_ingest_beliefs_enabled() {
        return;
    }
    if capped.trim().len() < min_belief_body_chars() {
        return;
    }

    let mut extras = AddBeliefExtras::default();
    extras.supporting_evidence = vec![truncate_body(&capped, 2800), context.clone()];

    let belief_one_liner = if l0.chars().count() > 110 {
        let short: String = l0.chars().take(100).collect();
        format!("{short}…")
    } else {
        l0.clone()
    };

    world.add_belief_with_extras(&belief_one_liner, 0.38, BeliefSource::Observation, extras);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_browser_get_text_json() {
        let j = r#"{"ok":true,"text":"Hello world page body","session_id":"x"}"#;
        let b = extract_ingest_body("browser_get_text", j);
        assert!(b.contains("Hello world"));
    }

    #[test]
    fn extract_web_search_plain() {
        let b = extract_ingest_body("web_search", "1. Title\n   URL: https://x\n   snip");
        assert!(b.starts_with("1. Title"));
    }

    #[test]
    fn extract_firecrawl_markdown() {
        let j = serde_json::json!({
            "markdown": "Title: hello from page",
            "data": {"markdown": "Title: hello from page"}
        })
        .to_string();
        let b = extract_ingest_body("firecrawl_scrape", &j);
        assert!(b.contains("hello from page"));
    }
}
