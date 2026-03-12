//! Real Telegram Bot Integration using Teloxide
//!
//! Provides two-way messaging between Telegram and HSM-II.

use anyhow::Result;
use std::sync::Arc;
use teloxide::dispatching::UpdateFilterExt;
use teloxide::prelude::*;
use teloxide::types::{ParseMode, Update};
use teloxide::Bot;
use tokio::sync::mpsc;
use tracing::{debug, error, info};

use crate::personal::gateway::{Message as GatewayMessage, MessageHandler, Platform};

/// Telegram bot configuration
#[derive(Clone, Debug)]
pub struct TelegramConfig {
    pub token: String,
    pub allowed_chats: Vec<i64>, // Empty = all chats allowed
    pub parse_mode: ParseMode,
    pub max_message_length: usize,
}

impl Default for TelegramConfig {
    fn default() -> Self {
        Self {
            token: String::new(),
            allowed_chats: vec![],
            parse_mode: ParseMode::MarkdownV2,
            max_message_length: 4096, // Telegram's limit
        }
    }
}

/// Real Telegram bot implementation
pub struct RealTelegramBot {
    config: TelegramConfig,
    handler: Option<Arc<dyn MessageHandler>>,
    shutdown_tx: Option<mpsc::Sender<()>>,
}

impl RealTelegramBot {
    pub fn new(config: TelegramConfig) -> Self {
        Self {
            config,
            handler: None,
            shutdown_tx: None,
        }
    }

    /// Start the Telegram bot
    pub async fn start(&mut self, handler: Arc<dyn MessageHandler>) -> Result<()> {
        let bot = Bot::new(&self.config.token);
        let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);
        self.shutdown_tx = Some(shutdown_tx);
        self.handler = Some(handler.clone());

        let config = self.config.clone();

        info!("Starting Telegram bot...");

        // Build the dispatcher with handler
        let dispatch_handler = dptree::entry()
            .branch(Update::filter_message().endpoint(handle_telegram_message));

        let mut dispatcher = Dispatcher::builder(bot.clone(), dispatch_handler)
            .dependencies(dptree::deps![handler, config])
            .default_handler(|upd| async move {
                debug!(update = ?upd, "Unhandled update");
            })
            .error_handler(LoggingErrorHandler::with_custom_text(
                "Error in Telegram dispatcher",
            ))
            .build();

        // Start dispatcher in background
        tokio::spawn(async move {
            tokio::select! {
                _ = dispatcher.dispatch() => {
                    info!("Telegram dispatcher stopped");
                }
                _ = shutdown_rx.recv() => {
                    info!("Telegram shutdown signal received");
                }
            }
        });

        info!("Telegram bot started");
        Ok(())
    }

    /// Send a message to a Telegram chat
    pub async fn send_message(&self, chat_id: &str, content: &str) -> Result<()> {
        let bot = Bot::new(&self.config.token);
        let chat_id: i64 = chat_id.parse()?;
        
        // Split long messages
        let chunks = Self::split_message(content, self.config.max_message_length);
        
        for chunk in chunks {
            match bot
                .send_message(ChatId(chat_id), &chunk)
                .parse_mode(self.config.parse_mode)
                .await
            {
                Ok(_) => debug!(chat_id, "Message sent to Telegram"),
                Err(e) => {
                    error!(error = %e, chat_id, "Failed to send Telegram message");
                    // Try without parse mode if formatting fails
                    if let Err(e2) = bot.send_message(ChatId(chat_id), &chunk).await {
                        error!(error = %e2, chat_id, "Failed to send plain message too");
                    }
                }
            }
        }

        Ok(())
    }

    /// Send a direct message using bot instance (for replies)
    pub async fn send_reply(bot: &Bot, chat_id: ChatId, content: &str, parse_mode: ParseMode) -> Result<()> {
        let chunks = Self::split_message(content, 4096);
        
        for chunk in chunks {
            match bot.send_message(chat_id, &chunk).parse_mode(parse_mode).await {
                Ok(_) => {}
                Err(_) => {
                    // Fallback to plain text
                    let _ = bot.send_message(chat_id, &chunk).await;
                }
            }
        }
        Ok(())
    }

    /// Shutdown the bot
    pub async fn shutdown(&mut self) -> Result<()> {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(()).await;
        }
        Ok(())
    }

    /// Split long messages into Telegram-compliant chunks
    pub fn split_message(content: &str, max_len: usize) -> Vec<String> {
        let mut chunks = Vec::new();
        let mut remaining = content;

        while !remaining.is_empty() {
            if remaining.len() <= max_len {
                chunks.push(remaining.to_string());
                break;
            }

            // Try to split at paragraph or sentence boundary
            let split_point = remaining[..max_len]
                .rfind("\n\n")
                .map(|i| i + 2)
                .or_else(|| remaining[..max_len].rfind(". ").map(|i| i + 2))
                .or_else(|| remaining[..max_len].rfind('\n').map(|i| i + 1))
                .unwrap_or(max_len);

            chunks.push(remaining[..split_point].to_string());
            remaining = &remaining[split_point..];
        }

        chunks
    }
}

