//! Prompt → scored tool choice → [`ToolPermissionContext::check`] → [`ToolRegistry::execute`].
//!
//! Ranks registered tools by **keyword overlap** between the user prompt and each tool’s OpenAI-style
//! function schema (`name`, `description`, `parameters.properties` keys and descriptions).
//! The winner is executed with **caller-supplied** parameters (e.g. `{}` or a map from a separate
//! parameter-extraction step). Permission denial is handled inside [`ToolRegistry::execute`].

use serde_json::Value;

use super::{ToolCall, ToolCallResult, ToolRegistry};

/// One tool with its keyword score (higher is a better match).
#[derive(Clone, Debug, PartialEq)]
pub struct ScoredTool {
    pub name: String,
    pub score: f64,
}

/// No registered tool met [`ScoredRouteConfig::min_score`].
#[derive(Clone, Debug)]
pub struct ScoredRouteError {
    pub reason: ScoredRouteFailReason,
    /// Top candidates for logging / UI (best first, capped).
    pub ranked: Vec<ScoredTool>,
}

#[derive(Clone, Debug)]
pub enum ScoredRouteFailReason {
    NoToolsRegistered,
    BelowThreshold,
}

#[derive(Clone, Debug)]
pub struct ScoredRouteConfig {
    /// Minimum keyword score for a tool to be selected.
    pub min_score: f64,
    /// Maximum tools returned from [`rank_tools_for_prompt`].
    pub rank_cap: usize,
}

impl Default for ScoredRouteConfig {
    fn default() -> Self {
        Self {
            min_score: 1.0,
            rank_cap: 64,
        }
    }
}

fn tokenize_prompt(prompt: &str) -> Vec<String> {
    let lower = prompt.to_lowercase();
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in lower.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            cur.push(c);
        } else if !cur.is_empty() {
            if cur.len() >= 2 {
                out.push(cur.clone());
            }
            cur.clear();
        }
    }
    if cur.len() >= 2 {
        out.push(cur);
    }
    out
}

fn build_schema_corpus(name: &str, description: &str, parameters: &Value) -> String {
    let mut s = format!("{name} {description} ");
    append_parameter_corpus(parameters, &mut s);
    s.make_ascii_lowercase();
    s
}

fn append_parameter_corpus(v: &Value, buf: &mut String) {
    let Some(obj) = v.as_object() else {
        return;
    };
    if let Some(props) = obj.get("properties").and_then(|p| p.as_object()) {
        for (key, prop) in props {
            buf.push_str(key);
            buf.push(' ');
            if let Some(desc) = prop.get("description").and_then(|x| x.as_str()) {
                buf.push_str(desc);
                buf.push(' ');
            }
            append_parameter_corpus(prop, buf);
        }
    }
}

fn keyword_score(tokens: &[String], tool_name: &str, corpus: &str) -> f64 {
    if tokens.is_empty() {
        return 0.0;
    }
    let name_l = tool_name.to_lowercase();
    let mut score = 0.0;
    for t in tokens {
        if name_l.contains(t.as_str()) {
            score += 3.0;
        } else if corpus.contains(t.as_str()) {
            score += 1.0;
        }
    }
    score
}

/// Score every tool in `registry` against `prompt` using registered function schemas.
pub fn rank_tools_for_prompt(registry: &ToolRegistry, prompt: &str, rank_cap: usize) -> Vec<ScoredTool> {
    let tokens = tokenize_prompt(prompt);
    let cap = rank_cap.max(1);
    let mut ranked: Vec<ScoredTool> = Vec::new();

    for schema in registry.get_schemas() {
        let Some(func) = schema.get("function") else {
            continue;
        };
        let Some(name) = func.get("name").and_then(|x| x.as_str()) else {
            continue;
        };
        let description = func
            .get("description")
            .and_then(|x| x.as_str())
            .unwrap_or("");
        let parameters = func.get("parameters").cloned().unwrap_or_else(|| Value::Object(Default::default()));
        let corpus = build_schema_corpus(name, description, &parameters);
        let score = keyword_score(&tokens, name, &corpus);
        ranked.push(ScoredTool {
            name: name.to_string(),
            score,
        });
    }

    ranked.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.name.cmp(&b.name))
    });
    ranked.truncate(cap);
    ranked
}

