use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::time::Duration;

use anyhow::Context;
use clap::{Parser, ValueEnum};
use regex::Regex;
use serde::{Deserialize, Serialize};
use tracing_subscriber::EnvFilter;

use hyper_stigmergy::eval::{
    eval_llm_model_from_env, HsmRunner, HsmRunnerConfig, RankedContextResult,
};
use hyper_stigmergy::llm::client::{LlmClient, LlmRequest, Message};

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum Mode {
    Hsm,
    Baseline,
}

#[derive(Parser, Debug)]
#[command(name = "hsm-longmemeval")]
#[command(about = "Run HSM-II or a plain baseline on LongMemEval and emit jsonl predictions")]
struct Cli {
    #[arg(long)]
    input: PathBuf,

    #[arg(long)]
    output: PathBuf,

    #[arg(long, value_enum, default_value = "hsm")]
    mode: Mode,

    #[arg(long)]
    limit: Option<usize>,

    #[arg(long)]
    max_sessions: Option<usize>,

    #[arg(long, default_value_t = false)]
    use_temporal_facts: bool,

    #[arg(long, default_value_t = false)]
    traces: bool,

    #[arg(long)]
    trace_output: Option<PathBuf>,

    #[arg(long, default_value_t = false)]
    no_quick_metrics: bool,

    #[arg(long, default_value_t = 4)]
    max_attempts: usize,

    #[arg(long, default_value_t = 20)]
    retry_sleep_secs: u64,

    #[arg(long, default_value_t = false)]
    resume: bool,

    #[arg(long, default_value_t = false)]
    stop_on_exhausted_retryable: bool,
}

#[derive(Clone, Debug, Deserialize)]
struct LongMemEvalEntry {
    question_id: String,
    question_type: String,
    question: String,
    answer: serde_json::Value,
    question_date: String,
    haystack_dates: Vec<String>,
    haystack_sessions: Vec<Vec<HistoryTurn>>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct HistoryTurn {
    role: String,
    content: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct PredictionRow {
    question_id: String,
    hypothesis: String,
}

#[derive(Clone, Debug)]
struct TemporalFact {
    session_date: String,
    source_session: usize,
    source_turn: usize,
    role: String,
    entity: String,
    event: String,
    date_hint: String,
    raw: String,
}

#[derive(Clone, Debug, Serialize)]
struct QuickMetricRow {
    question_id: String,
    question_type: String,
    abstention: bool,
    quick_match: bool,
}

#[derive(Clone, Debug, Serialize)]
struct QuickMetricSummary {
    mode: String,
    input: String,
    output: String,
    total: usize,
    quick_accuracy: f64,
    by_question_type: BTreeMap<String, f64>,
}

#[derive(Clone, Debug, Serialize)]
struct PauseSummary {
    mode: String,
    output: String,
    completed_rows: usize,
    paused_question_id: String,
    paused_reason: String,
}

fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_space = false;
    for ch in s.chars().flat_map(|c| c.to_lowercase()) {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            prev_space = false;
        } else if !prev_space {
            out.push(' ');
            prev_space = true;
        }
    }
    out.trim().to_string()
}

fn abstention_match(hypothesis: &str) -> bool {
    let hyp = normalize(hypothesis);
    [
        "do not know",
        "dont know",
        "cannot determine",
        "cant determine",
        "not enough information",
        "insufficient information",
        "not mentioned",
        "unanswerable",
        "unknown",
    ]
    .iter()
    .any(|needle| hyp.contains(needle))
}

fn quick_match(answer: &str, hypothesis: &str, abstention: bool) -> bool {
    if abstention {
        return abstention_match(hypothesis);
    }
    let ans = normalize(answer);
    let hyp = normalize(hypothesis);
    if ans.is_empty() || hyp.is_empty() {
        return false;
    }
    hyp.contains(&ans) || ans.contains(&hyp)
}

