//! Matrix gateway transport: messages, reactions, receipts, media ingest, room management.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::info;
use uuid::Uuid;

use crate::personal::gateway::{Message as GatewayMessage, MessageHandler, Platform};
use crate::personal::gateway::sanitize_media_url;

#[derive(Clone, Debug, Default)]
pub struct MatrixConfig {
    pub homeserver_url: Option<String>,
    pub access_token: Option<String>,
    pub bot_user_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MatrixReactionEvent {
    pub room_id: String,
    pub event_id: String,
    pub emoji: String,
    pub user_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MatrixReadReceiptEvent {
    pub room_id: String,
    pub event_id: String,
    pub user_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MatrixMediaEvent {
    pub room_id: String,
    pub event_id: String,
    pub user_id: String,
    pub media_url: String,
    pub content_type: String,
}

pub struct RealMatrixBot {
    config: MatrixConfig,
    handler: Option<Arc<dyn MessageHandler>>,
    client: reqwest::Client,
}

impl RealMatrixBot {
    pub fn new(config: MatrixConfig) -> Self {
        Self {
            config,
            handler: None,
            client: reqwest::Client::new(),
        }
    }

    fn base_url(&self) -> Option<String> {
        self.config
            .homeserver_url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.trim_end_matches('/').to_string())
    }

    fn auth_token(&self) -> Option<&str> {
        self.config
            .access_token
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
    }

    fn encode(s: &str) -> String {
        urlencoding::encode(s).to_string()
    }

    async fn send_event(&self, room_id: &str, event_type: &str, content: serde_json::Value) -> Result<()> {
        let base = self
            .base_url()
            .ok_or_else(|| anyhow::anyhow!("matrix homeserver_url missing"))?;
        let token = self
            .auth_token()
            .ok_or_else(|| anyhow::anyhow!("matrix access_token missing"))?;
        let txn_id = format!("hsm-{}", Uuid::new_v4());
        let url = format!(
            "{}/_matrix/client/v3/rooms/{}/send/{}/{}",
            base,
            Self::encode(room_id),
            Self::encode(event_type),
            Self::encode(&txn_id)
        );
        let resp = self
            .client
            .put(&url)
            .bearer_auth(token)
            .json(&content)
            .send()
            .await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("matrix send event failed ({}): {}", status.as_u16(), body);
        }
        Ok(())
    }

    async fn send_event_with_type(
        &self,
        room_id: &str,
        event_type: &str,
        content: serde_json::Value,
    ) -> Result<String> {
        let base = self
            .base_url()
            .ok_or_else(|| anyhow::anyhow!("matrix homeserver_url missing"))?;
        let token = self
            .auth_token()
            .ok_or_else(|| anyhow::anyhow!("matrix access_token missing"))?;
        let txn_id = format!("hsm-{}", Uuid::new_v4());
        let url = format!(
            "{}/_matrix/client/v3/rooms/{}/send/{}/{}",
            base,
            Self::encode(room_id),
            Self::encode(event_type),
            Self::encode(&txn_id)
        );
        let resp = self
            .client
            .put(&url)
            .bearer_auth(token)
            .json(&content)
            .send()
            .await?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            anyhow::bail!("matrix send event failed ({}): {}", status.as_u16(), body);
        }
        let parsed: serde_json::Value =
            serde_json::from_str(&body).unwrap_or_else(|_| serde_json::json!({}));
        Ok(parsed
            .get("event_id")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string())
    }

    fn normalize_media_url(&self, raw: &str) -> Option<String> {
        let t = raw.trim();
        if t.starts_with("mxc://") {
            let rest = t.trim_start_matches("mxc://");
            let mut parts = rest.splitn(2, '/');
            let server = parts.next()?.trim();
            let media_id = parts.next()?.trim();
            let base = self.base_url()?;
            return sanitize_media_url(&format!(
                "{}/_matrix/media/v3/download/{}/{}",
                base,
                Self::encode(server),
                Self::encode(media_id)
            ));
        }
        sanitize_media_url(t)
    }

    async fn probe_media(&self, url: &str) -> (usize, String) {
        let resp = self.client.head(url).send().await;
        if let Ok(r) = resp {
            let size = r
                .headers()
                .get(reqwest::header::CONTENT_LENGTH)
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(0);
            let content_type = r
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("application/octet-stream")
                .to_string();
            return (size, content_type);
        }
        (0, "application/octet-stream".to_string())
    }

    pub async fn start(&mut self, handler: Arc<dyn MessageHandler>) -> Result<()> {
        self.handler = Some(handler);
        info!(
            homeserver = ?self.config.homeserver_url,
            "Matrix gateway initialized"
        );
        Ok(())
    }

    pub async fn send_message(&self, room_id: &str, content: &str) -> Result<()> {
        self.send_event(
            room_id,
            "m.room.message",
            serde_json::json!({
                "msgtype": "m.text",
                "body": content,
            }),
        )
        .await
    }

