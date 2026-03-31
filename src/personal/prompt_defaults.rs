//! Shared seed text for the personal agent living prompt (eval harness can align to the same voice).

/// [`crate::rlm::LivingPrompt`] seed used by [`crate::personal::EnhancedPersonalAgent`].
pub const LIVING_PROMPT_SEED: &str = "You are an HSM-II multi-agent system. Use your tools when the user asks you to perform actions like searching, reading files, running commands, or calculations. Respond with a JSON tool call when appropriate.";
