use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
};
use clap::Parser;
use serde_json::json;
use tokio::sync::RwLock;

use hyper_stigmergy::optimize_anything::evaluator::{Evaluator, LlmJudgeEvaluator};
use hyper_stigmergy::optimize_anything::{
    Artifact, OptimizationConfig, OptimizationMode, OptimizationSession,
};

use hyper_stigmergy::{
    ApplyActionRequest, ApplyActionResponse, BidSubmission, DecisionResult, GrpoReward,
    GrpoUpdateRequest, TickResponse,
};

#[derive(Parser, Debug)]
#[command(name = "conductord", about = "HSM-II Conductor Service")]
struct Args {
    #[arg(long, default_value = "127.0.0.1:9001")]
    bind: String,
    #[arg(long, default_value = "http://127.0.0.1:8787")]
    hypergraph_url: String,
    #[arg(long)]
    auto_interval_ms: Option<u64>,
    #[arg(long, default_value_t = 0.7)]
    softmax_temperature: f64,
    #[arg(long, default_value_t = false)]
    llm_scorer: bool,
    #[arg(
        long,
        default_value = "hf.co/DavidAU/OpenAi-GPT-oss-20b-HERETIC-uncensored-NEO-Imatrix-gguf:IQ4_NL"
    )]
    llm_scorer_model: String,
    #[arg(long, default_value_t = false)]
    optimize_role_prompts: bool,
    #[arg(long, default_value_t = 300000)]
    optimize_interval_ms: u64,
    #[arg(long, default_value_t = 6)]
    optimize_max_iterations: usize,
    #[arg(long, default_value_t = 4)]
    optimize_population: usize,
    #[arg(
        long,
        default_value = "hf.co/DavidAU/OpenAi-GPT-oss-20b-HERETIC-uncensored-NEO-Imatrix-gguf:IQ4_NL"
    )]
    optimize_model: String,
}

#[derive(Clone)]
struct AppState {
    bids: Arc<RwLock<Vec<BidSubmission>>>,
    hypergraph_url: String,
    client: reqwest::Client,
    softmax_temperature: f64,
    llm_scorer: bool,
    llm_scorer_model: String,
    optimize_interval_ms: u64,
    optimize_max_iterations: usize,
    optimize_population: usize,
    optimize_model: String,
}

#[derive(serde::Deserialize)]
struct DecideRequest {
    #[serde(default)]
    tick_after: bool,
    #[serde(default)]
    tick_if_empty: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // Respect OLLAMA_MODEL env var: override CLI defaults when env is set
    let resolve = |cli_val: &str| -> String {
        match std::env::var("OLLAMA_MODEL") {
            Ok(m) if !m.is_empty() && m != "auto" => m,
            _ => cli_val.to_string(),
        }
    };

    let state = AppState {
        bids: Arc::new(RwLock::new(Vec::new())),
        hypergraph_url: args.hypergraph_url.clone(),
        client: reqwest::Client::new(),
        softmax_temperature: args.softmax_temperature,
        llm_scorer: args.llm_scorer,
        llm_scorer_model: resolve(&args.llm_scorer_model),
        optimize_interval_ms: args.optimize_interval_ms,
        optimize_max_iterations: args.optimize_max_iterations,
        optimize_population: args.optimize_population,
        optimize_model: resolve(&args.optimize_model),
    };

    if let Some(interval_ms) = args.auto_interval_ms {
        spawn_auto_loop(state.clone(), interval_ms);
    }
    if args.optimize_role_prompts {
        spawn_role_optimizer(state.clone());
    }

    let app = Router::new()
        .route("/submit_bid", post(submit_bid))
        .route("/bids", get(list_bids))
        .route("/decide", post(decide))
        .route("/status", get(status))
        .with_state(state);

    let addr: SocketAddr = args.bind.parse()?;
    println!("conductord listening on http://{addr}");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

fn spawn_auto_loop(state: AppState, interval_ms: u64) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_millis(interval_ms));
        loop {
            ticker.tick().await;
            let _ = decide_internal(&state, true, true).await;
        }
    });
}

fn spawn_role_optimizer(state: AppState) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_millis(state.optimize_interval_ms));
        loop {
            ticker.tick().await;
            if let Err(err) = optimize_role_prompts(&state).await {
                eprintln!("role optimizer error: {err}");
            }
        }
    });
}

async fn submit_bid(State(state): State<AppState>, Json(bid): Json<BidSubmission>) -> Json<()> {
    let mut bids = state.bids.write().await;
    bids.push(bid);
    Json(())
}

async fn list_bids(State(state): State<AppState>) -> Json<Vec<BidSubmission>> {
    let bids = state.bids.read().await;
    Json(bids.clone())
}

