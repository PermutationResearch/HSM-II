//! Self-critique loop that flags sycophancy and tightens assistant directives before revising output.
//!
//! Use this when you want the model to **audit its own answer** for over-agreement, flattery,
//! false certainty, or uncritical mirroring of the user—then **adjust constraints** and optionally rewrite.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::llm::client::{LlmClient, LlmRequest, Message};

/// Configuration for the quality loop.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AntiSycophancyConfig {
    /// Maximum critique→revise iterations (includes the initial draft as round 0).
    pub max_rounds: u32,
    pub temperature_critique: f64,
    pub temperature_revise: f64,
    /// If model-reported risk is below this, stop early (after recording critique).
    pub risk_stop_threshold: f64,
    pub model_override: Option<String>,
}

impl Default for AntiSycophancyConfig {
    fn default() -> Self {
        Self {
            max_rounds: 3,
            temperature_critique: 0.2,
            temperature_revise: 0.35,
            risk_stop_threshold: 0.32,
            model_override: None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CriticParse {
    pub raw: String,
    pub risk_score: f64,
    pub issues: Vec<String>,
    pub revised_directives: Vec<String>,
    pub verdict: CriticVerdict,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CriticVerdict {
    Accept,
    Revise,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AntiSycophancyRoundLog {
    pub index: u32,
    pub draft_excerpt: String,
    pub heuristic_risk: f64,
    pub critic: CriticParse,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AntiSycophancyRunResult {
    pub final_text: String,
    pub aggregated_directives: Vec<String>,
    pub rounds: Vec<AntiSycophancyRoundLog>,
    pub stopped_reason: String,
}

const CRITIC_SYSTEM: &str = r#"You are an independent quality critic (not the user's ally).
Detect sycophancy and related failures: excessive praise, agreeing without evidence, mirroring the user's
framing when it may be wrong, false certainty, avoiding respectful disagreement, flattery, or padding
that signals compliance over truth.

Output EXACTLY these sections (headings must match):

RISK_SCORE: <0.0-1.0 higher means more sycophancy / epistemic risk>

ISSUES:
- <bullet>

REVISED_DIRECTIVES:
- <bullet constraint for the NEXT assistant turn>

VERDICT: accept | revise

Use VERDICT accept only if RISK_SCORE is low AND issues are minor."#;

const REVISE_SYSTEM: &str = r#"You revise an assistant draft to be accurate, grounded, and appropriately skeptical.
Do not flatter. Disagree clearly when warranted. Follow the directives exactly."#;

/// Run critique / optional revision rounds on an existing model answer.
pub async fn run_anti_sycophancy_loop(
    llm: Arc<LlmClient>,
    cfg: AntiSycophancyConfig,
    user_message: &str,
    context: Option<&str>,
    initial_draft: &str,
    seed_directives: &[String],
) -> anyhow::Result<AntiSycophancyRunResult> {
    let mut draft = initial_draft.to_string();
    let mut aggregated: Vec<String> = seed_directives.to_vec();
    let mut rounds = Vec::new();
    let mut stopped_reason = "max_rounds".to_string();

    let max = cfg.max_rounds.max(1);
    for r in 0..max {
        let heur = sycophancy_heuristic(&draft);
        let critic_raw = call_critic(
            &llm,
            &cfg,
            user_message,
            context,
            &draft,
            &aggregated,
        )
        .await?;
        let critic = parse_critic_output(&critic_raw);
        let excerpt: String = draft.chars().take(480).collect();
        rounds.push(AntiSycophancyRoundLog {
            index: r,
            draft_excerpt: excerpt,
            heuristic_risk: heur,
            critic: critic.clone(),
        });

        let combined_risk = (critic.risk_score * 0.75 + heur * 0.25).clamp(0.0, 1.0);
        let accept = critic.verdict == CriticVerdict::Accept || combined_risk < cfg.risk_stop_threshold;
        if accept {
            stopped_reason = if turns_early_ok(combined_risk, &critic) {
                "low_risk_or_accept"
            } else {
                "critic_accept"
            }
            .into();
            break;
        }

        if r + 1 >= max {
            stopped_reason = "max_rounds_reached".into();
            break;
        }

        for d in &critic.revised_directives {
            if !d.trim().is_empty() && !aggregated.iter().any(|x| x == d) {
                aggregated.push(d.trim().to_string());
            }
        }

        draft = revise_draft(
            &llm,
            &cfg,
            user_message,
            context,
            &draft,
            &aggregated,
        )
        .await?;
    }

    Ok(AntiSycophancyRunResult {
        final_text: draft,
        aggregated_directives: aggregated,
        rounds,
        stopped_reason,
    })
}

fn turns_early_ok(combined_risk: f64, critic: &CriticParse) -> bool {
    combined_risk < 0.28 && critic.issues.len() <= 1
}

async fn call_critic(
    llm: &Arc<LlmClient>,
    cfg: &AntiSycophancyConfig,
    user_message: &str,
    context: Option<&str>,
    draft: &str,
    directives: &[String],
) -> anyhow::Result<String> {
    let mut user = String::new();
    user.push_str("USER_MESSAGE:\n");
    user.push_str(user_message);
    if let Some(ctx) = context {
        user.push_str("\n\nCONTEXT:\n");
        user.push_str(ctx);
    }
    if !directives.is_empty() {
        user.push_str("\n\nCURRENT_DIRECTIVES (assistant was asked to follow):\n");
        for d in directives {
            user.push_str("- ");
            user.push_str(d);
            user.push('\n');
        }
    }
    user.push_str("\n\nASSISTANT_DRAFT:\n");
    user.push_str(draft);

    chat(
        llm,
        cfg,
        CRITIC_SYSTEM,
        &user,
        cfg.temperature_critique,
        2048,
    )
    .await
}

async fn revise_draft(
    llm: &Arc<LlmClient>,
    cfg: &AntiSycophancyConfig,
    user_message: &str,
    context: Option<&str>,
    previous: &str,
    directives: &[String],
) -> anyhow::Result<String> {
    let mut user = String::new();
    user.push_str("USER_MESSAGE:\n");
    user.push_str(user_message);
    if let Some(ctx) = context {
        user.push_str("\n\nCONTEXT:\n");
        user.push_str(ctx);
    }
    user.push_str("\n\nYou MUST follow these directives:\n");
    for d in directives {
        user.push_str("- ");
        user.push_str(d);
        user.push('\n');
    }
    user.push_str("\nPREVIOUS_DRAFT:\n");
    user.push_str(previous);
    user.push_str(
        "\n\nRewrite into a single improved answer. Shorter is fine if it improves clarity.",
    );

    chat(
        llm,
        cfg,
        REVISE_SYSTEM,
        &user,
        cfg.temperature_revise,
        4096,
    )
    .await
}

async fn chat(
    llm: &Arc<LlmClient>,
    cfg: &AntiSycophancyConfig,
    system: &str,
    user: &str,
    temperature: f64,
    max_tokens: usize,
) -> anyhow::Result<String> {
    let mut req = LlmRequest {
        model: cfg
            .model_override
            .clone()
            .unwrap_or_else(|| LlmRequest::default().model),
        messages: vec![Message::system(system), Message::user(user)],
        temperature,
        max_tokens: Some(max_tokens),
        top_p: Some(0.9),
        stream: false,
    };
    if let Some(m) = &cfg.model_override {
        req.model = m.clone();
    }
    Ok(llm.chat(req).await?.content)
}

fn parse_critic_output(raw: &str) -> CriticParse {
    let mut risk_score = 0.45_f64;
    let mut issues = Vec::new();
    let mut revised_directives = Vec::new();
    let mut verdict = CriticVerdict::Revise;
    let mut mode = "";

    for line in raw.lines() {
        let t = line.trim();
        if t.eq_ignore_ascii_case("ISSUES:") || t.starts_with("ISSUES:") {
            mode = "issues";
            continue;
        }
        if t.eq_ignore_ascii_case("REVISED_DIRECTIVES:")
            || t.starts_with("REVISED_DIRECTIVES:")
        {
            mode = "dirs";
            continue;
        }
        if let Some(rest) = t.strip_prefix("RISK_SCORE:") {
            if let Ok(v) = rest.trim().parse::<f64>() {
                risk_score = v.clamp(0.0, 1.0);
            }
            mode = "";
            continue;
        }
        if let Some(rest) = t.strip_prefix("VERDICT:") {
            let v = rest.trim().to_ascii_lowercase();
            verdict = if v.starts_with("accept") {
                CriticVerdict::Accept
            } else {
                CriticVerdict::Revise
            };
            mode = "";
            continue;
        }

        match mode {
            "issues" if t.starts_with('-') => issues.push(t.trim_start_matches('-').trim().into()),
            "dirs" if t.starts_with('-') => {
                revised_directives.push(t.trim_start_matches('-').trim().into())
            }
            _ => {}
        }
    }

    CriticParse {
        raw: raw.trim().to_string(),
        risk_score,
        issues,
        revised_directives,
        verdict,
    }
}

/// Fast lexical cue: higher = more suspicious (coarse; use with model critic).
pub fn sycophancy_heuristic(text: &str) -> f64 {
    let t = text.to_ascii_lowercase();
    let cues = [
        "you're absolutely right",
        "you're completely right",
        "excellent question",
        "great question",
        "brilliant",
        "i completely agree",
        "i totally agree",
        "you nailed it",
        "perfect understanding",
        "you're correct about everything",
        "couldn't agree more",
        "love where you're going",
        "exactly what i was thinking",
    ];
    let mut hits = 0u32;
    for c in cues {
        if t.contains(c) {
            hits += 1;
        }
    }
    (hits as f64 * 0.12).min(0.9)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_critic() {
        let raw = "RISK_SCORE: 0.8\n\nISSUES:\n- too agreeable\n\nREVISED_DIRECTIVES:\n- cite limits\n\nVERDICT: revise\n";
        let p = parse_critic_output(raw);
        assert!((p.risk_score - 0.8).abs() < 0.001);
        assert_eq!(p.verdict, CriticVerdict::Revise);
        assert!(!p.issues.is_empty());
    }

    #[test]
    fn heuristic_hits() {
        let h = sycophancy_heuristic("You're absolutely right — great question!");
        assert!(h > 0.1);
    }
}
