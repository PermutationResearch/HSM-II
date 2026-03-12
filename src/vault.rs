use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;
use regex::Regex;
use serde_json::json;
use walkdir::WalkDir;

#[derive(Debug, Clone)]
pub struct VaultMergeStats {
    pub notes: usize,
    pub tags: usize,
    pub stubs: usize,
    pub links: usize,
}

#[derive(Debug, Clone)]
pub struct VaultNote {
    pub id: String,
    pub title: String,
    pub tags: Vec<String>,
    pub links: Vec<String>,
    pub wikilinks: Vec<String>,
    pub body: String,
    pub path: PathBuf,
    pub search_text: String,
    pub preview: String,
    pub note_type: Option<String>,
    pub template: Option<String>,
    pub properties: HashMap<String, String>,
    pub attachments: Vec<String>,
}

#[derive(Debug, Clone)]
struct ParsedNote {
    id: String,
    title: String,
    tags: Vec<String>,
    links: Vec<String>,
    wikilinks: Vec<String>,
    body: String,
    path: PathBuf,
    note_type: Option<String>,
    template: Option<String>,
    properties: HashMap<String, String>,
    attachments: Vec<String>,
}

fn normalize_key(input: &str) -> String {
    let lowered = input.trim().to_lowercase();
    let mut out = String::with_capacity(lowered.len());
    for ch in lowered.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' {
            out.push(ch);
        } else if ch.is_whitespace() || ch == '_' {
            out.push('-');
        }
    }
    while out.contains("--") {
        out = out.replace("--", "-");
    }
    out.trim_matches('-').to_string()
}

fn parse_list_value(value: &str) -> Vec<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let inner = trimmed
        .strip_prefix('[')
        .and_then(|v| v.strip_suffix(']'))
        .unwrap_or(trimmed);
    inner
        .split(',')
        .map(|v| v.trim().trim_matches('"').trim_matches('\'').to_string())
        .filter(|v| !v.is_empty())
        .collect()
}

fn parse_frontmatter(
    content: &str,
) -> (
    HashMap<String, Vec<String>>,
    HashMap<String, String>,
    String,
) {
    let mut list_fields: HashMap<String, Vec<String>> = HashMap::new();
    let mut scalar_fields: HashMap<String, String> = HashMap::new();

    let mut lines = content.lines();
    let first = lines.next().unwrap_or("");
    if first.trim() != "---" {
        return (list_fields, scalar_fields, content.to_string());
    }

    let mut frontmatter_lines = Vec::new();
    let mut body_lines = Vec::new();
    let mut in_frontmatter = true;
    for line in lines {
        if in_frontmatter && line.trim() == "---" {
            in_frontmatter = false;
            continue;
        }
        if in_frontmatter {
            frontmatter_lines.push(line.to_string());
        } else {
            body_lines.push(line.to_string());
        }
    }

    let mut i = 0;
    while i < frontmatter_lines.len() {
        let line = frontmatter_lines[i].trim();
        if line.is_empty() {
            i += 1;
            continue;
        }
        if let Some((key, rest)) = line.split_once(':') {
            let key = key.trim().to_string();
            let mut value = rest.trim().to_string();
            if value.is_empty() {
                let mut values = Vec::new();
                let mut j = i + 1;
                while j < frontmatter_lines.len() {
                    let next_line = frontmatter_lines[j].trim();
                    if next_line.starts_with('-') {
                        let item = next_line.trim_start_matches('-').trim();
                        if !item.is_empty() {
                            values.push(item.to_string());
                        }
                        j += 1;
                    } else if next_line.contains(':') {
                        break;
                    } else if next_line.is_empty() {
                        j += 1;
                    } else {
                        break;
                    }
                }
                if !values.is_empty() {
                    list_fields.insert(key, values);
                }
                i = j;
                continue;
            }

            if key == "tags" || key == "links" {
                let values = parse_list_value(&value);
                list_fields.insert(key, values);
            } else {
                if value.starts_with('"') && value.ends_with('"') && value.len() >= 2 {
                    value = value[1..value.len() - 1].to_string();
                }
                scalar_fields.insert(key, value);
            }
        }
        i += 1;
    }

    (list_fields, scalar_fields, body_lines.join("\n"))
}

