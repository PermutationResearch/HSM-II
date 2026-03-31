use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Context;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};
use clap::Parser;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::process::Command;
use tokio::sync::Mutex;
use tracing_subscriber::EnvFilter;
use hyper_stigmergy::harness::{
    Migration, MigrationRunner, ResumeSessionMap, RuntimeConfig, Scheduler,
};

#[derive(Parser, Debug, Clone)]
#[command(name = "hsm_a2a_adapter")]
#[command(about = "A2A JSON-RPC sidecar with Hermes CLI task execution")]
struct Cli {
    #[arg(long, default_value = "127.0.0.1:9797")]
    bind: String,
    #[arg(long, default_value = ".hsmii/a2a_adapter")]
    state_dir: PathBuf,
    #[arg(long, default_value = "hermes")]
    hermes_bin: String,
    #[arg(long, default_value_t = 180)]
    hermes_timeout_sec: u64,
    #[arg(long)]
    agent_id: Option<String>,
    #[arg(long)]
    capabilities_file: Option<PathBuf>,
    #[arg(long, default_value_t = false)]
    dry_run: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AgentCapability {
    agent_id: String,
    capabilities: Vec<String>,
    domain: Option<String>,
    reputation: Option<f64>,
    load: Option<f64>,
    availability: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DelegationRecord {
    delegation_id: String,
    task_id: String,
    trace_id: String,
    from_agent: String,
    to_agent: String,
    objective: String,
    transcript_path: String,
    stdout_path: String,
    stderr_path: String,
    exit_code: Option<i32>,
    created_unix: u64,
}

#[derive(Clone)]
struct AppState {
    cli: Cli,
    capabilities: Arc<Vec<AgentCapability>>,
    sessions: Arc<Mutex<ResumeSessionMap>>,
    artifacts: Arc<Mutex<HashMap<String, Value>>>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("warn,hsm_a2a_adapter=info")),
        )
        .compact()
        .init();

    let cli = Cli::parse();
    let runtime_cfg = RuntimeConfig::from_env();
    let _ran = MigrationRunner::new(runtime_cfg.state_dir.clone())
        .register(Migration {
            id: "runtime_dirs_v1",
            run: |state_dir: &std::path::Path| {
                fs::create_dir_all(state_dir.join("plugins"))?;
                fs::create_dir_all(state_dir.join("checkpoints"))?;
                Ok(())
            },
        })
        .run_pending()?;
    fs::create_dir_all(cli.state_dir.join("transcripts"))?;
    fs::create_dir_all(cli.state_dir.join("delegations"))?;
    fs::create_dir_all(cli.state_dir.join("artifacts"))?;

    let capabilities = load_capabilities(&cli)?;
    let sessions = ResumeSessionMap::load(&runtime_cfg.resume.session_map_path).unwrap_or_default();
    let state = AppState {
        cli: cli.clone(),
        capabilities: Arc::new(capabilities),
        sessions: Arc::new(Mutex::new(sessions)),
        artifacts: Arc::new(Mutex::new(HashMap::new())),
    };
    let scheduler = Scheduler::new();
    let _heartbeat_job = scheduler.spawn_interval("a2a_heartbeat", Duration::from_secs(30), || {
        tracing::debug!("a2a scheduler heartbeat");
    });

    let app = Router::new()
        .route("/rpc", post(handle_rpc))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&cli.bind).await?;
    println!("hsm_a2a_adapter listening on http://{}/rpc", cli.bind);
    axum::serve(listener, app).await?;
    Ok(())
}

