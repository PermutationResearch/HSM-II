//! Import Paperclip-style company packs from `hsmii_home` on disk into `company_agents` + context index.
//!
//! Layout (e.g. `paperclipai/companies` packs):
//! - `{hsmii_home}/agents/{id}/AGENTS.md` — YAML front matter + markdown briefing
//! - `{hsmii_home}/skills/{slug}/SKILL.md` — optional; indexed into `context_markdown`

use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use serde_json::json;
use sqlx::types::Json as SqlxJson;
use sqlx::PgPool;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

const MAX_CONTEXT_APPEND_BYTES: usize = 512 * 1024;

#[derive(Debug, Deserialize)]
struct AgentFrontmatter {
    name: Option<String>,
    title: Option<String>,
    #[serde(default, alias = "reportsTo", alias = "reports_to")]
    reports_to: Option<String>,
    #[serde(default)]
    skills: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct SkillFrontmatter {
    name: Option<String>,
    description: Option<String>,
}

struct ParsedAgent {
    dir_id: String,
    fm: AgentFrontmatter,
    briefing: String,
}

fn split_front_matter(raw: &str) -> Result<(String, String)> {
    let s = raw.trim_start();
    if !s.starts_with("---") {
        return Err(anyhow!("missing YAML front matter"));
    }
    let rest = &s[3..];
    let end = rest
        .find("\n---")
        .ok_or_else(|| anyhow!("unclosed front matter"))?;
    let yaml_part = rest[..end].trim();
    let body = rest[end + 4..].trim_start().to_string();
    Ok((yaml_part.to_string(), body))
}

fn parse_agents_md(path: &Path) -> Result<(AgentFrontmatter, String)> {
    let raw = fs::read_to_string(path)?;
    let (yaml, body) = split_front_matter(&raw)?;
    let fm: AgentFrontmatter = serde_yaml::from_str(&yaml).context("agent front matter YAML")?;
    Ok((fm, body))
}

fn parse_skill_md(path: &Path) -> Result<(SkillFrontmatter, String)> {
    let raw = fs::read_to_string(path)?;
    let (yaml, body) = split_front_matter(&raw)?;
    let fm: SkillFrontmatter = serde_yaml::from_str(&yaml).context("skill front matter YAML")?;
    Ok((fm, body))
}

fn strip_paperclip_skills_block(md: &str) -> String {
    let start = "<!-- hsm-paperclip-skills-start -->";
    let end = "<!-- hsm-paperclip-skills-end -->";
    if let (Some(i), Some(j)) = (md.find(start), md.find(end)) {
        if j > i {
            let before = md[..i].trim_end();
            let after = md[j + end.len()..].trim_start();
            if before.is_empty() {
                return after.to_string();
            }
            if after.is_empty() {
                return before.to_string();
            }
            return format!("{before}\n\n{after}");
        }
    }
    md.to_string()
}

/// Reads `hsmii_home`, inserts agents from `agents/*/AGENTS.md`, appends skills index to `context_markdown`.
pub async fn import_paperclip_pack(pool: &PgPool, company_id: Uuid) -> Result<serde_json::Value> {
    let home: Option<String> = sqlx::query_scalar("SELECT hsmii_home FROM companies WHERE id = $1")
        .bind(company_id)
        .fetch_optional(pool)
        .await?
        .flatten();

    let home = home
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| anyhow!("company has no hsmii_home; install the pack first"))?;

    let home_path = PathBuf::from(home.trim());
    if !home_path.is_dir() {
        return Err(anyhow!(
            "hsmii_home is not a directory: {}",
            home_path.display()
        ));
    }

    let agents_dir = home_path.join("agents");
    if !agents_dir.is_dir() {
        return Err(anyhow!(
            "no agents/ directory under {}",
            home_path.display()
        ));
    }