fn parse_note(path: &Path) -> anyhow::Result<ParsedNote> {
    let content =
        fs::read_to_string(path).with_context(|| format!("read vault note {}", path.display()))?;
    let (lists, scalars, body) = parse_frontmatter(&content);

    let filename = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("note")
        .to_string();
    let id = scalars
        .get("id")
        .cloned()
        .unwrap_or_else(|| filename.clone());
    let mut title = scalars
        .get("title")
        .cloned()
        .unwrap_or_else(|| filename.clone());
    if title == filename {
        if let Some(line) = body.lines().find(|l| l.trim_start().starts_with('#')) {
            let trimmed = line.trim_start().trim_start_matches('#').trim();
            if !trimmed.is_empty() {
                title = trimmed.to_string();
            }
        }
    }

    let tags = lists.get("tags").cloned().unwrap_or_default();
    let links = lists.get("links").cloned().unwrap_or_default();

    let mut wikilinks = Vec::new();
    let re = Regex::new(r"\[\[([^\]|]+)(?:\|[^\]]+)?\]\]").unwrap();
    for cap in re.captures_iter(&body) {
        if let Some(m) = cap.get(1) {
            let val = m.as_str().trim();
            if !val.is_empty() {
                wikilinks.push(val.to_string());
            }
        }
    }

    let attachments = lists.get("attachments").cloned().unwrap_or_default();
    let note_type = scalars
        .get("type")
        .cloned()
        .or_else(|| scalars.get("note_type").cloned());
    let template = scalars.get("template").cloned();
    let mut properties = scalars.clone();
    properties.remove("id");
    properties.remove("title");
    properties.remove("type");
    properties.remove("note_type");
    properties.remove("template");
    Ok(ParsedNote {
        id,
        title,
        tags,
        links,
        wikilinks,
        body,
        path: path.to_path_buf(),
        note_type,
        template,
        properties,
        attachments,
    })
}

pub fn scan_vault(vault_dir: &Path) -> anyhow::Result<Vec<VaultNote>> {
    if !vault_dir.exists() {
        return Ok(Vec::new());
    }

    let mut notes = Vec::new();
    for entry in WalkDir::new(vault_dir) {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        if entry.path().extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        if let Some(relative) = entry.path().strip_prefix(vault_dir).ok() {
            if let Some(first_component) = relative.components().next() {
                if let Some(os_str) = first_component.as_os_str().to_str() {
                    if os_str.eq_ignore_ascii_case("attachments")
                        || os_str.eq_ignore_ascii_case("templates")
                    {
                        continue;
                    }
                }
            }
        }
        let parsed = parse_note(entry.path())?;
        let preview = parsed
            .body
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("")
            .to_string();
        let search_text = parsed.body.clone();
        notes.push(VaultNote {
            id: parsed.id,
            title: parsed.title,
            tags: parsed.tags,
            links: parsed.links,
            wikilinks: parsed.wikilinks,
            body: parsed.body,
            path: parsed.path,
            search_text,
            preview,
            note_type: parsed.note_type,
            template: parsed.template,
            properties: parsed.properties,
            attachments: parsed.attachments,
        });
    }

    Ok(notes)
}

