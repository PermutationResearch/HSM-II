//! Autoreason loop: adversarial critique, three candidates (keep / rewrite / synthesize),
//! blind Borda judges, convergence when “keep” wins two rounds in a row.
//!
//! Inspired by multi-agent “no single metric” refinement; each round costs several LLM calls.

use rand::seq::SliceRandom;
use rand::thread_rng;
use serde::{Deserialize, Serialize};

use crate::llm::client::{LlmClient, LlmRequest, Message};

/// Configuration for [`run_autoreason`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AutoreasonConfig {
    /// Stop when “keep current draft” wins this many consecutive rounds (diagram default: 2).
    pub convergence_streak: u32,
    /// Safety cap on outer rounds.
    pub max_rounds: u32,
    pub temperature_author: f64,
    pub temperature_strawman: f64,
    pub temperature_candidates: f64,
    pub temperature_judge: f64,
    pub max_tokens_author: usize,
    pub max_tokens_critique: usize,
    pub max_tokens_rewrite: usize,
    pub max_tokens_judge: usize,
    /// Number of blind judges (diagram uses 3).
    pub num_judges: u32,
}

impl Default for AutoreasonConfig {
    fn default() -> Self {
        Self {
            convergence_streak: 2,
            max_rounds: 8,
            temperature_author: 0.4,
            temperature_strawman: 0.5,
            temperature_candidates: 0.4,
            temperature_judge: 0.0,
            max_tokens_author: 2500,
            max_tokens_critique: 1800,
            max_tokens_rewrite: 2500,
            max_tokens_judge: 120,
            num_judges: 3,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AutoreasonRoundRecord {
    pub round: u32,
    pub streak_after: u32,
    pub strawman: String,
    pub candidate_keep: String,
    pub candidate_rewrite: String,
    pub candidate_synth: String,
    /// Borda totals for [keep, rewrite, synth] after this round’s panel.
    pub borda_scores: [i32; 3],
    pub winner: usize,
    pub winner_is_keep: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AutoreasonOutput {
    pub final_text: String,
    pub rounds: Vec<AutoreasonRoundRecord>,
    pub converged: bool,
    pub stop_reason: String,
    pub total_prompt_tokens: usize,
    pub total_completion_tokens: usize,
    pub llm_calls: u32,
}

async fn chat(
    client: &LlmClient,
    model: &str,
    system: &str,
    user: &str,
    temperature: f64,
    max_tokens: usize,
    counters: &mut TokenCounter,
) -> anyhow::Result<String> {
    let req = LlmRequest {
        model: model.to_string(),
        messages: vec![Message::system(system), Message::user(user)],
        temperature,
        max_tokens: Some(max_tokens),
        ..LlmRequest::default()
    };
    let resp = client.chat(req).await?;
    counters.add(&resp.usage);
    Ok(resp.content)
}

struct TokenCounter {
    pub prompt: usize,
    pub completion: usize,
    pub calls: u32,
}

impl TokenCounter {
    fn new() -> Self {
        Self {
            prompt: 0,
            completion: 0,
            calls: 0,
        }
    }
    fn add(&mut self, u: &crate::llm::client::Usage) {
        self.prompt += u.prompt_tokens;
        self.completion += u.completion_tokens;
        self.calls += 1;
    }
}

/// Apply Borda: for 3 items, ranks 0..2 (0 = best among the three orderings) get points 2,1,0.
pub fn borda_points_from_slot_order(best_first_slots: &[usize; 3]) -> [i32; 3] {
    let mut pts = [0i32; 3];
    for (rank, &slot) in best_first_slots.iter().enumerate() {
        if slot < 3 {
            pts[slot] += (2 - rank) as i32;
        }
    }
    pts
}

/// Parse judge text: expect three distinct slot indices **1-based** (1,2,3) best-first.
/// Returns 0-based slot indices in best-first order.
pub fn parse_judge_ranking_1based(text: &str) -> Option<[usize; 3]> {
    let mut nums: Vec<usize> = Vec::new();
    for tok in text.replace(',', " ").split_whitespace() {
        let t = tok.trim_matches(|c| c == '[' || c == ']' || c == '.' || c == '(' || c == ')');
        if let Ok(n) = t.parse::<usize>() {
            if (1..=3).contains(&n) {
                nums.push(n);
            }
        }
    }
    if nums.len() < 3 {
        // Allow contiguous digits "213"
        let digits: String = text.chars().filter(|c| ('1'..='3').contains(c)).collect();
        if digits.len() == 3 {
            nums = digits.chars().filter_map(|c| c.to_digit(10).map(|d| d as usize)).collect();
        }
    }
    nums.truncate(3);
    if nums.len() != 3 {
        return None;
    }
    let mut seen = std::collections::HashSet::new();
    for &n in &nums {
        if !seen.insert(n) {
            return None;
        }
    }
    Some([nums[0] - 1, nums[1] - 1, nums[2] - 1])
}

fn aggregate_borda(
    slot_to_candidate: &[usize; 3],
    judge_best_first_slots: &[[usize; 3]],
) -> [i32; 3] {
    let mut total = [0i32; 3];
    for jf in judge_best_first_slots {
        let slot_pts = borda_points_from_slot_order(jf);
        for s in 0..3 {
            let c = slot_to_candidate[s];
            if c < 3 {
                total[c] += slot_pts[s];
            }
        }
    }
    total
}

fn argmax3(t: &[i32; 3]) -> usize {
    let mut best = 0usize;
    for i in 1..3 {
        if t[i] > t[best] {
            best = i;
        }
    }
    best
}

/// Run full Autoreason refinement on `task_prompt`.
pub async fn run_autoreason(
    client: &LlmClient,
    model: &str,
    task_prompt: &str,
    cfg: &AutoreasonConfig,
) -> anyhow::Result<AutoreasonOutput> {
    const SYS_AUTHOR: &str = "You are Author A. Write a clear, actionable answer to the task. Be concise but complete. No meta-commentary.";
    const SYS_STRAW: &str = "You are a strict adversarial reviewer (strawman). List concrete problems and gaps in the draft only — no fixes, no rewritten answer, no politeness. Numbered list.";
    const SYS_B: &str = "You rewrite the answer to address every critique while staying faithful to the original task. Output only the improved answer.";
    const SYS_AB: &str = "You merge the best parts of Draft A and Draft B into a single coherent answer to the task. Prefer correctness and clarity. Output only the merged answer.";
    const SYS_JUDGE: &str = "You are an impartial judge. Three anonymous candidates [1] [2] [3] answer the same task. Rank them BEST to WORST for correctness, usefulness, and task fit.\nReply with EXACTLY three digits 1–3 separated by commas (best first). Example: 2,3,1\nNo other text.";

    let mut counters = TokenCounter::new();
    let mut current = chat(
        client,
        model,
        SYS_AUTHOR,
        task_prompt,
        cfg.temperature_author,
        cfg.max_tokens_author,
        &mut counters,
    )
    .await?;

    let mut streak = 0u32;
    let mut rounds: Vec<AutoreasonRoundRecord> = Vec::new();
    let mut converged = false;
    let mut stop_reason = String::new();

    for round in 1..=cfg.max_rounds {
        let keeper = current.clone();
        let straw_user = format!(
            "## Task\n{task_prompt}\n\n## Draft\n{}\n\nList problems only.",
            &keeper
        );
        let strawman = chat(
            client,
            model,
            SYS_STRAW,
            &straw_user,
            cfg.temperature_strawman,
            cfg.max_tokens_critique,
            &mut counters,
        )
        .await?;

        let b_user = format!(
            "## Task\n{task_prompt}\n\n## Draft A\n{}\n\n## Critique\n{}\n\nRewrite into a single improved answer.",
            &keeper, &strawman
        );
        let cand_b = chat(
            client,
            model,
            SYS_B,
            &b_user,
            cfg.temperature_candidates,
            cfg.max_tokens_rewrite,
            &mut counters,
        )
        .await?;

        let ab_user = format!(
            "## Task\n{task_prompt}\n\n## Draft A\n{}\n\n## Draft B\n{}\n\nSynthesize one answer.",
            &keeper, &cand_b
        );
        let cand_ab = chat(
            client,
            model,
            SYS_AB,
            &ab_user,
            cfg.temperature_candidates,
            cfg.max_tokens_rewrite,
            &mut counters,
        )
        .await?;

        let candidates = [keeper.clone(), cand_b, cand_ab];

        // Shuffle display order: slot s shows candidate index perm[s]
        let mut perm = [0usize, 1, 2];
        perm.shuffle(&mut thread_rng());

        let mut blind = String::new();
        blind.push_str("## Task\n");
        blind.push_str(task_prompt);
        blind.push_str("\n\n");
        for s in 0..3 {
            blind.push_str(&format!("### [ {} ]\n{}\n\n", s + 1, candidates[perm[s]]));
        }
        blind.push_str("Rank BEST to WORST as three comma-separated numbers (1–3), best first.");

        let mut judge_rankings: Vec<[usize; 3]> = Vec::new();
        for j in 0..cfg.num_judges {
            let judge_user = format!(
                "{blind}\n\n(Judge #{}, independent ranking.)",
                j + 1
            );
            let raw = chat(
                client,
                model,
                SYS_JUDGE,
                &judge_user,
                cfg.temperature_judge,
                cfg.max_tokens_judge,
                &mut counters,
            )
            .await?;
            if let Some(slots) = parse_judge_ranking_1based(&raw) {
                judge_rankings.push(slots);
            }
        }

        if judge_rankings.is_empty() {
            stop_reason = "no_valid_judge_rankings".into();
            return Ok(AutoreasonOutput {
                final_text: current,
                rounds,
                converged: false,
                stop_reason,
                total_prompt_tokens: counters.prompt,
                total_completion_tokens: counters.completion,
                llm_calls: counters.calls,
            });
        }

        let slot_to_cand = perm;
        let mut total_borda = [0i32; 3];
        for jr in &judge_rankings {
            let part = aggregate_borda(&slot_to_cand, std::slice::from_ref(jr));
            for i in 0..3 {
                total_borda[i] += part[i];
            }
        }

        let winner = argmax3(&total_borda);
        let winner_is_keep = winner == 0;

        if winner_is_keep {
            streak += 1;
        } else {
            streak = 0;
            current = candidates[winner].clone();
        }

        rounds.push(AutoreasonRoundRecord {
            round,
            streak_after: streak,
            strawman,
            candidate_keep: candidates[0].clone(),
            candidate_rewrite: candidates[1].clone(),
            candidate_synth: candidates[2].clone(),
            borda_scores: total_borda,
            winner,
            winner_is_keep,
        });

        if streak >= cfg.convergence_streak {
            converged = true;
            stop_reason = "converged_keep_streak".into();
            break;
        }
    }

    if stop_reason.is_empty() {
        stop_reason = "max_rounds".into();
    }

    Ok(AutoreasonOutput {
        final_text: current,
        rounds,
        converged,
        stop_reason,
        total_prompt_tokens: counters.prompt,
        total_completion_tokens: counters.completion,
        llm_calls: counters.calls,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn borda_single_judge() {
        // slot 1 (0-based) best, then 0, then 2
        let pts = borda_points_from_slot_order(&[1, 0, 2]);
        assert_eq!(pts, [1, 2, 0]);
        let slot_to_cand = [0usize, 1, 2];
        let t = aggregate_borda(&slot_to_cand, &[[1, 0, 2]]);
        assert_eq!(t, [1, 2, 0]);
    }

    #[test]
    fn parse_ranking_variants() {
        assert_eq!(
            parse_judge_ranking_1based("2, 3, 1"),
            Some([1, 2, 0])
        );
        assert_eq!(parse_judge_ranking_1based("231"), Some([1, 2, 0]));
    }
}
