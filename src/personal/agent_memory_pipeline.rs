//! Lightweight prompt routing, memory prefetch (LLM-picked files), and post-turn memory extract.
//!
//! Config: `config/prompt_routes.yaml` under HSMII home. Env toggles: `HSM_MEMORY_PREFETCH`, `HSM_MEMORY_EXTRACT`.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;
use tracing::info;

use crate::ollama_client::{OllamaClient, OllamaConfig};

// ── Prompt router ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct PromptRouteHit {
    /// Overrides business pack persona for this turn when [`BusinessPack`] is loaded.
    pub persona_key: Option<String>,
    /// Injected after living prompt / before business block.
    pub system_block: String,
}

#[derive(Debug, Deserialize)]
struct PromptRoutesFile {
    #[serde(default)]
    routes: Vec<PromptRouteRule>,
}

#[derive(Clone, Debug, Deserialize)]
struct PromptRouteRule {
    #[serde(default)]
    keywords: Vec<String>,
    #[serde(default)]
    persona_key: Option<String>,
    #[serde(default)]
    system_template: String,
}

#[derive(Clone)]
pub struct PromptRouter {
    rules: Vec<PromptRouteRule>,
}

impl PromptRouter {
    #[cfg(test)]
    pub fn from_yaml_for_test(yaml: &str) -> Self {
        let file: PromptRoutesFile = serde_yaml::from_str(yaml).unwrap();
        Self { rules: file.routes }
    }

    pub async fn try_load(path: &Path) -> Option<Self> {
        if !path.is_file() {
            return None;
        }
        let raw = tokio::fs::read_to_string(path).await.ok()?;
        let file: PromptRoutesFile = serde_yaml::from_str(&raw).ok()?;
        if file.routes.is_empty() {
            return None;
        }
        info!(path = %path.display(), n = file.routes.len(), "loaded prompt routes");
        Some(Self { rules: file.routes })
    }

    pub fn route(&self, user_message: &str) -> PromptRouteHit {
        let lower = user_message.to_lowercase();
        for rule in &self.rules {
            let hit = rule.keywords.iter().any(|kw| {
                let k = kw.to_lowercase();
                !k.is_empty() && lower.contains(&k)
            });
            if !hit {
                continue;
            }
            return PromptRouteHit {
                persona_key: rule.persona_key.clone(),
                system_block: if rule.system_template.trim().is_empty() {
                    String::new()
                } else {
                    format!(
                        "\n\n## Routed mode (keyword match)\n{}\n",
                        rule.system_template.trim()
                    )
                },
            };
        }
        PromptRouteHit::default()
    }
}

// ── Memory manifest + prefetch ───────────────────────────────────────────────

const MAX_MANIFEST_FILES: usize = 48;
const MANIFEST_SNIPPET_CHARS: usize = 160;
const PREFETCH_PER_FILE_CAP: usize = 6000;
const PREFETCH_TOTAL_CAP: usize = 20_000;

#[derive(Clone, Debug)]
pub struct MemoryFileEntry {
    pub rel_path: String,
    pub snippet: String,
}

pub fn list_memory_markdown_files(home: &Path) -> Vec<MemoryFileEntry> {
    let mem_root = home.join("memory");
    if !mem_root.is_dir() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for entry in walkdir::WalkDir::new(&mem_root)
        .max_depth(6)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let p = entry.path();
        if !p.is_file() {
            continue;
        }
        if p.extension().and_then(|s| s.to_str()) != Some("md") {
            continue;
        }
        if let Ok(rel) = p.strip_prefix(home) {
            let rel_s = rel.to_string_lossy().replace('\\', "/");
            if rel_s.contains("/extracts/")
                && std::env::var("HSM_MEMORY_PREFETCH_INCLUDE_EXTRACTS")
                    .ok()
                    .as_deref()
                    != Some("1")
            {
                continue;
            }
            let snippet = std::fs::read_to_string(p)
                .ok()
                .map(|s| {
                    s.chars()
                        .take(MANIFEST_SNIPPET_CHARS)
                        .collect::<String>()
                        .replace('\n', " ")
                })
                .unwrap_or_default();
            out.push(MemoryFileEntry {
                rel_path: rel_s,
                snippet,
            });
        }
        if out.len() >= MAX_MANIFEST_FILES {
            break;
        }
    }
    out
}