async fn handle_rpc(
    State(state): State<AppState>,
    Json(req): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let id = req.get("id").cloned().unwrap_or(Value::Null);
    let method = req
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let params = req.get("params").cloned().unwrap_or_else(|| json!({}));

    let result = match method.as_str() {
        "discover_capabilities" => discover_capabilities(&state, &params).await,
        "delegate_task" => delegate_task(&state, &params).await,
        "heartbeat_tick" => heartbeat_tick(&state, &params).await,
        "status_update" => status_update(&state, &params).await,
        "handoff_artifact" => handoff_artifact(&state, &params).await,
        "close_task" => close_task(&state, &params).await,
        _ => Err(anyhow::anyhow!("unknown method {}", method)),
    };

    match result {
        Ok(v) => Ok(Json(json!({"jsonrpc":"2.0","id":id,"result":v}))),
        Err(e) => Ok(Json(json!({
            "jsonrpc":"2.0",
            "id":id,
            "error":{"code":1003,"message":e.to_string()}
        }))),
    }
}

async fn discover_capabilities(state: &AppState, params: &Value) -> anyhow::Result<Value> {
    let caps_any = params
        .get("query")
        .and_then(|q| q.get("capabilities_any"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let domain = params
        .get("query")
        .and_then(|q| q.get("domain"))
        .and_then(Value::as_str);
    let max_candidates = params
        .get("query")
        .and_then(|q| q.get("max_candidates"))
        .and_then(Value::as_u64)
        .unwrap_or(5) as usize;

    let wanted: Vec<String> = caps_any
        .into_iter()
        .filter_map(|v| v.as_str().map(|s| s.to_string()))
        .collect();

    let mut cands: Vec<&AgentCapability> = state
        .capabilities
        .iter()
        .filter(|c| {
            let cap_match = wanted.is_empty()
                || c.capabilities
                    .iter()
                    .any(|have| wanted.iter().any(|want| want == have));
            let dom_match = match (domain, c.domain.as_deref()) {
                (None, _) => true,
                (Some(d), Some(cd)) => d == cd,
                (Some(_), None) => false,
            };
            cap_match && dom_match
        })
        .collect();

    cands.sort_by(|a, b| {
        b.reputation
            .unwrap_or(0.5)
            .partial_cmp(&a.reputation.unwrap_or(0.5))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    cands.truncate(max_candidates);

    Ok(json!({
        "candidates": cands
    }))
}

async fn delegate_task(state: &AppState, params: &Value) -> anyhow::Result<Value> {
    let delegation_id = required_str(params, "delegation_id")?;
    let task_id = required_str(params, "task_id")?;
    let trace_id = required_str(params, "trace_id")?;
    let from_agent = required_str(params, "from_agent")?;
    let to_agent = required_str(params, "to_agent")?;
    let objective = required_str(params, "objective")?;

    let resume_id = {
        let sessions = state.sessions.lock().await;
        sessions.task_to_resume.get(task_id).cloned()
    };

    let exec = run_hermes_single_query(
        &state.cli.hermes_bin,
        objective,
        resume_id.as_deref(),
        Duration::from_secs(state.cli.hermes_timeout_sec),
    )
    .await?;

    let transcript_md = to_markdown_transcript(task_id, objective, &exec.stdout, &exec.stderr);
    let ts = now_unix();
    let transcript_path = state
        .cli
        .state_dir
        .join("transcripts")
        .join(format!("{}_{}.md", task_id, ts));
    let stdout_path = state
        .cli
        .state_dir
        .join("delegations")
        .join(format!("{}_stdout.txt", delegation_id));
    let stderr_path = state
        .cli
        .state_dir
        .join("delegations")
        .join(format!("{}_stderr.txt", delegation_id));
    fs::write(&transcript_path, transcript_md)?;
    fs::write(&stdout_path, &exec.stdout)?;
    fs::write(&stderr_path, &exec.stderr)?;

    if let Some(next_resume) = exec.resume_id.or_else(|| Some(task_id.to_string())) {
        let mut sessions = state.sessions.lock().await;
        sessions
            .task_to_resume
            .insert(task_id.to_string(), next_resume);
        let cfg = RuntimeConfig::from_env();
        sessions.save(&cfg.resume.session_map_path)?;
    }

    let record = DelegationRecord {
        delegation_id: delegation_id.to_string(),
        task_id: task_id.to_string(),
        trace_id: trace_id.to_string(),
        from_agent: from_agent.to_string(),
        to_agent: to_agent.to_string(),
        objective: objective.to_string(),
        transcript_path: transcript_path.display().to_string(),
        stdout_path: stdout_path.display().to_string(),
        stderr_path: stderr_path.display().to_string(),
        exit_code: exec.exit_code,
        created_unix: ts,
    };
    let record_path = state
        .cli
        .state_dir
        .join("delegations")
        .join(format!("{}.json", delegation_id));
    fs::write(record_path, serde_json::to_vec_pretty(&record)?)?;

    Ok(json!({
        "accepted": exec.exit_code == Some(0),
        "delegation_id": delegation_id,
        "eta_seconds": 30,
        "artifact": {
            "type":"transcript_markdown",
            "uri": format!("file://{}", transcript_path.display())
        }
    }))
}

async fn heartbeat_tick(state: &AppState, params: &Value) -> anyhow::Result<Value> {
    let ticket = params
        .get("ticket")
        .ok_or_else(|| anyhow::anyhow!("missing ticket object"))?;
    let trace_id = required_str(params, "trace_id")?;
    let from_agent = required_str(params, "from_agent")?;
    let task_id = ticket
        .get("task_id")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("ticket.task_id missing"))?;
    let objective = ticket
        .get("objective")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("ticket.objective missing"))?;

    let capabilities_any: Vec<String> = ticket
        .get("required_capabilities")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let domain = ticket.get("domain").and_then(Value::as_str);

    let discover_params = json!({
        "trace_id": trace_id,
        "task_id": task_id,
        "from_agent": from_agent,
        "query": {
            "capabilities_any": capabilities_any,
            "domain": domain,
            "max_candidates": 3
        }
    });
    let candidates = discover_capabilities(state, &discover_params)
        .await?
        .get("candidates")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let selected = candidates
        .first()
        .and_then(|v| v.get("agent_id"))
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("no eligible agent found for heartbeat ticket"))?;

    let delegation_id = format!("deleg_{}_{}", task_id, now_unix());
    let delegate_params = json!({
        "trace_id": trace_id,
        "task_id": task_id,
        "delegation_id": delegation_id,
        "from_agent": from_agent,
        "to_agent": selected,
        "objective": objective,
        "acceptance_criteria": ticket.get("acceptance_criteria").cloned().unwrap_or_else(|| json!(["produce useful result"])),
        "constraints": ticket.get("constraints").cloned().unwrap_or_else(|| json!({
            "deadline_unix": now_unix() + 3600,
            "max_tokens_budget": 8000,
            "allowed_tools": ["shell","edit","tests"]
        })),
        "inputs": ticket.get("inputs").cloned().unwrap_or_else(|| json!({"repo_ref":"main"}))
    });
    let request_dry_run = params.get("dry_run").and_then(Value::as_bool).unwrap_or(false);
    let dry_run = state.cli.dry_run || request_dry_run;
    let delegation_result = if dry_run {
        json!({
            "accepted": true,
            "delegation_id": delegation_id,
            "dry_run": true,
            "note": "Routing preview only; Hermes not invoked.",
            "planned_delegate_task_params": delegate_params
        })
    } else {
        delegate_task(state, &delegate_params).await?
    };

    Ok(json!({
        "ticket_processed": true,
        "dry_run": dry_run,
        "selected_agent": selected,
        "delegation_id": delegation_id,
        "delegation_result": delegation_result
    }))
}

async fn status_update(state: &AppState, params: &Value) -> anyhow::Result<Value> {
    let delegation_id = required_str(params, "delegation_id")?;
    let path = state
        .cli
        .state_dir
        .join("delegations")
        .join(format!("{}_status.jsonl", delegation_id));
    let line = serde_json::to_string(params)?;
    let mut content = String::new();
    if path.exists() {
        content = fs::read_to_string(&path).unwrap_or_default();
    }
    content.push_str(&line);
    content.push('\n');
    fs::write(path, content)?;
    Ok(json!({"received": true, "delegation_id": delegation_id}))
}

async fn handoff_artifact(state: &AppState, params: &Value) -> anyhow::Result<Value> {
    let artifact = params
        .get("artifact")
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("artifact missing"))?;
    let artifact_id = artifact
        .get("artifact_id")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("artifact.artifact_id missing"))?;

    {
        let mut artifacts = state.artifacts.lock().await;
        artifacts.insert(artifact_id.to_string(), artifact.clone());
    }
    let p = state
        .cli
        .state_dir
        .join("artifacts")
        .join(format!("{}.json", artifact_id));
    fs::write(p, serde_json::to_vec_pretty(&artifact)?)?;

    Ok(json!({"received": true, "artifact_id": artifact_id}))
}

