//! Gateway System - Multi-platform messaging like Hermes
//!
//! Connects HSM-II to Discord, Telegram, Slack, WhatsApp

use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use regex::Regex;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::OnceLock;

use crate::gateways::{
    DiscordConfig, MatrixConfig, RealDiscordBot, RealMatrixBot, RealSignalBot, RealTelegramBot,
    SignalConfig, TelegramConfig,
};
use crate::personal::PairingStore;

/// Unified gateway for all platforms
pub struct Gateway {
    config: Config,
    /// Platform-specific bots
    discord: Option<RealDiscordBot>,
    telegram: Option<RealTelegramBot>,
    matrix: Option<RealMatrixBot>,
    signal: Option<RealSignalBot>,
    /// Message handler callback
    handler: Option<Arc<dyn MessageHandler>>,
    /// Thread-safe cross-channel/session pairings.
    pairing_store: PairingStore,
}

impl Gateway {
    /// Create new gateway
    pub fn new(config: Config) -> Self {
        Self {
            config,
            discord: None,
            telegram: None,
            matrix: None,
            signal: None,
            handler: None,
            pairing_store: PairingStore::new(),
        }
    }

    /// Set message handler
    pub fn on_message<H: MessageHandler + 'static>(&mut self, handler: H) {
        self.handler = Some(Arc::new(handler));
    }

    /// Start all configured gateways
    pub async fn start(&mut self) -> Result<()> {
        // Start Discord if configured
        if let Some(token) = &self.config.discord_token {
            let discord_config = DiscordConfig {
                token: token.clone(),
                command_prefix: self
                    .config
                    .discord_prefix
                    .clone()
                    .unwrap_or_else(|| "!hsm ".to_string()),
                allowed_channels: vec!["all".to_string()],
                presence_text: "Hyper-Stigmergic Morphogenesis II".to_string(),
            };

            let mut discord = RealDiscordBot::new(discord_config);

            if let Some(handler) = &self.handler {
                discord.start(handler.clone()).await?;
                self.discord = Some(discord);
                tracing::info!("Discord gateway started");
            } else {
                tracing::warn!("No message handler set for Discord");
            }
        }

        // Start Telegram if configured
        if let Some(token) = &self.config.telegram_token {
            let telegram_config = TelegramConfig {
                token: token.clone(),
                allowed_chats: self
                    .config
                    .telegram_allowed_chats
                    .clone()
                    .unwrap_or_default(),
                parse_mode: teloxide::types::ParseMode::MarkdownV2,
                max_message_length: 4096,
            };

            let mut telegram = RealTelegramBot::new(telegram_config);

            if let Some(handler) = &self.handler {
                telegram.start(handler.clone()).await?;
                self.telegram = Some(telegram);
                tracing::info!("Telegram gateway started");
            } else {
                tracing::warn!("No message handler set for Telegram");
            }
        }

        // Start Matrix if configured
        if self.config.matrix_homeserver_url.is_some() && self.config.matrix_access_token.is_some() {
            let matrix_cfg = MatrixConfig {
                homeserver_url: self.config.matrix_homeserver_url.clone(),
                access_token: self.config.matrix_access_token.clone(),
                bot_user_id: self.config.matrix_bot_user_id.clone(),
            };
            let mut matrix = RealMatrixBot::new(matrix_cfg);
            if let Some(handler) = &self.handler {
                matrix.start(handler.clone()).await?;
                self.matrix = Some(matrix);
                tracing::info!("Matrix gateway started");
            }
        }

        // Start Signal if configured
        if self.config.signal_api_base.is_some() && self.config.signal_auth_token.is_some() {
            let signal_cfg = SignalConfig {
                api_base: self.config.signal_api_base.clone(),
                bot_number: self.config.signal_bot_number.clone(),
                auth_token: self.config.signal_auth_token.clone(),
            };
            let mut signal = RealSignalBot::new(signal_cfg);
            if let Some(handler) = &self.handler {
                signal.start(handler.clone()).await?;
                self.signal = Some(signal);
                tracing::info!("Signal gateway started");
            }
        }

        Ok(())
    }

    /// Send message to specific platform
    pub async fn send(&self, platform: Platform, channel: &str, message: &str) -> Result<()> {
        let sanitized = redact_secrets(message);
        match platform {
            Platform::Discord => {
                if let Some(discord) = &self.discord {
                    discord.send_message(channel, &sanitized).await?;
                }
            }
            Platform::Telegram => {
                if let Some(telegram) = &self.telegram {
                    telegram.send_message(channel, &sanitized).await?;
                }
            }
            Platform::Matrix => {
                if let Some(matrix) = &self.matrix {
                    matrix.send_message(channel, &sanitized).await?;
                }
            }
            Platform::Signal => {
                if let Some(signal) = &self.signal {
                    signal.send_message(channel, &sanitized).await?;
                }
            }
            _ => {
                tracing::warn!(?platform, "Platform not yet implemented");
            }
        }
        Ok(())
    }

    /// Send reply to incoming message
    pub async fn reply(&self, to: &Message, content: &str) -> Result<()> {
        self.send(to.platform, &to.channel_id, content).await
    }

    pub fn set_pairing(&self, scope_key: impl Into<String>, session_key: impl Into<String>) {
        let _ = self.pairing_store.set_pairing(scope_key, session_key);
    }

    pub fn get_pairing(&self, scope_key: &str) -> Option<String> {
        self.pairing_store.get_pairing(scope_key)
    }

    /// Shutdown all gateways
    pub async fn shutdown(&mut self) -> Result<()> {
        if let Some(discord) = &mut self.discord {
            discord.shutdown().await?;
        }
        if let Some(telegram) = &mut self.telegram {
            telegram.shutdown().await?;
        }
        // Matrix/Signal use stateless HTTP transport currently.
        Ok(())
    }
}

