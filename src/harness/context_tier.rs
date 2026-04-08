//! Phase 2 — tiered context (L0 summary → L1 detail → L2 raw) with env-driven budgets.

use serde::{Deserialize, Serialize};

/// Which logical stream is being compressed (for budgets and future summarizers).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextStreamKind {
    Chat,
    CouncilTrace,
    MemoryInject,
    SkillIndex,
    TaskTrail,
}

/// Tier selector: what depth to inject into a prompt.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextTier {
    /// Short summaries only (e.g. one line per turn).
    L0Summary,
    /// Normal detail (trimmed messages).
    #[default]
    L1Detail,
    /// Full raw (use sparingly).
    L2Raw,
}

/// Per-stream character budgets (approximate; UTF-8 safe truncation elsewhere).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TierBudget {
    pub max_chars_l0: usize,
    pub max_chars_l1: usize,
    pub max_chars_l2: usize,
}

impl Default for TierBudget {
    fn default() -> Self {
        Self {
            max_chars_l0: 4_000,
            max_chars_l1: 24_000,
            max_chars_l2: 120_000,
        }
    }
}

/// Policy table: one budget row per stream; max tier cap for injection sites.
#[derive(Clone, Debug, Default)]
pub struct TierPolicy {
    pub chat: TierBudget,
    pub max_inject_tier: ContextTier,
    /// Recent chat pairs kept at L1 before older pairs drop to L0.
    pub chat_l1_tail_pairs: usize,
    /// Max chars per older pair user/assistant line at L0.
    pub chat_l0_pair_line_cap: usize,
    /// Collapse middle turns into one placeholder pair (L1/L2 paths only).
    pub collapse_chat_middle: bool,
}

impl TierPolicy {
    pub fn from_env() -> Self {
        let chat = TierBudget {
            max_chars_l0: parse_usize_env("HSM_CTX_CHAT_L0_MAX", 4_000),
            max_chars_l1: parse_usize_env("HSM_CTX_CHAT_L1_MAX", 24_000),
            max_chars_l2: parse_usize_env("HSM_CTX_CHAT_L2_MAX", 120_000),
        };
        let max_inject_tier = match std::env::var("HSM_CTX_MAX_TIER")
            .map(|s| s.trim().to_ascii_lowercase())
            .unwrap_or_default()
            .as_str()
        {
            "l2" | "raw" => ContextTier::L2Raw,
            "l0" | "summary" => ContextTier::L0Summary,
            _ => ContextTier::L1Detail,
        };
        let collapse_chat_middle = std::env::var("HSM_CTX_COLLAPSE_MIDDLE")
            .map(|v| {
                let s = v.trim();
                !(s == "0" || s.eq_ignore_ascii_case("false") || s.eq_ignore_ascii_case("no"))
            })
            .unwrap_or(false);
        Self {
            chat,
            max_inject_tier,
            chat_l1_tail_pairs: parse_usize_env("HSM_CTX_CHAT_L1_TAIL_PAIRS", 8),
            chat_l0_pair_line_cap: parse_usize_env("HSM_CTX_CHAT_L0_LINE_CAP", 240),
            collapse_chat_middle,
        }
    }

    /// Clip chat history for LLM injection according to tier and chat budget.
    pub fn clip_chat_pairs(&self, history: &[(String, String)]) -> Vec<(String, String)> {
        let mut v = match self.max_inject_tier {
            ContextTier::L2Raw => history.to_vec(),
            ContextTier::L1Detail => clip_pairs_char_budget(history, self.chat.max_chars_l1),
            ContextTier::L0Summary => return self.clip_chat_l0(history),
        };
        if self.collapse_chat_middle && v.len() > 5 {
            let tail_keep = self
                .chat_l1_tail_pairs
                .max(1)
                .min(v.len().saturating_sub(2));
            v = collapse_middle_pairs(v, 1, tail_keep);
        }
        v
    }

    fn clip_chat_l0(&self, history: &[(String, String)]) -> Vec<(String, String)> {
        if history.is_empty() {
            return Vec::new();
        }
        let tail = self.chat_l1_tail_pairs.min(history.len());
        let start = history.len() - tail;
        let mut out: Vec<(String, String)> = Vec::new();
        let cap = self.chat_l0_pair_line_cap;
        for (i, (u, a)) in history.iter().enumerate() {
            if i >= start {
                out.push((u.clone(), a.clone()));
            } else {
                out.push((truncate_chars(u, cap), truncate_chars(a, cap)));
            }
        }
        clip_pairs_char_budget(&out, self.chat.max_chars_l0)
    }
}

fn parse_usize_env(key: &str, default: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|&n| n > 0)
        .unwrap_or(default)
}

fn truncate_chars(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let t: String = s.chars().take(max_chars.saturating_sub(1)).collect();
    format!("{t}…")
}

fn collapse_middle_pairs(
    pairs: Vec<(String, String)>,
    head_keep: usize,
    tail_keep: usize,
) -> Vec<(String, String)> {
    if pairs.len() <= head_keep + tail_keep + 1 {
        return pairs;
    }
    let n = pairs.len() - head_keep - tail_keep;
    let mut out: Vec<(String, String)> = pairs.iter().take(head_keep).cloned().collect();
    out.push((format!("… [collapsed {n} chat turn(s)] …"), String::new()));
    out.extend(pairs[pairs.len() - tail_keep..].iter().cloned());
    out
}

fn clip_pairs_char_budget(pairs: &[(String, String)], max_chars: usize) -> Vec<(String, String)> {
    if pairs.is_empty() {
        return Vec::new();
    }
    let mut out: Vec<(String, String)> = Vec::new();
    let mut used = 0usize;
    for (u, a) in pairs.iter().rev() {
        let pair_cost = u.chars().count() + a.chars().count() + 4;
        if used + pair_cost > max_chars && !out.is_empty() {
            break;
        }
        out.push((u.clone(), a.clone()));
        used += pair_cost;
    }
    out.reverse();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn l0_truncates_old_pairs() {
        let mut p = TierPolicy::from_env();
        p.chat_l1_tail_pairs = 1;
        p.chat_l0_pair_line_cap = 10;
        p.max_inject_tier = ContextTier::L0Summary;
        let h: Vec<_> = (0..3)
            .map(|i| {
                (
                    format!("user long message {i} xxxxxxxxx"),
                    format!("assistant long reply {i} yyyyyyyyy"),
                )
            })
            .collect();
        let c = p.clip_chat_pairs(&h);
        assert_eq!(c.len(), 3);
        assert!(c[2].0.contains("user long"));
    }
}
