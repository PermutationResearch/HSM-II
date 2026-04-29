//! Optional product telemetry (opt-in, policy-aligned).
//!
//! **Off by default.** When enabled, posts JSON envelopes to your `HSM_TELEMETRY_ENDPOINT`.
//! Categories mirror common AI-product privacy disclosures: account/billing context (hashed),
//! conversation (content only with explicit second flag), technical metadata, safety/abuse hooks,
//! and support/debug events.
//!
//! Configure via environment — see `.env.example` section `HSM_TELEMETRY_*`.

use chrono::{SecondsFormat, Utc};
use parking_lot::Mutex;
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

/// Policy-aligned telemetry bucket (names are stable for collectors).
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TelemetryCategory {
    /// Workspace / org identifiers (hashed), plan tier flags — only if explicitly allowed.
    AccountBilling,
    /// Prompts, completions, tool I/O — only if `HSM_TELEMETRY_INCLUDE_CONVERSATION=1`.
    Conversation,
    /// Versions, timings, feature counters, errors (no raw prompts by default).
    TechnicalUsage,
    /// Classifier outcomes, abuse reports — callers supply non-PII payloads.
    SafetyAbuse,
    /// Explicit diagnostics / support bundles.
    SupportDebug,
}

/// What the user allowed for this process (embedded in each event).
#[derive(Clone, Debug, Serialize)]
pub struct TelemetryConsent {
    pub technical_usage: bool,
    pub conversation_content: bool,
    pub account_metadata: bool,
}

#[derive(Clone)]
pub struct TelemetryClient {
    inner: Arc<Inner>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TelemetryMode {
    Minimal,
    Full,
}

struct Inner {
    enabled: bool,
    mode: TelemetryMode,
    endpoint: String,
    api_key: Option<String>,
    http: Option<reqwest::Client>,
    session_id: String,
    install_id: Option<String>,
    consent: TelemetryConsent,
    workspace_hash: Option<String>,
    binary_hint: Option<String>,
}

static INSTANCE: Mutex<Option<TelemetryClient>> = Mutex::new(None);

/// Load config from environment and store as the global client. Safe to call again after `dotenv`.
pub fn init_from_env() {
    crate::policy_config::ensure_loaded();
    let c = TelemetryClient::from_env();
    if c.is_enabled() {
        c.record_technical("hsm.session.start", json!({}));
        c.maybe_emit_daily_active_ping();
    }
    *INSTANCE.lock() = Some(c);
}

/// Clone of the configured client, or a no-op client if [`init_from_env`] was never called.
pub fn client() -> TelemetryClient {
    INSTANCE
        .lock()
        .clone()
        .unwrap_or_else(|| TelemetryClient::disabled())
}

impl TelemetryClient {
    /// Build from current `std::env` (call after loading `.env`).
    pub fn from_env() -> Self {
        let config = TelemetryConfig::from_env();
        Self::new(config)
    }

    /// Inert client (never sends).
    pub fn disabled() -> Self {
        Self {
            inner: Arc::new(Inner {
                enabled: false,
                mode: TelemetryMode::Minimal,
                endpoint: String::new(),
                api_key: None,
                http: None,
                session_id: Uuid::new_v4().to_string(),
                install_id: None,
                consent: TelemetryConsent {
                    technical_usage: false,
                    conversation_content: false,
                    account_metadata: false,
                },
                workspace_hash: None,
                binary_hint: None,
            }),
        }
    }

    fn new(config: TelemetryConfig) -> Self {
        let enabled = config.master_on && !config.endpoint.is_empty();
        let http = if enabled {
            reqwest::Client::builder()
                .timeout(Duration::from_secs(15))
                .build()
                .ok()
        } else {
            None
        };

        let workspace_hash = if config.include_account_metadata {
            std::env::var("HSM_TELEMETRY_WORKSPACE_LABEL")
                .ok()
                .filter(|s| !s.is_empty())
                .map(|s| short_hash(&s))
        } else {
            None
        };

        let binary_hint = std::env::current_exe()
            .ok()
            .and_then(|p| p.file_name()?.to_str().map(String::from));

        Self {
            inner: Arc::new(Inner {
                enabled,
                mode: config.mode,
                endpoint: config.endpoint,
                api_key: config.api_key,
                http,
                session_id: Uuid::new_v4().to_string(),
                install_id: config.install_id,
                consent: TelemetryConsent {
                    technical_usage: enabled,
                    conversation_content: config.include_conversation,
                    account_metadata: config.include_account_metadata,
                },
                workspace_hash,
                binary_hint,
            }),
        }
    }

    #[inline]
    pub fn is_enabled(&self) -> bool {
        self.inner.enabled
    }

