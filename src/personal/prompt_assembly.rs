//! Deterministic prompt section order + per-section byte caps (`HSM_PROMPT_SECTION_ORDER`, `HSM_PROMPT_CAP_<KEY>`).

use std::collections::HashMap;

use crate::context_manifest::{ContextManifest, ContextSectionStat, ContextTier};

/// Default section keys for `EnhancedPersonalAgent::process_with_skills`.
pub fn default_section_order() -> Vec<String> {
    vec![
        "living".into(),
        "memory".into(),
        "route".into(),
        "business".into(),
        "prefetch".into(),
        "belief".into(),
        "skill".into(),
        "autocontext".into(),
        "md_skills".into(),
        "tail".into(),
    ]
}

#[derive(Clone, Debug)]
pub struct PromptAssemblyPolicy {
    pub section_order: Vec<String>,
    pub caps: HashMap<String, usize>,
}

impl PromptAssemblyPolicy {
    pub fn from_env() -> Self {
        let section_order = std::env::var("HSM_PROMPT_SECTION_ORDER")
            .ok()
            .map(|s| {
                s.split(',')
                    .map(|x| x.trim().to_string())
                    .filter(|x| !x.is_empty())
                    .collect()
            })
            .filter(|v: &Vec<String>| !v.is_empty())
            .unwrap_or_else(default_section_order);

        let mut caps = HashMap::new();
        for (k, v) in std::env::vars() {
            let prefix = "HSM_PROMPT_CAP_";
            if let Some(rest) = k.strip_prefix(prefix) {
                if let Ok(n) = v.parse::<usize>() {
                    caps.insert(rest.to_ascii_lowercase(), n);
                }
            }
        }

        Self {
            section_order,
            caps,
        }
    }
}

fn truncate_bytes(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
}

/// Concatenate sections in policy order; unknown keys append after. Applies UTF-8 byte caps when set.
pub fn assemble_prompt_sections(
    parts: &[(String, String)],
    policy: &PromptAssemblyPolicy,
) -> String {
    let mut map: HashMap<String, String> = parts.iter().cloned().collect();
    let mut out = String::new();
    for key in &policy.section_order {
        let k = key.to_ascii_lowercase();
        if let Some(body) = map.remove(&k) {
            let cap = policy.caps.get(&k).copied().unwrap_or(usize::MAX);
            let piece = if cap == usize::MAX {
                body
            } else {
                truncate_bytes(&body, cap)
            };
            out.push_str(&piece);
        }
    }
    let mut rest: Vec<_> = map.into_iter().collect();
    rest.sort_by(|a, b| a.0.cmp(&b.0));
    for (_k, body) in rest {
        out.push_str(&body);
    }
    out
}

/// Same as [`assemble_prompt_sections`], plus a [`ContextManifest`] (tiers, byte counts, truncation).
pub fn assemble_prompt_sections_with_manifest<F>(
    parts: &[(String, String)],
    policy: &PromptAssemblyPolicy,
    tier_for: F,
) -> (String, ContextManifest)
where
    F: Fn(&str) -> ContextTier,
{
    let mut map: HashMap<String, String> = parts.iter().cloned().collect();
    let mut out = String::new();
    let mut manifest = ContextManifest::default();

    for key in &policy.section_order {
        let k = key.to_ascii_lowercase();
        let tier = tier_for(&k);
        if let Some(body) = map.remove(&k) {
            let raw_bytes = body.len();
            let cap = policy.caps.get(&k).copied().unwrap_or(usize::MAX);
            let (piece, truncated) = if cap == usize::MAX {
                (body, false)
            } else {
                let was_truncated = body.len() > cap;
                let t = truncate_bytes(&body, cap);
                (t, was_truncated)
            };
            let emitted_bytes = piece.len();
            manifest.sections.push(ContextSectionStat {
                key: k.clone(),
                tier,
                raw_bytes,
                emitted_bytes,
                truncated,
                included: true,
            });
            manifest.total_raw_bytes += raw_bytes;
            manifest.total_emitted_bytes += emitted_bytes;
            out.push_str(&piece);
        }
    }

    let mut rest: Vec<_> = map.into_iter().collect();
    rest.sort_by(|a, b| a.0.cmp(&b.0));
    for (k, body) in rest {
        let tier = tier_for(&k);
        let raw_bytes = body.len();
        manifest.sections.push(ContextSectionStat {
            key: k.clone(),
            tier,
            raw_bytes,
            emitted_bytes: raw_bytes,
            truncated: false,
            included: true,
        });
        manifest.total_raw_bytes += raw_bytes;
        manifest.total_emitted_bytes += raw_bytes;
        out.push_str(&body);
    }

    (out, manifest)
}

#[cfg(test)]
mod manifest_tests {
    use super::*;

    #[test]
    fn manifest_counts_bytes_and_truncation() {
        let policy = PromptAssemblyPolicy {
            section_order: vec!["a".into(), "b".into()],
            caps: [("a".to_string(), 4usize)].into_iter().collect(),
        };
        let parts = vec![("a".into(), "abcdef".into()), ("b".into(), "xy".into())];
        let (out, m) =
            assemble_prompt_sections_with_manifest(&parts, &policy, |_| ContextTier::Warm);
        assert!(out.contains('…') || out.len() < "abcdefxy".len());
        assert_eq!(m.sections.len(), 2);
        let a = m.sections.iter().find(|s| s.key == "a").unwrap();
        assert!(a.truncated);
        assert_eq!(a.raw_bytes, 6);
        assert!(a.emitted_bytes < a.raw_bytes);
    }
}
