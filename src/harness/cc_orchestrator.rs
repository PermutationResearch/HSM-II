//! Multi-agent harness: parallel drafts + cross-review + optional synthesis.
//!
//! **CC agents** here means *collaborating cloud/coding workers*: either distinct LLM
//! personas via [`crate::llm::client::LlmClient`] or HTTP endpoints (one URL per slot).
//!
//! Set `HSM_CC_AGENT_ENDPOINTS` to a comma-separated list of URLs with the same length as
//! configured agent slots to route each slot through `POST` JSON instead of the shared LLM client.
//!
//! Request body shape (POST, `application/json`):
//! ```json
//! {
//!   "phase": "draft" | "review",
//!   "task_id": "...",
//!   "agent_id": "...",
//!   "instruction": "...",
//!   "context": null,
//!   "subject_agent_id": null,
//!   "peer_output": null
//! }
//! ```
//! Response: JSON `{"text":"..."}` or `{"content":"..."}`, or a plain-text body.

use std::sync::Arc;
use std::time::Instant;

use anyhow::{anyhow, Context};
use futures_util::future::join_all;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::instrument;

use crate::llm::client::{LlmClient, LlmRequest, Message};

use super::runtime::HarnessRuntime;

/// How reviewers are paired with drafts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CcCrossReviewMode {
    /// Each agent *i* reviews agent *(i+1) mod n* (efficient, good for diversity).
    #[default]
    RoundRobin,
    /// Every ordered pair *i × j*, *i ≠ j* (stronger signal, O(n²) calls).
    FullMesh,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CcAgentSlot {
    pub id: String,
    /// Extra system instructions (persona, tool policy summary, etc.).
    pub persona: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CcTask {
    pub id: String,
    pub instruction: String,
    #[serde(default)]
    pub context: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CcDraft {
    pub agent_id: String,
    pub text: String,
    pub latency_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CcReview {
    pub reviewer_id: String,
    pub subject_agent_id: String,
    pub critique: String,
    pub score: f64,
    pub approve: bool,
    pub latency_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CcRunResult {
    pub task: CcTask,
    pub mode: CcCrossReviewMode,
    pub drafts: Vec<CcDraft>,
    pub reviews: Vec<CcReview>,
    #[serde(default)]
    pub synthesized_answer: Option<String>,
    pub chosen_draft_agent_id: Option<String>,
    pub errors: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct CcOrchestratorConfig {
    pub agents: Vec<CcAgentSlot>,
    pub review_mode: CcCrossReviewMode,
    /// When true, run a final single-LLM merge using all drafts + review summaries (LLM path only).
    pub synthesize: bool,
    pub model_override: Option<String>,
}

impl Default for CcOrchestratorConfig {
    fn default() -> Self {
        Self {
            agents: default_agent_roster(),
            review_mode: CcCrossReviewMode::RoundRobin,
            synthesize: true,
            model_override: None,
        }
    }
}

fn default_agent_roster() -> Vec<CcAgentSlot> {
    vec![
        CcAgentSlot {
            id: "cc_a".into(),
            persona: "You are Agent A: fast implementer. Prefer concrete steps and code-shaped answers.".into(),
        },
        CcAgentSlot {
            id: "cc_b".into(),
            persona: "You are Agent B: careful reviewer mindset even when drafting. Surface risks and edge cases.".into(),
        },
        CcAgentSlot {
            id: "cc_c".into(),
            persona: "You are Agent C: systems thinker. Emphasize architecture, invariants, and testability.".into(),
        },
    ]
}

#[derive(Clone, Debug, Serialize)]
struct CcHttpRequest<'a> {
    phase: &'a str,
    task_id: &'a str,
    agent_id: &'a str,
    instruction: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    context: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    subject_agent_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    peer_output: Option<&'a str>,
}

#[derive(Debug, Deserialize)]
struct CcHttpResponse {
    text: Option<String>,
    content: Option<String>,
}

/// Orchestrator: parallel drafts, cross-review, optional synthesis.
pub struct CcOrchestrator {
    llm: Arc<LlmClient>,
    http: Client,
    endpoints: Vec<Option<String>>,
    cfg: CcOrchestratorConfig,
}

impl CcOrchestrator {
    /// Wrap an existing shared LLM client.
    pub fn from_llm(llm: Arc<LlmClient>, cfg: CcOrchestratorConfig) -> anyhow::Result<Self> {
        let endpoints = parse_endpoints_from_env(cfg.agents.len())?;
        Ok(Self {
            llm,
            http: Client::builder()
                .timeout(std::time::Duration::from_secs(180))
                .build()
                .context("cc orchestrator http client")?,
            endpoints,
            cfg,
        })
    }

    /// Own a freshly built [`LlmClient`] (wrapped in [`Arc`]).
    pub fn new(llm: LlmClient, cfg: CcOrchestratorConfig) -> anyhow::Result<Self> {
        Self::from_llm(Arc::new(llm), cfg)
    }

    /// Run full pipeline: drafts → reviews → (optional) synthesis.
    #[instrument(skip(self, harness), fields(task_id = %task.id))]
    pub async fn run(
        &self,
        task: CcTask,
        harness: &mut Option<HarnessRuntime>,
    ) -> CcRunResult {
        let mut errors = Vec::new();
        if self.cfg.agents.is_empty() {
            errors.push("no agent slots configured".into());
            return CcRunResult {
                task,
                mode: self.cfg.review_mode,
                drafts: vec![],
                reviews: vec![],
                synthesized_answer: None,
                chosen_draft_agent_id: None,
                errors,
            };
        }

        let llm = self.llm.clone();
        let http = self.http.clone();
        let endpoints = self.endpoints.clone();
        let cfg = self.cfg.clone();

        if let Some(h) = harness.as_mut() {
            h.turn_begin(&task.id, 0);
        }
        let draft_start = Instant::now();
        let draft_futs: Vec<_> = cfg
            .agents
            .iter()
            .enumerate()
            .map(|(i, slot)| {
                let task = task.clone();
                let slot = slot.clone();
                let ep = endpoints.get(i).cloned().flatten();
                let llm = llm.clone();
                let http = http.clone();
                let cfg = cfg.clone();
                async move {
                    let t0 = Instant::now();
                    let r = exec_draft(&llm, &http, ep.as_deref(), &cfg, &slot, &task).await;
                    let ms = t0.elapsed().as_millis() as u64;
                    (slot.id, r, ms)
                }
            })
            .collect();
        let draft_outcomes = join_all(draft_futs).await;
        if let Some(h) = harness.as_mut() {
            h.turn_end(&task.id, 0, draft_start, None);
        }

        let mut drafts = Vec::new();
        for (agent_id, result, latency_ms) in draft_outcomes {
            match result {
                Ok(text) => drafts.push(CcDraft {
                    agent_id,
                    text,
                    latency_ms,
                }),
                Err(e) => errors.push(format!("draft {agent_id}: {e:#}")),
            }
        }

        let mut reviews = Vec::new();
        if drafts.len() >= 2 {
            if let Some(h) = harness.as_mut() {
                h.turn_begin(&task.id, 1);
            }
            let rev_start = Instant::now();
            let pairs = review_pairs(cfg.review_mode, cfg.agents.len());
            let review_futs: Vec<_> = pairs
                .into_iter()
                .filter_map(|(ri, si)| {
                    let reviewer = cfg.agents.get(ri)?.clone();
                    let subject = drafts.get(si)?;
                    let peer_text = subject.text.clone();
                    let subject_id = subject.agent_id.clone();
                    let task = task.clone();
                    let ep = endpoints.get(ri).cloned().flatten();
                    let llm = llm.clone();
                    let http = http.clone();
                    let cfg = cfg.clone();
                    Some(async move {
                        let t0 = Instant::now();
                        let r = exec_review(
                            &llm,
                            &http,
                            ep.as_deref(),
                            &cfg,
                            &reviewer,
                            &task,
                            &subject_id,
                            &peer_text,
                        )
                        .await;
                        let ms = t0.elapsed().as_millis() as u64;
                        (reviewer.id, subject_id, r, ms)
                    })
                })
                .collect();
            for (reviewer_id, subject_agent_id, result, latency_ms) in join_all(review_futs).await {
                match result {
                    Ok((critique, score, approve)) => reviews.push(CcReview {
                        reviewer_id,
                        subject_agent_id,
                        critique,
                        score,
                        approve,
                        latency_ms,
                    }),
                    Err(e) => errors.push(format!("review {reviewer_id}->{subject_agent_id}: {e:#}")),
                }
            }
            if let Some(h) = harness.as_mut() {
                h.turn_end(&task.id, 1, rev_start, None);
            }
        }

        let chosen_draft_agent_id = pick_preferred_draft(&drafts, &reviews);
        let mut synthesized_answer = None;

        let all_llm_slots = endpoints.iter().all(|e| e.is_none());
        if cfg.synthesize && !drafts.is_empty() && all_llm_slots {
            if let Some(h) = harness.as_mut() {
                h.turn_begin(&task.id, 2);
            }
            let syn_start = Instant::now();
            match exec_synthesize(&llm, &cfg, &task, &drafts, &reviews).await {
                Ok(s) => synthesized_answer = Some(s),
                Err(e) => errors.push(format!("synthesize: {e:#}")),
            }
            if let Some(h) = harness.as_mut() {
                h.turn_end(&task.id, 2, syn_start, None);
            }
        }

        CcRunResult {
            task,
            mode: cfg.review_mode,
            drafts,
            reviews,
            synthesized_answer,
            chosen_draft_agent_id,
            errors,
        }
    }
}

async fn exec_draft(
    llm: &Arc<LlmClient>,
    http: &Client,
    endpoint: Option<&str>,
    cfg: &CcOrchestratorConfig,
    slot: &CcAgentSlot,
    task: &CcTask,
) -> anyhow::Result<String> {
    if let Some(url) = endpoint {
        return post_agent(
            http,
            url,
            &CcHttpRequest {
                phase: "draft",
                task_id: &task.id,
                agent_id: &slot.id,
                instruction: &task.instruction,
                context: task.context.as_deref(),
                subject_agent_id: None,
                peer_output: None,
            },
        )
        .await;
    }

    let base = "You are one member of a multi-agent engineering team. Produce a standalone draft answer.";
    let sys = format!("{base}\n{}", slot.persona);
    let mut user = task.instruction.clone();
    if let Some(ctx) = &task.context {
        user.push_str("\n\n--- Context ---\n");
        user.push_str(ctx);
    }
    llm_chat(llm, cfg, &sys, &user).await
}

async fn exec_review(
    llm: &Arc<LlmClient>,
    http: &Client,
    endpoint: Option<&str>,
    cfg: &CcOrchestratorConfig,
    reviewer: &CcAgentSlot,
    task: &CcTask,
    subject_agent_id: &str,
    peer_output: &str,
) -> anyhow::Result<(String, f64, bool)> {
    let review_instructions = format!(
        "Another agent (`{subject_agent_id}`) drafted the following for the SAME task.\n\
         Critique it: correctness, completeness, clarity, risks.\n\
         End your reply with EXACTLY these two lines:\n\
         SCORE: <number 0.0-1.0>\n\
         VERDICT: approve | reject\n\
         --- Their draft ---\n{peer_output}"
    );

    let text = if let Some(url) = endpoint {
        post_agent(
            http,
            url,
            &CcHttpRequest {
                phase: "review",
                task_id: &task.id,
                agent_id: &reviewer.id,
                instruction: &task.instruction,
                context: task.context.as_deref(),
                subject_agent_id: Some(subject_agent_id),
                peer_output: Some(peer_output),
            },
        )
        .await?
    } else {
        let base = "You are a rigorous peer reviewer on a multi-agent team.";
        let sys = format!("{base}\n{}", reviewer.persona);
        let mut user = task.instruction.clone();
        if let Some(ctx) = &task.context {
            user.push_str("\n\n--- Context ---\n");
            user.push_str(ctx);
        }
        user.push_str("\n\n");
        user.push_str(&review_instructions);
        llm_chat(llm, cfg, &sys, &user).await?
    };

    Ok(parse_review_heuristic(&text))
}

async fn exec_synthesize(
    llm: &Arc<LlmClient>,
    cfg: &CcOrchestratorConfig,
    task: &CcTask,
    drafts: &[CcDraft],
    reviews: &[CcReview],
) -> anyhow::Result<String> {
    let mut user = String::new();
    user.push_str("Original task:\n");
    user.push_str(&task.instruction);
    if let Some(ctx) = &task.context {
        user.push_str("\n\nContext:\n");
        user.push_str(ctx);
    }
    user.push_str("\n\n--- Agent drafts ---\n");
    for d in drafts {
        user.push_str(&format!("\n### {}\n{}\n", d.agent_id, d.text));
    }
    user.push_str("\n--- Peer reviews ---\n");
    for r in reviews {
        user.push_str(&format!(
            "\n{} reviewed {} (score {:.2}, approve={}):\n{}\n",
            r.reviewer_id, r.subject_agent_id, r.score, r.approve, r.critique
        ));
    }
    user.push_str(
        "\nWrite the single best merged answer. Prefer consensus; note unresolved disagreements briefly.",
    );
    let sys = "You are the lead synthesizer: merge multiple agent drafts and critiques into one excellent answer.";
    llm_chat(llm, cfg, sys, &user).await
}

async fn llm_chat(
    llm: &Arc<LlmClient>,
    cfg: &CcOrchestratorConfig,
    system: &str,
    user: &str,
) -> anyhow::Result<String> {
    let req = LlmRequest {
        model: cfg
            .model_override
            .clone()
            .unwrap_or_else(|| LlmRequest::default().model),
        messages: vec![Message::system(system), Message::user(user)],
        temperature: 0.35,
        max_tokens: Some(4096),
        top_p: Some(0.9),
        stream: false,
    };
    let resp = llm.chat(req).await?;
    Ok(resp.content)
}

async fn post_agent(http: &Client, url: &str, body: &CcHttpRequest<'_>) -> anyhow::Result<String> {
    let res = http
        .post(url)
        .json(body)
        .send()
        .await
        .with_context(|| format!("POST {url}"))?;
    if !res.status().is_success() {
        let status = res.status();
        let txt = res.text().await.unwrap_or_default();
        return Err(anyhow!("HTTP {status}: {txt}"));
    }
    let bytes = res.bytes().await.context("read body")?;
    if let Ok(j) = serde_json::from_slice::<CcHttpResponse>(&bytes) {
        if let Some(t) = j.text.or(j.content) {
            return Ok(t);
        }
    }
    String::from_utf8(bytes.to_vec()).context("utf8 body")
}

fn parse_endpoints_from_env(n_agents: usize) -> anyhow::Result<Vec<Option<String>>> {
    let raw = match std::env::var("HSM_CC_AGENT_ENDPOINTS") {
        Ok(s) if !s.trim().is_empty() => s,
        _ => return Ok(vec![None; n_agents]),
    };
    let parts: Vec<String> = raw
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if parts.len() != n_agents {
        anyhow::bail!(
            "HSM_CC_AGENT_ENDPOINTS count {} must match agent slot count {}",
            parts.len(),
            n_agents
        );
    }
    Ok(parts.into_iter().map(Some).collect())
}

fn review_pairs(mode: CcCrossReviewMode, n: usize) -> Vec<(usize, usize)> {
    match mode {
        CcCrossReviewMode::RoundRobin => (0..n).map(|i| (i, (i + 1) % n)).collect(),
        CcCrossReviewMode::FullMesh => {
            let mut out = Vec::new();
            for i in 0..n {
                for j in 0..n {
                    if i != j {
                        out.push((i, j));
                    }
                }
            }
            out
        }
    }
}

fn pick_preferred_draft(drafts: &[CcDraft], reviews: &[CcReview]) -> Option<String> {
    if drafts.is_empty() {
        return None;
    }
    if reviews.is_empty() {
        return Some(drafts[0].agent_id.clone());
    }
    use std::collections::HashMap;
    let mut score_by_subject: HashMap<String, Vec<f64>> = HashMap::new();
    let mut veto: HashMap<String, bool> = HashMap::new();
    for r in reviews {
        score_by_subject
            .entry(r.subject_agent_id.clone())
            .or_default()
            .push(r.score);
        if !r.approve {
            veto.insert(r.subject_agent_id.clone(), true);
        }
    }
    let mut best_id: Option<String> = None;
    let mut best = -1.0f64;
    for d in drafts {
        if *veto.get(&d.agent_id).unwrap_or(&false) {
            continue;
        }
        let scores = score_by_subject.get(&d.agent_id);
        let avg = scores
            .map(|v| v.iter().sum::<f64>() / v.len().max(1) as f64)
            .unwrap_or(0.55);
        if avg > best {
            best = avg;
            best_id = Some(d.agent_id.clone());
        }
    }
    best_id.or_else(|| Some(drafts[0].agent_id.clone()))
}

fn parse_review_heuristic(text: &str) -> (String, f64, bool) {
    let mut score = 0.6f64;
    let mut approve = true;
    for line in text.lines() {
        let l = line.trim();
        let low = l.to_ascii_lowercase();
        if low.starts_with("score:") {
            if let Some(n) = l.split(':').nth(1) {
                if let Ok(v) = n.trim().parse::<f64>() {
                    score = v;
                }
            }
        }
        if low.starts_with("verdict:") {
            if let Some(rest) = l.split(':').nth(1) {
                let r = rest.trim().to_ascii_lowercase();
                approve = !r.starts_with("reject");
            }
        }
    }
    if !text.lines().any(|l| l.trim().to_ascii_lowercase().starts_with("verdict:")) {
        approve = score >= 0.72;
    }
    (text.trim().to_string(), score.clamp(0.0, 1.0), approve)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn review_parse() {
        let t = "Looks good.\nSCORE: 0.88\nVERDICT: approve\n";
        let (c, s, a) = parse_review_heuristic(t);
        assert!(c.contains("Looks good"));
        assert!((s - 0.88).abs() < 0.001);
        assert!(a);
    }

    #[test]
    fn round_robin_pairs() {
        let p = review_pairs(CcCrossReviewMode::RoundRobin, 3);
        assert_eq!(p, vec![(0, 1), (1, 2), (2, 0)]);
    }
}
