use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;

use anyhow::Context;
use clap::{Parser, ValueEnum};
use tracing_subscriber::EnvFilter;

use hyper_stigmergy::eval::{
    built_in_hsm_native_tasks, eval_llm_model_from_env, score_task, summarize_results,
    HsmNativeTask, HsmNativeTaskResult, HsmRunner, HsmRunnerConfig, HsmTurnTrace,
};
use hyper_stigmergy::llm::client::{LlmClient, LlmRequest, Message};

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum Variant {
    Baseline,
    Hsm,
    Both,
}

#[derive(Parser, Debug)]
#[command(name = "hsm-native-eval")]
#[command(about = "Run the HSM-native benchmark suites against a baseline or HSM-II")]
struct Cli {
    #[arg(long)]
    input: Option<PathBuf>,

    #[arg(long)]
    json: Option<PathBuf>,

    #[arg(long)]
    jsonl: Option<PathBuf>,

    #[arg(long)]
    trace_output: Option<PathBuf>,

    #[arg(long, value_enum, default_value = "both")]
    variant: Variant,

    #[arg(long)]
    suite: Option<String>,

    #[arg(long)]
    limit: Option<usize>,

    #[arg(long, default_value_t = false)]
    traces: bool,
}

fn built_in_prompt(task: &HsmNativeTask) -> String {
    format!(
        "You are answering a benchmark question about prior work sessions.\nQuestion: {}\nAnswer briefly and directly. Include decisive facts from the prior sessions. If a newer session corrected an older one, use the newer fact.",
        task.question
    )
}

fn session_to_messages(
    session_id: u32,
    agent: &str,
    turns: &[hyper_stigmergy::eval::HsmNativeTurn],
) -> Vec<Message> {
    let mut messages = vec![Message::user(format!(
        "Session metadata: session_id={}, agent={}.",
        session_id, agent
    ))];
    for turn in turns {
        match turn.role.as_str() {
            "user" => messages.push(Message::user(turn.content.clone())),
            "assistant" => messages.push(Message::assistant(turn.content.clone())),
            other => messages.push(Message {
                role: other.to_string(),
                content: turn.content.clone(),
            }),
        }
    }
    messages
}

fn baseline_messages(task: &HsmNativeTask) -> Vec<Message> {
    let mut sessions = task.sessions.clone();
    sessions.sort_by_key(|s| s.session_id);
    let current = sessions.last();
    let history = if let Some(session) = current {
        format!(
            "You only have access to the current active session.\nCurrent session {} agent={} transcript={}",
            session.session_id,
            session.agent,
            serde_json::to_string(&session.turns).unwrap_or_else(|_| "[]".to_string())
        )
    } else {
        "You have no session history.".to_string()
    };
    vec![
        Message::system(
            "You are a helpful AI assistant. Answer from the currently visible session only. If the answer requires missing prior-session context, say the information is insufficient.",
        ),
        Message::user(format!(
            "Here is the benchmark context:\n{}\n\n{}",
            history,
            built_in_prompt(task)
        )),
    ]
}

async fn call_baseline(client: &LlmClient, task: &HsmNativeTask) -> anyhow::Result<String> {
    let model = eval_llm_model_from_env();
    let request = LlmRequest {
        model,
        messages: baseline_messages(task),
        temperature: 0.2,
        max_tokens: Some(512),
        ..LlmRequest::default()
    };
    let response = client.chat(request).await?;
    Ok(response.content)
}

async fn call_hsm(
    task: &HsmNativeTask,
    traces: bool,
) -> anyhow::Result<(String, Vec<HsmTurnTrace>)> {
    let client =
        LlmClient::new().context("set OPENAI/OPENROUTER/ANTHROPIC env vars or start Ollama")?;
    let mut runner = HsmRunner::with_config(client, HsmRunnerConfig::default());
    runner.set_collect_traces(traces);
    let mut sessions = task.sessions.clone();
    sessions.sort_by_key(|s| s.session_id);
    for session in &sessions {
        let messages = session_to_messages(session.session_id, &session.agent, &session.turns);
        runner.ingest_session_history(&task.id, &task.suite, session.session_id, &messages);
    }
    let (response, _ctx, _pt, _ct, _err) = runner
        .answer_query(
            &task.id,
            &task.suite,
            sessions.len() as u32 + 1,
            &[],
            &built_in_prompt(task),
            true,
        )
        .await;
    let traces = if traces {
        runner.take_traces()
    } else {
        Vec::new()
    };
    Ok((response, traces))
}