fn answer_to_string(answer: &serde_json::Value) -> String {
    match answer {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn history_as_messages(date: &str, session: &[HistoryTurn]) -> Vec<Message> {
    let mut out = Vec::with_capacity(session.len() + 1);
    out.push(Message::user(format!(
        "Session metadata: this conversation took place on {}.",
        date
    )));
    for turn in session {
        match turn.role.as_str() {
            "user" => out.push(Message::user(turn.content.clone())),
            "assistant" => out.push(Message::assistant(turn.content.clone())),
            other => out.push(Message {
                role: other.to_string(),
                content: turn.content.clone(),
            }),
        }
    }
    out
}

fn temporal_markers() -> &'static [&'static str] {
    &[
        "january",
        "february",
        "march",
        "april",
        "may",
        "june",
        "july",
        "august",
        "september",
        "october",
        "november",
        "december",
        "jan",
        "feb",
        "mar",
        "apr",
        "jun",
        "jul",
        "aug",
        "sep",
        "sept",
        "oct",
        "nov",
        "dec",
        "today",
        "yesterday",
        "tomorrow",
        "last",
        "next",
        "ago",
        "before",
        "after",
        "first",
        "second",
        "third",
        "earlier",
        "later",
        "mid-",
        "week",
        "weeks",
        "month",
        "months",
        "day",
        "days",
        "sunday",
        "monday",
        "tuesday",
        "wednesday",
        "thursday",
        "friday",
        "saturday",
        "mass",
        "workshop",
        "webinar",
        "service",
        "repair",
        "festival",
        "fest",
        "tuesdays",
        "holi",
        "arrived",
        "pre-ordered",
        "bought",
        "attended",
        "washed",
    ]
}

fn infer_entity(text: &str) -> String {
    let quoted = Regex::new(r#""([^"]+)"|'([^']+)'"#).expect("quoted regex");
    if let Some(c) = quoted.captures(text) {
        return c
            .get(1)
            .or_else(|| c.get(2))
            .map(|m| m.as_str().to_string())
            .unwrap_or_else(|| "unknown".to_string());
    }
    let proper =
        Regex::new(r"\b([A-Z][a-z]+(?:\s+[A-Z][a-zA-Z0-9.'-]+){0,3})\b").expect("proper regex");
    proper
        .captures(text)
        .and_then(|c| c.get(1).map(|m| m.as_str().to_string()))
        .unwrap_or_else(|| "unknown".to_string())
}

fn infer_date_hint(session_date: &str, text: &str) -> String {
    let lower = text.to_lowercase();
    let explicit = temporal_markers()
        .iter()
        .filter(|m| lower.contains(**m))
        .copied()
        .collect::<Vec<_>>();
    if explicit.is_empty() {
        session_date.to_string()
    } else {
        format!("{} | markers={}", session_date, explicit.join(","))
    }
}

fn extract_temporal_facts(sessions: &[(String, Vec<HistoryTurn>)]) -> Vec<TemporalFact> {
    let digit_re = Regex::new(r"\b\d{1,4}\b").expect("digit regex");
    let mut facts = Vec::new();

    for (session_idx, (date, session)) in sessions.iter().enumerate() {
        for (turn_idx, turn) in session.iter().enumerate() {
            let lower = turn.content.to_lowercase();
            let temporal_hit = temporal_markers().iter().any(|m| lower.contains(m))
                || digit_re.is_match(&turn.content);
            if !temporal_hit {
                continue;
            }
            facts.push(TemporalFact {
                session_date: date.clone(),
                source_session: session_idx + 1,
                source_turn: turn_idx + 1,
                role: turn.role.clone(),
                entity: infer_entity(&turn.content),
                event: turn.content.chars().take(220).collect(),
                date_hint: infer_date_hint(date, &turn.content),
                raw: turn.content.replace('\n', " "),
            });
        }
    }
    facts
}

fn build_temporal_fact_block(sessions: &[(String, Vec<HistoryTurn>)]) -> String {
    let mut facts = extract_temporal_facts(sessions);
    facts.sort_by_key(|f| (f.source_session, f.source_turn));
    let user_facts = facts.iter().filter(|f| f.role == "user").take(28).map(|f| {
        format!(
            "- [S{}T{} role={} date_hint={} entity={}] event={} | raw={}",
            f.source_session, f.source_turn, f.role, f.date_hint, f.entity, f.event, f.raw
        )
    });
    let assistant_facts = facts.iter().filter(|f| f.role != "user").take(12).map(|f| {
        format!(
            "- [S{}T{} role={} date_hint={} entity={}] event={} | raw={}",
            f.source_session, f.source_turn, f.role, f.date_hint, f.entity, f.event, f.raw
        )
    });
    user_facts
        .chain(assistant_facts)
        .collect::<Vec<_>>()
        .join("\n")
}

fn flattened_history_messages(
    sessions: &[(String, Vec<HistoryTurn>)],
    max_sessions: Option<usize>,
) -> Vec<Message> {
    let slice: &[(String, Vec<HistoryTurn>)] = if let Some(max) = max_sessions {
        if sessions.len() > max {
            &sessions[sessions.len() - max..]
        } else {
            sessions
        }
    } else {
        sessions
    };

    let mut out = Vec::new();
    for (date, session) in slice {
        out.push(Message::user(format!(
            "Session boundary. The following conversation took place on {}.",
            date
        )));
        out.extend(history_as_messages(date, session));
    }
    out
}

fn build_baseline_prompt(entry: &LongMemEvalEntry, max_sessions: Option<usize>) -> String {
    let mut sessions: Vec<(String, Vec<HistoryTurn>)> = entry
        .haystack_dates
        .iter()
        .cloned()
        .zip(entry.haystack_sessions.iter().cloned())
        .collect();
    sessions.sort_by(|a, b| a.0.cmp(&b.0));
    if let Some(max) = max_sessions {
        if sessions.len() > max {
            sessions = sessions[sessions.len() - max..].to_vec();
        }
    }

    let mut history = String::new();
    for (idx, (date, session)) in sessions.iter().enumerate() {
        if idx > 0 {
            history.push_str("\n\n");
        }
        history.push_str(&format!(
            "Session Date: {}\nSession Content:\n{}",
            date,
            serde_json::to_string(session).unwrap_or_else(|_| "[]".to_string())
        ));
    }

    format!(
        "I will give you several history chats between you and a user. Please answer the question based on the relevant chat history.\n\nHistory Chats:\n\n{}\n\nCurrent Date: {}\nQuestion: {}\nAnswer briefly and directly.",
        history, entry.question_date, entry.question
    )
}

async fn run_baseline(
    client: &LlmClient,
    entry: &LongMemEvalEntry,
    max_sessions: Option<usize>,
) -> anyhow::Result<String> {
    let request = LlmRequest {
        model: eval_llm_model_from_env(),
        messages: vec![
            Message::system(
                "You are a helpful AI assistant. Answer the user's question from the provided chat history. Be concise and factual.",
            ),
            Message::user(build_baseline_prompt(entry, max_sessions)),
        ],
        temperature: 0.2,
        max_tokens: Some(512),
        ..LlmRequest::default()
    };
    let response = client.chat(request).await?;
    Ok(response.content)
}

fn is_retryable_provider_error(text: &str) -> bool {
    text.contains("[ERROR:") && (text.contains("HTTP 429") || text.contains("rate-limit"))
}

fn is_retryable_empty(text: &str) -> bool {
    text.trim().is_empty()
}

async fn run_hsm(
    entry: &LongMemEvalEntry,
    traces: bool,
    max_sessions: Option<usize>,
    use_temporal_facts: bool,
) -> anyhow::Result<(String, Option<RankedContextResult>)> {
    let mut runner = HsmRunner::with_config(LlmClient::new()?, HsmRunnerConfig::default());
    runner.set_collect_traces(traces);

    let mut sessions: Vec<(String, Vec<HistoryTurn>)> = entry
        .haystack_dates
        .iter()
        .cloned()
        .zip(entry.haystack_sessions.iter().cloned())
        .collect();
    sessions.sort_by(|a, b| a.0.cmp(&b.0));
    if let Some(max) = max_sessions {
        if sessions.len() > max {
            sessions = sessions[sessions.len() - max..].to_vec();
        }
    }

    for (idx, (date, session)) in sessions.iter().enumerate() {
        let messages = history_as_messages(date, session);
        runner.ingest_session_history(
            &entry.question_id,
            "longmemeval",
            (idx + 1) as u32,
            &messages,
        );
    }

    // LongMemEval's official setup feeds the timestamped raw history directly.
    // Keep the raw sessions in the active prompt and use HSM retrieval only as augmentation,
    // not as a lossy replacement for the evidence sessions.
    let mut session_history = flattened_history_messages(&sessions, None);
    let temporal_facts = build_temporal_fact_block(&sessions);
    if use_temporal_facts && !temporal_facts.is_empty() {
        session_history.insert(
            0,
            Message::user(format!(
                "Temporal fact sheet extracted from the prior sessions. Prefer these explicit dated facts when answering:\n{}",
                temporal_facts
            )),
        );
    }

    let question_prompt = format!(
        "Current Date: {}\nQuestion: {}\nUse the raw session history and the temporal fact sheet above. Prefer explicit dated facts from user messages over generic assistant explanations. Answer briefly and directly. If the answer is not supported by the prior sessions, say that the information is insufficient.",
        entry.question_date, entry.question
    );
    let (response, ctx, _pt, _ct, _err) = runner
        .answer_query(
            &entry.question_id,
            "longmemeval",
            (sessions.len() + 1) as u32,
            &session_history,
            &question_prompt,
            true,
        )
        .await;
    Ok((response, Some(ctx)))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("warn,hsm_longmemeval=info")),
        )
        .compact()
        .init();

    let cli = Cli::parse();
    let text = std::fs::read_to_string(&cli.input)
        .with_context(|| format!("read {}", cli.input.display()))?;
    let mut entries: Vec<LongMemEvalEntry> =
        serde_json::from_str(&text).with_context(|| "parse LongMemEval JSON")?;
    if let Some(limit) = cli.limit {
        entries.truncate(limit);
    }

    let client =
        LlmClient::new().context("set OPENAI/OPENROUTER/ANTHROPIC env vars or start Ollama")?;

    if let Some(parent) = cli.output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut completed = std::collections::BTreeSet::new();
    if cli.resume && cli.output.exists() {
        let existing = std::fs::read_to_string(&cli.output)?;
        for line in existing.lines().filter(|l| !l.trim().is_empty()) {
            if let Ok(row) = serde_json::from_str::<PredictionRow>(line) {
                completed.insert(row.question_id);
            }
        }
    }
    let out_file = if cli.resume && cli.output.exists() {
        std::fs::OpenOptions::new().append(true).open(&cli.output)?
    } else {
        File::create(&cli.output)?
    };
    let mut out = BufWriter::new(out_file);

    let mut quick_rows = Vec::new();
    let mut trace_rows = Vec::new();
    let mut newly_completed = 0usize;

    for entry in &entries {
        if completed.contains(&entry.question_id) {
            eprintln!("skipping completed {}", entry.question_id);
            continue;
        }
        let mut final_trace = None;
        let mut hypothesis = None;

        for attempt in 1..=cli.max_attempts.max(1) {
            let current = match cli.mode {
                Mode::Baseline => run_baseline(&client, entry, cli.max_sessions).await?,
                Mode::Hsm => {
                    let (response, trace) =
                        run_hsm(entry, cli.traces, cli.max_sessions, cli.use_temporal_facts)
                            .await?;
                    if trace.is_some() {
                        final_trace = trace;
                    }
                    response
                }
            };
            if !(is_retryable_provider_error(&current) || is_retryable_empty(&current))
                || attempt == cli.max_attempts.max(1)
            {
                hypothesis = Some(current);
                break;
            }
            eprintln!(
                "rate-limited on {} attempt {}/{}; sleeping {}s",
                entry.question_id, attempt, cli.max_attempts, cli.retry_sleep_secs
            );
            tokio::time::sleep(Duration::from_secs(cli.retry_sleep_secs)).await;
        }
        let hypothesis = hypothesis.unwrap_or_default();

        if cli.stop_on_exhausted_retryable
            && (is_retryable_provider_error(&hypothesis) || is_retryable_empty(&hypothesis))
        {
            out.flush()?;
            let pause = PauseSummary {
                mode: match cli.mode {
                    Mode::Hsm => "hsm".to_string(),
                    Mode::Baseline => "baseline".to_string(),
                },
                output: cli.output.display().to_string(),
                completed_rows: completed.len() + newly_completed,
                paused_question_id: entry.question_id.clone(),
                paused_reason: if is_retryable_empty(&hypothesis) {
                    "empty completion after exhausted retries".to_string()
                } else {
                    "rate-limited after exhausted retries".to_string()
                },
            };
            println!("{}", serde_json::to_string_pretty(&pause)?);
            return Ok(());
        }

        if let Some(trace) = final_trace {
            trace_rows.push(serde_json::json!({
                "question_id": entry.question_id,
                "question_type": entry.question_type,
                "trace": trace,
            }));
        }

        let pred = PredictionRow {
            question_id: entry.question_id.clone(),
            hypothesis: hypothesis.clone(),
        };
        writeln!(out, "{}", serde_json::to_string(&pred)?)?;
        out.flush()?;
        newly_completed += 1;

        if !cli.no_quick_metrics {
            quick_rows.push(QuickMetricRow {
                question_id: entry.question_id.clone(),
                question_type: entry.question_type.clone(),
                abstention: entry.question_id.ends_with("_abs"),
                quick_match: quick_match(
                    &answer_to_string(&entry.answer),
                    &hypothesis,
                    entry.question_id.ends_with("_abs"),
                ),
            });
        }
    }
    out.flush()?;

    if let Some(path) = cli.trace_output.as_ref() {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut trace_out = BufWriter::new(File::create(path)?);
        for row in trace_rows {
            writeln!(trace_out, "{}", serde_json::to_string(&row)?)?;
        }
        trace_out.flush()?;
    }

    if !cli.no_quick_metrics {
        let total = quick_rows.len();
        let correct = quick_rows.iter().filter(|r| r.quick_match).count();
        let mut grouped_counts: BTreeMap<String, (usize, usize)> = BTreeMap::new();
        for row in &quick_rows {
            let entry = grouped_counts
                .entry(row.question_type.clone())
                .or_insert((0usize, 0usize));
            entry.0 += 1;
            if row.quick_match {
                entry.1 += 1;
            }
        }
        let by_question_type = grouped_counts
            .into_iter()
            .map(|(k, (n, c))| (k, if n == 0 { 0.0 } else { c as f64 / n as f64 }))
            .collect::<BTreeMap<_, _>>();
        let summary = QuickMetricSummary {
            mode: match cli.mode {
                Mode::Hsm => "hsm".to_string(),
                Mode::Baseline => "baseline".to_string(),
            },
            input: cli.input.display().to_string(),
            output: cli.output.display().to_string(),
            total,
            quick_accuracy: if total == 0 {
                0.0
            } else {
                correct as f64 / total as f64
            },
            by_question_type,
        };
        println!("{}", serde_json::to_string_pretty(&summary)?);
    }

    Ok(())
}