pub fn merge_vault_into_export(
    export_path: &Path,
    vault_dir: &Path,
) -> anyhow::Result<VaultMergeStats> {
    if !vault_dir.exists() {
        return Ok(VaultMergeStats {
            notes: 0,
            tags: 0,
            stubs: 0,
            links: 0,
        });
    }

    let mut notes = Vec::new();
    for entry in WalkDir::new(vault_dir) {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        if entry.path().extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        notes.push(parse_note(entry.path())?);
    }

    let mut key_to_id: HashMap<String, String> = HashMap::new();
    for note in &notes {
        let id_key = normalize_key(&note.id);
        let title_key = normalize_key(&note.title);
        let file_key = normalize_key(note.path.file_stem().and_then(|s| s.to_str()).unwrap_or(""));
        key_to_id.insert(id_key, note.id.clone());
        key_to_id.insert(title_key, note.id.clone());
        if !file_key.is_empty() {
            key_to_id.insert(file_key, note.id.clone());
        }
        key_to_id.insert(note.id.clone(), note.id.clone());
        key_to_id.insert(note.title.clone(), note.id.clone());
    }

    let mut vault_nodes = Vec::new();
    let mut vault_links = Vec::new();
    let mut tag_nodes: HashSet<String> = HashSet::new();
    let mut stub_nodes: HashSet<String> = HashSet::new();
    let mut property_nodes: HashSet<String> = HashSet::new();
    let mut link_keys: HashSet<(String, String, String)> = HashSet::new();

    for note in &notes {
        let node_id = format!("vault:{}", note.id);
        vault_nodes.push(json!({
            "id": node_id,
            "label": note.title,
            "kind": "Vault",
            "path": note.path.display().to_string(),
            "tags": note.tags,
            "search_text": note.body.chars().take(4000).collect::<String>(),
            "preview": note.body.lines().find(|l| !l.trim().is_empty()).unwrap_or("").to_string(),
            "note_type": note.note_type,
            "template": note.template,
            "properties": note.properties,
            "attachments": note.attachments,
        }));

        for tag in &note.tags {
            let tag_id = format!("tag:{}", tag);
            if tag_nodes.insert(tag_id.clone()) {
                vault_nodes.push(json!({
                    "id": tag_id,
                    "label": format!("#{}", tag),
                    "kind": "Tag",
                }));
            }
            if link_keys.insert((node_id.clone(), tag_id.clone(), "vault_tag".to_string())) {
                vault_links.push(json!({
                    "source": node_id,
                    "target": tag_id,
                    "weight": 0.4,
                    "emergent": false,
                    "age": 0,
                    "link_type": "vault_tag",
                }));
            }
        }

        for (prop_key, prop_value) in &note.properties {
            let prop_label = format!("{}:{}", prop_key, prop_value);
            let prop_id = format!(
                "vault_prop:{}:{}",
                normalize_key(prop_key),
                normalize_key(prop_value)
            );
            if property_nodes.insert(prop_id.clone()) {
                vault_nodes.push(json!({
                    "id": prop_id.clone(),
                    "label": prop_label,
                    "kind": "VaultProperty",
                    "property": prop_key,
                    "value": prop_value,
                }));
            }
            if link_keys.insert((
                node_id.clone(),
                prop_id.clone(),
                "vault_property".to_string(),
            )) {
                vault_links.push(json!({
                    "source": node_id,
                    "target": prop_id,
                    "weight": 0.3,
                    "emergent": false,
                    "age": 0,
                    "link_type": "vault_property",
                }));
            }
        }

        for attachment in &note.attachments {
            let att_label = attachment.clone();
            let att_id = format!("vault_attachment:{}:{}", node_id, normalize_key(attachment));
            if link_keys.insert((
                node_id.clone(),
                att_id.clone(),
                "vault_attachment".to_string(),
            )) {
                vault_nodes.push(json!({
                    "id": att_id.clone(),
                    "label": att_label,
                    "kind": "VaultAttachment",
                }));
                vault_links.push(json!({
                    "source": node_id,
                    "target": att_id,
                    "weight": 0.2,
                    "emergent": false,
                    "age": 0,
                    "link_type": "vault_attachment",
                }));
            }
        }

        let mut targets: Vec<String> = Vec::new();
        targets.extend(note.links.iter().cloned());
        targets.extend(note.wikilinks.iter().cloned());
        for target in targets {
            let key = normalize_key(&target);
            let resolved = key_to_id
                .get(&key)
                .cloned()
                .unwrap_or_else(|| target.clone());
            let target_id = if key_to_id.contains_key(&key) {
                format!("vault:{}", resolved)
            } else {
                let stub_id = format!("vault_stub:{}", resolved);
                if stub_nodes.insert(stub_id.clone()) {
                    vault_nodes.push(json!({
                        "id": stub_id,
                        "label": resolved,
                        "kind": "VaultStub",
                    }));
                }
                stub_id
            };

            if link_keys.insert((node_id.clone(), target_id.clone(), "vault_link".to_string())) {
                vault_links.push(json!({
                    "source": node_id,
                    "target": target_id,
                    "weight": 0.8,
                    "emergent": false,
                    "age": 0,
                    "link_type": "vault_link",
                }));
            }
        }
    }

    let json_str = fs::read_to_string(export_path)
        .with_context(|| format!("read export graph {}", export_path.display()))?;
    let mut export_value: serde_json::Value = serde_json::from_str(&json_str)?;
    let nodes_array = export_value
        .get_mut("nodes")
        .and_then(|v| v.as_array_mut())
        .ok_or_else(|| anyhow::anyhow!("export graph missing nodes array"))?;

    let mut existing_ids = HashSet::new();
    for node in nodes_array.iter() {
        if let Some(id) = node.get("id").and_then(|v| v.as_str()) {
            existing_ids.insert(id.to_string());
        }
    }

    for node in vault_nodes {
        if let Some(id) = node.get("id").and_then(|v| v.as_str()) {
            if existing_ids.insert(id.to_string()) {
                nodes_array.push(node);
            }
        }
    }

    let mut existing_links = HashSet::new();
    let links_array = export_value
        .get_mut("links")
        .and_then(|v| v.as_array_mut())
        .ok_or_else(|| anyhow::anyhow!("export graph missing links array"))?;
    for link in links_array.iter() {
        if let (Some(source), Some(target), Some(link_type)) = (
            link.get("source").and_then(|v| v.as_str()),
            link.get("target").and_then(|v| v.as_str()),
            link.get("link_type").and_then(|v| v.as_str()),
        ) {
            existing_links.insert((
                source.to_string(),
                target.to_string(),
                link_type.to_string(),
            ));
        }
    }

    for link in &vault_links {
        if let (Some(source), Some(target), Some(link_type)) = (
            link.get("source").and_then(|v| v.as_str()),
            link.get("target").and_then(|v| v.as_str()),
            link.get("link_type").and_then(|v| v.as_str()),
        ) {
            if existing_links.insert((
                source.to_string(),
                target.to_string(),
                link_type.to_string(),
            )) {
                links_array.push(link.clone());
            }
        }
    }

    if let Some(meta) = export_value.get_mut("meta").and_then(|v| v.as_object_mut()) {
        meta.insert("vault_notes".to_string(), json!(notes.len()));
        meta.insert("vault_tags".to_string(), json!(tag_nodes.len()));
        meta.insert("vault_stubs".to_string(), json!(stub_nodes.len()));
        meta.insert("vault_links".to_string(), json!(vault_links.len()));
    }

    let output = serde_json::to_string_pretty(&export_value)?;
    fs::write(export_path, output)?;

    Ok(VaultMergeStats {
        notes: notes.len(),
        tags: tag_nodes.len(),
        stubs: stub_nodes.len(),
        links: vault_links.len(),
    })
}