    fn is_minimal_mode(&self) -> bool {
        self.inner.mode == TelemetryMode::Minimal
    }

    /// Record a technical / usage event (no conversation content).
    pub fn record_technical(&self, event_type: &str, payload: Value) {
        self.record(TelemetryCategory::TechnicalUsage, event_type, payload);
    }

    /// Optional account/billing context (hashed workspace only unless you extend callers).
    pub fn record_account(&self, event_type: &str, payload: Value) {
        if !self.inner.consent.account_metadata {
            return;
        }
        self.record(TelemetryCategory::AccountBilling, event_type, payload);
    }

    /// Conversation telemetry: lengths always; text only if `HSM_TELEMETRY_INCLUDE_CONVERSATION=1`.
    pub fn record_conversation_turn(
        &self,
        event_type: &str,
        user_chars: usize,
        assistant_chars: usize,
        user_text: Option<&str>,
        assistant_text: Option<&str>,
        tool_names: Option<Vec<String>>,
    ) {
        if !self.inner.enabled {
            return;
        }
        let mut payload = json!({
            "user_content_chars": user_chars,
            "assistant_content_chars": assistant_chars,
        });
        if let Some(names) = tool_names {
            payload["tool_names"] = json!(names);
        }
        if self.inner.consent.conversation_content {
            if let Some(u) = user_text {
                payload["user_content"] = json!(u);
            }
            if let Some(a) = assistant_text {
                payload["assistant_content"] = json!(a);
            }
        }
        self.record(TelemetryCategory::Conversation, event_type, payload);
    }

    /// Safety / moderation style signal (caller must avoid PII unless policy allows).
    pub fn record_safety(&self, event_type: &str, payload: Value) {
        self.record(TelemetryCategory::SafetyAbuse, event_type, payload);
    }

    /// Support or explicit diagnostic export.
    pub fn record_support(&self, event_type: &str, payload: Value) {
        self.record(TelemetryCategory::SupportDebug, event_type, payload);
    }

    fn record(&self, category: TelemetryCategory, event_type: &str, payload: Value) {
        if !self.inner.enabled {
            return;
        }
        if self.is_minimal_mode()
            && event_type != "hsm.session.start"
            && event_type != "hsm.session.end"
            && event_type != "dau.active_today"
        {
            return;
        }
        let Some(ref http) = self.inner.http else {
            return;
        };

        let account = if self.inner.consent.account_metadata {
            self.inner.workspace_hash.as_ref().map(|h| {
                json!({
                    "workspace_hash": h,
                })
            })
        } else {
            None
        };

        let envelope = TelemetryEnvelope {
            schema: "hsm.telemetry.v1",
            category,
            event_type: event_type.to_string(),
            timestamp_rfc3339: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            session_id: self.inner.session_id.clone(),
            install_id: self.inner.install_id.clone(),
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            binary: self.inner.binary_hint.clone(),
            consent: self.inner.consent.clone(),
            account,
            payload,
        };

        let Ok(body) = serde_json::to_value(&envelope) else {
            return;
        };

        let url = self.inner.endpoint.clone();
        let key = self.inner.api_key.clone();
        let http = http.clone();

        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                post_json(&http, &url, key.as_deref(), body).await;
            });
        } else {
            tracing::debug!(target: "hsm.telemetry", "skip event (no tokio runtime): {}", event_type);
        }
    }

    fn maybe_emit_daily_active_ping(&self) {
        if !self.inner.enabled {
            return;
        }
        let Some(state_path) = telemetry_state_path() else {
            return;
        };
        let today = Utc::now().format("%Y-%m-%d").to_string();
        let mut state = load_state(&state_path).unwrap_or_default();
        let install_uuid = self
            .inner
            .install_id
            .clone()
            .or_else(|| state.install_uuid.clone())
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let should_send = state.last_dau_date.as_deref() != Some(today.as_str());
        if should_send {
            self.record_technical("dau.active_today", json!({ "uuid": install_uuid }));
            state.last_dau_date = Some(today);
        }
        state.install_uuid = Some(install_uuid);
        let _ = save_state(&state_path, &state);
    }
}

/// Hold this for your process/session lifetime to emit `hsm.session.end` on drop.
pub struct SessionTelemetryGuard {
    _inner: TelemetryLifecycleGuard,
}

/// Start session lifecycle guard for the currently configured telemetry client.
pub fn start_session_guard() -> Option<SessionTelemetryGuard> {
    let c = client();
    if !c.is_enabled() {
        return None;
    }
    Some(SessionTelemetryGuard {
        _inner: TelemetryLifecycleGuard::new(c),
    })
}

struct TelemetryLifecycleGuard {
    client: TelemetryClient,
    started_at: std::time::Instant,
}

