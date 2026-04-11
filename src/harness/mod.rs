//! HarnessV1 — unified pause/resume substrate and observable turn lifecycle (incremental).
//!
//! Enable JSONL logging with:
//! - `HSM_HARNESS_LOG=/path/to/harness_events.jsonl`
//!
//! Optional:
//! - `HSM_HARNESS_TRACE_ID=...`
//! - `HSM_HARNESS_AGENT_ID=...`
//! - `HSM_HARNESS_CHECKPOINT_DIR=...`
//!
//! ## Long-horizon layers (phases 0–6)
//! - [`lh_contract`] — thread / lead–subagent envelope for gateways.
//! - [`context_tier`] — tiered context budgets (`HSM_CTX_*`).
//! - [`lh_trace`] — stable `tracing` span fields for collectors.
//! - [`thread_workspace`] — per-thread disk roots + tool path isolation (`HSM_THREAD_WORKSPACE`).
//! - [`docker_bash`] — **default** `docker run` for the `bash` tool when a thread workspace is active (`HSM_DOCKER_BASH=0` / `HSM_UNSAFE_HOST_BASH=1` to force host).
//! - [`posix_sandbox`] — optional Firejail / `unshare` wrapper for local bash (`HSM_BASH_ISOLATE`).
//! - [`coordinator`] — coordinator delegation tracing (`coordinator_step_span`, `HarnessRunEnvelope::subagent_delegate`).

mod anti_sycophancy;
mod approval;
mod cc_orchestrator;
mod context_tier;
mod control_plane;
mod coordinator;
mod council_socratic;
mod deeplink;
mod docker_bash;
mod events;
mod lh_contract;
mod lh_trace;
mod migrations;
mod posix_sandbox;
mod redact;
mod resume;
mod runtime;
mod scheduler;
mod session_persist;
mod store;
mod thread_workspace;
mod tool_checkpoint;
pub mod context_repo;
pub mod types;

pub use anti_sycophancy::{
    run_anti_sycophancy_loop, sycophancy_heuristic, AntiSycophancyConfig, AntiSycophancyRoundLog,
    AntiSycophancyRunResult, CriticParse, CriticVerdict,
};
pub use approval::{ApprovalOutcome, ApprovalService, ApprovalStore, PendingApproval};
pub use cc_orchestrator::{
    CcAgentSlot, CcCrossReviewMode, CcDraft, CcOrchestrator, CcOrchestratorConfig, CcReview,
    CcRunResult, CcTask,
};
pub use context_tier::{ContextStreamKind, ContextTier, TierBudget, TierPolicy};
pub use control_plane::{ApprovalConfig, PluginConfig, ResumeConfig, RuntimeConfig};
pub use coordinator::coordinator_step_span;
pub use council_socratic::{
    run_council_socratic_with_anti_sycophancy, CouncilRoleTurn, CouncilSocraticResult,
};
pub use deeplink::{parse_hsm_deeplink, DeepLinkAction};
pub use docker_bash::{docker_bash_enabled, run_in_docker};
pub use events::HarnessEvent;
pub use lh_contract::{
    HarnessRunEnvelope, LhActorRole, LhArtifactRef, LhContextScope, LhToolInvocationEnvelope,
    LhUploadRef, RunIdentity,
};
pub use lh_trace::{llm_chat_span, record_policy_result, tool_execution_span};
pub use migrations::{Migration, MigrationRunner};
pub use posix_sandbox::{bash_host_isolate_from_env, host_bash_command, BashHostIsolate};
pub use redact::redact_secrets;
pub use resume::ResumeSessionMap;
pub use runtime::HarnessRuntime;
pub use scheduler::Scheduler;
pub use session_persist::{append_session_event, load_recent_session_events, session_events_path};
pub use store::HarnessStore;
pub use thread_workspace::{
    activate_thread_workspace, appliance_home, current_root, deactivate_thread_workspace,
    ensure_thread_workspace_on_disk, resolve_tool_fs_path, sanitize_thread_id,
    thread_workspace_enabled, workspace_dirs, HarnessTurnCleanup,
};
pub use context_repo::{
    default_manifest_for_session, repo_root_for_company_home, repo_root_for_thread,
    sanitize_session_key, ContextRepoManifest, CONTEXT_REPO_FORMAT_VERSION, INDEX_FILE, MANIFEST_FILE,
    NOTES_DIR, SNAPSHOTS_DIR, THREAD_REPO_DIR,
};
pub use tool_checkpoint::append_tool_checkpoint;
pub use types::{ErrorClass, HarnessState, HarnessStepKey, ResumeToken, TaskOutcome};
