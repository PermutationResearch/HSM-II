//! Company template import/export — makes the Paperclip structure portable.
//!
//! A `CompanyTemplate` captures the full organizational blueprint:
//! capabilities, DRI assignments, role taxonomy, escalation chains, and
//! default goals. Templates can be exported, shared, and imported into
//! any HSM-II instance to bootstrap a new company-as-intelligence.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;

use super::capability::Capability;
use super::dri::{DriEntry, DriRegistry};
use super::goal::{EscalationAction, EscalationChain, EscalationLevel, Goal, GoalAssignee};
use super::intelligence::{IntelligenceConfig, IntelligenceLayer};
use super::org::{
    OrgBlueprint, TemplateEscalationLevel, TemplateGoal, TemplateRole, TemplateRoleType,
};

fn assignee_to_role_id(a: &GoalAssignee) -> String {
    match a {
        GoalAssignee::Ic { agent_ref, .. }
        | GoalAssignee::Dri { agent_ref, .. }
        | GoalAssignee::PlayerCoach { agent_ref, .. } => agent_ref.clone(),
        GoalAssignee::Unassigned => String::new(),
    }
}

fn escalation_action_to_template_action(a: &EscalationAction) -> String {
    match a {
        EscalationAction::Reassign => "reassign".into(),
        EscalationAction::Notify => "notify".into(),
        EscalationAction::SpawnSubGoal { .. } => "spawn_sub_goal".into(),
        EscalationAction::HumanReview => "human_review".into(),
    }
}

fn goal_to_template_goal(goal: &Goal) -> TemplateGoal {
    let assignee_role_id = match &goal.assignee {
        GoalAssignee::Unassigned => None,
        _ => {
            let id = assignee_to_role_id(&goal.assignee);
            if id.is_empty() {
                None
            } else {
                Some(id)
            }
        }
    };
    TemplateGoal {
        title: goal.title.clone(),
        description: goal.description.clone(),
        priority: goal.priority.clone(),
        assignee_role_id,
        required_capabilities: goal.required_capabilities.clone(),
        tags: goal.tags.clone(),
    }
}

// ── CompanyTemplate ──────────────────────────────────────────────────────────

/// A complete organizational blueprint that can be imported/exported.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompanyTemplate {
    /// Template format version.
    pub schema_version: u32,
    /// Template name.
    pub name: String,
    /// Description of the company structure.
    pub description: String,
    /// All capability definitions.
    pub capabilities: Vec<Capability>,
    /// Role definitions (IC/DRI/PlayerCoach).
    pub roles: Vec<TemplateRole>,
    /// Default goals to seed.
    pub seed_goals: Vec<TemplateGoal>,
    /// Default escalation chain template.
    pub default_escalation: Vec<TemplateEscalationLevel>,
    /// Intelligence Layer configuration overrides.
    pub intelligence_config: Option<IntelligenceConfig>,
    /// Template metadata.
    pub metadata: serde_json::Value,
}

fn parse_template_escalation_action(s: &str) -> EscalationAction {
    match s.trim() {
        "notify" => EscalationAction::Notify,
        "human_review" => EscalationAction::HumanReview,
        "spawn_sub_goal" | "spawn_subgoal" => EscalationAction::SpawnSubGoal {
            sub_title: "Escalation sub-goal".into(),
        },
        _ => EscalationAction::Reassign,
    }
}

fn resolve_assignee_from_template(template: &CompanyTemplate, rid: &str) -> GoalAssignee {
    let role = template.roles.iter().find(|r| r.id == rid);
    match role.map(|r| &r.role_type) {
        Some(TemplateRoleType::Ic) => GoalAssignee::Ic {
            agent_ref: rid.to_string(),
            capability_id: role
                .and_then(|r| r.capabilities.first().cloned())
                .unwrap_or_default(),
        },
        Some(TemplateRoleType::Dri) => GoalAssignee::Dri {
            agent_ref: rid.to_string(),
            domain: role
                .and_then(|r| r.domains.first().cloned())
                .unwrap_or_default(),
        },
        Some(TemplateRoleType::PlayerCoach) => GoalAssignee::PlayerCoach {
            agent_ref: rid.to_string(),
            mentee_refs: role.map(|r| r.mentees.clone()).unwrap_or_default(),
        },
        None => GoalAssignee::Dri {
            agent_ref: rid.to_string(),
            domain: String::new(),
        },
    }
}

