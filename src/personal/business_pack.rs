//! Config-driven **business context** for personal / integrated agents.
//!
//! ## Precedence & limits
//! See `documentation/guides/BUSINESS_PACK.md` in the repo for:
//! - `HSM_BUSINESS_PERSONA` vs `config.json` → `business_persona`
//! - Injection merge order and byte/token budgets
//!
//! Drop a pack under the agent home (same tree as `MEMORY.md` / `config.json`):
//! - **`business/pack.yaml`** (preferred), or
//! - **`business_pack.yaml`** at the home root.
//!
//! Email / ticket ingestion is **your** pipeline: export threads to `business/knowledge/*.md`
//! and list them under `knowledge_files`.

use anyhow::Context;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Only `1` is accepted until a migration story exists for newer schemas.
pub const SUPPORTED_SCHEMA_VERSION: u32 = 1;

pub const MAX_POLICY_FILE_BYTES: usize = 48 * 1024;
pub const MAX_KNOWLEDGE_FILE_BYTES: usize = 64 * 1024;
pub const MAX_TOTAL_INJECTED_BYTES: usize = 120 * 1024;

/// Result of [`BusinessPack::validate`].
#[derive(Debug, Clone, Default)]
pub struct PackValidationReport {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

impl PackValidationReport {
    pub fn ok(&self) -> bool {
        self.errors.is_empty()
    }

    pub fn format_cli(&self) -> String {
        let mut s = String::new();
        if !self.errors.is_empty() {
            s.push_str("ERRORS:\n");
            for e in &self.errors {
                s.push_str("  - ");
                s.push_str(e);
                s.push('\n');
            }
        }
        if !self.warnings.is_empty() {
            s.push_str("WARNINGS:\n");
            for w in &self.warnings {
                s.push_str("  - ");
                s.push_str(w);
                s.push('\n');
            }
        }
        if s.is_empty() {
            s.push_str("OK: no issues.\n");
        }
        s
    }
}

/// Root document (YAML).
#[derive(Debug, Clone, Deserialize)]
pub struct BusinessPack {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    pub company: CompanyProfile,
    #[serde(default)]
    pub shared_policies: Vec<String>,
    #[serde(default)]
    pub personas: HashMap<String, BusinessPersona>,
    #[serde(default)]
    pub knowledge_files: Vec<String>,
    #[serde(default)]
    pub policy_files: Vec<String>,
    /// ISO-8601 date string recommended (e.g. `2026-03-31`) for ops review.
    #[serde(default)]
    pub last_reviewed: Option<String>,
    #[serde(skip)]
    loaded_policy_text: Vec<String>,
    #[serde(skip)]
    loaded_knowledge_text: Vec<String>,
    #[serde(skip)]
    pub pack_root: PathBuf,
}

fn default_schema_version() -> u32 {
    1
}

#[derive(Debug, Clone, Deserialize)]
pub struct CompanyProfile {
    pub name: String,
    #[serde(default)]
    pub industry: String,
    #[serde(default)]
    pub jurisdictions: Vec<String>,
    #[serde(default)]
    pub one_liner: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct BusinessPersona {
    pub title: String,
    #[serde(default)]
    pub responsibilities: Vec<String>,
    #[serde(default)]
    pub system_instructions: String,
    #[serde(default)]
    pub must_never: Vec<String>,
    #[serde(default)]
    pub escalation: String,
    #[serde(default)]
    pub extra_files: Vec<String>,
}

fn path_safe_rel(rel: &str) -> bool {
    let t = rel.trim();
    !t.is_empty()
        && !t.contains("..")
        && !Path::new(t).is_absolute()
}

impl BusinessPack {
    /// Resolve `business/pack.yaml` or `business_pack.yaml` under `agent_home`.
    pub fn resolve_yaml_path(agent_home: &Path) -> Option<(PathBuf, PathBuf)> {
        let opt1 = agent_home.join("business").join("pack.yaml");
        let opt2 = agent_home.join("business_pack.yaml");
        if opt1.is_file() {
            Some((agent_home.join("business"), opt1))
        } else if opt2.is_file() {
            Some((agent_home.to_path_buf(), opt2))
        } else {
            None
        }
    }

