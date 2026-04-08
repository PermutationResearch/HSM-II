//! Phase 1 — single policy gate for tools (extends [`ToolPermissionContext`]) + skill path checks.

use std::collections::HashSet;

use crate::harness::{HarnessRunEnvelope, LhActorRole, LhContextScope};

use super::ToolPermissionContext;

#[derive(Clone, Debug)]
pub struct HarnessPolicyGate {
    pub tool_permissions: ToolPermissionContext,
    subagent_isolated_deny: HashSet<String>,
    subagent_isolated_deny_prefixes: Vec<String>,
    /// Tools always denied when this set is non-empty (gap 7 — network-ish knobs).
    network_denied_tools: HashSet<String>,
    /// `tools.deny` from `HSM_POLICY_FILE` (YAML), if loaded.
    policy_denied_tools: HashSet<String>,
}

impl HarnessPolicyGate {
    pub fn new(tool_permissions: ToolPermissionContext) -> Self {
        let (subagent_isolated_deny, subagent_isolated_deny_prefixes) = subagent_deny_from_env();
        Self {
            tool_permissions,
            subagent_isolated_deny,
            subagent_isolated_deny_prefixes,
            network_denied_tools: network_deny_from_env(),
            policy_denied_tools: policy_file_deny_tools(),
        }
    }

    pub fn from_env_permissions() -> Self {
        Self::new(ToolPermissionContext::from_env())
    }

    pub fn validate_skill_relative_path(rel: &str) -> Result<(), String> {
        let s = rel.trim().trim_start_matches(['/', '\\']);
        if s.is_empty() {
            return Err("empty relative path".into());
        }
        if s.contains("..") {
            return Err("path must not contain '..'".into());
        }
        Ok(())
    }

    pub fn check_tool(
        &self,
        tool_name: &str,
        envelope: Option<&HarnessRunEnvelope>,
    ) -> Result<(), String> {
        if self.policy_denied_tools.contains(tool_name) {
            return Err(format!(
                "tool '{tool_name}' denied by policy file (tools.deny in HSM_POLICY_FILE)"
            ));
        }
        self.tool_permissions.check(tool_name)?;

        if !self.network_denied_tools.is_empty() && self.network_denied_tools.contains(tool_name) {
            return Err(format!(
                "tool '{tool_name}' denied by HSM_TOOL_DENY_NETWORK"
            ));
        }

        let Some(env) = envelope else {
            return Ok(());
        };

        if env.actor == LhActorRole::Subagent && env.context_scope == LhContextScope::Isolated {
            if self.subagent_isolated_deny.contains(tool_name) {
                return Err(format!(
                    "tool '{tool_name}' denied for isolated subagent (HSM_SUBAGENT_ISOLATED_DENY)"
                ));
            }
            for p in &self.subagent_isolated_deny_prefixes {
                if tool_name.starts_with(p) {
                    return Err(format!(
                        "tool '{tool_name}' denied for isolated subagent (prefix '{p}', HSM_SUBAGENT_ISOLATED_DENY_PREFIXES)"
                    ));
                }
            }
        }

        Ok(())
    }
}

fn policy_file_deny_tools() -> HashSet<String> {
    crate::policy_config::ensure_loaded();
    crate::policy_config::get()
        .policy_tool_deny()
        .iter()
        .cloned()
        .collect()
}

fn network_deny_from_env() -> HashSet<String> {
    std::env::var("HSM_TOOL_DENY_NETWORK")
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn subagent_deny_from_env() -> (HashSet<String>, Vec<String>) {
    let default = "bash,write_file,edit_file,git_push,git_commit,webhook_send,http_request";
    let raw = std::env::var("HSM_SUBAGENT_ISOLATED_DENY").unwrap_or_else(|_| default.to_string());
    let exact: HashSet<String> = raw
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let prefixes = std::env::var("HSM_SUBAGENT_ISOLATED_DENY_PREFIXES")
        .unwrap_or_else(|_| "mcp_".to_string())
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    (exact, prefixes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skill_path_rejects_dotdot() {
        assert!(HarnessPolicyGate::validate_skill_relative_path("a/../b").is_err());
        assert!(HarnessPolicyGate::validate_skill_relative_path("references/x.md").is_ok());
    }

    #[test]
    fn isolated_subagent_denies_bash() {
        let gate = HarnessPolicyGate::new(ToolPermissionContext::permissive());
        let env = HarnessRunEnvelope {
            actor: LhActorRole::Subagent,
            context_scope: LhContextScope::Isolated,
            ..HarnessRunEnvelope::lead_thread("t1")
        };
        assert!(gate.check_tool("bash", Some(&env)).is_err());
        assert!(gate.check_tool("bash", None).is_ok());
    }

    #[test]
    fn allowlist_still_applies() {
        let gate = HarnessPolicyGate::new(ToolPermissionContext::allow_only(["read_file"]));
        let env = HarnessRunEnvelope::lead_thread("t1");
        assert!(gate.check_tool("read_file", Some(&env)).is_ok());
        assert!(gate.check_tool("bash", Some(&env)).is_err());
    }
}
