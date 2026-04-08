//! Coordinator semantics (Claude layer 12 parity): multi-step delegation tracing + envelope helpers.
//!
//! Use [`super::lh_contract::HarnessRunEnvelope::subagent_delegate`] when a lead run spawns an
//! isolated sub-thread; emit [`coordinator_step_span`] around each delegation for OTLP-friendly logs.

use tracing::Span;

/// Span for one coordinator → subagent handoff (parent thread remains visible for correlation).
pub fn coordinator_step_span(
    orchestrator_thread_id: &str,
    step_index: u32,
    delegate_thread_id: &str,
    correlation_id: Option<&str>,
) -> Span {
    tracing::info_span!(
        "hsm.harness.coordinator",
        orchestrator_thread_id = %orchestrator_thread_id,
        coordinator_step = step_index,
        delegate_thread_id = %delegate_thread_id,
        correlation_id = correlation_id.unwrap_or(""),
    )
}
