//! Real Discord Bot Integration using Serenity
//!
//! Provides two-way messaging between Discord and HSM-II.

use anyhow::Result;
use async_trait::async_trait;
use serenity::all::{
    ActivityData, Context as SerenityContext, EventHandler, GatewayIntents,
    Message as DiscordMessage, Ready,
};
use serenity::Client;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info};

use crate::personal::gateway::{Message as GatewayMessage, MessageHandler, Platform};
use crate::personal::gateway::{redact_secrets, sanitize_media_url};

/// Discord bot configuration
#[derive(Clone, Debug)]
pub struct DiscordConfig {
    pub token: String,
    pub command_prefix: String,
    pub allowed_channels: Vec<String>,
    pub presence_text: String,
}

impl Default for DiscordConfig {
    fn default() -> Self {
        Self {
            token: String::new(),
            command_prefix: "!hsm ".to_string(),
            allowed_channels: vec!["all".to_string()],
            presence_text: "Hyper-Stigmergic Morphogenesis II".to_string(),
        }
    }
}

/// Real Discord bot implementation
pub struct RealDiscordBot {
    config: DiscordConfig,
    handler: Option<Arc<dyn MessageHandler>>,
    shutdown_tx: Option<mpsc::Sender<()>>,
}

impl RealDiscordBot {
    pub fn new(config: DiscordConfig) -> Self {
        Self {
            config,
            handler: None,
            shutdown_tx: None,
        }
    }

    /// Start the Discord bot
    pub async fn start(&mut self, handler: Arc<dyn MessageHandler>) -> Result<()> {
        let token = self.config.token.clone();
        let config = self.config.clone();

        let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);
        self.shutdown_tx = Some(shutdown_tx);
        self.handler = Some(handler.clone());

        let intents = GatewayIntents::GUILD_MESSAGES
            | GatewayIntents::DIRECT_MESSAGES
            | GatewayIntents::MESSAGE_CONTENT
            | GatewayIntents::GUILDS;

        let discord_handler = DiscordEventHandler {
            inner_handler: handler,
            config: config.clone(),
        };

        info!("Starting Discord bot...");

        // Start in background
        tokio::spawn(async move {
            loop {
                let token_clone = token.clone();
                let mut client = match Client::builder(token_clone, intents)
                    .event_handler(discord_handler.clone())
                    .await
                {
                    Ok(c) => c,
                    Err(e) => {
                        error!(error = %e, "Failed to create Discord client, retrying in 10s");
                        tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
                        continue;
                    }
                };

                tokio::select! {
                    result = client.start() => {
                        match result {
                            Ok(()) => {
                                info!("Discord client stopped normally");
                                break;
                            }
                            Err(e) => {
                                error!(error = %e, "Discord client error, reconnecting in 5s...");
                                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                            }
                        }
                    }
                    _ = shutdown_rx.recv() => {
                        info!("Discord shutdown signal received");
                        let _ = client.shard_manager.shutdown_all().await;
                        break;
                    }
                }
            }
        });

        info!("Discord bot started");
        Ok(())
    }

    /// Send a message to a Discord channel
    pub async fn send_message(&self, _channel_id: &str, _content: &str) -> Result<()> {
        // Implementation would store http client reference
        // For now, this is a placeholder
        info!("Discord send_message called (implementation needs http client storage)");
        Ok(())
    }

    /// Shutdown the bot
    pub async fn shutdown(&mut self) -> Result<()> {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(()).await;
        }
        Ok(())
    }

    /// Split long messages into Discord-compliant chunks
    pub fn split_message(content: &str, max_len: usize) -> Vec<String> {
        let mut chunks = Vec::new();
        let mut remaining = content;

        while !remaining.is_empty() {
            if remaining.len() <= max_len {
                chunks.push(remaining.to_string());
                break;
            }

            // Try to split at newline
            let split_point = remaining[..max_len]
                .rfind('\n')
                .map(|i| i + 1)
                .unwrap_or(max_len);

            chunks.push(remaining[..split_point].to_string());
            remaining = &remaining[split_point..];
        }

        chunks
    }
}

/// Discord event handler
#[derive(Clone)]
struct DiscordEventHandler {
    inner_handler: Arc<dyn MessageHandler>,
    config: DiscordConfig,
}

#[async_trait]
impl EventHandler for DiscordEventHandler {
    async fn ready(&self, ctx: SerenityContext, ready: Ready) {
        info!(username = %ready.user.name, "Discord bot connected");

        let activity = ActivityData::playing(&self.config.presence_text);
        ctx.set_presence(Some(activity), serenity::all::OnlineStatus::Online);
    }

    async fn message(&self, _ctx: SerenityContext, msg: DiscordMessage) {
        if msg.author.bot {
            return;
        }

        let channel_id = msg.channel_id.to_string();
        if !self.is_channel_allowed(&channel_id) {
            return;
        }

        let gateway_msg = GatewayMessage {
            id: msg.id.to_string(),
            platform: Platform::Discord,
            channel_id: msg.channel_id.to_string(),
            channel_name: None,
            user_id: msg.author.id.to_string(),
            user_name: msg.author.name.clone(),
            content: msg.content.clone(),
            timestamp: chrono::Utc::now(),
            attachments: msg
                .attachments
                .iter()
                .filter_map(|a| {
                    let safe = sanitize_media_url(&a.url)?;
                    Some(crate::personal::gateway::Attachment {
                        id: a.id.to_string(),
                        filename: a.filename.clone(),
                        url: safe,
                        content_type: a.content_type.clone().unwrap_or_else(|| "application/octet-stream".to_string()),
                        size: a.size as usize,
                    })
                })
                .collect(),
            reply_to: msg.referenced_message.as_ref().map(|m| m.id.to_string()),
        };

        debug!(user = %msg.author.name, message_id = %msg.id, "Discord message");

        match self.inner_handler.handle(gateway_msg).await {
            Ok(response) => {
                if !response.is_empty() {
                    let safe_response = redact_secrets(&response);
                    let chunks = RealDiscordBot::split_message(&safe_response, 2000);
                    for chunk in chunks {
                        if let Err(e) = msg.channel_id.say(&_ctx.http, &chunk).await {
                            error!(error = %e, "Failed to send Discord response");
                        }
                    }
                }
            }
            Err(e) => {
                error!(error = %e, "Error handling message");
                let _ = msg
                    .channel_id
                    .say(&_ctx.http, "❌ Error processing your message.")
                    .await;
            }
        }
    }
}

impl DiscordEventHandler {
    fn is_channel_allowed(&self, channel_id: &str) -> bool {
        self.config.allowed_channels.contains(&"all".to_string())
            || self
                .config
                .allowed_channels
                .contains(&channel_id.to_string())
    }
}