/// Gateway configuration
#[derive(Clone, Debug, Default)]
pub struct Config {
    pub discord_token: Option<String>,
    pub discord_prefix: Option<String>,
    pub telegram_token: Option<String>,
    pub telegram_allowed_chats: Option<Vec<i64>>,
    pub slack_token: Option<String>,
    pub slack_signing_secret: Option<String>,
    pub matrix_homeserver_url: Option<String>,
    pub matrix_access_token: Option<String>,
    pub matrix_bot_user_id: Option<String>,
    pub signal_api_base: Option<String>,
    pub signal_bot_number: Option<String>,
    pub signal_auth_token: Option<String>,
}

/// Message from any platform
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub platform: Platform,
    pub channel_id: String,
    pub channel_name: Option<String>,
    pub user_id: String,
    pub user_name: String,
    pub content: String,
    pub timestamp: DateTime<Utc>,
    pub attachments: Vec<Attachment>,
    pub reply_to: Option<String>,
}

/// Message attachment
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Attachment {
    pub id: String,
    pub filename: String,
    pub url: String,
    pub content_type: String,
    pub size: usize,
}

/// Platform types
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum Platform {
    Discord,
    Telegram,
    Slack,
    WhatsApp,
    Matrix,
    Signal,
    Cli,
    Web,
}

impl std::fmt::Display for Platform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Platform::Discord => write!(f, "Discord"),
            Platform::Telegram => write!(f, "Telegram"),
            Platform::Slack => write!(f, "Slack"),
            Platform::WhatsApp => write!(f, "WhatsApp"),
            Platform::Matrix => write!(f, "Matrix"),
            Platform::Signal => write!(f, "Signal"),
            Platform::Cli => write!(f, "CLI"),
            Platform::Web => write!(f, "Web"),
        }
    }
}

/// Message handler trait
#[async_trait]
pub trait MessageHandler: Send + Sync {
    async fn handle(&self, msg: Message) -> Result<String>;
}

/// Convert message to stigmergic signal format
impl Message {
    pub fn to_stigmergic_signal(&self) -> serde_json::Value {
        serde_json::json!({
            "source": format!("{}:{}", self.platform, self.user_id),
            "timestamp": self.timestamp,
            "payload": {
                "content": self.content,
                "platform": self.platform.to_string(),
                "channel": self.channel_id,
            },
            "coherence": 1.0, // User messages are high-coherence
        })
    }
}

fn secret_assignment_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(?i)\b(api[_-]?key|token|secret)\b\s*[:=]\s*([^\s,;]+)"#)
            .expect("valid secret assignment regex")
    })
}

fn token_like_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r#"(?i)\b(?:sk-[A-Za-z0-9_-]{16,}|xox[baprs]-[A-Za-z0-9-]{10,}|ghp_[A-Za-z0-9]{20,}|AIza[0-9A-Za-z_-]{20,}|Bearer\s+[A-Za-z0-9._-]{16,})\b"#,
        )
        .expect("valid token regex")
    })
}

pub fn redact_secrets(input: &str) -> String {
    let first = secret_assignment_re()
        .replace_all(input, "$1=[redacted]")
        .to_string();
    token_like_re()
        .replace_all(&first, "[redacted]")
        .to_string()
}

pub fn sanitize_media_url(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut url = Url::parse(trimmed).ok()?;
    match url.scheme() {
        "http" | "https" => {}
        _ => return None,
    }
    if !url.username().is_empty() || url.password().is_some() {
        return None;
    }
    url.set_query(None);
    url.set_fragment(None);
    Some(url.to_string())
}

#[cfg(test)]
mod tests {
    use super::{redact_secrets, sanitize_media_url};

    #[test]
    fn redacts_inline_secrets() {
        let src = "OPENAI_API_KEY=sk-abc12345678901234567890 token: ghp_abcdefghijklmnopqrstuvwx";
        let out = redact_secrets(src);
        assert!(!out.contains("sk-abc"));
        assert!(!out.contains("ghp_abc"));
        assert!(out.contains("[redacted]"));
    }

    #[test]
    fn sanitizes_media_urls() {
        let u = sanitize_media_url("https://cdn.example.com/a.png?sig=123#frag").unwrap();
        assert_eq!(u, "https://cdn.example.com/a.png");
        assert!(sanitize_media_url("javascript:alert(1)").is_none());
    }
}
