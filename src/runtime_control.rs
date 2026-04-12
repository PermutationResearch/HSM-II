use serde::Serialize;
use serde_json::Value;
use std::sync::{OnceLock, RwLock};
use tokio::sync::broadcast;

#[derive(Clone, Debug, Serialize)]
pub struct RuntimeActivitySnapshot {
    pub last_activity_ms: i64,
    pub tool_name: Option<String>,
    pub call_id: Option<String>,
    pub phase: String,
}

#[derive(Clone, Debug)]
struct RuntimeActivityState {
    last_activity_ms: i64,
    tool_name: Option<String>,
    call_id: Option<String>,
    phase: String,
}

impl Default for RuntimeActivityState {
    fn default() -> Self {
        Self {
            last_activity_ms: chrono::Utc::now().timestamp_millis(),
            tool_name: None,
            call_id: None,
            phase: "idle".to_string(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct CompletionEvent {
    pub event_type: String,
    pub task_key: Option<String>,
    pub tool_name: Option<String>,
    pub call_id: Option<String>,
    pub success: bool,
    pub message: String,
    pub ts_ms: i64,
    /// Structured tool input payload for `tool_start` notifications.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<Value>,
    /// When `event_type` is `stream_event`, carries an Anthropic Messages API stream
    /// event object (e.g. `content_block_delta` with `text_delta`). Company Console
    /// NDJSON mirrors these as top-level `{ "type": "stream_event", "event": … }`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_event: Option<Value>,
}

impl CompletionEvent {
    pub fn tool_start(tool_name: &str, call_id: &str, message: String) -> Self {
        Self {
            event_type: "tool_start".to_string(),
            task_key: None,
            tool_name: Some(tool_name.to_string()),
            call_id: Some(call_id.to_string()),
            success: true,
            message,
            ts_ms: chrono::Utc::now().timestamp_millis(),
            input: None,
            stream_event: None,
        }
    }

    pub fn tool_start_with_input(
        tool_name: &str,
        call_id: &str,
        input: Value,
        message: String,
    ) -> Self {
        Self {
            event_type: "tool_start".to_string(),
            task_key: None,
            tool_name: Some(tool_name.to_string()),
            call_id: Some(call_id.to_string()),
            success: true,
            message,
            ts_ms: chrono::Utc::now().timestamp_millis(),
            input: Some(input),
            stream_event: None,
        }
    }

    /// Provider-agnostic incremental tool-input preview event emitted before execution starts.
    pub fn tool_start_delta(tool_name: &str, call_id: &str, input: Value, message: String) -> Self {
        Self {
            event_type: "tool_start_delta".to_string(),
            task_key: None,
            tool_name: Some(tool_name.to_string()),
            call_id: Some(call_id.to_string()),
            success: true,
            message,
            ts_ms: chrono::Utc::now().timestamp_millis(),
            input: Some(input),
            stream_event: None,
        }
    }

    pub fn tool_error(tool_name: &str, call_id: &str, message: String) -> Self {
        Self {
            event_type: "tool_error".to_string(),
            task_key: None,
            tool_name: Some(tool_name.to_string()),
            call_id: Some(call_id.to_string()),
            success: false,
            message,
            ts_ms: chrono::Utc::now().timestamp_millis(),
            input: None,
            stream_event: None,
        }
    }

    pub fn tool_completion(tool_name: &str, call_id: &str, success: bool, message: String) -> Self {
        Self {
            event_type: "tool_complete".to_string(),
            task_key: None,
            tool_name: Some(tool_name.to_string()),
            call_id: Some(call_id.to_string()),
            success,
            message,
            ts_ms: chrono::Utc::now().timestamp_millis(),
            input: None,
            stream_event: None,
        }
    }

    pub fn background_completion(task_key: &str, success: bool, message: String) -> Self {
        Self {
            event_type: "background_completion".to_string(),
            task_key: Some(task_key.to_string()),
            tool_name: None,
            call_id: None,
            success,
            message,
            ts_ms: chrono::Utc::now().timestamp_millis(),
            input: None,
            stream_event: None,
        }
    }

    /// Publish a Claude-compatible partial stream event for operator UIs.
    pub fn anthropic_stream_event(event: Value) -> Self {
        Self {
            event_type: "stream_event".to_string(),
            task_key: None,
            tool_name: None,
            call_id: None,
            success: true,
            message: String::new(),
            ts_ms: chrono::Utc::now().timestamp_millis(),
            input: None,
            stream_event: Some(event),
        }
    }
}

static ACTIVITY: OnceLock<RwLock<RuntimeActivityState>> = OnceLock::new();
static EVENTS: OnceLock<broadcast::Sender<CompletionEvent>> = OnceLock::new();
static CALLBACK_URL: OnceLock<Option<String>> = OnceLock::new();

fn activity_state() -> &'static RwLock<RuntimeActivityState> {
    ACTIVITY.get_or_init(|| RwLock::new(RuntimeActivityState::default()))
}

fn event_sender() -> &'static broadcast::Sender<CompletionEvent> {
    EVENTS.get_or_init(|| {
        let (tx, _rx) = broadcast::channel(1024);
        tx
    })
}

fn callback_url() -> Option<&'static str> {
    CALLBACK_URL
        .get_or_init(|| {
            std::env::var("HSM_COMPLETION_CALLBACK_URL")
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
        .as_deref()
}

pub fn mark_tool_activity(tool_name: &str, call_id: &str, phase: &str) {
    if let Ok(mut g) = activity_state().write() {
        g.last_activity_ms = chrono::Utc::now().timestamp_millis();
        g.tool_name = Some(tool_name.to_string());
        g.call_id = Some(call_id.to_string());
        g.phase = phase.to_string();
    }
}

pub fn mark_runtime_idle() {
    if let Ok(mut g) = activity_state().write() {
        g.last_activity_ms = chrono::Utc::now().timestamp_millis();
        g.phase = "idle".to_string();
    }
}

pub fn activity_snapshot() -> RuntimeActivitySnapshot {
    if let Ok(g) = activity_state().read() {
        RuntimeActivitySnapshot {
            last_activity_ms: g.last_activity_ms,
            tool_name: g.tool_name.clone(),
            call_id: g.call_id.clone(),
            phase: g.phase.clone(),
        }
    } else {
        RuntimeActivitySnapshot {
            last_activity_ms: chrono::Utc::now().timestamp_millis(),
            tool_name: None,
            call_id: None,
            phase: "unknown".to_string(),
        }
    }
}

pub fn idle_for_ms() -> i64 {
    let now = chrono::Utc::now().timestamp_millis();
    let last = activity_snapshot().last_activity_ms;
    now.saturating_sub(last)
}

pub fn is_truly_idle(timeout_ms: i64) -> bool {
    let snap = activity_snapshot();
    snap.phase == "idle" && chrono::Utc::now().timestamp_millis().saturating_sub(snap.last_activity_ms) > timeout_ms
}

pub fn subscribe_completions() -> broadcast::Receiver<CompletionEvent> {
    event_sender().subscribe()
}

pub fn publish_completion(event: CompletionEvent) {
    let _ = event_sender().send(event.clone());
    if let Some(url) = callback_url() {
        let url = url.to_string();
        tokio::spawn(async move {
            let client = reqwest::Client::new();
            let _ = client.post(url).json(&event).send().await;
        });
    }
}
