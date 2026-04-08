//! Phase 3 — structured tracing for tool/policy paths (OTLP-ready span shapes via `tracing`).

use tracing::{field::Empty, Span};

use super::lh_contract::{HarnessRunEnvelope, LhActorRole, LhContextScope};

/// Root span attributes for a tool execution (compatible with OTLP JSON mapping).
pub fn tool_execution_span(tool_name: &str, envelope: Option<&HarnessRunEnvelope>) -> Span {
    let thread_id = envelope.map(|e| e.run.thread_id.as_str()).unwrap_or("none");
    let actor = envelope
        .map(|e| match e.actor {
            LhActorRole::Lead => "lead",
            LhActorRole::Subagent => "subagent",
        })
        .unwrap_or("none");
    let scope = envelope
        .map(|e| match e.context_scope {
            LhContextScope::Isolated => "isolated",
            LhContextScope::SharedRead => "shared_read",
            LhContextScope::SharedWrite => "shared_write",
        })
        .unwrap_or("none");

    let correlation_id = envelope
        .and_then(|e| e.run.correlation_id.as_deref())
        .unwrap_or("");
    tracing::info_span!(
        "hsm.harness.tool",
        tool_name = %tool_name,
        harness_thread_id = %thread_id,
        harness_actor = actor,
        harness_context_scope = scope,
        correlation_id = %correlation_id,
        policy_allowed = Empty,
        policy_reason = Empty,
    )
}

pub fn record_policy_result(span: &Span, allowed: bool, reason: Option<&str>) {
    span.record("policy_allowed", allowed);
    if let Some(r) = reason {
        span.record("policy_reason", r);
    }
}

/// LLM call span (parent may be set by caller).
pub fn llm_chat_span(
    model_hint: &str,
    thread_id: Option<&str>,
    correlation_id: Option<&str>,
) -> Span {
    tracing::info_span!(
        "hsm.harness.llm.chat",
        llm_model_hint = %model_hint,
        harness_thread_id = thread_id.unwrap_or("none"),
        correlation_id = correlation_id.unwrap_or(""),
    )
}
