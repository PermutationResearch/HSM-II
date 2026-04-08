//! Provider streaming parsers (SSE + Ollama NDJSON) for [`super::client::LlmClient::chat_stream`].

use std::pin::Pin;

use async_stream::try_stream;
use futures_util::Stream;
use futures_util::StreamExt;
use serde_json::Value;

use super::client::{LlmProvider, Usage};

/// One chunk from an upstream streaming chat completion.
#[derive(Clone, Debug)]
pub enum LlmStreamEvent {
    Delta {
        text: String,
    },
    Done {
        model: String,
        usage: Option<Usage>,
        provider: LlmProvider,
    },
}

fn take_line(buf: &mut String) -> Option<String> {
    if let Some(i) = buf.find('\n') {
        let line = buf[..i].to_string();
        buf.drain(..=i);
        Some(line)
    } else {
        None
    }
}

fn usage_from_openai_json(u: &Value) -> Usage {
    Usage {
        prompt_tokens: u["prompt_tokens"].as_u64().unwrap_or(0) as usize,
        completion_tokens: u["completion_tokens"].as_u64().unwrap_or(0) as usize,
        total_tokens: u["total_tokens"].as_u64().unwrap_or(0) as usize,
    }
}

/// Ollama may send either incremental tokens or growing cumulative `message.content`; this returns
/// only the new suffix and keeps `prev_total` in sync for the next chunk.
fn ollama_content_delta(prev_total: &mut String, current: &str) -> Option<String> {
    if current.is_empty() {
        return None;
    }
    let delta = if current.starts_with(prev_total.as_str()) && current.len() >= prev_total.len() {
        current[prev_total.len()..].to_string()
    } else {
        current.to_string()
    };
    if current.starts_with(prev_total.as_str()) && current.len() >= prev_total.len() {
        prev_total.clear();
        prev_total.push_str(current);
    } else {
        prev_total.push_str(current);
    }
    if delta.is_empty() {
        None
    } else {
        Some(delta)
    }
}

fn usage_from_anthropic_json(u: &Value) -> Usage {
    let in_t = u["input_tokens"].as_u64().unwrap_or(0) as usize;
    let out_t = u["output_tokens"].as_u64().unwrap_or(0) as usize;
    Usage {
        prompt_tokens: in_t,
        completion_tokens: out_t,
        total_tokens: in_t + out_t,
    }
}

/// OpenAI / OpenRouter-compatible `data: {json}` SSE.
pub fn openai_sse_stream(
    response: reqwest::Response,
    provider: LlmProvider,
) -> Pin<Box<dyn Stream<Item = anyhow::Result<LlmStreamEvent>> + Send>> {
    Box::pin(try_stream! {
        let mut buf = String::new();
        let mut bytes = response.bytes_stream();
        let mut model = String::new();
        let mut last_usage: Option<Usage> = None;

        while let Some(chunk) = bytes.next().await {
            let chunk = chunk.map_err(anyhow::Error::from)?;
            buf.push_str(&String::from_utf8_lossy(&chunk));
            while let Some(line) = take_line(&mut buf) {
                let t = line.trim_end_matches('\r').trim();
                if t.is_empty() || t.starts_with(':') {
                    continue;
                }
                let rest = if let Some(r) = t.strip_prefix("data:") {
                    r.trim()
                } else {
                    continue;
                };
                if rest == "[DONE]" {
                    yield LlmStreamEvent::Done {
                        model: model.clone(),
                        usage: last_usage.clone(),
                        provider,
                    };
                    return;
                }
                let v: Value = serde_json::from_str(rest)?;
                if model.is_empty() {
                    if let Some(m) = v["model"].as_str() {
                        model = m.to_string();
                    }
                }
                if let Some(u) = v.get("usage") {
                    if !u.is_null() {
                        last_usage = Some(usage_from_openai_json(u));
                    }
                }
                if let Some(s) = v["choices"]
                    .get(0)
                    .and_then(|c| c["delta"]["content"].as_str())
                {
                    if !s.is_empty() {
                        yield LlmStreamEvent::Delta {
                            text: s.to_string(),
                        };
                    }
                }
            }
        }
        yield LlmStreamEvent::Done {
            model,
            usage: last_usage,
            provider,
        };
    })
}