/// Escape special characters for Telegram MarkdownV2
pub fn escape_markdown(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('_', "\\_")
        .replace('*', "\\*")
        .replace('[', "\\[")
        .replace(']', "\\]")
        .replace('(', "\\(")
        .replace(')', "\\)")
        .replace('~', "\\~")
        .replace('`', "\\`")
        .replace('>', "\\>")
        .replace('#', "\\#")
        .replace('+', "\\+")
        .replace('-', "\\-")
        .replace('=', "\\=")
        .replace('|', "\\|")
        .replace('{', "\\{")
        .replace('}', "\\}")
        .replace('.', "\\.")
        .replace('!', "\\!")
}

/// Handle incoming Telegram messages
async fn handle_telegram_message(
    bot: Bot,
    msg: Message,
    handler: Arc<dyn MessageHandler>,
    config: TelegramConfig,
) -> Result<(), teloxide::RequestError> {
    // Check if chat is allowed
    if !config.allowed_chats.is_empty() {
        let chat_id = msg.chat.id.0;
        if !config.allowed_chats.contains(&chat_id) {
            debug!(chat_id, "Chat not in allowlist, ignoring");
            return Ok(());
        }
    }

    // Only process text messages
    let text = match msg.text() {
        Some(t) => t,
        None => {
            debug!("Non-text message received, ignoring");
            return Ok(());
        }
    };

    let user = msg.from.as_ref();
    let user_name = user
        .map(|u| u.username.clone().unwrap_or_else(|| u.first_name.clone()))
        .unwrap_or_else(|| "Unknown".to_string());

    let gateway_msg = GatewayMessage {
        id: msg.id.0.to_string(),
        platform: Platform::Telegram,
        channel_id: msg.chat.id.0.to_string(),
        channel_name: msg.chat.title().map(|s| s.to_string()),
        user_id: user.map(|u| u.id.0.to_string()).unwrap_or_default(),
        user_name,
        content: text.to_string(),
        timestamp: chrono::Utc::now(),
        attachments: vec![],
        reply_to: msg.reply_to_message().map(|m| m.id.0.to_string()),
    };

    debug!(
        user = %gateway_msg.user_name,
        content = %gateway_msg.content,
        "Telegram message received"
    );

    // Show typing indicator
    let _ = bot.send_chat_action(msg.chat.id, teloxide::types::ChatAction::Typing).await;

    // Handle the message
    match handler.handle(gateway_msg).await {
        Ok(response) => {
            if !response.is_empty() {
                // Try to send with Markdown, fall back to plain text
                if let Err(_) = RealTelegramBot::send_reply(&bot, msg.chat.id, &response, config.parse_mode).await {
                    // Fallback: try without escaping
                    let plain = response;
                    let _ = bot.send_message(msg.chat.id, &plain).await;
                }
            }
        }
        Err(e) => {
            error!(error = %e, "Error handling Telegram message");
            let _ = bot
                .send_message(msg.chat.id, "❌ Error processing your message.")
                .await;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_message() {
        let text = "a".repeat(5000);
        let chunks = RealTelegramBot::split_message(&text, 4096);
        assert!(chunks.len() > 1);
        assert!(chunks.iter().all(|c| c.len() <= 4096));
    }

    #[test]
    fn test_escape_markdown() {
        let text = "Hello_world *bold* [link](url)";
        let escaped = escape_markdown(text);
        assert!(escaped.contains("\\_"));
        assert!(escaped.contains("\\*"));
    }
}
