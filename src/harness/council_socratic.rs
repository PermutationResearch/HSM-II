//! Multi-role Socratic council: sequential role turns, then synthesis, then anti-sycophancy on the draft.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::llm::client::{LlmClient, LlmRequest, Message};

use super::{run_anti_sycophancy_loop, AntiSycophancyConfig, AntiSycophancyRunResult};

const TEMP_ROLE: f64 = 0.42;
const TEMP_SYNTH: f64 = 0.35;
const MAX_TOKENS_ROLE: usize = 1024;
const MAX_TOKENS_SYNTH: usize = 2048;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CouncilRoleTurn {
    pub round: u32,
    pub role: String,
    pub content: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CouncilSocraticResult {
    pub proposition: String,
    pub roles_used: Vec<String>,
    pub turns: Vec<CouncilRoleTurn>,
    pub synthesis_draft: String,
    pub anti_sycophancy: AntiSycophancyRunResult,
}

fn default_roles() -> Vec<String> {
    vec![
        "socratic_questioner".into(),
        "epistemic_critic".into(),
        "integrator".into(),
    ]
}

fn system_for_role(role_key: &str) -> &'static str {
    let k = role_key.to_ascii_lowercase();
    if k.contains("question") || k.contains("socratic") {
        return "You are a Socratic interlocutor in a council. Do not flatter or praise the user. \
Ask concise probing questions that expose unstated assumptions, hidden premises, or missing definitions. \
You may add one short sentence on why the question matters. No bullet list of praise. \
Output plain text only, under 12 sentences.";
    }
    if k.contains("critic") || k.contains("challenge") || k.contains("devil") {
        return "You are an epistemic critic. Challenge weak reasoning, overconfidence, and false consensus. \
Name risks, counterexamples, or missing evidence. Be direct, not hostile. Do not agree just to move on. \
Plain text only, under 14 sentences.";
    }
    if k.contains("synth") || k.contains("integrat") {
        return "You integrate competing lines of reasoning. Acknowledge tradeoffs and residual uncertainty. \
Do not paper over disagreement. Plain text only, under 14 sentences.";
    }
    "You are a council member with a distinct viewpoint. Respond constructively without sycophancy. \
Plain text only, under 12 sentences."
}

const SYNTH_SYSTEM: &str = "You produce the council's single draft answer to the PROPOSITION. \
Reflect tensions raised in the transcript; state uncertainties; avoid flattery. Plain prose, 8–22 sentences.";

async fn chat_one(
    llm: &Arc<LlmClient>,
    model: &str,
    system: &str,
    user: &str,
    temperature: f64,
    max_tokens: usize,
) -> anyhow::Result<String> {
    let req = LlmRequest {
        model: model.to_string(),
        messages: vec![Message::system(system), Message::user(user)],
        temperature,
        max_tokens: Some(max_tokens),
        top_p: Some(0.9),
        stream: false,
    };
    Ok(llm.chat(req).await?.content)
}

/// Run role agents in lockstep rounds, synthesize, then [`run_anti_sycophancy_loop`] on the synthesis.
pub async fn run_council_socratic_with_anti_sycophancy(
    llm: Arc<LlmClient>,
    anti_cfg: AntiSycophancyConfig,
    proposition: &str,
    context: Option<&str>,
    roles: &[String],
    council_rounds: u32,
    seed_directives: &[String],
) -> anyhow::Result<CouncilSocraticResult> {
    let roles: Vec<String> = if roles.is_empty() {
        default_roles()
    } else {
        roles.to_vec()
    };
    let rounds = council_rounds.clamp(1, 4);
    let model = anti_cfg
        .model_override
        .clone()
        .unwrap_or_else(|| LlmRequest::default().model);

    let mut turns: Vec<CouncilRoleTurn> = Vec::new();
    let mut transcript = String::new();

    for r in 0..rounds {
        for role in &roles {
            let sys = system_for_role(role);
            let mut u = String::new();
            u.push_str("PROPOSITION:\n");
            u.push_str(proposition);
            if let Some(ctx) = context {
                u.push_str("\n\nCONTEXT:\n");
                u.push_str(ctx);
            }
            if !transcript.is_empty() {
                u.push_str("\n\nCOUNCIL_TRANSCRIPT_SO_FAR:\n");
                u.push_str(&transcript);
            }
            u.push_str("\n\nYOUR_ASSIGNED_ROLE_ID: ");
            u.push_str(role);
            u.push_str("\nThis is council round ");
            u.push_str(&(r + 1).to_string());
            u.push_str(" of ");
            u.push_str(&rounds.to_string());
            u.push_str(". Speak only as this role for this turn.");

            let content = chat_one(&llm, &model, sys, &u, TEMP_ROLE, MAX_TOKENS_ROLE).await?;
            let line = format!("--- Round {} · {} ---\n{}\n", r + 1, role, content.trim());
            transcript.push_str(&line);
            turns.push(CouncilRoleTurn {
                round: r,
                role: role.clone(),
                content: content.trim().to_string(),
            });
        }
    }

    let mut synth_user = String::new();
    synth_user.push_str("PROPOSITION:\n");
    synth_user.push_str(proposition);
    if let Some(ctx) = context {
        synth_user.push_str("\n\nCONTEXT:\n");
        synth_user.push_str(ctx);
    }
    synth_user.push_str("\n\nFULL_COUNCIL_TRANSCRIPT:\n");
    synth_user.push_str(&transcript);
    synth_user.push_str(
        "\n\nWrite the INTEGRATED_DRAFT: one answer a human could act on. \
No meta-commentary about the council process unless essential.",
    );

    let synthesis_draft = chat_one(
        &llm,
        &model,
        SYNTH_SYSTEM,
        &synth_user,
        TEMP_SYNTH,
        MAX_TOKENS_SYNTH,
    )
    .await?;

    let anti = run_anti_sycophancy_loop(
        llm,
        anti_cfg,
        proposition,
        Some(transcript.as_str()),
        synthesis_draft.trim(),
        seed_directives,
    )
    .await?;

    Ok(CouncilSocraticResult {
        proposition: proposition.to_string(),
        roles_used: roles,
        turns,
        synthesis_draft: synthesis_draft.trim().to_string(),
        anti_sycophancy: anti,
    })
}
