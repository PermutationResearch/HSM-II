//! Platform Gateways for HSM-II
//!
//! Connects to Discord, Telegram, Slack, and other messaging platforms.

pub mod discord;
pub mod matrix;
pub mod signal;
pub mod telegram;

pub use discord::{DiscordConfig, RealDiscordBot};
pub use matrix::{MatrixConfig, MatrixMediaEvent, MatrixReactionEvent, MatrixReadReceiptEvent, RealMatrixBot};
pub use signal::{RealSignalBot, SignalConfig, SignalMediaEvent, SignalReactionEvent, SignalReadReceiptEvent};
pub use telegram::{RealTelegramBot, TelegramConfig};

#[derive(Clone, Debug, serde::Serialize)]
pub struct GatewayCapabilityRow {
    pub platform: &'static str,
    pub reactions: &'static str,
    pub read_receipts: &'static str,
    pub rich_formatting: &'static str,
    pub room_management: &'static str,
    pub media_delivery: &'static str,
    pub tier: &'static str,
}

pub fn tier1_compatibility_matrix() -> Vec<GatewayCapabilityRow> {
    vec![
        GatewayCapabilityRow {
            platform: "matrix",
            reactions: "http-api",
            read_receipts: "http-api",
            rich_formatting: "markdown+events",
            room_management: "create-room api",
            media_delivery: "download+attachment",
            tier: "tier-1 in-progress",
        },
        GatewayCapabilityRow {
            platform: "signal",
            reactions: "event-ingest",
            read_receipts: "event-ingest",
            rich_formatting: "limited",
            room_management: "n/a",
            media_delivery: "url-sanitized ingest",
            tier: "tier-1 in-progress",
        },
        GatewayCapabilityRow {
            platform: "telegram",
            reactions: "emoji-fallback",
            read_receipts: "platform-limited",
            rich_formatting: "supported",
            room_management: "platform-native",
            media_delivery: "supported",
            tier: "active",
        },
        GatewayCapabilityRow {
            platform: "discord",
            reactions: "supported-via-events",
            read_receipts: "platform-limited",
            rich_formatting: "supported",
            room_management: "platform-native",
            media_delivery: "supported",
            tier: "active",
        },
    ]
}