    /// Pack root for a standalone `pack.yaml` file (for CLI validate).
    pub fn pack_root_for_yaml_file(yaml_path: &Path) -> PathBuf {
        let parent = yaml_path.parent().unwrap_or_else(|| Path::new("."));
        if yaml_path.file_name().and_then(|n| n.to_str()) == Some("pack.yaml")
            && parent.file_name().and_then(|n| n.to_str()) == Some("business")
        {
            parent.to_path_buf()
        } else {
            parent.to_path_buf()
        }
    }

    /// Parse YAML from disk (sync). Does not load attached file bodies.
    pub fn load_yaml_file_sync(yaml_path: &Path) -> anyhow::Result<Self> {
        let raw = std::fs::read_to_string(yaml_path)
            .with_context(|| format!("read {}", yaml_path.display()))?;
        let mut pack: BusinessPack = serde_yaml::from_str(&raw)
            .with_context(|| format!("parse YAML {}", yaml_path.display()))?;
        pack.pack_root = Self::pack_root_for_yaml_file(yaml_path);
        Ok(pack)
    }

    /// Validate logical consistency and referenced paths (files must exist on disk).
    pub fn validate(&self) -> PackValidationReport {
        let mut r = PackValidationReport::default();

        if self.schema_version != SUPPORTED_SCHEMA_VERSION {
            r.errors.push(format!(
                "schema_version must be {} (got {})",
                SUPPORTED_SCHEMA_VERSION, self.schema_version
            ));
        }

        if self.company.name.trim().is_empty() {
            r.errors.push("company.name must be non-empty".into());
        }

        if self.last_reviewed.as_ref().map(|s| s.trim().is_empty()).unwrap_or(true) {
            r.warnings.push(
                "last_reviewed is unset or empty — set an ISO date when you review this pack."
                    .into(),
            );
        }

        if self.personas.is_empty() {
            r.warnings.push(
                "no personas defined — only shared company context will apply when a persona is selected."
                    .into(),
            );
        }

        for rel in &self.policy_files {
            if !path_safe_rel(rel) {
                r.errors
                    .push(format!("policy_files: unsafe or empty path {:?}", rel));
                continue;
            }
            let p = self.pack_root.join(rel);
            if !p.is_file() {
                r.warnings
                    .push(format!("policy_files: missing file {}", p.display()));
            }
        }

        for rel in &self.knowledge_files {
            if !path_safe_rel(rel) {
                r.errors
                    .push(format!("knowledge_files: unsafe or empty path {:?}", rel));
                continue;
            }
            let p = self.pack_root.join(rel);
            if !p.is_file() {
                r.warnings
                    .push(format!("knowledge_files: missing file {}", p.display()));
            }
        }

        for (pid, pers) in &self.personas {
            if pid.trim().is_empty() {
                r.errors.push("persona id must not be empty".into());
            }
            if pers.title.trim().is_empty() {
                r.warnings.push(format!("persona {:?}: empty title", pid));
            }
            if pers.system_instructions.trim().is_empty() {
                r.warnings.push(format!(
                    "persona {:?}: system_instructions empty — model may drift",
                    pid
                ));
            }
            for rel in &pers.extra_files {
                if !path_safe_rel(rel) {
                    r.errors.push(format!(
                        "persona {:?} extra_files: unsafe or empty path {:?}",
                        pid, rel
                    ));
                    continue;
                }
                let p = self.pack_root.join(rel);
                if !p.is_file() {
                    r.warnings.push(format!(
                        "persona {:?} extra_files: missing file {}",
                        pid,
                        p.display()
                    ));
                }
            }
        }

        r
    }

    /// Policy + knowledge + optional persona `extra_files` basenames (for logging only).
    pub fn bound_asset_paths(&self, persona_id: Option<&str>) -> Vec<String> {
        let mut v: Vec<String> = self.policy_files.iter().cloned().collect();
        v.extend(self.knowledge_files.iter().cloned());
        if let Some(pid) = persona_id.filter(|s| !s.trim().is_empty()) {
            if let Some(p) = self.personas.get(pid) {
                v.extend(p.extra_files.iter().cloned());
            }
        }
        v
    }