impl TelemetryLifecycleGuard {
    fn new(client: TelemetryClient) -> Self {
        Self {
            client,
            started_at: std::time::Instant::now(),
        }
    }
}

impl Drop for TelemetryLifecycleGuard {
    fn drop(&mut self) {
        if !self.client.is_enabled() {
            return;
        }
        let duration_ms = self.started_at.elapsed().as_millis() as u64;
        self.client
            .record_technical("hsm.session.end", json!({ "duration_ms": duration_ms }));
    }
}

#[derive(Serialize)]
struct TelemetryEnvelope {
    schema: &'static str,
    category: TelemetryCategory,
    event_type: String,
    timestamp_rfc3339: String,
    session_id: String,
    install_id: Option<String>,
    app_version: String,
    os: String,
    arch: String,
    binary: Option<String>,
    consent: TelemetryConsent,
    #[serde(skip_serializing_if = "Option::is_none")]
    account: Option<Value>,
    payload: Value,
}

async fn post_json(client: &reqwest::Client, url: &str, bearer: Option<&str>, body: Value) {
    let mut req = client.post(url).json(&body);
    if let Some(t) = bearer {
        req = req.bearer_auth(t);
    }
    match req.send().await {
        Ok(resp) if resp.status().is_success() => {}
        Ok(resp) => {
            tracing::debug!(
                target: "hsm.telemetry",
                status = %resp.status(),
                "telemetry endpoint returned non-success"
            );
        }
        Err(e) => {
            tracing::debug!(target: "hsm.telemetry", error = %e, "telemetry POST failed");
        }
    }
}

fn env_truthy(key: &str) -> bool {
    match std::env::var(key) {
        Ok(s) => {
            matches!(s.to_lowercase().as_str(), "1" | "true" | "yes" | "on")
        }
        Err(_) => false,
    }
}

fn short_hash(input: &str) -> String {
    let d = Sha256::digest(input.as_bytes());
    d.iter().take(8).map(|b| format!("{:02x}", b)).collect()
}

/// Parsed telemetry environment (for testing / inspection).
#[derive(Clone, Debug)]
pub struct TelemetryConfig {
    pub master_on: bool,
    mode: TelemetryMode,
    pub endpoint: String,
    pub api_key: Option<String>,
    pub include_conversation: bool,
    pub include_account_metadata: bool,
    pub install_id: Option<String>,
}

impl TelemetryConfig {
    pub fn from_env() -> Self {
        let mode = match std::env::var("HSM_TELEMETRY_MODE")
            .unwrap_or_else(|_| "minimal".to_string())
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "full" => TelemetryMode::Full,
            _ => TelemetryMode::Minimal,
        };
        Self {
            master_on: env_truthy("HSM_TELEMETRY"),
            mode,
            endpoint: std::env::var("HSM_TELEMETRY_ENDPOINT")
                .unwrap_or_default()
                .trim()
                .to_string(),
            api_key: std::env::var("HSM_TELEMETRY_KEY")
                .ok()
                .filter(|s| !s.is_empty()),
            include_conversation: env_truthy("HSM_TELEMETRY_INCLUDE_CONVERSATION"),
            include_account_metadata: env_truthy("HSM_TELEMETRY_INCLUDE_ACCOUNT_METADATA"),
            install_id: std::env::var("HSM_TELEMETRY_INSTALL_ID")
                .ok()
                .filter(|s| !s.is_empty()),
        }
    }
}

#[derive(Default, serde::Deserialize, serde::Serialize)]
struct TelemetryState {
    install_uuid: Option<String>,
    last_dau_date: Option<String>,
}

fn telemetry_state_path() -> Option<PathBuf> {
    if let Ok(home) = std::env::var("HSMII_HOME") {
        let p = PathBuf::from(home);
        if !p.as_os_str().is_empty() {
            return Some(p.join("state").join("telemetry_state.json"));
        }
    }
    let home = std::env::var("HOME").ok()?;
    let p = PathBuf::from(home).join(".hsmii").join("state").join("telemetry_state.json");
    Some(p)
}

fn load_state(path: &PathBuf) -> Option<TelemetryState> {
    let raw = fs::read_to_string(path).ok()?;
    serde_json::from_str::<TelemetryState>(&raw).ok()
}

fn save_state(path: &PathBuf, state: &TelemetryState) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let body = serde_json::to_string_pretty(state).map_err(|e| e.to_string())?;
    fs::write(path, body).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_client_no_panic() {
        let c = TelemetryClient::disabled();
        c.record_technical("test", json!({}));
        assert!(!c.is_enabled());
    }

    #[test]
    fn short_hash_stable() {
        assert_eq!(short_hash("workspace-a"), short_hash("workspace-a"));
        assert_ne!(short_hash("a"), short_hash("b"));
    }
}