async fn status(State(state): State<AppState>) -> Json<usize> {
    let bids = state.bids.read().await;
    Json(bids.len())
}

async fn decide(
    State(state): State<AppState>,
    Json(req): Json<DecideRequest>,
) -> Json<DecisionResult> {
    let result = decide_internal(&state, req.tick_after, req.tick_if_empty).await;
    Json(result)
}

async fn decide_internal(
    state: &AppState,
    tick_after: bool,
    tick_if_empty: bool,
) -> DecisionResult {
    let bids = {
        let mut guard = state.bids.write().await;
        std::mem::take(&mut *guard)
    };

    if bids.is_empty() {
        if tick_if_empty {
            let _ = tick_world(state).await;
            return DecisionResult {
                chosen: None,
                snapshot: None,
                ticked: true,
            };
        }
        return DecisionResult {
            chosen: None,
            snapshot: None,
            ticked: false,
        };
    }

    let chosen = select_bid(
        &bids,
        state.softmax_temperature,
        state.llm_scorer,
        &state.llm_scorer_model,
    )
    .await;
    let best = chosen.unwrap_or_else(|| bids[0].clone());

    let snapshot = match apply_action(state, &best).await {
        Ok(resp) => Some(resp.snapshot),
        Err(_) => None,
    };

    if tick_after {
        let _ = tick_world(state).await;
    }

    let _ = apply_grpo_update(state, &bids).await;

    DecisionResult {
        chosen: Some(best),
        snapshot,
        ticked: tick_after,
    }
}

async fn apply_action(
    state: &AppState,
    bid: &BidSubmission,
) -> anyhow::Result<ApplyActionResponse> {
    let url = format!("{}/apply_action", state.hypergraph_url);
    let resp = state
        .client
        .post(url)
        .json(&ApplyActionRequest {
            action: bid.action.clone(),
            agent_id: Some(bid.agent_id),
        })
        .send()
        .await?
        .error_for_status()?;
    Ok(resp.json::<ApplyActionResponse>().await?)
}

async fn tick_world(state: &AppState) -> anyhow::Result<TickResponse> {
    let url = format!("{}/tick", state.hypergraph_url);
    let resp = state.client.post(url).send().await?.error_for_status()?;
    Ok(resp.json::<TickResponse>().await?)
}

async fn apply_grpo_update(state: &AppState, bids: &[BidSubmission]) -> anyhow::Result<()> {
    if bids.is_empty() {
        return Ok(());
    }
    let rewards: Vec<GrpoReward> = bids
        .iter()
        .map(|b| GrpoReward {
            agent_id: b.agent_id,
            reward: reward_from_bid(b),
        })
        .collect();

    let url = format!("{}/grpo_update", state.hypergraph_url);
    state
        .client
        .post(url)
        .json(&GrpoUpdateRequest { rewards })
        .send()
        .await?
        .error_for_status()?;
    Ok(())
}

fn reward_from_bid(bid: &BidSubmission) -> f64 {
    let o = &bid.objectives;
    let reward = o.coherence * 0.5 + o.novelty * 0.3 + o.safety * 0.2;
    reward.clamp(0.0, 1.0)
}

async fn select_bid(
    bids: &[BidSubmission],
    temperature: f64,
    llm_scorer: bool,
    llm_model: &str,
) -> Option<BidSubmission> {
    if bids.is_empty() {
        return None;
    }
    let frontier = pareto_frontier(bids);
    let candidates = if frontier.is_empty() { bids } else { &frontier };
    let scores: Vec<f64> = if llm_scorer {
        score_with_llm(candidates, llm_model).await
    } else {
        candidates.iter().map(combined_score).collect()
    };
    let idx = sample_softmax(&scores, temperature)?;
    Some(candidates[idx].clone())
}

fn combined_score(bid: &BidSubmission) -> f64 {
    let o = &bid.objectives;
    let obj_avg = (o.coherence + o.novelty + o.safety) / 3.0;
    (0.5 * bid.bid + 0.5 * obj_avg).clamp(0.0, 1.0)
}

fn pareto_frontier(bids: &[BidSubmission]) -> Vec<BidSubmission> {
    let mut frontier = Vec::new();
    for (i, bid) in bids.iter().enumerate() {
        let mut dominated = false;
        for (j, other) in bids.iter().enumerate() {
            if i == j {
                continue;
            }
            if dominates(other, bid) {
                dominated = true;
                break;
            }
        }
        if !dominated {
            frontier.push(bid.clone());
        }
    }
    frontier
}

