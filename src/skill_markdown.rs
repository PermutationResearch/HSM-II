//! On-disk **SKILL.md** catalogs aligned with [Agent Skills](https://github.com/agentskills/agentskills)
//! ([specification](https://agentskills.io/specification)) for the personal agent.
//!
//! Scans `<HSMII_HOME>/skills` plus paths in `HSM_SKILL_EXTERNAL_DIRS` (comma-separated).
//! Progressive disclosure: keep only slug, display name, short description, and path in RAM;
//! full body is loaded via [`SkillMdCatalog::read_body`] or the `skill_md_read` tool.
//! Use [`SkillMdCatalog::read_skill_resource`] for `scripts/`, `references/`, `assets/` (on demand).

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use serde_json::json;
use serde_yaml::Value as YamlValue;

/// [Agent Skills](https://agentskills.io/specification) YAML front matter (all optional at parse time;
/// the spec requires `name` + `description` for compliant skills).
#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
struct AgentSkillFrontmatter {
    name: Option<String>,
    #[serde(alias = "title")]
    title: Option<String>,
    description: Option<String>,
    #[serde(default, alias = "summary")]
    summary: Option<String>,
    license: Option<String>,
    compatibility: Option<String>,
    #[serde(default)]
    metadata: Option<YamlValue>,
    #[serde(default)]
    platforms: Option<Vec<String>>,
    /// Experimental in the spec: space-delimited tool list.
    #[serde(default)]
    allowed_tools: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
struct AgentSkillMetadataBlock {
    hermes: Option<AgentSkillHermesMeta>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
struct AgentSkillHermesMeta {
    tags: Option<Vec<String>>,
    category: Option<String>,
    #[serde(alias = "fallback_for_toolsets", alias = "fallbackForToolsets")]
    fallback_for_toolsets: Option<Vec<String>>,
    #[serde(alias = "requires_toolsets", alias = "requiresToolsets")]
    requires_toolsets: Option<Vec<String>>,
    #[serde(alias = "fallback_for_tools", alias = "fallbackForTools")]
    fallback_for_tools: Option<Vec<String>>,
    #[serde(alias = "requires_tools", alias = "requiresTools")]
    requires_tools: Option<Vec<String>>,
}

#[derive(Clone, Debug)]
pub struct SkillMdSummary {
    pub slug: String,
    /// Display label for prompts (often derived from spec `name` or slug).
    pub name: String,
    pub description: String,
    pub path: PathBuf,
    /// Skill root directory (parent of `SKILL.md`); `scripts/`, `references/`, etc. live here.
    pub skill_dir: PathBuf,
    /// Agent Skills `name` from front matter when present.
    pub skill_id: Option<String>,
    pub license: Option<String>,
    pub compatibility: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub allowed_tools: Option<String>,
    pub platforms: Vec<String>,
    pub tags: Vec<String>,
    pub category: Option<String>,
    pub fallback_for_toolsets: Vec<String>,
    pub requires_toolsets: Vec<String>,
    pub fallback_for_tools: Vec<String>,
    pub requires_tools: Vec<String>,
    pub contract_complete: bool,
    pub missing_sections: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct SkillMdCatalog {
    entries: BTreeMap<String, SkillMdSummary>,
}

impl SkillMdCatalog {
    pub fn new() -> Self {
        Self::default()
    }

    /// Build catalog from agent home and optional extra roots (`HSM_SKILL_EXTERNAL_DIRS`).
    pub fn refresh_from_env_home(home: &Path) -> Self {
        let roots = collect_skill_roots(home);
        Self::from_roots(&roots)
    }

    pub fn from_roots(roots: &[PathBuf]) -> Self {
        let mut cat = Self::new();
        for root in roots {
            if let Err(e) = cat.merge_root(root) {
                tracing::warn!(
                    target: "hsm_skill_md",
                    root = %root.display(),
                    "skill markdown scan failed: {}",
                    e
                );
            }
        }
        cat
    }

