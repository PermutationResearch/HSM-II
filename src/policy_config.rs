//! Optional YAML policy (`HSM_POLICY_FILE`) — tool hints + context tier mapping for manifests.
//!
//! Does not replace env-based security gates; extends them for operators who want file-based config.

use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

use serde::Deserialize;

use crate::context_manifest::ContextTier;

static POLICY: OnceLock<LoadedPolicy> = OnceLock::new();

/// Call after `dotenv` so `HSM_POLICY_FILE` is visible.
pub fn ensure_loaded() {
    let _ = POLICY.get_or_init(LoadedPolicy::from_env);
}

pub fn get() -> &'static LoadedPolicy {
    POLICY.get_or_init(LoadedPolicy::from_env)
}

#[derive(Clone, Debug, Default)]
pub struct LoadedPolicy {
    /// Section key (lowercase) → tier for personal-agent prompt manifests.
    pub section_tiers: HashMap<String, ContextTier>,
    /// Keys for company task LLM context (`company`, `shared_memory`, `agent_memory`, `task`, `agent_profile`).
    pub company_llm_section_tiers: HashMap<String, ContextTier>,
    /// Extra tool names to treat as denied (merged with dedicated env gates elsewhere).
    pub tools_deny: Vec<String>,
    pub source_path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PolicyFileRaw {
    #[serde(default)]
    context_tiers: Option<ContextTiersRaw>,
    /// Tiers for `GET /api/company/tasks/.../llm-context` section byte accounting.
    #[serde(default)]
    company_llm_context_tiers: Option<ContextTiersRaw>,
    #[serde(default)]
    tools: Option<ToolsRaw>,
}

#[derive(Debug, Deserialize)]
struct ContextTiersRaw {
    #[serde(default)]
    hot: Vec<String>,
    #[serde(default)]
    warm: Vec<String>,
    #[serde(default)]
    cold: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ToolsRaw {
    #[serde(default)]
    deny: Vec<String>,
}

fn default_company_llm_section_tiers() -> HashMap<String, ContextTier> {
    let mut m = HashMap::new();
    for k in ["company", "task", "agent_profile"] {
        m.insert(k.to_string(), ContextTier::Hot);
    }
    for k in ["shared_memory", "agent_memory"] {
        m.insert(k.to_string(), ContextTier::Warm);
    }
    m
}

impl LoadedPolicy {
    fn from_env() -> Self {
        let path = std::env::var("HSM_POLICY_FILE").ok();
        let Some(ref p) = path else {
            return Self::default_builtins();
        };
        let p = Path::new(p);
        if !p.is_file() {
            tracing::warn!(
                target: "hsm.policy",
                path = %p.display(),
                "HSM_POLICY_FILE set but file missing — using built-in tier map"
            );
            return Self::default_builtins();
        }
        match std::fs::read_to_string(p) {
            Ok(s) => Self::parse_yaml(&s, Some(p.display().to_string())),
            Err(e) => {
                tracing::warn!(target: "hsm.policy", error = %e, "failed to read policy file");
                Self::default_builtins()
            }
        }
    }

    fn default_builtins() -> Self {
        let mut section_tiers = HashMap::new();
        for k in ["living", "memory"] {
            section_tiers.insert(k.to_string(), ContextTier::Hot);
        }
        for k in [
            "route",
            "business",
            "belief",
            "skill",
            "autocontext",
            "md_skills",
        ] {
            section_tiers.insert(k.to_string(), ContextTier::Warm);
        }
        for k in ["prefetch", "tail"] {
            section_tiers.insert(k.to_string(), ContextTier::Cold);
        }
        Self {
            section_tiers,
            company_llm_section_tiers: default_company_llm_section_tiers(),
            tools_deny: Vec::new(),
            source_path: None,
        }
    }

    fn parse_yaml(content: &str, source_path: Option<String>) -> Self {
        let raw: Result<PolicyFileRaw, _> = serde_yaml::from_str(content);
        let Ok(raw) = raw else {
            tracing::warn!(target: "hsm.policy", "invalid policy YAML — using built-in tier map");
            return Self::default_builtins();
        };

        let mut section_tiers = Self::default_builtins().section_tiers;
        if let Some(t) = raw.context_tiers {
            for k in t.hot {
                section_tiers.insert(k.trim().to_ascii_lowercase(), ContextTier::Hot);
            }
            for k in t.warm {
                section_tiers.insert(k.trim().to_ascii_lowercase(), ContextTier::Warm);
            }
            for k in t.cold {
                section_tiers.insert(k.trim().to_ascii_lowercase(), ContextTier::Cold);
            }
        }

        let mut company_llm_section_tiers = Self::default_builtins().company_llm_section_tiers;
        if let Some(t) = raw.company_llm_context_tiers {
            for k in t.hot {
                company_llm_section_tiers.insert(k.trim().to_ascii_lowercase(), ContextTier::Hot);
            }
            for k in t.warm {
                company_llm_section_tiers.insert(k.trim().to_ascii_lowercase(), ContextTier::Warm);
            }
            for k in t.cold {
                company_llm_section_tiers.insert(k.trim().to_ascii_lowercase(), ContextTier::Cold);
            }
        }

        let tools_deny = raw
            .tools
            .map(|t| {
                t.deny
                    .into_iter()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default();

        Self {
            section_tiers,
            company_llm_section_tiers,
            tools_deny,
            source_path,
        }
    }

    pub fn tier_for_section(&self, key: &str) -> ContextTier {
        self.section_tiers
            .get(&key.to_ascii_lowercase())
            .copied()
            .unwrap_or(ContextTier::Warm)
    }

    pub fn tier_for_company_llm_section(&self, key: &str) -> ContextTier {
        self.company_llm_section_tiers
            .get(&key.to_ascii_lowercase())
            .copied()
            .unwrap_or(ContextTier::Warm)
    }

    /// Deny list from policy file only (env-based denies stay in harness_gate etc.).
    pub fn policy_tool_deny(&self) -> &[String] {
        &self.tools_deny
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_yaml() {
        let yaml = r#"
version: 1
context_tiers:
  hot: [living]
  cold: [tail]
tools:
  deny: [danger_tool]
"#;
        let p = LoadedPolicy::parse_yaml(yaml, None);
        assert_eq!(p.tier_for_section("living"), ContextTier::Hot);
        assert_eq!(p.tier_for_section("tail"), ContextTier::Cold);
        assert!(p.tools_deny.contains(&"danger_tool".to_string()));
    }

    #[test]
    fn parse_company_llm_tiers_yaml() {
        let yaml = r#"
company_llm_context_tiers:
  hot: [task]
  cold: [shared_memory]
"#;
        let p = LoadedPolicy::parse_yaml(yaml, None);
        assert_eq!(p.tier_for_company_llm_section("task"), ContextTier::Hot);
        assert_eq!(
            p.tier_for_company_llm_section("shared_memory"),
            ContextTier::Cold
        );
    }
}