fn dominates(a: &BidSubmission, b: &BidSubmission) -> bool {
    let ao = &a.objectives;
    let bo = &b.objectives;
    let not_worse =
        ao.coherence >= bo.coherence && ao.novelty >= bo.novelty && ao.safety >= bo.safety;
    let strictly_better =
        ao.coherence > bo.coherence || ao.novelty > bo.novelty || ao.safety > bo.safety;
    not_worse && strictly_better
}

fn sample_softmax(scores: &[f64], temperature: f64) -> Option<usize> {
    if scores.is_empty() {
        return None;
    }
    if temperature <= 0.0 {
        let mut best = 0usize;
        let mut best_score = scores[0];
        for (i, s) in scores.iter().enumerate().skip(1) {
            if *s > best_score {
                best_score = *s;
                best = i;
            }
        }
        return Some(best);
    }
    let max_score = scores.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let mut exp_scores = Vec::with_capacity(scores.len());
    let mut sum = 0.0;
    for s in scores {
        let v = ((s - max_score) / temperature).exp();
        exp_scores.push(v);
        sum += v;
    }
    if sum <= 0.0 {
        return Some(0);
    }
    let mut pick = rand::random::<f64>() * sum;
    for (i, v) in exp_scores.iter().enumerate() {
        if pick <= *v {
            return Some(i);
        }
        pick -= *v;
    }
    Some(exp_scores.len() - 1)
}

async fn score_with_llm(candidates: &[BidSubmission], model: &str) -> Vec<f64> {
    let rubric = "Score 0-1 for action quality given objectives (coherence, novelty, safety) and bid strength. Prefer balanced gains and safety.";
    let evaluator = LlmJudgeEvaluator::new(model.to_string(), rubric);
    let mut scores = Vec::with_capacity(candidates.len());
    for bid in candidates {
        let artifact = Artifact::new(format!(
            "agent_id: {}\nrole: {:?}\nbid: {:.4}\nobjectives: coherence={:.4}, novelty={:.4}, safety={:.4}\naction: {:?}\nrationale: {}",
            bid.agent_id,
            bid.role,
            bid.bid,
            bid.objectives.coherence,
            bid.objectives.novelty,
            bid.objectives.safety,
            bid.action,
            bid.rationale
        ));
        let score = evaluator
            .evaluate(&artifact)
            .await
            .map(|r| r.score)
            .unwrap_or(0.5);
        scores.push(score);
    }
    scores
}

async fn optimize_role_prompts(state: &AppState) -> anyhow::Result<()> {
    let roles = [
        "Architect",
        "Catalyst",
        "Chronicler",
        "Critic",
        "Explorer",
        "Coder",
    ];
    for role in roles {
        let current = fetch_role_prompt(&state.client, &state.hypergraph_url, role).await?;
        let objective = format!(
            "Write a concise policy prompt for role {role}. Include role-specific guidance and the keywords: structure, coherence, novelty, risk, evidence, explore, implement. Keep under 80 words."
        );
        let mut config = OptimizationConfig::default();
        config.max_iterations = state.optimize_max_iterations;
        config.population_size = state.optimize_population;
        config.model = state.optimize_model.clone();
        let mut session = OptimizationSession::new(objective, config, OptimizationMode::SingleTask);
        if !current.is_empty() {
            session.add_candidate(hyper_stigmergy::optimize_anything::Candidate::new(
                Artifact::new(current.clone()),
                0.4,
                hyper_stigmergy::optimize_anything::ASI::new().log("seed".to_string()),
                0,
            ));
        }
        let (tx, _rx) = tokio::sync::broadcast::channel(8);
        session.run(tx).await;
        if let Some(best) = session.candidates.iter().max_by(|a, b| {
            a.score
                .partial_cmp(&b.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        }) {
            post_role_prompt(
                &state.client,
                &state.hypergraph_url,
                role,
                &best.artifact.content,
            )
            .await?;
        }
    }
    Ok(())
}

async fn fetch_role_prompt(
    client: &reqwest::Client,
    base_url: &str,
    role: &str,
) -> anyhow::Result<String> {
    let url = format!("{}/api/role_prompts", base_url);
    let resp = client.get(url).send().await?.error_for_status()?;
    let map = resp
        .json::<std::collections::HashMap<String, String>>()
        .await?;
    Ok(map.get(role).cloned().unwrap_or_default())
}

async fn post_role_prompt(
    client: &reqwest::Client,
    base_url: &str,
    role: &str,
    prompt: &str,
) -> anyhow::Result<()> {
    let url = format!("{}/api/role_prompts", base_url);
    let payload = json!({ "role": role, "prompt": prompt });
    client
        .post(url)
        .json(&payload)
        .send()
        .await?
        .error_for_status()?;
    Ok(())
}
