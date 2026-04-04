//! Platform Gateways for HSM-II
//!
//! Connects to Discord, Telegram, Slack, and other messaging platforms.

pub mod discord;
pub mod telegram;

pub use discord::{DiscordConfig, RealDiscordBot};
pub use telegram::{RealTelegramBot, TelegramConfig};
