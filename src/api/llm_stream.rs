//! SSE streaming chat — `POST /api/llm/chat/stream` (OpenAI-compatible request body).

use std::convert::Infallible;

use axum::{
    extract::State,
    http::StatusCode,
    response::sse::{Event, KeepAlive, Sse},
    Json,
};
use futures_util::StreamExt;
use serde::Deserialize;

use super::ApiState;
use crate::llm::{LlmClient, LlmRequest, LlmStreamEvent, Message};

#[derive(Deserialize)]
pub struct ChatStreamBody {
    #[serde(default)]
    pub model: Option<String>,
    pub messages: Vec<Message>,
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub max_tokens: Option<usize>,
    #[serde(default)]
    pub top_p: Option<f64>,
}

fn provider_label(p: &crate::llm::LlmProvider) -> &'static str {
    match p {
        crate::llm::LlmProvider::OpenAi => "openai_compat",
        crate::llm::LlmProvider::Anthropic => "anthropic",
        crate::llm::LlmProvider::Ollama => "ollama",
    }
}

pub async fn llm_chat_stream(
    State(_state): State<ApiState>,
    Json(body): Json<ChatStreamBody>,
) -> Result<Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>>, (StatusCode, String)>
{
    if body.messages.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "messages must be non-empty".to_string(),
        ));
    }

    let client = LlmClient::new().map_err(|e| (StatusCode::SERVICE_UNAVAILABLE, e.to_string()))?;

    let model = body
        .model
        .clone()
        .unwrap_or_else(|| client.default_model().to_string());

    let req = LlmRequest {
        model,
        messages: body.messages,
        temperature: body.temperature.unwrap_or(0.7),
        max_tokens: body.max_tokens.or(Some(2000)),
        top_p: body.top_p.or(Some(0.9)),
        stream: true,
    };

    let upstream = client
        .chat_stream(req)
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, e.to_string()))?;

    let s = upstream.map(|item| {
        let payload = match item {
            Ok(LlmStreamEvent::Delta { text }) => serde_json::json!({
                "type": "delta",
                "text": text,
            }),
            Ok(LlmStreamEvent::Done {
                model,
                usage,
                provider,
            }) => serde_json::json!({
                "type": "done",
                "model": model,
                "usage": usage,
                "provider": provider_label(&provider),
            }),
            Err(e) => serde_json::json!({
                "type": "error",
                "message": e.to_string(),
            }),
        };
        let data =
            serde_json::to_string(&payload).unwrap_or_else(|_| "{\"type\":\"error\"}".into());
        Ok::<_, Infallible>(Event::default().data(data))
    });

    Ok(Sse::new(s).keep_alive(KeepAlive::default()))
}
