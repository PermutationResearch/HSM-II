//! Persistent human-approval store for sensitive tool actions.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

use super::control_plane::RuntimeConfig;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalOutcome {
    Allow,
    Deny,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApprovalRule {
    pub key: String,
    pub outcome: ApprovalOutcome,
    pub scope: String,
    pub actor: String,
    pub updated_unix: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PendingApproval {
    pub id: String,
    pub key: String,
    pub summary: String,
    pub created_unix: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ApprovalStore {
    pub rules: Vec<ApprovalRule>,
    pub pending: Vec<PendingApproval>,
}

#[derive(Clone, Debug)]
pub struct ApprovalService {
    store_path: PathBuf,
    interactive: bool,
}

impl ApprovalService {
    pub fn from_env() -> Self {
        let cfg = RuntimeConfig::from_env();
        Self {
            store_path: cfg.approvals.store_path,
            interactive: cfg.approvals.interactive,
        }
    }

    fn load(&self) -> Result<ApprovalStore> {
        if !self.store_path.exists() {
            return Ok(ApprovalStore::default());
        }
        let raw = fs::read_to_string(&self.store_path)?;
        Ok(serde_json::from_str(&raw)?)
    }

    fn save(&self, store: &ApprovalStore) -> Result<()> {
        if let Some(parent) = self.store_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&self.store_path, serde_json::to_vec_pretty(store)?)?;
        Ok(())
    }

    pub fn evaluate_or_queue(&self, key: &str, summary: &str) -> Result<ApprovalOutcome> {
        let mut store = self.load()?;
        if let Some(rule) = store.rules.iter().find(|r| r.key == key) {
            return Ok(rule.outcome.clone());
        }
        if self.interactive {
            if !store.pending.iter().any(|p| p.key == key) {
                store.pending.push(PendingApproval {
                    id: format!("appr_{}", uuid::Uuid::new_v4()),
                    key: key.to_string(),
                    summary: summary.to_string(),
                    created_unix: now_unix(),
                });
                self.save(&store)?;
            }
            return Err(anyhow!(
                "approval required for `{}`; use /approve list then /approve allow|deny <key>",
                key
            ));
        }
        Ok(ApprovalOutcome::Deny)
    }

    pub fn list_pending(&self) -> Result<Vec<PendingApproval>> {
        Ok(self.load()?.pending)
    }

    pub fn decide(&self, key: &str, outcome: ApprovalOutcome, actor: &str) -> Result<()> {
        let mut store = self.load()?;
        store.pending.retain(|p| p.key != key);
        let mut index: HashMap<String, usize> = HashMap::new();
        for (i, rule) in store.rules.iter().enumerate() {
            index.insert(rule.key.clone(), i);
        }
        let rule = ApprovalRule {
            key: key.to_string(),
            outcome,
            scope: "global".to_string(),
            actor: actor.to_string(),
            updated_unix: now_unix(),
        };
        if let Some(i) = index.get(key).copied() {
            store.rules[i] = rule;
        } else {
            store.rules.push(rule);
        }
        self.save(&store)
    }
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
