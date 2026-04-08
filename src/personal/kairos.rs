//! **KAIROS-style** wall-clock background maintenance (Claude Code parity-lite).
//!
//! Claude Code’s `KAIROS` feature flag enables assistant-mode plumbing: proactive ticks, cron,
//! brief checkpoints, etc. HSM-II already has [`super::heartbeat`] and [`super::autodream`]
//! but they only run when [`super::EnhancedPersonalAgent::handle_message`] is entered — so a quiet
//! daemon never consolidates memory or runs `HEARTBEAT.md` on schedule unless you enable this module.
//!
//! Enable **`HSM_KAIROS=1`** on the **unified** personal agent (`hsmii start --daemon`) so a
//! background task calls [`run_idle_maintenance`] every [`tick_interval_secs()`]. Use with
//! `HSM_AUTODREAM=1`, `HSM_HEARTBEAT_INTERVAL_SECS`, and `HEARTBEAT.md` as needed.
//!
//! **Note:** `integrated_agent`’s daemon loop reloads the agent from disk each minute, which resets
//! per-process heartbeat/autodream timers; prefer `personal_agent start --daemon` for full KAIROS
//! idle behavior.

use tracing::debug;

use super::EnhancedPersonalAgent;

fn env_truthy(name: &str) -> bool {
    std::env::var(name)
        .map(|v| {
            let s = v.trim();
            s == "1" || s.eq_ignore_ascii_case("true") || s.eq_ignore_ascii_case("yes")
        })
        .unwrap_or(false)
}

/// Master switch: spawn wall-clock idle loop in `personal_agent` daemon.
pub fn kairos_enabled() -> bool {
    env_truthy("HSM_KAIROS")
}

/// Seconds between idle maintenance passes (default `60`).
pub fn tick_interval_secs() -> u64 {
    std::env::var("HSM_KAIROS_TICK_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|&n| n > 0)
        .unwrap_or(60)
}

/// Run scheduled subsystems that are normally triggered at the start of each user message.
pub async fn run_idle_maintenance(agent: &mut EnhancedPersonalAgent) {
    if !kairos_enabled() {
        return;
    }
    debug!(target: "hsm.kairos", "idle maintenance tick");
    agent.maybe_run_autodream_tick().await;
    agent.maybe_run_heartbeat_tick().await;
}