    fn merge_root(&mut self, root: &Path) -> std::io::Result<()> {
        if !root.is_dir() {
            return Ok(());
        }
        let mut paths = Vec::new();
        collect_skill_md_paths(root, root, &mut paths)?;
        for p in paths {
            if let Ok(summary) = summarize_skill_file(root, &p) {
                // First root in `from_roots` wins (e.g. `<home>/skills` beats `HSM_SKILL_EXTERNAL_DIRS`).
                self.entries.entry(summary.slug.clone()).or_insert(summary);
            }
        }
        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn get(&self, slug: &str) -> Option<&SkillMdSummary> {
        self.entries.get(slug)
    }

    /// Full markdown body (for tools / slash commands). Caps size.
    pub fn read_body(&self, slug: &str, max_bytes: usize) -> Result<String> {
        let s = self
            .get(slug)
            .ok_or_else(|| anyhow!("unknown skill slug: {}", slug))?;
        let raw = std::fs::read_to_string(&s.path).with_context(|| s.path.display().to_string())?;
        let (_, _, body) = parse_skill_md_raw(&raw)?;
        Ok(truncate_utf8(&body, max_bytes))
    }

    /// Read a file under the skill root (`references/…`, `scripts/…`, `assets/…`). Paths must be relative with no `..`.
    pub fn read_skill_resource(
        &self,
        slug: &str,
        relative: &str,
        max_bytes: usize,
    ) -> Result<String> {
        let s = self
            .get(slug)
            .ok_or_else(|| anyhow!("unknown skill slug: {}", slug))?;
        let target = safe_join_under_skill_dir(&s.skill_dir, relative)?;
        let raw = std::fs::read_to_string(&target).with_context(|| target.display().to_string())?;
        Ok(truncate_utf8(&raw, max_bytes))
    }

    /// JSON list for `skills_list` tool.
    pub fn to_json_list(&self, limit: Option<usize>) -> serde_json::Value {
        let mut v: Vec<_> = self.entries.values().cloned().collect();
        v.sort_by(|a, b| a.slug.cmp(&b.slug));
        if let Some(n) = limit {
            v.truncate(n.max(1).min(500));
        }
        let rows: Vec<serde_json::Value> = v
            .iter()
            .map(|e| {
                json!({
                    "slug": e.slug,
                    "name": e.name,
                    "skill_id": e.skill_id,
                    "description": e.description,
                    "path": e.path.to_string_lossy(),
                    "skill_dir": e.skill_dir.to_string_lossy(),
                    "license": e.license,
                    "compatibility": e.compatibility,
                    "metadata": e.metadata,
                    "allowed_tools": e.allowed_tools,
                    "platforms": e.platforms,
                    "tags": e.tags,
                    "category": e.category,
                    "fallback_for_toolsets": e.fallback_for_toolsets,
                    "requires_toolsets": e.requires_toolsets,
                    "fallback_for_tools": e.fallback_for_tools,
                    "requires_tools": e.requires_tools,
                    "contract_complete": e.contract_complete,
                    "missing_sections": e.missing_sections,
                })
            })
            .collect();
        json!(rows)
    }

    /// Short index block for system prompt (names + blurbs only).
    pub fn format_prompt_index(&self, max_entries: usize, max_line_chars: usize) -> String {
        if self.entries.is_empty() {
            return String::new();
        }
        let mut v: Vec<_> = self.entries.values().cloned().collect();
        v.sort_by(|a, b| a.slug.cmp(&b.slug));
        v.truncate(max_entries.max(1).min(200));
        let mut lines: Vec<String> = vec![
            "## Markdown skills (index only)".to_string(),
            "Full instructions are **not** loaded here. Use `skills_list`, `skill_md_read`, `skill_resource_read` (or `/skills`, `/skill <slug>`). Format: [Agent Skills](https://agentskills.io/specification).".to_string(),
            String::new(),
        ];
        for e in v {
            let blurb = if e.description.is_empty() {
                "(no description)".to_string()
            } else {
                clamp_line(&e.description, max_line_chars)
            };
            lines.push(format!("- `{}` — **{}** — {}", e.slug, e.name, blurb));
        }
        lines.join("\n")
    }

    /// User-facing markdown list (slash `/skills`).
    pub fn format_list_markdown(&self, limit: Option<usize>) -> String {
        if self.entries.is_empty() {
            return "No **SKILL.md** entries found. Add folders under `<HSMII_HOME>/skills/<slug>/SKILL.md` or set `HSM_SKILL_EXTERNAL_DIRS`.".to_string();
        }
        let mut v: Vec<_> = self.entries.values().cloned().collect();
        v.sort_by(|a, b| a.slug.cmp(&b.slug));
        if let Some(n) = limit {
            v.truncate(n);
        }
        let mut out = String::from("## Markdown skills\n\n");
        for e in v {
            let blurb = if e.description.is_empty() {
                String::new()
            } else {
                format!(" — {}", e.description)
            };
            out.push_str(&format!("- **`{}`** — {}{}\n", e.slug, e.name, blurb));
        }
        out.push_str(
            "\nUse `/skill <slug>` or the `skill_md_read` tool for the full SKILL.md body.\n",
        );
        out
    }
}

/// Paths to scan: `<home>/skills`, then each entry in `HSM_SKILL_EXTERNAL_DIRS`.
pub fn collect_skill_roots(home: &Path) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    roots.push(home.join("skills"));
    roots.extend(external_skill_dir_roots_from_env());
    roots
}

/// Extra skill trees from `HSM_SKILL_EXTERNAL_DIRS` only (comma-separated, `~` expanded).
/// Does not include `<home>/skills`. Used by Company OS pack import to upsert shared skills for every agent.
pub fn external_skill_dir_roots_from_env() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(raw) = std::env::var("HSM_SKILL_EXTERNAL_DIRS") {
        for part in raw.split(',') {
            let p = part.trim();
            if p.is_empty() {
                continue;
            }
            roots.push(expand_tilde_path(p));
        }
    }
    roots
}

/// Recursive Hermes / Agent Skills layout: each `**/SKILL.md` under `root` becomes `(slug, path)` where
/// `slug` is the relative parent directory path (e.g. `github`, `devops/webhook-subscriptions`).
pub fn enumerate_skill_md_under_root(root: &Path) -> std::io::Result<Vec<(String, PathBuf)>> {
    if !root.is_dir() {
        return Ok(Vec::new());
    }
    let mut paths = Vec::new();
    collect_skill_md_paths(root, root, &mut paths)?;
    let mut out = Vec::new();
    for p in paths {
        if let Some(slug) = slug_for_skill_path(root, &p) {
            out.push((slug, p));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

fn expand_tilde_path(s: &str) -> PathBuf {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix("~/") {
        if let Ok(h) = std::env::var("HOME") {
            return PathBuf::from(h).join(rest);
        }
    }
    if s == "~" {
        if let Ok(h) = std::env::var("HOME") {
            return PathBuf::from(h);
        }
    }
    PathBuf::from(s)
}

fn collect_skill_md_paths(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for e in std::fs::read_dir(dir)? {
        let e = e?;
        let name = e.file_name();
        let name_s = name.to_string_lossy();
        if name_s.starts_with('.') {
            continue;
        }
        let p = e.path();
        if p.is_dir() {
            collect_skill_md_paths(root, &p, out)?;
        } else if name_s.eq_ignore_ascii_case("skill.md") {
            out.push(p);
        }
    }
    Ok(())
}

fn slug_for_skill_path(root: &Path, skill_md: &Path) -> Option<String> {
    let rel = skill_md.strip_root(root).ok()?;
    let parent = rel.parent()?;
    let slug = if parent.as_os_str().is_empty() {
        rel.file_stem()?.to_string_lossy().into_owned()
    } else {
        parent.to_string_lossy().replace('\\', "/")
    };
    let slug = slug.trim_matches('/').to_string();
    if slug.is_empty() {
        None
    } else {
        Some(slug)
    }
}

trait StripRoot {
    fn strip_root(&self, root: &Path) -> std::result::Result<PathBuf, ()>;
}

impl StripRoot for Path {
    fn strip_root(&self, root: &Path) -> std::result::Result<PathBuf, ()> {
        self.strip_prefix(root)
            .map(|p| p.to_path_buf())
            .map_err(drop)
    }
}

fn agentskills_strict() -> bool {
    std::env::var("HSM_AGENT_SKILLS_STRICT")
        .map(|v| {
            let s = v.trim();
            s == "1" || s.eq_ignore_ascii_case("true") || s.eq_ignore_ascii_case("yes")
        })
        .unwrap_or(false)
}

/// Spec: `name` is lowercase `[a-z0-9-]{1,64}`, no leading/trailing `-`, no `--`.
fn agentskills_name_valid(name: &str) -> bool {
    if name.is_empty() || name.len() > 64 {
        return false;
    }
    if name.starts_with('-') || name.ends_with('-') || name.contains("--") {
        return false;
    }
    name.chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn leaf_dir_of_skill_md(skill_md: &Path) -> String {
    skill_md
        .parent()
        .and_then(|p| p.file_name())
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn parse_frontmatter_block(raw: &str) -> Result<(Option<AgentSkillFrontmatter>, String)> {
    let s = raw.trim_start();
    if !s.starts_with("---") {
        return Ok((None, s.to_string()));
    }
    let rest = &s[3..];
    let end = rest
        .find("\n---")
        .ok_or_else(|| anyhow!("unclosed YAML front matter"))?;
    let yaml_part = rest[..end].trim();
    let body = rest[end + 4..].trim_start().to_string();
    let fm: AgentSkillFrontmatter =
        serde_yaml::from_str(yaml_part).context("skill front matter YAML")?;
    Ok((Some(fm), body))
}

fn summarize_skill_file(root: &Path, skill_md: &Path) -> Result<SkillMdSummary> {
    let slug = slug_for_skill_path(root, skill_md)
        .ok_or_else(|| anyhow!("could not derive slug for {}", skill_md.display()))?;
    let skill_dir = skill_md
        .parent()
        .map(|p| p.to_path_buf())
        .ok_or_else(|| anyhow!("SKILL.md has no parent directory"))?;
    let leaf = leaf_dir_of_skill_md(skill_md);
    let raw = std::fs::read_to_string(skill_md)?;
    let (fm_opt, body) = parse_frontmatter_block(&raw)?;
    let strict = agentskills_strict();
    let missing_sections = skill_contract_missing_sections(&body);
    let contract_complete = missing_sections.is_empty();

    let (
        display_name,
        description,
        skill_id,
        license,
        compatibility,
        metadata,
        allowed_tools,
        platforms,
        tags,
        category,
        fallback_for_toolsets,
        requires_toolsets,
        fallback_for_tools,
        requires_tools,
    ) = match &fm_opt {
            Some(fm) => {
                let id = fm
                    .name
                    .as_ref()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty());
                let desc = fm
                    .description
                    .clone()
                    .or(fm.summary.clone())
                    .unwrap_or_default()
                    .trim()
                    .to_string();

                if let Some(ref n) = id {
                    if !leaf.is_empty() && n != &leaf {
                        let msg = format!(
                        "Agent Skills: `name` `{n}` must match parent directory `{leaf}` (skill `{slug}`)"
                    );
                        if strict {
                            return Err(anyhow!(msg));
                        }
                        tracing::warn!(target: "hsm_skill_md", "{}", msg);
                    }
                    if !agentskills_name_valid(n) {
                        let msg = format!(
                        "Agent Skills: invalid `name` `{n}` — use 1–64 chars [a-z0-9-], no leading/trailing/double hyphen (skill `{slug}`)"
                    );
                        if strict {
                            return Err(anyhow!(msg));
                        }
                        tracing::warn!(target: "hsm_skill_md", "{}", msg);
                    }
                }

                if id.is_some() && desc.is_empty() {
                    let msg = format!(
                    "Agent Skills: `description` is required and must be non-empty (skill `{slug}`)"
                );
                    if strict {
                        return Err(anyhow!(msg));
                    }
                    tracing::warn!(target: "hsm_skill_md", "{}", msg);
                }

                let disp = match &id {
                    Some(n) => titlecase_kebab(n),
                    None => fm
                        .title
                        .as_ref()
                        .map(|t| t.trim().to_string())
                        .filter(|t| !t.is_empty())
                        .unwrap_or_else(|| String::new()),
                };
                let metadata_json = fm
                    .metadata
                    .as_ref()
                    .and_then(|m| serde_json::to_value(m).ok());
                let hermes_meta = fm.metadata.as_ref().and_then(extract_hermes_meta);

                (
                    disp,
                    desc,
                    id,
                    fm.license.clone(),
                    fm.compatibility.clone(),
                    metadata_json,
                    fm.allowed_tools.clone(),
                    normalize_string_vec(fm.platforms.clone().unwrap_or_default()),
                    normalize_string_vec(
                        hermes_meta
                            .as_ref()
                            .and_then(|h| h.tags.clone())
                            .unwrap_or_default(),
                    ),
                    hermes_meta.as_ref().and_then(|h| clean_opt(&h.category)),
                    normalize_string_vec(
                        hermes_meta
                            .as_ref()
                            .and_then(|h| h.fallback_for_toolsets.clone())
                            .unwrap_or_default(),
                    ),
                    normalize_string_vec(
                        hermes_meta
                            .as_ref()
                            .and_then(|h| h.requires_toolsets.clone())
                            .unwrap_or_default(),
                    ),
                    normalize_string_vec(
                        hermes_meta
                            .as_ref()
                            .and_then(|h| h.fallback_for_tools.clone())
                            .unwrap_or_default(),
                    ),
                    normalize_string_vec(
                        hermes_meta
                            .as_ref()
                            .and_then(|h| h.requires_tools.clone())
                            .unwrap_or_default(),
                    ),
                )
            }
            None => (
                String::new(),
                String::new(),
                None,
                None,
                None,
                None,
                None,
                Vec::new(),
                Vec::new(),
                None,
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
            ),
        };

    let name = if display_name.is_empty() {
        humanize_slug(&slug)
    } else {
        display_name
    };

    Ok(SkillMdSummary {
        slug,
        name,
        description,
        path: skill_md.to_path_buf(),
        skill_dir,
        skill_id,
        license,
        compatibility,
        metadata,
        allowed_tools,
        platforms,
        tags,
        category,
        fallback_for_toolsets,
        requires_toolsets,
        fallback_for_tools,
        requires_tools,
        contract_complete,
        missing_sections,
    })
}

/// Returns `(display_name, description, body)`; name/desc may be empty if no front matter.
pub fn parse_skill_md_raw(raw: &str) -> Result<(String, String, String)> {
    let (fm, body) = parse_frontmatter_block(raw)?;
    match fm {
        Some(fm) => {
            let id = fm
                .name
                .clone()
                .or(fm.title.clone())
                .unwrap_or_default()
                .trim()
                .to_string();
            let description = fm
                .description
                .clone()
                .or(fm.summary.clone())
                .unwrap_or_default()
                .trim()
                .to_string();
            let display = if id.is_empty() {
                String::new()
            } else {
                titlecase_kebab(&id)
            };
            Ok((display, description, body))
        }
        None => Ok((String::new(), String::new(), body)),
    }
}

fn safe_join_under_skill_dir(skill_dir: &Path, relative: &str) -> Result<PathBuf> {
    let rel = relative.trim().trim_start_matches(['/', '\\']);
    if rel.is_empty() {
        return Err(anyhow!("empty relative path"));
    }
    let rel_path = Path::new(rel);
    for c in rel_path.components() {
        if matches!(
            c,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        ) {
            return Err(anyhow!(
                "invalid path (use relative paths under the skill folder only)"
            ));
        }
    }
    let mut out = skill_dir.to_path_buf();
    for c in rel_path.components() {
        if let Component::Normal(p) = c {
            out.push(p);
        }
    }
    let base = skill_dir
        .canonicalize()
        .with_context(|| format!("skill_dir {}", skill_dir.display()))?;
    let target = out
        .canonicalize()
        .with_context(|| format!("resource {}", out.display()))?;
    if !target.starts_with(&base) {
        return Err(anyhow!("path escapes skill directory"));
    }
    Ok(target)
}

fn titlecase_kebab(s: &str) -> String {
    s.split('-')
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn humanize_slug(slug: &str) -> String {
    let base = slug.rsplit('/').next().unwrap_or(slug);
    base.replace(['-', '_'], " ")
        .split_whitespace()
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn clamp_line(s: &str, max_chars: usize) -> String {
    let t = s.trim().replace('\n', " ");
    if t.chars().count() <= max_chars {
        t
    } else {
        format!(
            "{}…",
            t.chars()
                .take(max_chars.saturating_sub(1))
                .collect::<String>()
        )
    }
}

fn truncate_utf8(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}… [truncated]", &s[..end])
}

fn normalize_string_vec(values: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for v in values {
        let t = v.trim();
        if t.is_empty() {
            continue;
        }
        let lower = t.to_ascii_lowercase();
        if seen.insert(lower) {
            out.push(t.to_string());
        }
    }
    out
}

fn clean_opt(v: &Option<String>) -> Option<String> {
    v.as_ref().map(|x| x.trim().to_string()).filter(|x| !x.is_empty())
}

fn extract_hermes_meta(metadata: &YamlValue) -> Option<AgentSkillHermesMeta> {
    serde_yaml::from_value::<AgentSkillMetadataBlock>(metadata.clone())
        .ok()
        .and_then(|m| m.hermes)
}

fn skill_contract_missing_sections(body: &str) -> Vec<String> {
    let required = ["When to Use", "Procedure", "Pitfalls", "Verification"];
    let mut found = std::collections::BTreeSet::new();
    for raw in body.lines() {
        let line = raw.trim();
        if !line.starts_with('#') {
            continue;
        }
        let heading = line.trim_start_matches('#').trim();
        let norm = normalize_heading(heading);
        if !norm.is_empty() {
            found.insert(norm);
        }
    }
    required
        .iter()
        .filter_map(|name| {
            let norm = normalize_heading(name);
            if found.contains(&norm) {
                None
            } else {
                Some((*name).to_string())
            }
        })
        .collect()
}

fn normalize_heading(s: &str) -> String {
    s.to_ascii_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn slug_nested_skill() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let p = root.join("research/arxiv/SKILL.md");
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, "body").unwrap();
        assert_eq!(
            slug_for_skill_path(root, &p).as_deref(),
            Some("research/arxiv")
        );
    }

    #[test]
    fn parse_optional_frontmatter() {
        let (_, _, body) = parse_skill_md_raw("hello\nworld").unwrap();
        assert_eq!(body, "hello\nworld");
        let raw = "---\nname: plan\ndescription: Short\n---\n\nDo X\n";
        let (n, d, b) = parse_skill_md_raw(raw).unwrap();
        assert_eq!(n, "Plan");
        assert_eq!(d, "Short");
        assert_eq!(b.trim(), "Do X");
    }

    #[test]
    fn catalog_merge_first_root_wins() {
        let tmp = tempfile::tempdir().unwrap();
        let r1 = tmp.path().join("a");
        let r2 = tmp.path().join("b");
        fs::create_dir_all(r1.join("dup")).unwrap();
        fs::create_dir_all(r2.join("dup")).unwrap();
        fs::write(
            r1.join("dup/SKILL.md"),
            "---\nname: dup\ndescription: First\n---\n",
        )
        .unwrap();
        fs::write(
            r2.join("dup/SKILL.md"),
            "---\nname: dup\ndescription: Second\n---\n",
        )
        .unwrap();
        let cat = SkillMdCatalog::from_roots(&[r1, r2]);
        assert_eq!(
            cat.get("dup").map(|e| e.description.as_str()),
            Some("First")
        );
    }

    #[test]
    fn read_skill_resource_under_root() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let skill = root.join("my-skill");
        fs::create_dir_all(skill.join("references")).unwrap();
        fs::write(
            skill.join("SKILL.md"),
            "---\nname: my-skill\ndescription: test\n---\n",
        )
        .unwrap();
        fs::write(skill.join("references/note.txt"), "hello").unwrap();
        let cat = SkillMdCatalog::from_roots(&[root.to_path_buf()]);
        let body = cat
            .read_skill_resource("my-skill", "references/note.txt", 1024)
            .unwrap();
        assert_eq!(body, "hello");
        assert!(cat
            .read_skill_resource("my-skill", "../my-skill/references/note.txt", 1024)
            .is_err());
    }

    #[test]
    fn contract_sections_and_hermes_metadata_are_exposed() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let skill = root.join("my-skill");
        fs::create_dir_all(&skill).unwrap();
        fs::write(
            skill.join("SKILL.md"),
            r#"---
name: my-skill
description: test
platforms: [macos, linux]
metadata:
  hermes:
    tags: [python, automation]
    category: devops
    requires_tools: [bash]
---
# Skill Title

## When to Use
X

## Procedure
Y
"#,
        )
        .unwrap();
        let cat = SkillMdCatalog::from_roots(&[root.to_path_buf()]);
        let s = cat.get("my-skill").unwrap();
        assert_eq!(s.platforms, vec!["macos".to_string(), "linux".to_string()]);
        assert_eq!(s.tags, vec!["python".to_string(), "automation".to_string()]);
        assert_eq!(s.category.as_deref(), Some("devops"));
        assert_eq!(s.requires_tools, vec!["bash".to_string()]);
        assert!(!s.contract_complete);
        assert!(s.missing_sections.contains(&"Pitfalls".to_string()));
        assert!(s.missing_sections.contains(&"Verification".to_string()));
    }
}
