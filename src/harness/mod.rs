//! HarnessV1 — unified pause/resume substrate and observable turn lifecycle (incremental).
//!
//! Enable JSONL logging with:
//! - `HSM_HARNESS_LOG=/path/to/harness_events.jsonl`
//!
//! Optional:
//! - `HSM_HARNESS_TRACE_ID=...`
//! - `HSM_HARNESS_AGENT_ID=...`
//! - `HSM_HARNESS_CHECKPOINT_DIR=...`

mod anti_sycophancy;
mod approval;
mod cc_orchestrator;
mod control_plane;
mod council_socratic;
mod deeplink;
mod events;
mod migrations;
mod resume;
mod runtime;
mod scheduler;
mod store;
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
pub use control_plane::{ApprovalConfig, PluginConfig, ResumeConfig, RuntimeConfig};
pub use council_socratic::{
    run_council_socratic_with_anti_sycophancy, CouncilRoleTurn, CouncilSocraticResult,
};
pub use deeplink::{parse_hsm_deeplink, DeepLinkAction};
pub use events::HarnessEvent;
pub use migrations::{Migration, MigrationRunner};
pub use resume::ResumeSessionMap;
pub use runtime::HarnessRuntime;
pub use scheduler::Scheduler;
pub use store::HarnessStore;
pub use types::{ErrorClass, HarnessState, HarnessStepKey, ResumeToken, TaskOutcome};
