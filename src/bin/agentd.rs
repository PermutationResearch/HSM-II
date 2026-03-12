use std::time::Duration;

use clap::Parser;
use rand::seq::SliceRandom;

use hyper_stigmergy::{Action, Agent, BidSubmission, Drives, Objectives, WorldSnapshot};

#[derive(Parser, Debug)]
#[command(name = "agentd", about = "HSM-II Agent Service")]
struct Args {
    #[arg(long)]
    agent_id: u64,
    #[arg(long, default_value = "http://127.0.0.1:9001")]
    conductor_url: String,
    #[arg(long, default_value = "http://127.0.0.1:8787")]
    hypergraph_url: String,
    #[arg(long, default_value_t = 1200)]
    interval_ms: u64,
    #[arg(long, default_value_t = 0.2)]
    temperature: f64,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let client = reqwest::Client::new();
    let mut ticker = tokio::time::interval(Duration::from_millis(args.interval_ms));

    loop {
        ticker.tick().await;
        if let Err(err) = tick_agent(&client, &args).await {
            eprintln!("agent {} error: {}", args.agent_id, err);
        }
    }
}

async fn tick_agent(client: &reqwest::Client, args: &Args) -> anyhow::Result<()> {
    let snapshot = fetch_snapshot(client, &args.hypergraph_url).await?;
    let agent = snapshot
        .agents
        .iter()
        .find(|a| a.id == args.agent_id)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("agent {} not found", args.agent_id))?;

    let mut rng = rand::thread_rng();
    let mut other_ids: Vec<u64> = snapshot
        .agents
        .iter()
        .filter(|a| a.id != args.agent_id)
        .map(|a| a.id)
        .collect();
    if other_ids.is_empty() {
        return Ok(());
    }
    other_ids.shuffle(&mut rng);
    let target_id = other_ids[0];

    let role_prompt = fetch_role_prompt(client, &args.hypergraph_url, &agent.role)
        .await
        .unwrap_or_default();
    let context = if snapshot.coherence < 0.4 {
        "coherence risk structure"
    } else {
        "explore novelty discover"
    };
    let full_context = if role_prompt.is_empty() {
        context.to_string()
    } else {
        format!("{}\n{}", role_prompt, context)
    };

    let bid_value = compute_bid(&agent, &full_context, args.temperature);
    let weight = (0.2 + bid_value.min(1.0)) as f32;

    let action = Action::LinkAgents {
        vertices: vec![agent.id as usize, target_id as usize],
        weight,
    };

    let objectives = derive_objectives(&snapshot, bid_value);
    let bid = BidSubmission {
        agent_id: agent.id,
        role: agent.role,
        bid: bid_value,
        objectives,
        action,
        rationale: format!("context={}", context),
    };

    submit_bid(client, &args.conductor_url, &bid).await?;
    let _ = send_message(
        client,
        &args.hypergraph_url,
        agent.id,
        target_id,
        "bid",
        format!("bid={:.3} context={}", bid_value, context),
    )
    .await;
    let _ = send_message(
        client,
        &args.hypergraph_url,
        agent.id,
        target_id,
        "task",
        format!(
            "propose LinkAgents -> agent:{} weight={:.3}",
            target_id, weight
        ),
    )
    .await;
    Ok(())
}

fn compute_bid(agent: &hyper_stigmergy::AgentSnapshot, context: &str, temperature: f64) -> f64 {
    let drives = Drives {
        curiosity: agent.curiosity,
        harmony: agent.harmony,
        growth: agent.growth,
        transcendence: agent.transcendence,
    };
    let mut local = Agent::new(agent.id, drives, agent.learning_rate);
    local.role = agent.role;
    local.bid_bias = agent.bid_bias;
    local.calculate_bid(context, temperature)
}

async fn fetch_role_prompt(
    client: &reqwest::Client,
    base_url: &str,
    role: &hyper_stigmergy::Role,
) -> anyhow::Result<String> {
    let url = format!("{}/api/role_prompts", base_url);
    let resp = client.get(url).send().await?.error_for_status()?;
    let map = resp
        .json::<std::collections::HashMap<String, String>>()
        .await?;
    let key = format!("{:?}", role);
    Ok(map.get(&key).cloned().unwrap_or_default())
}

fn derive_objectives(snapshot: &WorldSnapshot, bid_value: f64) -> Objectives {
    let coherence = (1.0 - snapshot.coherence).clamp(0.0, 1.0);
    let novelty = {
        let scale = snapshot.edge_count.max(1) as f64 / (snapshot.agents.len().max(1) as f64 * 2.0);
        scale.clamp(0.0, 1.0)
    };
    let safety = snapshot.coherence.clamp(0.0, 1.0);
    let boost = (0.5 + 0.5 * bid_value.clamp(0.0, 1.0)).clamp(0.1, 1.0);
    Objectives {
        coherence: (coherence * boost).clamp(0.0, 1.0),
        novelty: (novelty * boost).clamp(0.0, 1.0),
        safety: (safety * boost).clamp(0.0, 1.0),
    }
}

async fn fetch_snapshot(client: &reqwest::Client, base_url: &str) -> anyhow::Result<WorldSnapshot> {
    let url = format!("{}/snapshot", base_url);
    let resp = client.get(url).send().await?.error_for_status()?;
    Ok(resp.json::<WorldSnapshot>().await?)
}

async fn submit_bid(
    client: &reqwest::Client,
    conductor_url: &str,
    bid: &BidSubmission,
) -> anyhow::Result<()> {
    let url = format!("{}/submit_bid", conductor_url);
    client
        .post(url)
        .json(bid)
        .send()
        .await?
        .error_for_status()?;
    Ok(())
}

async fn send_message(
    client: &reqwest::Client,
    base_url: &str,
    sender: u64,
    target: u64,
    kind: &str,
    content: String,
) -> anyhow::Result<()> {
    let url = format!("{}/api/message", base_url);
    let payload = serde_json::json!({
        "sender": sender,
        "target": format!("agent:{}", target),
        "kind": kind,
        "content": content,
    });
    client
        .post(url)
        .json(&payload)
        .send()
        .await?
        .error_for_status()?;
    Ok(())
}