    /// Load pack if present; validates before returning. Warnings are logged via `tracing`.
    pub async fn try_load(agent_home: &Path) -> anyhow::Result<Option<(Self, PathBuf)>> {
        let Some((pack_root, yaml_path)) = Self::resolve_yaml_path(agent_home) else {
            return Ok(None);
        };

        let raw = tokio::fs::read_to_string(&yaml_path)
            .await
            .with_context(|| format!("read {}", yaml_path.display()))?;
        let mut pack: BusinessPack = serde_yaml::from_str(&raw)
            .with_context(|| format!("parse business pack {}", yaml_path.display()))?;

        pack.pack_root = pack_root.clone();
        let report = pack.validate();
        for w in &report.warnings {
            tracing::warn!(target: "hsm_business_pack", "{}", w);
        }
        if !report.ok() {
            anyhow::bail!(
                "business pack validation failed:\n{}",
                report.format_cli()
            );
        }

        pack.load_attached_files(&pack_root).await?;

        Ok(Some((pack, pack_root)))
    }

    async fn load_attached_files(&mut self, pack_root: &Path) -> anyhow::Result<()> {
        for rel in &self.policy_files.clone() {
            if !path_safe_rel(rel) {
                continue;
            }
            if let Some(text) = Self::read_capped(pack_root.join(rel), MAX_POLICY_FILE_BYTES).await?
            {
                self.loaded_policy_text
                    .push(format!("### policy file: {rel}\n{text}"));
            }
        }
        for rel in &self.knowledge_files.clone() {
            if !path_safe_rel(rel) {
                continue;
            }
            if let Some(text) =
                Self::read_capped(pack_root.join(rel), MAX_KNOWLEDGE_FILE_BYTES).await?
            {
                self.loaded_knowledge_text
                    .push(format!("### knowledge: {rel}\n{text}"));
            }
        }
        Ok(())
    }

    async fn read_capped(path: PathBuf, max: usize) -> anyhow::Result<Option<String>> {
        if !path.is_file() {
            tracing::warn!(path = %path.display(), "business pack referenced file missing");
            return Ok(None);
        }
        let bytes = tokio::fs::read(&path).await?;
        let slice = if bytes.len() > max {
            tracing::warn!(
                path = %path.display(),
                len = bytes.len(),
                max,
                "business pack file truncated"
            );
            &bytes[..max]
        } else {
            &bytes[..]
        };
        let text = String::from_utf8_lossy(slice).to_string();
        Ok(Some(text))
    }