pub async fn prefetch_memory_context(
    llm: &OllamaClient,
    home: &Path,
    user_query: &str,
    pick_n: usize,
) -> Result<String> {
    let candidates = list_memory_markdown_files(home);
    if candidates.is_empty() {
        return Ok(String::new());
    }

    let list_lines: Vec<String> = candidates
        .iter()
        .map(|c| format!("- `{}`: {}", c.rel_path, c.snippet))
        .collect();
    let manifest = list_lines.join("\n");

    let system = format!(
        "You pick markdown memory files relevant to the user query. Reply with ONLY valid JSON: {{\"paths\":[\"memory/foo.md\"]}} \
         using paths exactly as listed (relative to agent home). At most {pick_n} paths. If none fit, use {{\"paths\":[]}}."
    );
    let user = format!("## User query\n{user_query}\n\n## Candidate files\n{manifest}");
    let out = llm.chat(&system, &user, &[]).await;
    if out.timed_out || out.text.is_empty() {
        return Ok(String::new());
    }

    let paths: Vec<String> = parse_prefetch_json_paths(&out.text).unwrap_or_default();
    if paths.is_empty() {
        return Ok(String::new());
    }

    let mut buf = String::from("\n\n## Prefetched memory (selected)\n\n");
    let mut total = 0usize;
    for rel in paths.into_iter().take(pick_n) {
        let p = home.join(&rel);
        if !p.is_file() {
            continue;
        }
        let body = tokio::fs::read_to_string(&p).await.unwrap_or_default();
        let mut chunk = body;
        if chunk.len() > PREFETCH_PER_FILE_CAP {
            let mut n = PREFETCH_PER_FILE_CAP;
            while n > 0 && !chunk.is_char_boundary(n) {
                n -= 1;
            }
            chunk.truncate(n);
            chunk.push_str("\n\n_(truncated)_\n");
        }
        let block = format!("### `{}`\n\n{}\n\n", rel, chunk);
        if total + block.len() > PREFETCH_TOTAL_CAP {
            break;
        }
        total += block.len();
        buf.push_str(&block);
    }

    Ok(buf)
}

fn parse_prefetch_json_paths(text: &str) -> Option<Vec<String>> {
    let v: serde_json::Value = serde_json::from_str(&strip_json_fences(text)).ok()?;
    let arr = v.get("paths")?.as_array()?;
    Some(
        arr.iter()
            .filter_map(|x| x.as_str().map(|s| s.to_string()))
            .collect(),
    )
}

fn strip_json_fences(s: &str) -> String {
    let t = s.trim();
    if let Some(rest) = t.strip_prefix("```") {
        let body = rest.trim_start_matches(|c| c != '\n');
        if let Some(i) = body.find('\n') {
            let after = &body[i + 1..];
            if let Some(end) = after.rfind("```") {
                return after[..end].trim().to_string();
            }
        }
    }
    t.to_string()
}

// ── Post-turn extract ─────────────────────────────────────────────────────────

pub async fn run_post_turn_extract(home: &Path, user: &str, assistant: &str) -> Result<()> {
    let mut cfg = OllamaConfig::default();
    if let Ok(m) = std::env::var("HSM_MEMORY_EXTRACT_MODEL") {
        let t = m.trim();
        if !t.is_empty() {
            cfg.model = t.to_string();
        }
    }
    cfg.max_tokens = 512;
    cfg.temperature = 0.2;
    let llm = OllamaClient::new(cfg);

    let system = "Extract 0–3 durable memory bullets for future sessions. Output ONLY valid JSON: \
                  {\"memories\":[{\"title\":\"short title\",\"body\":\"markdown body\",\"tags\":[\"tag\"]}]}. \
                  Skip trivial chat. No secrets or credentials.";
    let user_msg = format!(
        "## User\n{}\n\n## Assistant\n{}\n\nRespond with JSON only.",
        user.chars().take(1500).collect::<String>(),
        assistant.chars().take(2500).collect::<String>()
    );

    let res = llm.chat(system, &user_msg, &[]).await;
    if res.timed_out || res.text.is_empty() {
        return Ok(());
    }

    let parsed: serde_json::Value =
        serde_json::from_str(&strip_json_fences(&res.text)).context("extract JSON parse")?;
    let items = parsed
        .get("memories")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if items.is_empty() {
        return Ok(());
    }

    let dir = home.join("memory/extracts");
    tokio::fs::create_dir_all(&dir).await?;
    let ts = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    let id = uuid::Uuid::new_v4()
        .to_string()
        .chars()
        .take(8)
        .collect::<String>();
    let path = dir.join(format!("extract-{ts}-{id}.md"));

    let mut md = String::from("---\nsource: post_turn_extract\n---\n\n");
    for m in items {
        let title = m.get("title").and_then(|v| v.as_str()).unwrap_or("Note");
        let body = m.get("body").and_then(|v| v.as_str()).unwrap_or("");
        let tags = m
            .get("tags")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        md.push_str(&format!("## {title}\n"));
        if !tags.is_empty() {
            md.push_str(&format!("*{tags}*\n\n"));
        }
        md.push_str(body);
        md.push_str("\n\n");
    }

    tokio::fs::write(&path, md).await?;
    info!(path = %path.display(), "wrote post-turn memory extract");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn router_matches_keyword() {
        let r = PromptRouter::from_yaml_for_test(
            "routes:\n  - keywords: [finance]\n    persona_key: acct\n    system_template: Be brief.\n",
        );
        let h = r.route("Help with finance tax");
        assert_eq!(h.persona_key.as_deref(), Some("acct"));
        assert!(h.system_block.contains("Be brief"));
    }
}
