//! List and read on-disk `SKILL.md` entries (Hermes-style progressive disclosure).

use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::skill_markdown::SkillMdCatalog;
use crate::tools::HarnessPolicyGate;

use super::{object_schema, Tool, ToolOutput, ToolRegistry};

const LIST_NAME: &str = "skills_list";
const READ_NAME: &str = "skill_md_read";
const RESOURCE_NAME: &str = "skill_resource_read";

/// Register tools that read from the shared [`SkillMdCatalog`] (refreshed by the personal agent).
pub fn register_skill_md_tools(registry: &mut ToolRegistry, catalog: Arc<RwLock<SkillMdCatalog>>) {
    registry.register(Arc::new(SkillsListTool {
        catalog: catalog.clone(),
    }));
    registry.register(Arc::new(SkillMdReadTool {
        catalog: catalog.clone(),
    }));
    registry.register(Arc::new(SkillResourceReadTool { catalog }));
}

struct SkillsListTool {
    catalog: Arc<RwLock<SkillMdCatalog>>,
}

struct SkillMdReadTool {
    catalog: Arc<RwLock<SkillMdCatalog>>,
}

struct SkillResourceReadTool {
    catalog: Arc<RwLock<SkillMdCatalog>>,
}

#[async_trait]
impl Tool for SkillsListTool {
    fn name(&self) -> &str {
        LIST_NAME
    }

    fn description(&self) -> &str {
        "List on-disk Agent Skills (SKILL.md under <HSMII_HOME>/skills and HSM_SKILL_EXTERNAL_DIRS). JSON includes slug, skill_id, description, skill_dir, optional license/compatibility/metadata. Use skill_md_read / skill_resource_read for content."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "limit": {
                    "type": "integer",
                    "description": "Optional max entries (default 80, max 500)."
                }
            },
            "required": []
        })
    }

    async fn execute(&self, params: Value) -> ToolOutput {
        let limit = params
            .get("limit")
            .and_then(|v| v.as_u64())
            .map(|n| n as usize)
            .unwrap_or(80)
            .clamp(1, 500);
        let cat = match self.catalog.read() {
            Ok(g) => g,
            Err(e) => return ToolOutput::error(format!("skills catalog lock: {e}")),
        };
        let list = cat.to_json_list(Some(limit));
        ToolOutput::success(
            serde_json::to_string_pretty(&json!({ "skills": list }))
                .unwrap_or_else(|_| "{}".into()),
        )
    }
}

#[async_trait]
impl Tool for SkillMdReadTool {
    fn name(&self) -> &str {
        READ_NAME
    }

    fn description(&self) -> &str {
        "Load the full SKILL.md body (markdown after front matter) by catalog slug. Paths are not arbitrary — only registered skills."
    }

    fn parameters_schema(&self) -> Value {
        object_schema(vec![(
            "slug",
            "Skill slug, e.g. plan or research/arxiv.",
            true,
        )])
    }

    async fn execute(&self, params: Value) -> ToolOutput {
        let slug = match params.get("slug").and_then(|v| v.as_str()) {
            Some(s) if !s.trim().is_empty() => s.trim(),
            _ => return ToolOutput::error("missing or empty \"slug\"".to_string()),
        };
        let cat = match self.catalog.read() {
            Ok(g) => g,
            Err(e) => return ToolOutput::error(format!("skills catalog lock: {e}")),
        };
        const MAX_BODY: usize = 96 * 1024;
        match cat.read_body(slug, MAX_BODY) {
            Ok(body) => {
                let meta = cat.get(slug);
                ToolOutput::success(
                    serde_json::to_string_pretty(&json!({
                        "slug": slug,
                        "name": meta.map(|m| &m.name),
                        "path": meta.map(|m| m.path.to_string_lossy()),
                        "body": body,
                    }))
                    .unwrap_or_else(|_| body),
                )
            }
            Err(e) => ToolOutput::error(e.to_string()),
        }
    }
}

#[async_trait]
impl Tool for SkillResourceReadTool {
    fn name(&self) -> &str {
        RESOURCE_NAME
    }

    fn description(&self) -> &str {
        "Read a file under a skill directory (Agent Skills: references/, scripts/, assets/). Parameters: slug, relative_path. No .. or absolute paths."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "slug": { "type": "string", "description": "Catalog slug from skills_list." },
                "relative_path": { "type": "string", "description": "Path relative to skill root, e.g. references/REFERENCE.md" }
            },
            "required": ["slug", "relative_path"]
        })
    }

    async fn execute(&self, params: Value) -> ToolOutput {
        let slug = match params.get("slug").and_then(|v| v.as_str()) {
            Some(s) if !s.trim().is_empty() => s.trim(),
            _ => return ToolOutput::error("missing or empty \"slug\"".to_string()),
        };
        let rel = match params.get("relative_path").and_then(|v| v.as_str()) {
            Some(s) if !s.trim().is_empty() => s.trim(),
            _ => return ToolOutput::error("missing or empty \"relative_path\"".to_string()),
        };
        if let Err(e) = HarnessPolicyGate::validate_skill_relative_path(rel) {
            return ToolOutput::error(e);
        }
        let cat = match self.catalog.read() {
            Ok(g) => g,
            Err(e) => return ToolOutput::error(format!("skills catalog lock: {e}")),
        };
        const MAX: usize = 256 * 1024;
        match cat.read_skill_resource(slug, rel, MAX) {
            Ok(content) => ToolOutput::success(
                serde_json::to_string_pretty(&json!({
                    "slug": slug,
                    "relative_path": rel,
                    "content": content,
                }))
                .unwrap_or_else(|_| content),
            ),
            Err(e) => ToolOutput::error(e.to_string()),
        }
    }
}
