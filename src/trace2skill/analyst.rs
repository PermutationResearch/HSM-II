//! Per-trajectory “lessons” and **sectioned** merge (dedupe, no clustering dependency).

use std::collections::HashSet;

use crate::llm::client::{LlmClient, LlmRequest, Message};

use super::{truncate, TrajectoryOutcome, TrajectoryRecord};

fn matches_llm_env() -> bool {
    std::env::var("HSM_TRACE2SKILL_LLM")
        .map(|v| {
            let s = v.trim().to_ascii_lowercase();
            s == "1" || s == "true" || s == "yes"
        })
        .unwrap_or(false)
}

/// Heuristic one-line lesson (no LLM).
pub fn heuristic_lesson(r: &TrajectoryRecord) -> String {
    let tools = r
        .tool_steps
        .iter()
        .map(|t| format!("{}({})", t.name, if t.ok { "ok" } else { "fail" }))
        .collect::<Vec<_>>()
        .join(" → ");
    let task = truncate(&r.user_task, 120);
    match &r.outcome {
        TrajectoryOutcome::Success => {
            if tools.is_empty() {
                format!(
                    "[success conf={:.2} route={}] {}",
                    r.confidence, r.turn_route, task
                )
            } else {
                format!(
                    "[success conf={:.2} route={}] {} | tools: {}",
                    r.confidence, r.turn_route, task, tools
                )
            }
        }
        TrajectoryOutcome::Partial => {
            format!(
                "[partial conf={:.2} route={}] {} | tools: {}",
                r.confidence,
                r.turn_route,
                task,
                if tools.is_empty() { "(none)" } else { &tools }
            )
        }
        TrajectoryOutcome::Failure => {
            let hint = r
                .tool_steps
                .iter()
                .find(|t| !t.ok)
                .map(|t| t.result_summary.as_str())
                .unwrap_or(&r.response_preview);
            format!(
                "[failure route={}] {} | first error hint: {}",
                r.turn_route,
                task,
                truncate(hint, 200)
            )
        }
    }
}

fn llm_lesson_sync(r: &TrajectoryRecord) -> anyhow::Result<String> {
    let client = LlmClient::new()?;
    let model = std::env::var("OLLAMA_MODEL")
        .or_else(|_| std::env::var("DEFAULT_LLM_MODEL"))
        .unwrap_or_else(|_| "gpt-4o-mini".to_string());
    let payload = serde_json::json!({
        "user_task": r.user_task,
        "outcome": format!("{:?}", r.outcome),
        "confidence": r.confidence,
        "route": r.turn_route,
        "tools": r.tool_steps,
        "response_preview": r.response_preview,
    });
    let body = serde_json::to_string(&payload)?;
    let prompt = format!(
        "Extract ONE transferable playbook bullet (max 35 words) from this trajectory. \
         Prefer concrete tool/skill/memory actions. No preamble. JSON context:\n{}",
        truncate(&body, 2800)
    );
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let text = rt.block_on(async {
        let req = LlmRequest {
            model: model.clone(),
            messages: vec![Message::user(&prompt)],
            temperature: 0.2,
            max_tokens: Some(120),
            ..Default::default()
        };
        let resp = client.chat(req).await?;
        Ok::<_, anyhow::Error>(resp.content)
    })?;
    Ok(truncate(text.trim(), 400))
}

/// Single lesson line: heuristics, or LLM when `HSM_TRACE2SKILL_LLM=1`.
pub fn lesson_for_record(r: &TrajectoryRecord) -> String {
    if matches_llm_env() {
        match llm_lesson_sync(r) {
            Ok(s) if !s.trim().is_empty() => {
                format!(
                    "[{} route={}] {}",
                    format!("{:?}", r.outcome).to_lowercase(),
                    r.turn_route,
                    s
                )
            }
            Ok(_) => heuristic_lesson(r),
            Err(_) => heuristic_lesson(r),
        }
    } else {
        heuristic_lesson(r)
    }
}

fn normalization_fingerprint(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .take(120)
        .collect()
}

/// Parallel chunking when LLM is off; sequential LLM calls when on.
pub fn parallel_lessons(records: &[TrajectoryRecord]) -> Vec<String> {
    if matches_llm_env() {
        return records.iter().map(lesson_for_record).collect();
    }
    if records.len() < 8 {
        return records.iter().map(lesson_for_record).collect();
    }
    let n = std::thread::available_parallelism()
        .map(|x| x.get())
        .unwrap_or(2)
        .clamp(2, 8);
    let chunk = (records.len() + n - 1) / n;
    std::thread::scope(|s| {
        let mut handles = Vec::new();
        for sl in records.chunks(chunk.max(1)) {
            handles.push(s.spawn(|| sl.iter().map(lesson_for_record).collect::<Vec<_>>()));
        }
        let mut merged = Vec::new();
        for h in handles {
            merged.extend(h.join().unwrap_or_default());
        }
        merged
    })
}

fn section_lines(
    outcome: TrajectoryOutcome,
    records: &[TrajectoryRecord],
    lessons: &[String],
) -> Vec<String> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut out = Vec::new();
    for (r, lesson) in records.iter().zip(lessons.iter()) {
        if r.outcome != outcome {
            continue;
        }
        let fp = normalization_fingerprint(lesson);
        if fp.len() < 8 || seen.contains(&fp) {
            continue;
        }
        seen.insert(fp);
        out.push(lesson.clone());
    }
    out
}

/// Group deduped lessons by outcome with markdown-ish headers (one document for `SkillBank::principle`).
pub fn merge_sectioned_principle(records: &[TrajectoryRecord]) -> String {
    if records.is_empty() {
        return String::new();
    }
    let lessons = parallel_lessons(records);
    let succ = section_lines(TrajectoryOutcome::Success, records, &lessons);
    let part = section_lines(TrajectoryOutcome::Partial, records, &lessons);
    let fail = section_lines(TrajectoryOutcome::Failure, records, &lessons);
    let mut blocks = Vec::new();
    if !succ.is_empty() {
        blocks.push(format!("## Success patterns\n{}", succ.join("\n")));
    }
    if !part.is_empty() {
        blocks.push(format!("## Partial / ambiguous\n{}", part.join("\n")));
    }
    if !fail.is_empty() {
        blocks.push(format!("## Failure / recovery\n{}", fail.join("\n")));
    }
    if blocks.is_empty() {
        lessons.join("\n")
    } else {
        blocks.join("\n\n")
    }
}