async fn close_task(state: &AppState, params: &Value) -> anyhow::Result<Value> {
    let task_id = required_str(params, "task_id")?;
    let delegation_id = required_str(params, "delegation_id")?;
    let outcome = required_str(params, "outcome")?;
    let p = state
        .cli
        .state_dir
        .join("delegations")
        .join(format!("{}_close.json", delegation_id));
    fs::write(&p, serde_json::to_vec_pretty(params)?)?;

    if matches!(outcome, "accepted" | "rejected" | "cancelled") {
        let mut sessions = state.sessions.lock().await;
        sessions.task_to_resume.remove(task_id);
        let cfg = RuntimeConfig::from_env();
        sessions.save(&cfg.resume.session_map_path)?;
    }

    Ok(json!({
        "closed": true,
        "task_id": task_id,
        "delegation_id": delegation_id,
        "outcome": outcome
    }))
}

struct HermesExec {
    stdout: String,
    stderr: String,
    exit_code: Option<i32>,
    resume_id: Option<String>,
}

async fn run_hermes_single_query(
    hermes_bin: &str,
    objective: &str,
    resume_id: Option<&str>,
    timeout: Duration,
) -> anyhow::Result<HermesExec> {
    // Default invocation contract for Hermes CLI:
    //   hermes --single-query "<objective>" [--resume "<session_id>"]
    let mut cmd = Command::new(hermes_bin);
    cmd.arg("--single-query").arg(objective);
    if let Some(r) = resume_id {
        cmd.arg("--resume").arg(r);
    }
    let fut = cmd.output();
    let out = tokio::time::timeout(timeout, fut)
        .await
        .context("hermes execution timeout")??;
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    Ok(HermesExec {
        stdout,
        stderr,
        exit_code: out.status.code(),
        resume_id: resume_id.map(|s| s.to_string()),
    })
}