/// Best-scoring tool if `score >= min_score`.
pub fn pick_tool_for_prompt(registry: &ToolRegistry, prompt: &str, min_score: f64) -> Option<ScoredTool> {
    let ranked = rank_tools_for_prompt(registry, prompt, 1);
    ranked.into_iter().next().filter(|t| t.score >= min_score)
}

/// Select a tool from `prompt`, then [`ToolRegistry::execute`] (permission check + run) with `parameters`.
pub async fn route_prompt_execute(
    registry: &mut ToolRegistry,
    prompt: &str,
    parameters: Value,
    config: ScoredRouteConfig,
) -> Result<ToolCallResult, ScoredRouteError> {
    let ranked = rank_tools_for_prompt(registry, prompt, config.rank_cap);
    if ranked.is_empty() {
        return Err(ScoredRouteError {
            reason: ScoredRouteFailReason::NoToolsRegistered,
            ranked,
        });
    }
    let best = ranked[0].clone();
    if best.score < config.min_score {
        return Err(ScoredRouteError {
            reason: ScoredRouteFailReason::BelowThreshold,
            ranked: ranked.into_iter().take(8).collect(),
        });
    }
    let call_id = format!("scored-{}", uuid::Uuid::new_v4());
    let call = ToolCall {
        name: best.name,
        parameters,
        call_id,
    };
    Ok(registry.execute(call).await)
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use serde_json::json;
    use std::sync::Arc;

    use super::*;
    use crate::tools::{Tool, ToolOutput, ToolRegistry, tool_permissions::ToolPermissionContext};

    struct DummyTool {
        name: &'static str,
        desc: &'static str,
        schema: Value,
    }

    #[async_trait]
    impl Tool for DummyTool {
        fn name(&self) -> &str {
            self.name
        }
        fn description(&self) -> &str {
            self.desc
        }
        fn parameters_schema(&self) -> Value {
            self.schema.clone()
        }
        async fn execute(&self, _params: Value) -> ToolOutput {
            ToolOutput::success("ok")
        }
    }

    #[test]
    fn ranks_web_search_higher_for_search_prompt() {
        let mut reg = ToolRegistry::new_with_permissions(ToolPermissionContext::permissive());
        reg.register(Arc::new(DummyTool {
            name: "read_file",
            desc: "Read a file from disk",
            schema: json!({"type":"object","properties":{"path":{"type":"string","description":"file path"}}}),
        }));
        reg.register(Arc::new(DummyTool {
            name: "web_search",
            desc: "Search the public web for information",
            schema: json!({"type":"object","properties":{"query":{"type":"string","description":"search query"}}}),
        }));

        let ranked = rank_tools_for_prompt(&reg, "Please search the web for rust async tutorial", 8);
        assert_eq!(ranked[0].name, "web_search");
        assert!(ranked[0].score >= ranked.get(1).map(|x| x.score).unwrap_or(0.0));
    }

    #[tokio::test]
    async fn execute_respects_firewall() {
        let mut reg = ToolRegistry::new_with_permissions(ToolPermissionContext::with_blocked_prefixes([
            "web_",
        ]));
        reg.register(Arc::new(DummyTool {
            name: "web_search",
            desc: "Search the web",
            schema: json!({"type":"object","properties":{}}),
        }));
        reg.register(Arc::new(DummyTool {
            name: "read_file",
            desc: "Read files",
            schema: json!({"type":"object","properties":{}}),
        }));

        let res = route_prompt_execute(
            &mut reg,
            "search the web for news",
            json!({}),
            ScoredRouteConfig {
                min_score: 0.5,
                rank_cap: 8,
            },
        )
        .await
        .expect("route should still return a result");

        assert_eq!(res.call.name, "web_search");
        assert!(!res.output.success);
        let err = res.output.error.unwrap_or_default();
        assert!(
            err.contains("blocked") || err.contains("policy"),
            "expected firewall denial, got {err:?}"
        );
    }

    #[tokio::test]
    async fn below_threshold_errors() {
        let mut reg = ToolRegistry::new_with_permissions(ToolPermissionContext::permissive());
        reg.register(Arc::new(DummyTool {
            name: "alpha",
            desc: "alpha tool",
            schema: json!({"type":"object","properties":{}}),
        }));
        let err = route_prompt_execute(
            &mut reg,
            "qqqqqqq",
            json!({}),
            ScoredRouteConfig {
                min_score: 100.0,
                rank_cap: 4,
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(err.reason, ScoredRouteFailReason::BelowThreshold));
    }
}
