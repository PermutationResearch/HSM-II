//! Platform Gateways for HSM-II
//!
//! Connects to Discord, Telegram, Slack, and other messaging platforms.

pub mod discord;

pub use discord::{RealDiscordBot, DiscordConfig};