    pub async fn send_reaction(&self, room_id: &str, event_id: &str, emoji: &str) -> Result<String> {
        self.send_event_with_type(
            room_id,
            "m.reaction",
            serde_json::json!({
                "m.relates_to": {
                    "rel_type": "m.annotation",
                    "event_id": event_id,
                    "key": emoji,
                }
            }),
        )
        .await
    }

    pub async fn mark_read(&self, room_id: &str, event_id: &str) -> Result<()> {
        let base = self
            .base_url()
            .ok_or_else(|| anyhow::anyhow!("matrix homeserver_url missing"))?;
        let token = self
            .auth_token()
            .ok_or_else(|| anyhow::anyhow!("matrix access_token missing"))?;
        let url = format!(
            "{}/_matrix/client/v3/rooms/{}/receipt/m.read/{}",
            base,
            Self::encode(room_id),
            Self::encode(event_id)
        );
        let resp = self.client.post(url).bearer_auth(token).send().await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("matrix mark_read failed ({}): {}", status.as_u16(), body);
        }
        Ok(())
    }

    pub async fn create_room(&self, name: &str, topic: Option<&str>) -> Result<String> {
        let base = self
            .base_url()
            .ok_or_else(|| anyhow::anyhow!("matrix homeserver_url missing"))?;
        let token = self
            .auth_token()
            .ok_or_else(|| anyhow::anyhow!("matrix access_token missing"))?;
        let mut body = serde_json::json!({
            "name": name,
            "preset": "private_chat",
        });
        if let Some(t) = topic {
            body["topic"] = serde_json::json!(t);
        }
        let url = format!("{}/_matrix/client/v3/createRoom", base);
        let resp = self
            .client
            .post(url)
            .bearer_auth(token)
            .json(&body)
            .send()
            .await?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            anyhow::bail!("matrix create_room failed ({}): {}", status.as_u16(), text);
        }
        let parsed: serde_json::Value =
            serde_json::from_str(&text).unwrap_or_else(|_| serde_json::json!({}));
        Ok(parsed
            .get("room_id")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string())
    }

    pub async fn ingest_reaction(&self, ev: MatrixReactionEvent) -> Result<String> {
        if let Some(h) = &self.handler {
            let msg = GatewayMessage {
                id: ev.event_id.clone(),
                platform: Platform::Matrix,
                channel_id: ev.room_id.clone(),
                channel_name: None,
                user_id: ev.user_id.clone(),
                user_name: ev.user_id,
                content: format!(
                    "/reaction emoji={} event_id={} platform=matrix",
                    ev.emoji, ev.event_id
                ),
                timestamp: chrono::Utc::now(),
                attachments: vec![],
                reply_to: Some(ev.event_id),
            };
            return h.handle(msg).await;
        }
        Ok(String::new())
    }

    pub async fn ingest_read_receipt(&self, ev: MatrixReadReceiptEvent) -> Result<String> {
        let _ = self.mark_read(&ev.room_id, &ev.event_id).await;
        if let Some(h) = &self.handler {
            let msg = GatewayMessage {
                id: format!("rr-{}", ev.event_id),
                platform: Platform::Matrix,
                channel_id: ev.room_id,
                channel_name: None,
                user_id: ev.user_id.clone(),
                user_name: ev.user_id,
                content: "/receipt platform=matrix kind=read".to_string(),
                timestamp: chrono::Utc::now(),
                attachments: vec![],
                reply_to: Some(ev.event_id),
            };
            return h.handle(msg).await;
        }
        Ok(String::new())
    }

    pub async fn ingest_media(&self, ev: MatrixMediaEvent) -> Result<String> {
        let safe_url = self.normalize_media_url(&ev.media_url);
        let (size, detected_type) = if let Some(ref u) = safe_url {
            self.probe_media(u).await
        } else {
            (0, "application/octet-stream".to_string())
        };
        let content_type = if ev.content_type.trim().is_empty() {
            detected_type
        } else {
            ev.content_type.clone()
        };
        if let Some(h) = &self.handler {
            let msg = GatewayMessage {
                id: ev.event_id.clone(),
                platform: Platform::Matrix,
                channel_id: ev.room_id,
                channel_name: None,
                user_id: ev.user_id.clone(),
                user_name: ev.user_id,
                content: format!("/media platform=matrix event_id={}", ev.event_id),
                timestamp: chrono::Utc::now(),
                attachments: safe_url
                    .into_iter()
                    .map(|url| crate::personal::gateway::Attachment {
                        id: ev.event_id.clone(),
                        filename: "matrix-media".to_string(),
                        url,
                        content_type: content_type.clone(),
                        size,
                    })
                    .collect(),
                reply_to: Some(ev.event_id),
            };
            return h.handle(msg).await;
        }
        Ok(String::new())
    }
}
