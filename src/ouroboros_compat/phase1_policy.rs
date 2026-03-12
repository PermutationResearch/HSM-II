use super::{ActionKind, ProposedAction};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Policy decision outcome for a proposed action.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum PolicyDecision {
    Allow,
    ReviewRequired,
    Deny,
}

/// Full policy verdict with concrete reasons.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PolicyVerdict {
    pub decision: PolicyDecision,
    pub reasons: Vec<String>,
}

/// Release invariants expected before self-modification can proceed.
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct ReleaseState {
    pub version: Option<String>,
    pub git_tag: Option<String>,
    pub readme_version: Option<String>,
}

/// Execution context for policy checks.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PolicyContext {
    pub requested_by: String,
    pub release_state: ReleaseState,
}

/// Constitution and identity guard settings.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConstitutionConfig {
    pub creator_id: String,
    pub identity_core_paths: Vec<String>,
    pub enforce_release_invariants: bool,
    pub non_creator_self_mod_requires_review: bool,
}

impl Default for ConstitutionConfig {
    fn default() -> Self {
        Self {
            creator_id: "creator".to_string(),
            identity_core_paths: vec![
                "BIBLE.md".to_string(),
                "identity.md".to_string(),
                "supervisor/state.py".to_string(),
            ],
            enforce_release_invariants: true,
            non_creator_self_mod_requires_review: true,
        }
    }
}

pub struct PolicyEngine {
    config: ConstitutionConfig,
}

impl PolicyEngine {
    pub fn new(config: ConstitutionConfig) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &ConstitutionConfig {
        &self.config
    }

    pub fn evaluate(&self, action: &ProposedAction, ctx: &PolicyContext) -> PolicyVerdict {
        let mut reasons = Vec::new();
        let mut decision = PolicyDecision::Allow;

        if self.is_identity_core_write(action) {
            decision = PolicyDecision::Deny;
            reasons.push("identity core path is protected".to_string());
        }

        if self.config.non_creator_self_mod_requires_review
            && matches!(action.kind, ActionKind::SelfModification)
            && ctx.requested_by != self.config.creator_id
        {
            if decision != PolicyDecision::Deny {
                decision = PolicyDecision::ReviewRequired;
            }
            reasons.push("non-creator self-modification requires council review".to_string());
        }

        if self.config.enforce_release_invariants
            && matches!(action.kind, ActionKind::SelfModification)
            && !release_invariants_hold(&ctx.release_state)
        {
            if decision != PolicyDecision::Deny {
                decision = PolicyDecision::ReviewRequired;
            }
            reasons.push(
                "release invariants are not satisfied (version/tag/readme mismatch)".to_string(),
            );
        }

        PolicyVerdict { decision, reasons }
    }

    fn is_identity_core_write(&self, action: &ProposedAction) -> bool {
        if !matches!(
            action.kind,
            ActionKind::SelfModification | ActionKind::ExternalWrite
        ) {
            return false;
        }
        let Some(path) = &action.target_path else {
            return false;
        };
        let path_norm = normalize(path);
        self.config
            .identity_core_paths
            .iter()
            .map(|p| normalize(p))
            .any(|core| path_norm == core || path_norm.ends_with(&format!("/{core}")))
    }
}

fn normalize(path: &str) -> String {
    Path::new(path)
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("/")
}

fn release_invariants_hold(state: &ReleaseState) -> bool {
    match (&state.version, &state.git_tag, &state.readme_version) {
        (Some(v), Some(tag), Some(readme)) => v == tag && v == readme,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_action(kind: ActionKind, path: Option<&str>) -> ProposedAction {
        ProposedAction {
            id: "a1".to_string(),
            title: "x".to_string(),
            description: "x".to_string(),
            actor_id: "agent".to_string(),
            kind,
            target_path: path.map(|v| v.to_string()),
            target_peer: None,
            metadata: Default::default(),
        }
    }

    #[test]
    fn denies_identity_core_write() {
        let engine = PolicyEngine::new(ConstitutionConfig::default());
        let action = sample_action(ActionKind::SelfModification, Some("identity.md"));
        let verdict = engine.evaluate(
            &action,
            &PolicyContext {
                requested_by: "creator".to_string(),
                release_state: ReleaseState {
                    version: Some("1.0.0".to_string()),
                    git_tag: Some("1.0.0".to_string()),
                    readme_version: Some("1.0.0".to_string()),
                },
            },
        );
        assert_eq!(verdict.decision, PolicyDecision::Deny);
    }

    #[test]
    fn marks_non_creator_self_mod_for_review() {
        let engine = PolicyEngine::new(ConstitutionConfig::default());
        let action = sample_action(ActionKind::SelfModification, Some("src/app.py"));
        let verdict = engine.evaluate(
            &action,
            &PolicyContext {
                requested_by: "not-creator".to_string(),
                release_state: ReleaseState {
                    version: Some("1.0.0".to_string()),
                    git_tag: Some("1.0.0".to_string()),
                    readme_version: Some("1.0.0".to_string()),
                },
            },
        );
        assert_eq!(verdict.decision, PolicyDecision::ReviewRequired);
    }
}
