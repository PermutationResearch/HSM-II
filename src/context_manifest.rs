//! Per-turn context assembly stats (tiers, bytes, truncation) for debugging and cost control.

use serde::{Deserialize, Serialize};

/// Rough lifecycle tier (hot = always try to keep; cold = cut first under budget pressure — today used for manifest only).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContextTier {
    Hot,
    Warm,
    Cold,
}

/// One prompt section after assembly.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContextSectionStat {
    pub key: String,
    pub tier: ContextTier,
    /// UTF-8 length before cap/truncation.
    pub raw_bytes: usize,
    /// Bytes actually concatenated into the system prompt.
    pub emitted_bytes: usize,
    pub truncated: bool,
    pub included: bool,
}

/// Full snapshot for a single `assemble` call.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ContextManifest {
    pub sections: Vec<ContextSectionStat>,
    pub total_raw_bytes: usize,
    pub total_emitted_bytes: usize,
}

impl ContextManifest {
    pub fn summary_line(&self) -> String {
        let mut hot = 0usize;
        let mut warm = 0usize;
        let mut cold = 0usize;
        for s in &self.sections {
            if !s.included {
                continue;
            }
            match s.tier {
                ContextTier::Hot => hot += s.emitted_bytes,
                ContextTier::Warm => warm += s.emitted_bytes,
                ContextTier::Cold => cold += s.emitted_bytes,
            }
        }
        format!(
            "sections={} raw_bytes={} emitted_bytes={} (hot={} warm={} cold={})",
            self.sections.len(),
            self.total_raw_bytes,
            self.total_emitted_bytes,
            hot,
            warm,
            cold
        )
    }
}

/// Logical sections for [`GET /api/company/tasks/:task_id/llm-context`](crate::company_os::agents).
/// Keys are stable: `company`, `shared_memory`, `agent_memory`, `task`, `agent_profile`.
pub fn company_task_llm_context_manifest<F>(
    chunks: Vec<(&str, usize)>,
    tier_for: F,
) -> ContextManifest
where
    F: Fn(&str) -> ContextTier,
{
    let mut manifest = ContextManifest::default();
    for (key, bytes) in chunks {
        let tier = tier_for(key);
        manifest.total_raw_bytes += bytes;
        manifest.total_emitted_bytes += bytes;
        manifest.sections.push(ContextSectionStat {
            key: key.to_string(),
            tier,
            raw_bytes: bytes,
            emitted_bytes: bytes,
            truncated: false,
            included: true,
        });
    }
    manifest
}
