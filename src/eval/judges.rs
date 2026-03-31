//! Turn-level rubrics: deterministic checks, optional LLM judge, grounding, tool JSON shape.

use serde_json::Value;

use crate::llm::client::{LlmClient, LlmRequest, Message};

use super::tasks::Turn;

/// Threshold for keyword-only deterministic pass (`HSM_EVAL_KEYWORD_PASS` overrides).
pub fn deterministic_keyword_threshold() -> f64 {
    std::env::var("HSM_EVAL_KEYWORD_PASS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.45)
}

pub fn grounding_pass_threshold() -> f64 {
    std::env::var("HSM_EVAL_GROUNDING_PASS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.12)
}

/// Enable extra LLM call per turn for pass/fail (`HSM_EVAL_LLM_JUDGE=1`).
pub fn llm_judge_enabled() -> bool {
    matches_env_truthy("HSM_EVAL_LLM_JUDGE")
}

fn matches_env_truthy(key: &str) -> bool {
    std::env::var(key)
        .map(|v| {
            let s = v.trim().to_ascii_lowercase();
            s == "1" || s == "true" || s == "yes"
        })
        .unwrap_or(false)
}

#[derive(Clone, Debug, Default)]
pub struct RubricExtras {
    pub deterministic_pass: bool,
    pub grounding_applicable: bool,
    pub grounding_score: f64,
    pub grounding_pass: bool,
    pub tool_check_applicable: bool,
    pub tool_pass: Option<bool>,
    pub llm_judge_pass: Option<bool>,
    pub llm_judge_notes: Option<String>,
    pub judge_prompt_tokens: usize,
    pub judge_completion_tokens: usize,
    pub judge_llm_calls: u32,
}

pub fn evaluate_turn_rubric(
    turn: &Turn,
    response: &str,
    injected_memory_context: &str,
    keyword_score: f64,
) -> RubricExtras {
    let kw_thr = deterministic_keyword_threshold();
    let deterministic_pass = keyword_score >= kw_thr;

    let (grounding_applicable, grounding_score, grounding_pass) =
        grounding_metrics(turn.requires_recall, injected_memory_context, response);

    let (tool_check_applicable, tool_pass) = tool_metrics(turn, response);

    RubricExtras {
        deterministic_pass,
        grounding_applicable,
        grounding_score,
        grounding_pass,
        tool_check_applicable,
        tool_pass,
        ..Default::default()
    }
}

pub fn rubric_turn_pass(extras: &RubricExtras) -> bool {
    let tool_ok = extras
        .tool_pass
        .map(|p| p)
        .unwrap_or(true);
    extras.deterministic_pass && extras.grounding_pass && tool_ok
}

pub fn rubric_turn_pass_with_llm(extras: &RubricExtras) -> bool {
    if let Some(false) = extras.llm_judge_pass {
        return false;
    }
    rubric_turn_pass(extras)
}

/// Overlap of content words from injected context found in response (recall / grounding).
pub fn grounding_metrics(
    requires_recall: bool,
    injected_memory_context: &str,
    response: &str,
) -> (bool, f64, bool) {
    if !requires_recall || injected_memory_context.trim().is_empty() {
        return (false, 1.0, true);
    }
    let ctx_words = tokenize_for_overlap(injected_memory_context);
    if ctx_words.is_empty() {
        return (true, 1.0, true);
    }
    let resp_lower = response.to_lowercase();
    let hits = ctx_words
        .iter()
        .filter(|w| resp_lower.contains(*w))
        .count();
    let score = hits as f64 / ctx_words.len() as f64;
    let pass = score >= grounding_pass_threshold();
    (true, score, pass)
}

fn tokenize_for_overlap(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 3)
        .map(std::string::ToString::to_string)
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect()
}

/// Best-effort extraction of `{"tool":...,"parameters":{...}}` from model text.
pub fn parse_tool_json(text: &str) -> Option<(String, Value)> {
    let trimmed = text.trim();
    if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
        return tool_from_value(v);
    }
    // Markdown fence
    if let Some(start) = trimmed.find("```") {
        let after = &trimmed[start + 3..];
        let after = after.strip_prefix("json").unwrap_or(after).trim();
        if let Some(end) = after.find("```") {
            let inner = after[..end].trim();
            if let Ok(v) = serde_json::from_str::<Value>(inner) {
                return tool_from_value(v);
            }
        }
    }
    // Last {...} block
    if let Some(open) = trimmed.rfind('{') {
        if let Some(close) = trimmed.rfind('}') {
            if close > open {
                let slice = &trimmed[open..=close];
                if let Ok(v) = serde_json::from_str::<Value>(slice) {
                    return tool_from_value(v);
                }
            }
        }
    }
    None
}

fn tool_from_value(v: Value) -> Option<(String, Value)> {
    let obj = v.as_object()?;
    let name = obj.get("tool")?.as_str()?.to_string();
    let params = obj.get("parameters").cloned().unwrap_or(Value::Object(Default::default()));
    Some((name, params))
}

pub fn tool_metrics(turn: &Turn, response: &str) -> (bool, Option<bool>) {
    let Some(ref expected) = turn.expected_tool else {
        return (false, None);
    };
    let Some((name, params)) = parse_tool_json(response) else {
        return (true, Some(false));
    };
    if name != *expected {
        return (true, Some(false));
    }
    let obj = params.as_object();
    let Some(obj) = obj else {
        return (true, Some(false));
    };
    for key in &turn.expected_arg_keys {
        if !obj.contains_key(key) {
            return (true, Some(false));
        }
    }
    (true, Some(true))
}

pub async fn llm_judge_turn(
    client: &LlmClient,
    model: &str,
    turn: &Turn,
    response: &str,
) -> anyhow::Result<(Option<bool>, Option<String>, usize, usize, u32)> {
    if !llm_judge_enabled() {
        return Ok((None, None, 0, 0, 0u32));
    }
    let kws = turn.expected_keywords.join(", ");
    let sys = "You grade an assistant turn. Reply with ONLY compact JSON: {\"pass\":true|false,\"reason\":\"one short sentence\"}. Pass if the answer is on-topic, helpful, and covers the task; be lenient on wording.";
    let user = format!(
        "User request:\n{}\n\nAssistant answer:\n{}\n\nReference keywords (not exhaustive): {}",
        turn.user, response, kws
    );
    let req = LlmRequest {
        model: model.to_string(),
        messages: vec![Message::system(sys), Message::user(&user)],
        temperature: 0.0,
        max_tokens: Some(200),
        ..LlmRequest::default()
    };
    let resp = client.chat(req).await?;
    let parsed = parse_judge_json(&resp.content);
    Ok((
        parsed.as_ref().map(|(p, _)| *p),
        parsed.map(|(_, r)| r),
        resp.usage.prompt_tokens,
        resp.usage.completion_tokens,
        1u32,
    ))
}

fn parse_judge_json(text: &str) -> Option<(bool, String)> {
    let v: Value = serde_json::from_str(text.trim()).ok()?;
    let pass = v.get("pass")?.as_bool()?;
    let reason = v
        .get("reason")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    Some((pass, reason))
}
