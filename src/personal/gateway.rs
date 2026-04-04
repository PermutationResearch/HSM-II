//! Gateway System - Multi-platform messaging like Hermes
//!
//! Connects HSM-II to Discord, Telegram, Slack, WhatsApp

use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::gateways::{DiscordConfig, RealDiscordBot, RealTelegramBot, TelegramConfig};

/// Unified gateway for all platforms
pub struct Gateway {
    config: Config,
    /// Platform-specific bots
    discord: Option<RealDiscordBot>,
    telegram: Option<RealTelegramBot>,
    /// Message handler callback
    handler: Option<Arc<dyn MessageHandler>>,
}

impl Gateway {
    /// Create new gateway
    pub fn new(config: Config) -> Self {
        Self {
            config,
            discord: None,
            telegram: None,
            handler: None,
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

        Ok(())
    }

    /// Send message to specific platform
    pub async fn send(&self, platform: Platform, channel: &str, message: &str) -> Result<()> {
        match platform {
            Platform::Discord => {
                if let Some(discord) = &self.discord {
                    discord.send_message(channel, message).await?;
                }
            }
            Platform::Telegram => {
                if let Some(telegram) = &self.telegram {
                    telegram.send_message(channel, message).await?;
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

    /// Shutdown all gateways
    pub async fn shutdown(&mut self) -> Result<()> {
        if let Some(discord) = &mut self.discord {
            discord.shutdown().await?;
        }
        if let Some(telegram) = &mut self.telegram {
            telegram.shutdown().await?;
        }
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
