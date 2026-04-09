//! Signal gateway transport with media + receipts + reaction events.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::info;

use crate::personal::gateway::{Message as GatewayMessage, MessageHandler, Platform};
use crate::personal::gateway::sanitize_media_url;

#[derive(Clone, Debug, Default)]
pub struct SignalConfig {
    pub api_base: Option<String>,
    pub bot_number: Option<String>,
    pub auth_token: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SignalMediaEvent {
    pub conversation_id: String,
    pub message_id: String,
    pub sender_id: String,
    pub media_url: String,
    pub content_type: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SignalReadReceiptEvent {
    pub conversation_id: String,
    pub message_id: String,
    pub reader_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SignalReactionEvent {
    pub conversation_id: String,
    pub message_id: String,
    pub sender_id: String,
    pub emoji: String,
}

pub struct RealSignalBot {
    config: SignalConfig,
    handler: Option<Arc<dyn MessageHandler>>,
    client: reqwest::Client,
}

impl RealSignalBot {
    pub fn new(config: SignalConfig) -> Self {
        Self {
            config,
            handler: None,
            client: reqwest::Client::new(),
        }
    }

    fn api_base(&self) -> Option<String> {
        self.config
            .api_base
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.trim_end_matches('/').to_string())
    }

    fn auth_token(&self) -> Option<&str> {
        self.config
            .auth_token
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
    }

    fn bot_number(&self) -> Option<&str> {
        self.config
            .bot_number
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
    }

    pub async fn start(&mut self, handler: Arc<dyn MessageHandler>) -> Result<()> {
        self.handler = Some(handler);
        info!(
            api_base = ?self.config.api_base,
            "Signal gateway initialized"
        );
        Ok(())
    }

    pub async fn send_message(&self, conversation_id: &str, content: &str) -> Result<()> {
        let base = self
            .api_base()
            .ok_or_else(|| anyhow::anyhow!("signal api_base missing"))?;
        let token = self
            .auth_token()
            .ok_or_else(|| anyhow::anyhow!("signal auth_token missing"))?;
        let number = self
            .bot_number()
            .ok_or_else(|| anyhow::anyhow!("signal bot_number missing"))?;
        let body = serde_json::json!({
            "number": number,
            "recipients": [conversation_id],
            "message": content,
        });
        let url = format!("{}/v2/send", base);
        let resp = self
            .client
            .post(url)
            .bearer_auth(token)
            .json(&body)
            .send()
            .await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("signal send_message failed ({}): {}", status.as_u16(), body);
        }
        Ok(())
    }

    pub async fn ingest_read_receipt(&self, ev: SignalReadReceiptEvent) -> Result<String> {
        if let Some(h) = &self.handler {
            let msg = GatewayMessage {
                id: format!("signal-rr-{}", ev.message_id),
                platform: Platform::Signal,
                channel_id: ev.conversation_id,
                channel_name: None,
                user_id: ev.reader_id.clone(),
                user_name: ev.reader_id,
                content: "/receipt platform=signal kind=read".to_string(),
                timestamp: chrono::Utc::now(),
                attachments: vec![],
                reply_to: Some(ev.message_id),
            };
            return h.handle(msg).await;
        }
        Ok(String::new())
    }

    pub async fn ingest_reaction(&self, ev: SignalReactionEvent) -> Result<String> {
        if let Some(h) = &self.handler {
            let msg = GatewayMessage {
                id: format!("signal-react-{}", ev.message_id),
                platform: Platform::Signal,
                channel_id: ev.conversation_id,
                channel_name: None,
                user_id: ev.sender_id.clone(),
                user_name: ev.sender_id,
                content: format!(
                    "/reaction emoji={} event_id={} platform=signal",
                    ev.emoji, ev.message_id
                ),
                timestamp: chrono::Utc::now(),
                attachments: vec![],
                reply_to: Some(ev.message_id),
            };
            return h.handle(msg).await;
        }
        Ok(String::new())
    }

    pub async fn ingest_media(&self, ev: SignalMediaEvent) -> Result<String> {
        let safe_url = sanitize_media_url(&ev.media_url);
        if let Some(h) = &self.handler {
            let msg = GatewayMessage {
                id: ev.message_id.clone(),
                platform: Platform::Signal,
                channel_id: ev.conversation_id,
                channel_name: None,
                user_id: ev.sender_id.clone(),
                user_name: ev.sender_id,
                content: format!("/media platform=signal event_id={}", ev.message_id),
                timestamp: chrono::Utc::now(),
                attachments: safe_url
                    .into_iter()
                    .map(|url| crate::personal::gateway::Attachment {
                        id: ev.message_id.clone(),
                        filename: "signal-media".to_string(),
                        url,
                        content_type: if ev.content_type.trim().is_empty() {
                            "application/octet-stream".to_string()
                        } else {
                            ev.content_type.clone()
                        },
                        size: 0,
                    })
                    .collect(),
                reply_to: Some(ev.message_id),
            };
            return h.handle(msg).await;
        }
        Ok(String::new())
    }
}
