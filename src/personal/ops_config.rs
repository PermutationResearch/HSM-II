//! Enterprise operations YAML (`config/operations.yaml`) — deserialize + validate.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

/// Supported `schema_version` in YAML.
pub const OPS_SCHEMA_VERSION: u32 = 1;

/// Default filename under `<home>/config/`.
pub const OPS_CONFIG_FILENAME: &str = "operations.yaml";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OperationsConfig {
    pub schema_version: u32,
    pub company_id: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub goals: Vec<Goal>,
    #[serde(default)]
    pub org: Option<OrgChart>,
    #[serde(default)]
    pub budgets: Vec<BudgetEntry>,
    #[serde(default)]
    pub governance: Option<GovernanceHints>,
    #[serde(default)]
    pub heartbeats: Vec<HeartbeatSpec>,
    #[serde(default)]
    pub tickets: Vec<Ticket>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Goal {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub period: String,
    #[serde(default)]
    pub key_results: Vec<KeyResult>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KeyResult {
    pub metric: String,
    pub target: serde_json::Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OrgChart {
    #[serde(default)]
    pub root_role: String,
    #[serde(default)]
    pub roles: Vec<OrgRole>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OrgRole {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub delegates_to: Vec<String>,
    #[serde(default)]
    pub agent_persona: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BudgetScope {
    Company,
    Role,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BudgetEntry {
    pub scope: BudgetScope,
    pub id: String,
    #[serde(default)]
    pub role_id: Option<String>,
    pub kind: String,
    pub cap_monthly: f64,
    #[serde(default)]
    pub hard_stop: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct GovernanceHints {
    #[serde(default)]
    pub default_tool_allow: Vec<String>,
    #[serde(default)]
    pub approval_required_prefixes: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HeartbeatSpec {
    pub id: String,
    #[serde(default)]
    pub interval_minutes: u64,
    pub action: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Ticket {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub owner_role: String,
    #[serde(default)]
    pub requester_role: String,
    #[serde(default)]
    pub budget_ticket_usd: Option<f64>,
    #[serde(default)]
    pub delegated_to: Option<String>,
    #[serde(default)]
    pub created: String,
    #[serde(default)]
    pub updated: String,
}

/// Resolve path: `HSM_OPERATIONS_YAML` if set, else `<home>/config/operations.yaml`.
pub fn resolve_ops_config_path(home: &Path) -> PathBuf {
    if let Ok(p) = std::env::var("HSM_OPERATIONS_YAML") {
        let t = p.trim();
        if !t.is_empty() {
            return PathBuf::from(t);
        }
    }
    home.join("config").join(OPS_CONFIG_FILENAME)
}

/// Read and parse YAML. Fails if file is missing (caller may handle).
pub fn load_ops_config(path: &Path) -> Result<OperationsConfig> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("read operations config {}", path.display()))?;
    let cfg: OperationsConfig =
        serde_yaml::from_str(&raw).with_context(|| format!("parse YAML {}", path.display()))?;
    Ok(cfg)
}

impl OperationsConfig {
    /// Validate cross-field rules. Call after deserialize.
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != OPS_SCHEMA_VERSION {
            return Err(anyhow!(
                "unsupported schema_version {} (expected {})",
                self.schema_version,
                OPS_SCHEMA_VERSION
            ));
        }
        if self.company_id.trim().is_empty() {
            return Err(anyhow!("company_id must be non-empty"));
        }

        let goal_ids: HashSet<&str> = self.goals.iter().map(|g| g.id.as_str()).collect();
        if goal_ids.len() != self.goals.len() {
            return Err(anyhow!("duplicate goal id"));
        }

        if let Some(org) = &self.org {
            let role_ids: HashSet<&str> = org.roles.iter().map(|r| r.id.as_str()).collect();
            if role_ids.len() != org.roles.len() {
                return Err(anyhow!("duplicate org role id"));
            }
            if !org.root_role.is_empty() && !role_ids.contains(org.root_role.as_str()) {
                return Err(anyhow!(
                    "org.root_role {:?} not found in org.roles",
                    org.root_role
                ));
            }
        }

        let budget_ids: HashSet<&str> = self.budgets.iter().map(|b| b.id.as_str()).collect();
        if budget_ids.len() != self.budgets.len() {
            return Err(anyhow!("duplicate budget id"));
        }
        for b in &self.budgets {
            if b.cap_monthly < 0.0 {
                return Err(anyhow!("budget {}: cap_monthly must be >= 0", b.id));
            }
            if b.scope == BudgetScope::Role {
                let rid = b
                    .role_id
                    .as_ref()
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty());
                if rid.is_none() {
                    return Err(anyhow!(
                        "budget {}: role scope requires non-empty role_id",
                        b.id
                    ));
                }
            }
        }

        let ticket_ids: HashSet<&str> = self.tickets.iter().map(|t| t.id.as_str()).collect();
        if ticket_ids.len() != self.tickets.len() {
            return Err(anyhow!("duplicate ticket id"));
        }

        Ok(())
    }

    /// Subset for agents: goals, org, budgets, governance, heartbeats (no tickets).
    pub fn summary_without_tickets(&self) -> serde_json::Value {
        serde_json::json!({
            "schema_version": self.schema_version,
            "company_id": self.company_id,
            "display_name": self.display_name,
            "goals": self.goals,
            "org": self.org,
            "budgets": self.budgets,
            "governance": self.governance,
            "heartbeats": self.heartbeats,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_example_shape() {
        let yaml = r#"
schema_version: 1
company_id: test-co
display_name: Test
goals:
  - id: g1
    title: T
    key_results:
      - metric: m
        target: 10
org:
  root_role: ceo
  roles:
    - id: ceo
      title: CEO
      delegates_to: [sales]
    - id: sales
      title: Sales
budgets:
  - scope: company
    id: b1
    kind: llm_usd
    cap_monthly: 100
    hard_stop: true
tickets:
  - id: t1
    title: Do thing
    state: open
"#;
        let cfg: OperationsConfig = serde_yaml::from_str(yaml).unwrap();
        cfg.validate().unwrap();
    }

    #[test]
    fn rejects_role_budget_without_role_id() {
        let yaml = r#"
schema_version: 1
company_id: x
budgets:
  - scope: role
    id: b1
    kind: llm_usd
    cap_monthly: 1
tickets: []
"#;
        let cfg: OperationsConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(cfg.validate().is_err());
    }
}