fn to_markdown_transcript(task_id: &str, objective: &str, stdout: &str, stderr: &str) -> String {
    format!(
        "# Hermes Delegation Transcript\n\n- task_id: `{}`\n- generated_unix: `{}`\n\n## Objective\n\n{}\n\n## Hermes stdout\n\n```\n{}\n```\n\n## Hermes stderr\n\n```\n{}\n```\n",
        task_id,
        now_unix(),
        objective,
        stdout.trim(),
        stderr.trim()
    )
}

fn load_capabilities(cli: &Cli) -> anyhow::Result<Vec<AgentCapability>> {
    if let Some(ref p) = cli.capabilities_file {
        let txt = fs::read_to_string(p)
            .with_context(|| format!("read capabilities file {}", p.display()))?;
        let caps = serde_json::from_str::<Vec<AgentCapability>>(&txt)
            .context("parse capabilities JSON")?;
        return Ok(caps);
    }
    let me = cli
        .agent_id
        .clone()
        .unwrap_or_else(|| "eng.local.1".to_string());
    Ok(vec![AgentCapability {
        agent_id: me,
        capabilities: vec![
            "code.api.rest".to_string(),
            "test.integration".to_string(),
            "docs.spec".to_string(),
        ],
        domain: Some("software_engineering".to_string()),
        reputation: Some(0.5),
        load: Some(0.2),
        availability: Some("online".to_string()),
    }])
}

fn required_str<'a>(v: &'a Value, key: &str) -> anyhow::Result<&'a str> {
    v.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("missing required param {}", key))
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
