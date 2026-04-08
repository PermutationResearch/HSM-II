//! Paperclip Intelligence Layer — company-as-intelligence runtime.
//!
//! Bridges the in-memory HSM-II world model with the Postgres-backed `company_os`
//! to create a living, proactive organization. Four building blocks:
//!
//! 1. **Capabilities** — Atomic composable primitives (code, research, sales, …)
//! 2. **World Model** — Dual internal (HSM-II state) + external (market/customer signals)
//! 3. **Intelligence Layer** — Composition engine: signal → compose → execute → escalate
//! 4. **Interfaces** — Delivery surfaces (dashboard, API, bots)
//!
//! Three role types collapse traditional org structure:
//! - **IC** (Individual Contributor) — Deep specialist building one capability
//! - **DRI** (Directly Responsible Individual) — Cross-cutting outcome owner
//! - **PlayerCoach** — Hands-on builder who also mentors/improves other agents

pub mod capability;
pub mod dri;
pub mod goal;
pub mod intelligence;
pub mod org;
pub mod template;

pub use capability::{Capability, CapabilityId, CapabilityRegistry, CapabilityTarget};
pub use dri::{DriAuthority, DriEntry, DriRegistry};
pub use goal::{
    ArtifactOutput, EscalationChain, EscalationLevel, Goal, GoalAssignee, GoalId, GoalPriority,
    GoalStatus,
};
pub use intelligence::{CompositionResult, IntelligenceLayer, Signal, SignalKind};
pub use org::{
    OrgBlueprint, TemplateEscalationLevel, TemplateGoal, TemplateRole, TemplateRoleType,
};
pub use template::CompanyTemplate;