    /// Markdown-ish block for system prompt injection.
    pub fn render_prompt_addon(&self, persona_id: Option<&str>) -> String {
        let mut out = String::from("\n\n## Business pack (configured)\n");
        if let Some(ref d) = self.last_reviewed {
            if !d.trim().is_empty() {
                out.push_str(&format!("- **Pack last reviewed:** {d}\n"));
            }
        }
        out.push_str(&format!(
            "- **Company:** {} — _{}_\n",
            self.company.name,
            self.company.one_liner
        ));
        if !self.company.industry.is_empty() {
            out.push_str(&format!("- **Industry / vertical:** {}\n", self.company.industry));
        }
        if !self.company.jurisdictions.is_empty() {
            out.push_str(&format!(
                "- **Jurisdictions:** {}\n",
                self.company.jurisdictions.join(", ")
            ));
        }
        if !self.shared_policies.is_empty() {
            out.push_str("\n### Shared policies\n");
            for p in &self.shared_policies {
                out.push_str(&format!("- {p}\n"));
            }
        }
        if !self.loaded_policy_text.is_empty() {
            out.push_str("\n### Policy excerpts (files)\n");
            for chunk in &self.loaded_policy_text {
                out.push_str(chunk);
                out.push_str("\n\n");
            }
        }
        if !self.loaded_knowledge_text.is_empty() {
            out.push_str("\n### Knowledge excerpts (files)\n");
            let mut total = out.len();
            for chunk in &self.loaded_knowledge_text {
                if total + chunk.len() > MAX_TOTAL_INJECTED_BYTES {
                    out.push_str("\n_(Further knowledge truncated to token budget.)_\n");
                    break;
                }
                out.push_str(chunk);
                out.push_str("\n\n");
                total = out.len();
            }
        }

        if let Some(pid) = persona_id.filter(|s| !s.is_empty()) {
            if let Some(p) = self.personas.get(pid) {
                out.push_str(&format!("\n### Active persona: `{pid}` — {}\n", p.title));
                if !p.responsibilities.is_empty() {
                    out.push_str("**Focus:**\n");
                    for r in &p.responsibilities {
                        out.push_str(&format!("- {r}\n"));
                    }
                }
                if !p.system_instructions.is_empty() {
                    out.push_str("\n**Instructions:**\n");
                    out.push_str(&p.system_instructions);
                    out.push('\n');
                }
                if !p.must_never.is_empty() {
                    out.push_str("\n**Must never:**\n");
                    for m in &p.must_never {
                        out.push_str(&format!("- {m}\n"));
                    }
                }
                if !p.escalation.is_empty() {
                    out.push_str(&format!("\n**Escalation:** {}\n", p.escalation));
                }
                if !p.extra_files.is_empty() {
                    out.push_str("\n**Attached notes (this persona):**\n");
                    for rel in &p.extra_files {
                        let path = self.pack_root.join(rel);
                        match std::fs::read(&path) {
                            Ok(bytes) => {
                                let n = bytes.len().min(MAX_KNOWLEDGE_FILE_BYTES);
                                let text = String::from_utf8_lossy(&bytes[..n]);
                                out.push_str(&format!("--- `{rel}` ---\n{text}\n"));
                            }
                            Err(_) => {
                                out.push_str(&format!("- _(missing file `{rel}`)_\n"));
                            }
                        }
                    }
                }
            } else {
                out.push_str(&format!(
                    "\n_(Persona `{pid}` not found in pack; using company + shared context only.)_\n"
                ));
            }
        } else {
            out.push_str("\n_(No `HSM_BUSINESS_PERSONA` / `business_persona` set — shared context only.)_\n");
        }

        out.push_str(&format!(
            "\n_(Injection caps: ~{} KB total knowledge excerpt in prompt; per-file policy {}, knowledge {}.)_\n",
            MAX_TOTAL_INJECTED_BYTES / 1024,
            MAX_POLICY_FILE_BYTES / 1024,
            MAX_KNOWLEDGE_FILE_BYTES / 1024
        ));
        out.push_str(
            "\nUse this block to stay aligned with the business. If facts are missing, say so and suggest what to confirm.\n",
        );
        out
    }
}

/// Validate a `pack.yaml` on disk (CLI). Does not require agent home layout.
pub fn validate_pack_yaml_file(path: &Path) -> anyhow::Result<PackValidationReport> {
    let pack = BusinessPack::load_yaml_file_sync(path)?;
    Ok(pack.validate())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_yaml() {
        let y = r#"
schema_version: 1
company:
  name: TestCo
  industry: property_management
  one_liner: Demo
last_reviewed: "2026-01-01"
shared_policies:
  - Be accurate
personas:
  admin:
    title: Admin
    system_instructions: "Keep emails short."
"#;
        let mut p: BusinessPack = serde_yaml::from_str(y).unwrap();
        p.pack_root = PathBuf::from("/tmp");
        assert_eq!(p.company.name, "TestCo");
        let block = p.render_prompt_addon(Some("admin"));
        assert!(block.contains("TestCo"));
        assert!(block.contains("admin"));
        let v = p.validate();
        assert!(v.ok(), "{:?}", v);
    }

    #[test]
    fn validate_rejects_bad_schema() {
        let y = r#"
schema_version: 99
company:
  name: X
  one_liner: "y"
personas: {}
"#;
        let mut p: BusinessPack = serde_yaml::from_str(y).unwrap();
        p.pack_root = PathBuf::from("/tmp");
        let v = p.validate();
        assert!(!v.ok());
    }

    #[test]
    fn validate_rejects_empty_company() {
        let y = r#"
schema_version: 1
company:
  name: "   "
  one_liner: "x"
personas: {}
"#;
        let mut p: BusinessPack = serde_yaml::from_str(y).unwrap();
        p.pack_root = PathBuf::from("/tmp");
        let v = p.validate();
        assert!(!v.ok());
    }
}