fn merge_blueprint_roles_with_dri(
    blueprint: Option<&OrgBlueprint>,
    dri: &DriRegistry,
) -> Vec<TemplateRole> {
    let mut out: Vec<TemplateRole> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    if let Some(bp) = blueprint {
        for r in &bp.roles {
            let mut tr = r.clone();
            if let Some(e) = dri.get(&tr.id) {
                tr.domains = e.domains.clone();
                tr.mentees = e.managed_agent_refs.clone();
                if !e.name.is_empty() {
                    tr.title = e.name.clone();
                }
            }
            seen.insert(tr.id.clone());
            out.push(tr);
        }
    }

    for e in dri.all() {
        if seen.contains(&e.id) {
            continue;
        }
        out.push(TemplateRole {
            id: e.id.clone(),
            title: e.name.clone(),
            role_type: TemplateRoleType::Dri,
            capabilities: Vec::new(),
            domains: e.domains.clone(),
            mentees: e.managed_agent_refs.clone(),
            briefing: None,
        });
    }

    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

impl CompanyTemplate {
    /// Create the default Paperclip template with the standard capability set
    /// and three-role taxonomy.
    pub fn paperclip_default() -> Self {
        let capabilities = vec![
            Capability::new("code_engineering", "Code & Engineering")
                .with_domains(vec!["engineering".into()])
                .with_description("Code generation, review, deployment, CI/CD, security"),
            Capability::new("research_data", "Research & Data")
                .with_domains(vec!["research".into(), "data".into()])
                .with_description("Web/search/tool-use, knowledge graphs, market analysis"),
            Capability::new("customer_sales", "Customer & Sales")
                .with_domains(vec!["customer".into(), "sales".into()])
                .with_description("Outreach, support, personalization, transaction analysis"),
            Capability::new("finance_ops", "Finance & Operations")
                .with_domains(vec!["finance".into(), "operations".into()])
                .with_description("Budget tracking, invoicing, cost optimization, governance"),
            Capability::new("content_marketing", "Content & Marketing")
                .with_domains(vec!["marketing".into(), "content".into()])
                .with_description("Copy, design, campaigns, A/B testing, engagement analysis"),
            Capability::new("quality_compliance", "Quality & Compliance")
                .with_domains(vec!["quality".into(), "compliance".into()])
                .with_description("Testing, security audits, regulatory checks, anomaly detection"),
        ];

        let roles = vec![
            // ICs
            TemplateRole {
                id: "ic_code".into(),
                title: "Code & Engineering IC".into(),
                role_type: TemplateRoleType::Ic,
                capabilities: vec!["code_engineering".into()],
                domains: vec![],
                mentees: vec![],
                briefing: None,
            },
            TemplateRole {
                id: "ic_research".into(),
                title: "Research & Data IC".into(),
                role_type: TemplateRoleType::Ic,
                capabilities: vec!["research_data".into()],
                domains: vec![],
                mentees: vec![],
                briefing: None,
            },
            TemplateRole {
                id: "ic_customer".into(),
                title: "Customer & Sales IC".into(),
                role_type: TemplateRoleType::Ic,
                capabilities: vec!["customer_sales".into()],
                domains: vec![],
                mentees: vec![],
                briefing: None,
            },
            TemplateRole {
                id: "ic_finance".into(),
                title: "Finance & Ops IC".into(),
                role_type: TemplateRoleType::Ic,
                capabilities: vec!["finance_ops".into()],
                domains: vec![],
                mentees: vec![],
                briefing: None,
            },
            TemplateRole {
                id: "ic_content".into(),
                title: "Content & Marketing IC".into(),
                role_type: TemplateRoleType::Ic,
                capabilities: vec!["content_marketing".into()],
                domains: vec![],
                mentees: vec![],
                briefing: None,
            },
            TemplateRole {
                id: "ic_quality".into(),
                title: "Quality & Compliance IC".into(),
                role_type: TemplateRoleType::Ic,
                capabilities: vec!["quality_compliance".into()],
                domains: vec![],
                mentees: vec![],
                briefing: None,
            },
            // DRIs
            TemplateRole {
                id: "dri_retention".into(),
                title: "DRI – Customer Retention".into(),
                role_type: TemplateRoleType::Dri,
                capabilities: vec![],
                domains: vec!["customer_retention".into(), "customer".into()],
                mentees: vec![],
                briefing: None,
            },
            TemplateRole {
                id: "dri_capability_dev".into(),
                title: "DRI – Capability Development".into(),
                role_type: TemplateRoleType::Dri,
                capabilities: vec![],
                domains: vec!["capability_development".into(), "engineering".into()],
                mentees: vec![],
                briefing: None,
            },
            TemplateRole {
                id: "dri_cost".into(),
                title: "DRI – Cost & Efficiency".into(),
                role_type: TemplateRoleType::Dri,
                capabilities: vec![],
                domains: vec!["cost_optimization".into(), "finance".into()],
                mentees: vec![],
                briefing: None,
            },
            TemplateRole {
                id: "dri_crisis".into(),
                title: "DRI – Crisis Response".into(),
                role_type: TemplateRoleType::Dri,
                capabilities: vec![],
                domains: vec!["crisis".into()],
                mentees: vec![],
                briefing: None,
            },
            // Player-Coaches
            TemplateRole {
                id: "pc_capabilities".into(),
                title: "Player-Coach – Capabilities Stack".into(),
                role_type: TemplateRoleType::PlayerCoach,
                capabilities: vec!["code_engineering".into(), "quality_compliance".into()],
                domains: vec!["capabilities".into()],
                mentees: vec!["ic_code".into(), "ic_quality".into()],
                briefing: None,
            },
            TemplateRole {
                id: "pc_intelligence".into(),
                title: "Player-Coach – Models & Intelligence".into(),
                role_type: TemplateRoleType::PlayerCoach,
                capabilities: vec!["research_data".into()],
                domains: vec!["intelligence".into()],
                mentees: vec!["ic_research".into()],
                briefing: None,
            },
            TemplateRole {
                id: "pc_interfaces".into(),
                title: "Player-Coach – Interfaces & Edge".into(),
                role_type: TemplateRoleType::PlayerCoach,
                capabilities: vec!["content_marketing".into(), "customer_sales".into()],
                domains: vec!["interfaces".into()],
                mentees: vec!["ic_content".into(), "ic_customer".into()],
                briefing: None,
            },
        ];

        let default_escalation = vec![
            TemplateEscalationLevel {
                role_id: "dri_capability_dev".into(),
                timeout_secs: 3600,
                action: "reassign".into(),
            },
            TemplateEscalationLevel {
                role_id: "dri_crisis".into(),
                timeout_secs: 7200,
                action: "reassign".into(),
            },
            TemplateEscalationLevel {
                role_id: "pc_capabilities".into(),
                timeout_secs: 14400,
                action: "human_review".into(),
            },
        ];

        Self {
            schema_version: 1,
            name: "Paperclip Default".into(),
            description: "Standard company-as-intelligence template with 6 capability ICs, 4 DRIs, and 3 Player-Coaches".into(),
            capabilities,
            roles,
            seed_goals: Vec::new(),
            default_escalation,
            intelligence_config: None,
            metadata: serde_json::json!({}),
        }
    }

    /// Escalation chain from [`CompanyTemplate::default_escalation`], resolved against `roles`.
    fn built_in_escalation_chain(&self) -> EscalationChain {
        let mut chain = EscalationChain::new();
        for lvl in &self.default_escalation {
            chain.push(EscalationLevel {
                assignee: resolve_assignee_from_template(self, &lvl.role_id),
                timeout_secs: lvl.timeout_secs,
                action: parse_template_escalation_action(&lvl.action),
            });
        }
        chain
    }

    /// Build a portable template from the **live** [`IntelligenceLayer`].
    ///
    /// - **Capabilities** and **goals** come from the registries / goal map.
    /// - **Roles** merge [`IntelligenceLayer::org_blueprint`] (IC / DRI / PlayerCoach taxonomy) with
    ///   the live [`DriRegistry`] so domains and mentees reflect runtime updates; DRIs only in the
    ///   registry are appended.
    /// - **Default escalation** prefers the stored blueprint; otherwise the first goal with a chain.
    /// - **Name / description** prefer the last imported template when `org_blueprint` is set.
    pub fn from_intelligence_layer(layer: &IntelligenceLayer) -> Self {
        let mut capabilities: Vec<Capability> = layer.capabilities.all().cloned().collect();
        capabilities.sort_by(|a, b| a.id.cmp(&b.id));
        let caps_len = capabilities.len();

        let roles = merge_blueprint_roles_with_dri(layer.org_blueprint.as_ref(), &layer.dri_registry);

        let mut seed_goals: Vec<TemplateGoal> =
            layer.goals.values().map(|g| goal_to_template_goal(g)).collect();
        seed_goals.sort_by(|a, b| a.title.cmp(&b.title));

        let default_escalation = layer
            .org_blueprint
            .as_ref()
            .filter(|b| !b.default_escalation.is_empty())
            .map(|b| b.default_escalation.clone())
            .unwrap_or_else(|| {
                layer
                    .goals
                    .values()
                    .find(|g| !g.escalation.levels.is_empty())
                    .map(|g| {
                        g.escalation
                            .levels
                            .iter()
                            .filter_map(|lvl| {
                                let role_id = assignee_to_role_id(&lvl.assignee);
                                if role_id.is_empty() {
                                    return None;
                                }
                                Some(TemplateEscalationLevel {
                                    role_id,
                                    timeout_secs: lvl.timeout_secs,
                                    action: escalation_action_to_template_action(&lvl.action),
                                })
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default()
            });

        let has_blueprint = layer.org_blueprint.is_some();
        let (name, description) = layer
            .org_blueprint
            .as_ref()
            .map(|b| (b.template_name.clone(), b.template_description.clone()))
            .unwrap_or_else(|| {
                (
                    "HSM-II Intelligence Layer snapshot".into(),
                    "Exported from the live runtime. Import a template (or start with one) to persist full IC/DRI/PlayerCoach taxonomy in org_blueprint.".into(),
                )
            });

        Self {
            schema_version: 1,
            name,
            description,
            capabilities,
            roles,
            seed_goals,
            default_escalation,
            intelligence_config: Some(layer.config.clone()),
            metadata: serde_json::json!({
                "source": "intelligence_layer",
                "capability_count": caps_len,
                "dri_count": layer.dri_registry.len(),
                "goal_count": layer.goals.len(),
                "has_org_blueprint": has_blueprint,
            }),
        }
    }

    /// Import this template into an IntelligenceLayer instance.
    pub fn apply_to(&self, layer: &mut IntelligenceLayer) {
        layer.org_blueprint = Some(OrgBlueprint {
            template_name: self.name.clone(),
            template_description: self.description.clone(),
            roles: self.roles.clone(),
            default_escalation: self.default_escalation.clone(),
        });

        // 1. Register capabilities
        for cap in &self.capabilities {
            layer.capabilities.register(cap.clone());
        }

        // 2. Register DRIs from role definitions
        for role in &self.roles {
            match role.role_type {
                TemplateRoleType::Dri => {
                    let entry = DriEntry::new(&role.id, &role.title, &role.id)
                        .with_domains(role.domains.clone());
                    layer.dri_registry.register(entry);
                }
                TemplateRoleType::PlayerCoach => {
                    // Player-coaches can also act as DRIs for their domains
                    if !role.domains.is_empty() {
                        let entry = DriEntry::new(&role.id, &role.title, &role.id)
                            .with_domains(role.domains.clone());
                        layer.dri_registry.register(entry);
                    }
                }
                TemplateRoleType::Ic => {}
            }
        }

        // 3. Seed goals (attach template default escalation when defined)
        let default_esc = self.built_in_escalation_chain();
        for tg in &self.seed_goals {
            let assignee = match &tg.assignee_role_id {
                None => GoalAssignee::Unassigned,
                Some(rid) if self.roles.iter().any(|r| r.id == *rid) => {
                    resolve_assignee_from_template(self, rid.as_str())
                }
                Some(_) => GoalAssignee::Unassigned,
            };

            let mut goal = Goal::new(uuid::Uuid::new_v4().to_string(), tg.title.clone())
                .with_description(tg.description.clone())
                .with_priority(tg.priority.clone())
                .with_assignee(assignee)
                .with_capabilities(tg.required_capabilities.clone());
            goal.tags = tg.tags.clone();
            if !default_esc.levels.is_empty() {
                goal.escalation = default_esc.clone();
            }
            layer.add_goal(goal);
        }

        // 4. Apply intelligence config overrides
        if let Some(ref cfg) = self.intelligence_config {
            layer.config = cfg.clone();
        }
    }

    /// Export to JSON file.
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    /// Load from JSON file.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let raw = std::fs::read_to_string(path)?;
        Ok(serde_json::from_str(&raw)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::goal::GoalPriority;

    #[test]
    fn apply_stores_blueprint_and_export_round_trips_taxonomy() {
        let mut layer = IntelligenceLayer::new();
        let t = CompanyTemplate::paperclip_default();
        t.apply_to(&mut layer);

        assert!(layer.org_blueprint.is_some());
        let bp = layer.org_blueprint.as_ref().unwrap();
        assert_eq!(bp.roles.len(), t.roles.len());
        assert_eq!(bp.default_escalation.len(), t.default_escalation.len());

        let exported = CompanyTemplate::from_intelligence_layer(&layer);
        assert_eq!(exported.roles.len(), t.roles.len());
        assert_eq!(exported.default_escalation.len(), t.default_escalation.len());
        assert_eq!(exported.name, t.name);
        assert!(exported.metadata["has_org_blueprint"].as_bool().unwrap());
    }

    #[test]
    fn seed_goal_gets_default_escalation_chain() {
        let mut t = CompanyTemplate::paperclip_default();
        t.seed_goals.push(TemplateGoal {
            title: "Smoke goal".into(),
            description: String::new(),
            priority: GoalPriority::Medium,
            assignee_role_id: Some("ic_code".into()),
            required_capabilities: vec!["code_engineering".into()],
            tags: vec![],
        });

        let mut layer = IntelligenceLayer::new();
        t.apply_to(&mut layer);

        let g = layer.goals.values().next().expect("one goal");
        assert_eq!(
            g.escalation.levels.len(),
            t.default_escalation.len(),
            "seed goals should inherit template default_escalation"
        );
    }
}
