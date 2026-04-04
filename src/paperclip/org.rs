//! Org taxonomy types shared by templates and the Intelligence Layer.
//!
//! Lives in its own module so `intelligence` can hold an [`OrgBlueprint`] without a dependency
//! cycle with `template`.

use serde::{Deserialize, Serialize};

use super::goal::GoalPriority;

/// Role definition within a company template (IC / DRI / PlayerCoach).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TemplateRole {
    /// Role identifier (unique within template).
    pub id: String,
    /// Human-readable title.
    pub title: String,
    /// IC / DRI / PlayerCoach.
    pub role_type: TemplateRoleType,
    /// Capability IDs this role provides or owns.
    pub capabilities: Vec<String>,
    /// Domains this role is responsible for (DRI-specific).
    pub domains: Vec<String>,
    /// Agent IDs this role mentors (PlayerCoach-specific).
    pub mentees: Vec<String>,
    /// Optional briefing markdown.
    pub briefing: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TemplateRoleType {
    Ic,
    Dri,
    PlayerCoach,
}

/// Simplified goal for template seeding.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TemplateGoal {
    pub title: String,
    pub description: String,
    pub priority: GoalPriority,
    /// Role ID of the assignee (resolved during import).
    pub assignee_role_id: Option<String>,
    /// Required capability IDs.
    pub required_capabilities: Vec<String>,
    pub tags: Vec<String>,
}

/// Simplified escalation level for templates (string `action` for JSON portability).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TemplateEscalationLevel {
    /// Role ID to escalate to.
    pub role_id: String,
    pub timeout_secs: u64,
    pub action: String,
}

/// Last-applied company template org slice — kept on [`super::intelligence::IntelligenceLayer`]
/// so export can recover IC / PlayerCoach rows and `default_escalation`, while DRIs stay in sync
/// with the live [`super::dri::DriRegistry`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OrgBlueprint {
    pub template_name: String,
    pub template_description: String,
    pub roles: Vec<TemplateRole>,
    pub default_escalation: Vec<TemplateEscalationLevel>,
}