fn load_tasks(cli: &Cli) -> anyhow::Result<Vec<HsmNativeTask>> {
    let mut tasks = if let Some(path) = &cli.input {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        serde_json::from_str::<Vec<HsmNativeTask>>(&text)
            .with_context(|| format!("parse {}", path.display()))?
    } else {
        built_in_hsm_native_tasks()
    };
    if let Some(suite) = &cli.suite {
        tasks.retain(|task| task.suite == *suite);
    }
    if let Some(limit) = cli.limit {
        tasks.truncate(limit);
    }
    Ok(tasks)
}

async fn run_variant(
    client: &LlmClient,
    variant: Variant,
    tasks: &[HsmNativeTask],
    traces: bool,
) -> anyhow::Result<(Vec<HsmNativeTaskResult>, Vec<HsmTurnTrace>)> {
    let variant_name = match variant {
        Variant::Baseline => "baseline",
        Variant::Hsm => "hsm-full",
        Variant::Both => unreachable!(),
    };
    let mut rows = Vec::with_capacity(tasks.len());
    let mut trace_rows = Vec::new();
    for task in tasks {
        let hypothesis = match variant {
            Variant::Baseline => call_baseline(client, task).await?,
            Variant::Hsm => {
                let (response, task_traces) = call_hsm(task, traces).await?;
                trace_rows.extend(task_traces);
                response
            }
            Variant::Both => unreachable!(),
        };
        rows.push(score_task(task, variant_name, &hypothesis));
    }
    Ok((rows, trace_rows))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("warn,hsm_native_eval=info")),
        )
        .compact()
        .init();

    let cli = Cli::parse();
    let tasks = load_tasks(&cli)?;
    let client =
        LlmClient::new().context("set OPENAI/OPENROUTER/ANTHROPIC env vars or start Ollama")?;

    let variants = match cli.variant {
        Variant::Baseline => vec![Variant::Baseline],
        Variant::Hsm => vec![Variant::Hsm],
        Variant::Both => vec![Variant::Baseline, Variant::Hsm],
    };

    let mut all_rows = Vec::new();
    let mut all_traces = Vec::new();
    for variant in variants {
        let (rows, traces) = run_variant(&client, variant, &tasks, cli.traces).await?;
        let report = summarize_results(
            match variant {
                Variant::Baseline => "baseline",
                Variant::Hsm => "hsm-full",
                Variant::Both => unreachable!(),
            },
            &rows,
        );
        println!("{}", serde_json::to_string_pretty(&report)?);
        all_rows.extend(rows);
        all_traces.extend(traces);
    }

    if let Some(path) = &cli.jsonl {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = File::create(path)?;
        let mut writer = BufWriter::new(file);
        for row in &all_rows {
            serde_json::to_writer(&mut writer, row)?;
            writer.write_all(b"\n")?;
        }
        writer.flush()?;
    }

    if let Some(path) = &cli.json {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let reports = {
            let mut out = Vec::new();
            for variant in ["baseline", "hsm-full"] {
                let rows = all_rows
                    .iter()
                    .filter(|row| row.variant == variant)
                    .cloned()
                    .collect::<Vec<_>>();
                if !rows.is_empty() {
                    out.push(summarize_results(variant, &rows));
                }
            }
            out
        };
        std::fs::write(path, serde_json::to_vec_pretty(&reports)?)?;
    }

    if let Some(path) = &cli.trace_output {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = File::create(path)?;
        let mut writer = BufWriter::new(file);
        for row in &all_traces {
            serde_json::to_writer(&mut writer, row)?;
            writer.write_all(b"\n")?;
        }
        writer.flush()?;
    }

    Ok(())
}