    let mut parsed: Vec<ParsedAgent> = Vec::new();
    for entry in fs::read_dir(&agents_dir).with_context(|| format!("read {}", agents_dir.display()))? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let dir_id = entry.file_name().to_string_lossy().to_string();
        if dir_id.starts_with('.') {
            continue;
        }
        let agents_md = entry.path().join("AGENTS.md");
        if !agents_md.is_file() {
            continue;
        }
        let (fm, briefing) =
            parse_agents_md(&agents_md).with_context(|| format!("{}", agents_md.display()))?;
        parsed.push(ParsedAgent {
            dir_id,
            fm,
            briefing,
        });
    }

    if parsed.is_empty() {
        return Err(anyhow!(
            "no agents with AGENTS.md under {}",
            agents_dir.display()
        ));
    }

    let agents_existing: Vec<(String, Uuid)> = sqlx::query_as(
        "SELECT name, id FROM company_agents WHERE company_id = $1",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;

    let mut id_map: HashMap<String, Uuid> = agents_existing.into_iter().collect();

    let mut pending: Vec<&ParsedAgent> = parsed
        .iter()
        .filter(|p| !id_map.contains_key(&p.dir_id))
        .collect();
    let skipped_existing = parsed.len() - pending.len();

    let guard = pending.len().saturating_mul(5).max(1);
    let mut iterations = 0usize;
    let mut inserted = 0usize;

    while !pending.is_empty() {
        iterations += 1;
        if iterations > guard {
            return Err(anyhow!(
                "agent import stalled: check reportsTo references form a valid tree"
            ));
        }
        let mut next: Vec<&ParsedAgent> = Vec::new();
        let mut made_progress = false;

        for a in pending {
            let mgr = a
                .fm
                .reports_to
                .as_ref()
                .map(|s| s.trim().to_lowercase())
                .filter(|s| !s.is_empty() && s != "null");

            let ready = match &mgr {
                None => true,
                Some(m) => id_map.contains_key(m),
            };
            if !ready {
                next.push(a);
                continue;
            }

            let reports_uuid = mgr.as_ref().and_then(|m| id_map.get(m).copied());
            let role = a
                .fm
                .title
                .clone()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "paperclip_agent".to_string());
            let title = a.fm.name.clone().or_else(|| Some(a.dir_id.clone()));
            let caps = if a.fm.skills.is_empty() {
                None
            } else {
                Some(a.fm.skills.join(", "))
            };
            let adapter = json!({
                "paperclip": {
                    "agent_dir": format!("agents/{}", a.dir_id),
                    "skills": a.fm.skills,
                }
            });

            let new_id: Uuid = sqlx::query_scalar(
                r#"INSERT INTO company_agents (
                    company_id, name, role, title, capabilities, reports_to,
                    adapter_type, adapter_config, budget_monthly_cents, briefing, sort_order
                ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8::jsonb,$9,$10,$11)
                RETURNING id"#,
            )
            .bind(company_id)
            .bind(&a.dir_id)
            .bind(&role)
            .bind(&title)
            .bind(&caps)
            .bind(reports_uuid)
            .bind(Some("paperclip/v1"))
            .bind(SqlxJson(adapter))
            .bind(None::<i32>)
            .bind(&a.briefing)
            .bind(inserted as i32)
            .fetch_one(pool)
            .await
            .with_context(|| format!("insert agent {}", a.dir_id))?;

            id_map.insert(a.dir_id.clone(), new_id);
            inserted += 1;
            made_progress = true;
        }

        if !made_progress && !next.is_empty() {
            return Err(anyhow!(
                "cannot resolve manager references (reportsTo) for: {:?}",
                next.iter().map(|x| &x.dir_id).collect::<Vec<_>>()
            ));
        }
        pending = next;
    }

    let skills_dir = home_path.join("skills");
    let mut skill_count = 0usize;
    let mut skills_block = String::new();

    if skills_dir.is_dir() {
        let mut lines: Vec<String> = vec![
            String::new(),
            "<!-- hsm-paperclip-skills-start -->".to_string(),
            "## Paperclip pack skills (on disk)".to_string(),
            String::new(),
            format!(
                "Pack root: `{}`. Skills live under `skills/<slug>/SKILL.md` — edit those files or adjust agents in **Team & roles**.",
                home_path.display()
            ),
            String::new(),
        ];

        let mut entries: Vec<_> = fs::read_dir(&skills_dir)?.filter_map(|e| e.ok()).collect();
        entries.sort_by_key(|e| e.file_name());

        for entry in entries {
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let slug = entry.file_name().to_string_lossy().to_string();
            if slug.starts_with('.') {
                continue;
            }
            let skill_md = entry.path().join("SKILL.md");
            if !skill_md.is_file() {
                continue;
            }
            if let Ok((fm, _body)) = parse_skill_md(&skill_md) {
                let dn = fm.name.unwrap_or_else(|| slug.clone());
                let desc = fm.description.unwrap_or_default();
                let one_line: String = desc.split_whitespace().collect::<Vec<_>>().join(" ");
                let short: String = one_line.chars().take(240).collect();
                lines.push(format!(
                    "- **`{dn}`** (`skills/{slug}/`) — {short}"
                ));
                skill_count += 1;
            }
        }
        lines.push("<!-- hsm-paperclip-skills-end -->".to_string());
        skills_block = lines.join("\n");
    }

    if !skills_block.is_empty() {
        let current: Option<String> =
            sqlx::query_scalar("SELECT context_markdown FROM companies WHERE id = $1")
                .bind(company_id)
                .fetch_one(pool)
                .await?;
        let base = current.unwrap_or_default();
        let stripped = strip_paperclip_skills_block(&base);
        let mut merged = format!("{}{}", stripped.trim_end(), skills_block);
        if merged.len() > MAX_CONTEXT_APPEND_BYTES {
            merged.truncate(MAX_CONTEXT_APPEND_BYTES);
            merged.push_str("\n\n_(context truncated to size limit)_\n");
        }
        sqlx::query("UPDATE companies SET context_markdown = $2 WHERE id = $1")
            .bind(company_id)
            .bind(&merged)
            .execute(pool)
            .await?;
    }

    Ok(json!({
        "agents_inserted": inserted,
        "agents_skipped_existing": skipped_existing,
        "skills_indexed": skill_count,
    }))
}