/// Anthropic messages SSE (`event:` + `data:` lines).
pub fn anthropic_sse_stream(
    response: reqwest::Response,
) -> Pin<Box<dyn Stream<Item = anyhow::Result<LlmStreamEvent>> + Send>> {
    Box::pin(try_stream! {
        let mut buf = String::new();
        let mut bytes = response.bytes_stream();
        let mut model = String::new();
        let mut last_usage: Option<Usage> = None;
        let mut pending_event: Option<String> = None;

        while let Some(chunk) = bytes.next().await {
            let chunk = chunk.map_err(anyhow::Error::from)?;
            buf.push_str(&String::from_utf8_lossy(&chunk));
            while let Some(line) = take_line(&mut buf) {
                let t = line.trim_end_matches('\r').trim();
                if t.is_empty() {
                    continue;
                }
                if let Some(ev) = t.strip_prefix("event:") {
                    pending_event = Some(ev.trim().to_string());
                    continue;
                }
                if let Some(d) = t.strip_prefix("data:") {
                    let data = d.trim();
                    let v: Value = serde_json::from_str(data)?;
                    let ev = pending_event.take();
                    if v["type"].as_str() == Some("content_block_delta")
                        || matches!(ev.as_deref(), Some("content_block_delta"))
                    {
                        let delta = &v["delta"];
                        if let Some(text) = delta["text"].as_str() {
                            if !text.is_empty() {
                                yield LlmStreamEvent::Delta {
                                    text: text.to_string(),
                                };
                            }
                        }
                    } else {
                        match ev.as_deref() {
                            Some("message_start") => {
                                if let Some(m) = v["message"]["model"].as_str() {
                                    model = m.to_string();
                                }
                            }
                            Some("message_delta") => {
                                if let Some(u) = v.get("usage") {
                                    last_usage = Some(usage_from_anthropic_json(u));
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        yield LlmStreamEvent::Done {
            model,
            usage: last_usage,
            provider: LlmProvider::Anthropic,
        };
    })
}

/// Ollama `/api/chat` with `stream: true` — newline-delimited JSON.
pub fn ollama_ndjson_stream(
    response: reqwest::Response,
) -> Pin<Box<dyn Stream<Item = anyhow::Result<LlmStreamEvent>> + Send>> {
    Box::pin(try_stream! {
        let mut buf = String::new();
        let mut bytes = response.bytes_stream();
        let mut model = String::new();
        let mut ollama_prev_content = String::new();

        while let Some(chunk) = bytes.next().await {
            let chunk = chunk.map_err(anyhow::Error::from)?;
            buf.push_str(&String::from_utf8_lossy(&chunk));
            while let Some(line) = take_line(&mut buf) {
                let t = line.trim_end_matches('\r').trim();
                if t.is_empty() {
                    continue;
                }
                let v: Value = serde_json::from_str(t)?;
                if model.is_empty() {
                    if let Some(m) = v["model"].as_str() {
                        model = m.to_string();
                    }
                }
                if let Some(s) = v["message"]["content"].as_str() {
                    if let Some(d) = ollama_content_delta(&mut ollama_prev_content, s) {
                        yield LlmStreamEvent::Delta { text: d };
                    }
                }
                if v["done"].as_bool() == Some(true) {
                    yield LlmStreamEvent::Done {
                        model: model.clone(),
                        usage: None,
                        provider: LlmProvider::Ollama,
                    };
                    return;
                }
            }
        }
        yield LlmStreamEvent::Done {
            model,
            usage: None,
            provider: LlmProvider::Ollama,
        };
    })
}

#[cfg(test)]
mod tests {
    use super::ollama_content_delta;

    #[test]
    fn ollama_incremental_chunks() {
        let mut p = String::new();
        assert_eq!(ollama_content_delta(&mut p, "He").as_deref(), Some("He"));
        assert_eq!(p, "He");
        assert_eq!(ollama_content_delta(&mut p, "llo").as_deref(), Some("llo"));
        assert_eq!(p, "Hello");
    }

    #[test]
    fn ollama_cumulative_chunks() {
        let mut p = String::new();
        assert_eq!(ollama_content_delta(&mut p, "He").as_deref(), Some("He"));
        assert_eq!(
            ollama_content_delta(&mut p, "Hello").as_deref(),
            Some("llo")
        );
        assert_eq!(p, "Hello");
    }

    #[test]
    fn ollama_single_shot() {
        let mut p = String::new();
        assert_eq!(
            ollama_content_delta(&mut p, "Hello").as_deref(),
            Some("Hello")
        );
        assert_eq!(ollama_content_delta(&mut p, "").as_deref(), None);
    }
}
