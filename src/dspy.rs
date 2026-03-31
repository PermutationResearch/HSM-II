use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::time::{timeout, Duration};

use ollama_rs::generation::chat::request::ChatMessageRequest;
use ollama_rs::generation::chat::ChatMessage as OllamaChatMsg;
use ollama_rs::Ollama;

use crate::database::{DspyDemonstrationRow, DspyOptimizedConfigRow, DspyTraceRow};
use crate::RooDb;

// ─── Stage 0: Compile-Time Signature Templates (backward compat) ───

/// The compile-time signature template — static strings, baked at build time.
/// All 15 existing signatures remain exactly as-is.
#[derive(Clone, Debug)]
pub struct DspySignature {
    pub name: &'static str,
    pub system: &'static str,
    pub prompt: &'static str,
    pub roodb_query: Option<&'static str>,
    pub max_output_chars: usize,
}

#[derive(Clone, Debug)]
pub struct DspyContext<'a> {
    pub question: &'a str,
    pub grounded: &'a str,
    pub agents: &'a str,
    pub prior: &'a str,
}

// ─── Stage 2: Runtime-Resolved Signature (with demonstrations) ───

/// A demonstration (few-shot example) to inject into the prompt.
#[derive(Clone, Debug)]
pub struct Demonstration {
    pub id: i64,
    pub input: String,
    pub output: String,
    pub score: f64,
}

/// The runtime-resolved signature: may include optimized system/prompt text
/// and selected demonstrations. Built from a DspySignature template + optimizer config.
#[derive(Clone, Debug)]
pub struct ResolvedSignature {
    pub name: String,
    pub system: String,
    pub prompt: String,
    pub demonstrations: Vec<Demonstration>,
    pub roodb_query: Option<String>,
    pub max_output_chars: usize,
    pub version: u32,
}

impl ResolvedSignature {
    /// Create from a static template with no demonstrations (backward compat).
    pub fn from_template(sig: &DspySignature) -> Self {
        ResolvedSignature {
            name: sig.name.to_string(),
            system: sig.system.to_string(),
            prompt: sig.prompt.to_string(),
            demonstrations: Vec::new(),
            roodb_query: sig.roodb_query.map(|s| s.to_string()),
            max_output_chars: sig.max_output_chars,
            version: 0,
        }
    }

    /// Create from a static template + optimized config + demonstrations.
    pub fn from_optimized(
        sig: &DspySignature,
        config: &DspyOptimizedConfigRow,
        demos: Vec<DspyDemonstrationRow>,
    ) -> Self {
        ResolvedSignature {
            name: sig.name.to_string(),
            system: config.system_text.clone(),
            prompt: config.prompt_template.clone(),
            demonstrations: demos
                .into_iter()
                .map(|d| Demonstration {
                    id: d.id,
                    input: d.input_summary,
                    output: d.output,
                    score: d.score,
                })
                .collect(),
            roodb_query: sig.roodb_query.map(|s| s.to_string()),
            max_output_chars: sig.max_output_chars,
            version: config.version as u32,
        }
    }
}

// ─── Stage 1: Trace Result (for logging) ───

/// Result from run_signature, includes metadata for trace persistence.
#[derive(Clone, Debug)]
pub struct TraceResult {
    pub output: String,
    pub score: f64,
    pub semantic_ok: bool,
    pub repair_count: i32,
    pub latency_ms: i32,
    pub signature_name: String,
    pub input_question: String,
    pub input_context_hash: String,
    pub model: String,
    /// Optional GEPA fields; [`persist_trace`] fills from [`infer_failure_metadata`] when absent.
    pub failure_code: Option<String>,
    pub failure_detail: Option<String>,
    pub signals_json: Option<String>,
}

// ─── Stage 2: Signature Store (in-memory cache of optimized configs) ───

/// In-memory cache of resolved signatures. Refreshed from RooDB periodically.
/// Maps signature_name → ResolvedSignature with demonstrations.
pub struct SignatureStore {
    cache: HashMap<String, ResolvedSignature>,
    last_refresh: u64,
}

impl SignatureStore {
    pub fn new() -> Self {
        SignatureStore {
            cache: HashMap::new(),
            last_refresh: 0,
        }
    }

    /// Resolve a static signature template to its runtime form.
    /// Uses cached optimized config if available, otherwise returns the template as-is.
    pub fn resolve(&self, sig: &DspySignature) -> ResolvedSignature {
        if let Some(cached) = self.cache.get(sig.name) {
            cached.clone()
        } else {
            ResolvedSignature::from_template(sig)
        }
    }

    /// Refresh cache from RooDB — load all active optimized configs + their demonstrations.
    pub async fn refresh_from_db(&mut self, db: &RooDb) {
        let sig_names = match db.list_dspy_signature_names().await {
            Ok(names) => names,
            Err(_) => return,
        };

        for (name, _count) in &sig_names {
            if let Ok(Some(config)) = db.load_dspy_optimized_config(name).await {
                let demos = db
                    .fetch_dspy_demonstrations_by_ids(&config.demo_ids)
                    .await
                    .unwrap_or_default();

                // We need the static template to get roodb_query and max_output_chars.
                // Look up from the known signature list.
                let template = get_template_by_name(name);
                if let Some(tmpl) = template {
                    let resolved = ResolvedSignature::from_optimized(&tmpl, &config, demos);
                    self.cache.insert(name.clone(), resolved);
                }
            }
        }

        // Also load demonstrations for signatures that have demos but no optimized config
        for (name, _count) in &sig_names {
            if self.cache.contains_key(name) {
                continue;
            }
            let demos = match db.fetch_dspy_demonstrations(name, 5).await {
                Ok(d) if !d.is_empty() => d,
                _ => continue,
            };
            if let Some(tmpl) = get_template_by_name(name) {
                let mut resolved = ResolvedSignature::from_template(&tmpl);
                resolved.demonstrations = demos
                    .into_iter()
                    .map(|d| Demonstration {
                        id: d.id,
                        input: d.input_summary,
                        output: d.output,
                        score: d.score,
                    })
                    .collect();
                self.cache.insert(name.clone(), resolved);
            }
        }

        self.last_refresh = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
    }

    /// Check if cache is stale (older than 5 minutes).
    pub fn needs_refresh(&self) -> bool {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        now - self.last_refresh > 300
    }

    pub fn cached_count(&self) -> usize {
        self.cache.len()
    }
}

// ─── Constants ───

const ROODB_AGENT_SNAPSHOT: &str =
    "SELECT agent_id, role, curiosity, harmony, growth, transcendence, learning_rate, jw \
     FROM agents \
     WHERE snapshot_id = (SELECT MAX(snapshot_id) FROM agents) \
     ORDER BY curiosity DESC LIMIT 8";

const ROODB_BELIEFS_TOP: &str = "SELECT content, confidence, source \
     FROM beliefs \
     WHERE snapshot_id = (SELECT MAX(snapshot_id) FROM beliefs) \
     ORDER BY confidence DESC LIMIT 8";

// ─── Prompt Rendering ───

fn format_rows(headers: &[String], rows: &[Vec<String>], max_rows: usize) -> String {
    if headers.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    out.push_str(&headers.join(" | "));
    out.push('\n');
    out.push_str(
        &headers
            .iter()
            .map(|h| "-".repeat(std::cmp::max(h.len(), 3)))
            .collect::<Vec<_>>()
            .join("-+-"),
    );
    out.push('\n');
    for row in rows.iter().take(max_rows) {
        out.push_str(&row.join(" | "));
        out.push('\n');
    }
    out.trim_end().to_string()
}

fn build_context_block(ctx: &DspyContext<'_>, roodb_context: &str) -> String {
    let mut sections = Vec::new();
    if !ctx.agents.trim().is_empty() {
        sections.push(format!("AGENT SNAPSHOT:\n{}", ctx.agents.trim()));
    }
    if !ctx.grounded.trim().is_empty() {
        sections.push(format!("LIVE WORLD DATA:\n{}", ctx.grounded.trim()));
    }
    if !roodb_context.trim().is_empty() {
        sections.push(format!("ROODB CONTEXT:\n{}", roodb_context.trim()));
    }
    if !ctx.prior.trim().is_empty() {
        sections.push(format!("PRIOR OUTPUT:\n{}", ctx.prior.trim()));
    }
    sections.join("\n\n")
}

/// Build the demonstrations block for injection into the prompt.
fn build_demo_block(demos: &[Demonstration]) -> String {
    if demos.is_empty() {
        return String::new();
    }
    let mut out =
        String::from("EXAMPLES OF CORRECT OUTPUT (follow this exact style and format):\n");
    for (i, d) in demos.iter().enumerate() {
        out.push_str(&format!(
            "\n--- Example {} ---\nInput: {}\nOutput:\n{}\n",
            i + 1,
            d.input,
            d.output
        ));
    }
    out.push_str("--- End Examples ---\n");
    out
}

fn render_prompt_resolved(
    sig: &ResolvedSignature,
    ctx: &DspyContext<'_>,
    roodb_context: &str,
) -> String {
    let context_block = build_context_block(ctx, roodb_context);
    let demo_block = build_demo_block(&sig.demonstrations);

    let mut prompt = sig.prompt.replace("{question}", ctx.question);
    prompt = prompt.replace("{context}", &context_block);

    if !demo_block.is_empty() {
        format!("{}\n\n{}", demo_block, prompt)
    } else {
        prompt
    }
}

/// Legacy render for static DspySignature (backward compat).
#[allow(dead_code)]
fn render_prompt(sig: &DspySignature, ctx: &DspyContext<'_>, roodb_context: &str) -> String {
    let context_block = build_context_block(ctx, roodb_context);
    let mut prompt = sig.prompt.replace("{question}", ctx.question);
    prompt = prompt.replace("{context}", &context_block);
    prompt
}

fn strip_think_tags(text: &str) -> String {
    let mut result = text.to_string();
    while let Some(start) = result.find("<think>") {
        if let Some(end) = result[start..].find("</think>") {
            let remove_start = start;
            let remove_end = start + end + "</think>".len();
            result.replace_range(remove_start..remove_end, "");
        } else {
            result.replace_range(start.., "");
        }
    }
    result.trim().to_string()
}

/// Simple hash for deduplication of input contexts.
fn hash_context(ctx: &DspyContext<'_>) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    ctx.grounded.hash(&mut hasher);
    ctx.agents.hash(&mut hasher);
    ctx.prior.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

// ─── Core Execution ───

/// Run a signature with trace logging. This is the new primary entry point.
/// Falls back gracefully if no optimized config or RooDB is available.
pub async fn run_signature_traced(
    ollama: &Ollama,
    model: &str,
    sig: &DspySignature,
    ctx: &DspyContext<'_>,
    roodb: Option<Arc<RooDb>>,
    store: Option<&SignatureStore>,
) -> Result<TraceResult, String> {
    let start = Instant::now();

    // Resolve to runtime signature (with demos if available)
    let resolved = if let Some(s) = store {
        s.resolve(sig)
    } else {
        ResolvedSignature::from_template(sig)
    };

    // Fetch RooDB context
    let mut roodb_context = String::new();
    if let (Some(ref db), Some(ref sql)) = (&roodb, &resolved.roodb_query) {
        if !ctx.grounded.trim().is_empty() {
            match db.raw_query(sql).await {
                Ok((headers, rows)) => {
                    roodb_context = format_rows(&headers, &rows, 8);
                }
                Err(e) => {
                    roodb_context = format!("[RooDB query failed: {}]", e);
                }
            }
        }
    }

    let prompt = render_prompt_resolved(&resolved, ctx, &roodb_context);
    let messages = vec![
        OllamaChatMsg::system(resolved.system.clone()),
        OllamaChatMsg::user(prompt),
    ];
    let request = ChatMessageRequest::new(model.to_string(), messages);

    let resp = timeout(Duration::from_secs(120), ollama.send_chat_messages(request))
        .await
        .map_err(|_| format!("{} timed out", sig.name))?;

    let latency_ms = start.elapsed().as_millis() as i32;
    let context_hash = hash_context(ctx);

    match resp {
        Ok(msg) => {
            let mut content = strip_think_tags(&msg.message.content);
            if resolved.max_output_chars > 0 && content.len() > resolved.max_output_chars {
                content.truncate(resolved.max_output_chars);
                content.push_str("\n[truncated]");
            }

            let trace = TraceResult {
                output: content,
                score: 0.0,        // Caller sets this after semantic verification
                semantic_ok: true, // Caller updates after verification
                repair_count: 0,   // Caller updates after repairs
                latency_ms,
                signature_name: sig.name.to_string(),
                input_question: ctx.question.to_string(),
                input_context_hash: context_hash,
                model: model.to_string(),
                failure_code: None,
                failure_detail: None,
                signals_json: None,
            };
            Ok(trace)
        }
        Err(e) => Err(format!("{} failed: {}", sig.name, e)),
    }
}

/// Original run_signature — backward compatible, now delegates to run_signature_traced.
pub async fn run_signature(
    ollama: &Ollama,
    model: &str,
    sig: &DspySignature,
    ctx: &DspyContext<'_>,
    roodb: Option<Arc<RooDb>>,
) -> Result<String, String> {
    // Backward compat: call traced version, return just the output string
    let trace = run_signature_traced(ollama, model, sig, ctx, roodb, None).await?;
    Ok(trace.output)
}

/// Heuristic “why did this fail?” metadata for GEPA collect / clustering (local only).
pub fn infer_failure_metadata(
    score: f64,
    semantic_ok: bool,
    repair_count: i32,
    output: &str,
) -> (String, String, String) {
    let signals = serde_json::json!({
        "score": score,
        "semantic_ok": semantic_ok,
        "repair_count": repair_count,
        "output_len": output.len(),
    });
    let sj = signals.to_string();

    if score >= 0.65 && semantic_ok {
        return (String::new(), String::new(), sj);
    }

    let out = output.trim();
    if out.is_empty() {
        return (
            "empty_output".into(),
            "Model returned empty text".into(),
            sj,
        );
    }
    if output.contains("[truncated]") {
        return (
            "truncation".into(),
            "Output hit max length truncation".into(),
            sj,
        );
    }
    if repair_count > 0 {
        return (
            "repair_loop".into(),
            format!("Needed {} semantic repair pass(es)", repair_count),
            sj,
        );
    }
    if !semantic_ok {
        let lower = output.to_lowercase();
        if lower.contains("claim:") && !lower.contains("evidence") {
            return (
                "claim_evidence".into(),
                "Claim present but evidence section weak or missing".into(),
                sj,
            );
        }
        return ("semantic_fail".into(), "Semantic check failed".into(), sj);
    }
    if score < 0.55 {
        return (
            "low_score".into(),
            format!("Low score {:.2}", score),
            sj,
        );
    }
    (
        "low_score".into(),
        format!("score {:.2}, semantic_ok={}", score, semantic_ok),
        sj,
    )
}

/// Persist a trace result to RooDB (fire-and-forget from caller).
pub async fn persist_trace(db: &RooDb, trace: &TraceResult) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let (mut fc, mut fd, mut sj) = infer_failure_metadata(
        trace.score,
        trace.semantic_ok,
        trace.repair_count,
        &trace.output,
    );
    if let Some(ref c) = trace.failure_code {
        if !c.trim().is_empty() {
            fc = c.clone();
        }
    }
    if let Some(ref d) = trace.failure_detail {
        if !d.trim().is_empty() {
            fd = d.clone();
        }
    }
    if let Some(ref s) = trace.signals_json {
        if !s.trim().is_empty() {
            sj = s.clone();
        }
    }

    let row = DspyTraceRow {
        id: 0,
        signature_name: trace.signature_name.clone(),
        input_question: trace.input_question.clone(),
        input_context_hash: trace.input_context_hash.clone(),
        output: trace.output.clone(),
        score: trace.score,
        semantic_ok: trace.semantic_ok,
        repair_count: trace.repair_count,
        model: trace.model.clone(),
        latency_ms: trace.latency_ms,
        created_at: now,
        failure_code: fc,
        failure_detail: fd,
        signals_json: sj,
    };

    let _ = db.insert_dspy_trace(&row).await;
}

// ─── Stage 2: Bootstrap Demonstration Selection ───

/// Word overlap ratio between two strings (for diversity filtering).
fn word_overlap(a: &str, b: &str) -> f64 {
    let words_a: std::collections::HashSet<&str> =
        a.split_whitespace().filter(|w| w.len() > 3).collect();
    let words_b: std::collections::HashSet<&str> =
        b.split_whitespace().filter(|w| w.len() > 3).collect();
    if words_a.is_empty() || words_b.is_empty() {
        return 0.0;
    }
    let intersection = words_a.intersection(&words_b).count();
    let union = words_a.union(&words_b).count();
    if union == 0 {
        0.0
    } else {
        intersection as f64 / union as f64
    }
}

/// Bootstrap demonstrations from high-scoring traces.
/// Selects diverse, high-quality examples for each signature.
pub async fn bootstrap_demonstrations(
    db: &RooDb,
    signature_name: &str,
    min_score: f64,
    max_demos: usize,
    max_overlap: f64,
) -> anyhow::Result<Vec<DspyDemonstrationRow>> {
    // Fetch best traces
    let traces = db.fetch_dspy_traces(signature_name, min_score, 50).await?;
    if traces.is_empty() {
        return Ok(Vec::new());
    }

    // Check existing active demonstrations to avoid duplicates
    let existing = db.fetch_dspy_demonstrations(signature_name, 100).await?;
    let existing_hashes: std::collections::HashSet<String> =
        existing.iter().map(|d| d.input_summary.clone()).collect();

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let mut selected: Vec<DspyDemonstrationRow> = Vec::new();
    let mut selected_inputs: Vec<String> = Vec::new();

    for trace in &traces {
        if selected.len() >= max_demos {
            break;
        }

        // Truncate input for prompt injection (keep it compact)
        let input_summary = if trace.input_question.len() > 500 {
            format!("{}...", &trace.input_question[..497])
        } else {
            trace.input_question.clone()
        };

        // Skip if we already have this exact input
        if existing_hashes.contains(&input_summary) {
            continue;
        }

        // Check diversity — skip if too similar to already-selected demos
        let too_similar = selected_inputs
            .iter()
            .any(|existing| word_overlap(&input_summary, existing) > max_overlap);
        if too_similar {
            continue;
        }

        let demo = DspyDemonstrationRow {
            id: 0, // auto-increment
            signature_name: signature_name.to_string(),
            input_summary: input_summary.clone(),
            output: trace.output.clone(),
            score: trace.score,
            source: "bootstrapped".to_string(),
            source_trace_id: Some(trace.id),
            active: true,
            promoted_by: None,
            created_at: now,
        };

        selected_inputs.push(input_summary);
        selected.push(demo);
    }

    // Persist to DB
    let mut persisted = Vec::new();
    for demo in &selected {
        match db.insert_dspy_demonstration(demo).await {
            Ok(id) => {
                let mut d = demo.clone();
                d.id = id;
                persisted.push(d);
            }
            Err(_) => {} // skip failures silently
        }
    }

    Ok(persisted)
}

// ─── Stage 3: Signature Optimizer ───

/// Optimization result for a single signature.
#[derive(Debug, Clone)]
pub struct OptimizationResult {
    pub signature_name: String,
    pub previous_score: f64,
    pub new_score: f64,
    pub trials_run: i32,
    pub improved: bool,
    pub version: i32,
    pub demo_count: i32,
}

#[derive(Debug, Clone, Copy)]
pub enum DspyMutationStyle {
    DemoSubset,
    DemoReorder,
    SystemRephrase,
    NotebookFirst,
    XmlConverged,
    LateInteraction,
}

impl DspyMutationStyle {
    fn from_trial(trial: usize) -> Self {
        match trial % 6 {
            0 => Self::DemoSubset,
            1 => Self::DemoReorder,
            2 => Self::SystemRephrase,
            3 => Self::NotebookFirst,
            4 => Self::XmlConverged,
            _ => Self::LateInteraction,
        }
    }

    /// Names must match [`crate::gepa::mutation_style_names_from_bundle`] output.
    pub fn from_name(name: &str) -> Option<Self> {
        match name.trim() {
            "DemoSubset" => Some(Self::DemoSubset),
            "DemoReorder" => Some(Self::DemoReorder),
            "SystemRephrase" => Some(Self::SystemRephrase),
            "NotebookFirst" => Some(Self::NotebookFirst),
            "XmlConverged" => Some(Self::XmlConverged),
            "LateInteraction" => Some(Self::LateInteraction),
            _ => None,
        }
    }
}

/// Run the optimizer for a single signature.
///
/// 1. Loads traces (split into train/eval)
/// 2. Bootstraps demonstrations from train set
/// 3. Tries demo subset mutations
/// 4. Tries instruction rephrasing via LLM
/// 5. Persists winning config
///
/// When `gepa_mutation_names` is set (from [`crate::gepa::collect_bundle`] / bundle JSON),
/// mutation order follows failure clusters first instead of a flat `trial % 6` rotation.
pub async fn optimize_signature(
    ollama: &Ollama,
    model: &str,
    db: &RooDb,
    sig: &DspySignature,
    max_trials: usize,
    gepa_mutation_names: Option<Vec<String>>,
) -> anyhow::Result<OptimizationResult> {
    let sig_name = sig.name;

    // Load all high-scoring traces
    let all_traces = db.fetch_dspy_traces(sig_name, 0.5, 200).await?;
    if all_traces.len() < 10 {
        return Ok(OptimizationResult {
            signature_name: sig_name.to_string(),
            previous_score: 0.0,
            new_score: 0.0,
            trials_run: 0,
            improved: false,
            version: 0,
            demo_count: 0,
        });
    }

    // Split: 80% for demo pool, 20% for evaluation
    let eval_size = std::cmp::max(all_traces.len() / 5, 3);
    let eval_traces = &all_traces[..eval_size];
    let _train_traces = &all_traces[eval_size..];

    // Ensure we have bootstrapped demonstrations
    let demos = db.fetch_dspy_demonstrations(sig_name, 20).await?;
    let demos = if demos.len() < 3 {
        // Need to bootstrap first
        bootstrap_demonstrations(db, sig_name, 0.7, 10, 0.3).await?
    } else {
        demos
    };

    if demos.is_empty() {
        return Ok(OptimizationResult {
            signature_name: sig_name.to_string(),
            previous_score: 0.0,
            new_score: 0.0,
            trials_run: 0,
            improved: false,
            version: 0,
            demo_count: 0,
        });
    }

    // Load current config (if any)
    let current_config = db.load_dspy_optimized_config(sig_name).await?;
    let current_version = current_config.as_ref().map(|c| c.version).unwrap_or(0);

    // Evaluate current configuration
    let current_system = current_config
        .as_ref()
        .map(|c| c.system_text.clone())
        .unwrap_or_else(|| sig.system.to_string());
    let current_prompt = current_config
        .as_ref()
        .map(|c| c.prompt_template.clone())
        .unwrap_or_else(|| sig.prompt.to_string());
    let current_demo_ids: Vec<i64> = current_config
        .as_ref()
        .map(|c| c.demo_ids.clone())
        .unwrap_or_default();

    // Score current config on eval set
    let current_score = evaluate_config(
        ollama,
        model,
        &current_system,
        &current_prompt,
        &demos,
        &current_demo_ids,
        eval_traces,
        sig,
    )
    .await;

    let mut best_score = current_score;
    let mut best_system = current_system.clone();
    let mut best_prompt = current_prompt.clone();
    let mut best_demo_ids = current_demo_ids.clone();
    let mut trials_run = 0;

    let style_order: Vec<DspyMutationStyle> = if let Some(names) = gepa_mutation_names {
        let mut v: Vec<DspyMutationStyle> = names
            .iter()
            .filter_map(|n| DspyMutationStyle::from_name(n))
            .collect();
        if v.is_empty() {
            v = (0..6).map(DspyMutationStyle::from_trial).collect();
        }
        v
    } else {
        (0..6).map(DspyMutationStyle::from_trial).collect()
    };

    // ─── Mutation trials ───
    for trial in 0..max_trials {
        trials_run += 1;

        let mutation_style = style_order[trial % style_order.len()];
        let (trial_system, trial_prompt, trial_demo_ids) = match mutation_style {
            // Mutation 0: Different demo subset (random K of N)
            DspyMutationStyle::DemoSubset => {
                let k = std::cmp::min(3 + (trial / 4) % 4, demos.len()); // vary K: 3,4,5,6
                let mut subset_ids: Vec<i64> = demos.iter().map(|d| d.id).collect();
                // Simple deterministic shuffle based on trial number
                for i in (1..subset_ids.len()).rev() {
                    let j = (trial * 7 + i * 13) % (i + 1);
                    subset_ids.swap(i, j);
                }
                subset_ids.truncate(k);
                (best_system.clone(), best_prompt.clone(), subset_ids)
            }
            // Mutation 1: Demo reorder
            DspyMutationStyle::DemoReorder => {
                let mut reordered = best_demo_ids.clone();
                if reordered.len() > 1 {
                    // Rotate by trial offset
                    let rotate_by = trial % reordered.len();
                    reordered.rotate_left(rotate_by);
                }
                (best_system.clone(), best_prompt.clone(), reordered)
            }
            // Mutation 2: Instruction rephrase via LLM
            DspyMutationStyle::SystemRephrase => {
                let rephrased = rephrase_instruction(ollama, model, &best_system).await;
                (rephrased, best_prompt.clone(), best_demo_ids.clone())
            }
            // Mutation 3: Prefer bottom-up tinkering / notebook-first validation
            DspyMutationStyle::NotebookFirst => {
                let rewritten_system = rewrite_system_with_bias(
                    ollama,
                    model,
                    &best_system,
                    SystemRewriteBias::NotebookFirst,
                )
                .await;
                let rewritten_prompt = rewrite_prompt_with_bias(
                    ollama,
                    model,
                    &best_prompt,
                    PromptRewriteBias::NotebookFirst,
                )
                .await;
                (rewritten_system, rewritten_prompt, best_demo_ids.clone())
            }
            // Mutation 4: Structured XML / model-converged formatting
            DspyMutationStyle::XmlConverged => {
                let rewritten_system = rewrite_system_with_bias(
                    ollama,
                    model,
                    &best_system,
                    SystemRewriteBias::XmlStructured,
                )
                .await;
                let rewritten_prompt = rewrite_prompt_with_bias(
                    ollama,
                    model,
                    &best_prompt,
                    PromptRewriteBias::XmlStructured,
                )
                .await;
                (rewritten_system, rewritten_prompt, best_demo_ids.clone())
            }
            // Mutation 5: Late interaction / multi-step evidence gathering
            DspyMutationStyle::LateInteraction => {
                let rewritten_system = rewrite_system_with_bias(
                    ollama,
                    model,
                    &best_system,
                    SystemRewriteBias::LateInteraction,
                )
                .await;
                let rewritten_prompt = rewrite_prompt_with_bias(
                    ollama,
                    model,
                    &best_prompt,
                    PromptRewriteBias::LateInteraction,
                )
                .await;
                let mut subset_ids: Vec<i64> = demos.iter().map(|d| d.id).collect();
                for i in (1..subset_ids.len()).rev() {
                    let j = (trial * 11 + i * 17) % (i + 1);
                    subset_ids.swap(i, j);
                }
                subset_ids.truncate(std::cmp::min(4, subset_ids.len()));
                (rewritten_system, rewritten_prompt, subset_ids)
            }
        };

        let trial_score = evaluate_config(
            ollama,
            model,
            &trial_system,
            &trial_prompt,
            &demos,
            &trial_demo_ids,
            eval_traces,
            sig,
        )
        .await;

        if trial_score > best_score {
            best_score = trial_score;
            best_system = trial_system;
            best_prompt = trial_prompt;
            best_demo_ids = trial_demo_ids;
        }
    }

    let improved = best_score > current_score + 0.01; // require meaningful improvement
    let new_version = current_version + 1;

    // Persist winning config
    if improved || current_config.is_none() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let config_row = DspyOptimizedConfigRow {
            id: 0,
            signature_name: sig_name.to_string(),
            system_text: best_system,
            prompt_template: best_prompt,
            demo_ids: best_demo_ids.clone(),
            demo_count: best_demo_ids.len() as i32,
            eval_score: best_score,
            eval_set_size: eval_traces.len() as i32,
            trials_run,
            version: new_version,
            created_at: now,
            active: true,
        };
        let _ = db.save_dspy_optimized_config(&config_row).await;
    }

    Ok(OptimizationResult {
        signature_name: sig_name.to_string(),
        previous_score: current_score,
        new_score: best_score,
        trials_run,
        improved,
        version: new_version,
        demo_count: best_demo_ids.len() as i32,
    })
}

/// Evaluate a config variant on the eval set.
/// Runs the signature with the given system/prompt/demos on each eval trace's question,
/// then scores outputs by checking if they match the expected format.
async fn evaluate_config(
    ollama: &Ollama,
    model: &str,
    system: &str,
    prompt_template: &str,
    all_demos: &[DspyDemonstrationRow],
    demo_ids: &[i64],
    eval_traces: &[DspyTraceRow],
    sig: &DspySignature,
) -> f64 {
    if eval_traces.is_empty() {
        return 0.0;
    }

    // Build demonstrations from selected IDs
    let selected_demos: Vec<Demonstration> = all_demos
        .iter()
        .filter(|d| demo_ids.contains(&d.id))
        .map(|d| Demonstration {
            id: d.id,
            input: d.input_summary.clone(),
            output: d.output.clone(),
            score: d.score,
        })
        .collect();

    let resolved = ResolvedSignature {
        name: sig.name.to_string(),
        system: system.to_string(),
        prompt: prompt_template.to_string(),
        demonstrations: selected_demos,
        roodb_query: None, // Skip RooDB in eval — too slow
        max_output_chars: sig.max_output_chars,
        version: 0,
    };

    // Evaluate on up to 5 eval traces (balance speed vs accuracy)
    let eval_count = std::cmp::min(eval_traces.len(), 5);
    let mut total_score = 0.0;

    for trace in eval_traces.iter().take(eval_count) {
        let ctx = DspyContext {
            question: &trace.input_question,
            grounded: "",
            agents: "",
            prior: "",
        };

        let prompt = render_prompt_resolved(&resolved, &ctx, "");
        let messages = vec![
            OllamaChatMsg::system(resolved.system.clone()),
            OllamaChatMsg::user(prompt),
        ];
        let request = ChatMessageRequest::new(model.to_string(), messages);

        match timeout(Duration::from_secs(60), ollama.send_chat_messages(request)).await {
            Ok(Ok(msg)) => {
                let content = strip_think_tags(&msg.message.content);
                total_score += score_output(&content, &trace.output);
            }
            _ => {
                // Timeout or error — score 0 for this eval
            }
        }
    }

    total_score / eval_count as f64
}

/// Score an output against a reference trace output.
/// Checks format compliance and content similarity.
fn score_output(output: &str, reference: &str) -> f64 {
    let mut score = 0.0;

    // Format check: does it contain Claim/Evidence blocks?
    let has_claim = output.contains("Claim:");
    let has_evidence = output.contains("Evidence:");
    let has_xml_claim = output.contains("<claim>") && output.contains("</claim>");
    let has_xml_evidence = output.contains("<evidence>") && output.contains("</evidence>");
    let has_stepwise_structure = output.contains("Observation:")
        || output.contains("Experiment:")
        || output.contains("Validation:")
        || output.contains("<observation>")
        || output.contains("<validation>");
    // Base format bonuses — XML tags subsume plain Claim:/Evidence:
    if has_claim || has_xml_claim {
        score += 0.3;
    }
    if has_evidence || has_xml_evidence {
        score += 0.3;
    }
    // Additional XML structure bonus (on top of base)
    if has_xml_claim {
        score += 0.15;
    }
    if has_xml_evidence {
        score += 0.15;
    }
    if has_stepwise_structure {
        score += 0.1;
    }

    // Length sanity: not too short, not too long
    let len = output.len();
    if len > 50 && len < 5000 {
        score += 0.1;
    }

    // Content overlap with reference (word-level Jaccard)
    let overlap = word_overlap(output, reference);
    score += overlap * 0.3;

    score.min(1.0)
}

/// Ask the LLM to rephrase a system instruction for better results.
async fn rephrase_instruction(ollama: &Ollama, model: &str, current_system: &str) -> String {
    let rephrase_prompt = format!(
        "Below is a system instruction for an AI assistant.\n\
         Rephrase it to be clearer and more precise, while keeping the same meaning and constraints.\n\
         Output ONLY the new system instruction — nothing else.\n\n\
         Current instruction:\n{}\n\n\
         New instruction:",
        current_system
    );

    let messages = vec![
        OllamaChatMsg::system(
            "You are a prompt engineer. Output only the rephrased instruction.".to_string(),
        ),
        OllamaChatMsg::user(rephrase_prompt),
    ];
    let request = ChatMessageRequest::new(model.to_string(), messages);

    match timeout(Duration::from_secs(30), ollama.send_chat_messages(request)).await {
        Ok(Ok(msg)) => {
            let rephrased = strip_think_tags(&msg.message.content);
            // Sanity check: rephrased should be reasonable length
            if rephrased.len() > 20 && rephrased.len() < current_system.len() * 3 {
                rephrased
            } else {
                current_system.to_string() // fallback to original
            }
        }
        _ => current_system.to_string(), // fallback on failure
    }
}

#[derive(Clone, Copy)]
enum SystemRewriteBias {
    NotebookFirst,
    XmlStructured,
    LateInteraction,
}

#[derive(Clone, Copy)]
enum PromptRewriteBias {
    NotebookFirst,
    XmlStructured,
    LateInteraction,
}

async fn rewrite_system_with_bias(
    ollama: &Ollama,
    model: &str,
    current_system: &str,
    bias: SystemRewriteBias,
) -> String {
    let bias_instruction = match bias {
        SystemRewriteBias::NotebookFirst => {
            "Rewrite the instruction so the model prefers bottom-up validation: inspect small examples first, tinker with APIs in a notebook-like way, verify assumptions before writing large scripts, and avoid jumping to complex orchestration prematurely."
        }
        SystemRewriteBias::XmlStructured => {
            "Rewrite the instruction so the model emits compact XML-like structured outputs that are easy for downstream models to parse and converge on. Preserve the original task intent."
        }
        SystemRewriteBias::LateInteraction => {
            "Rewrite the instruction so the model uses late interaction: gather evidence in multiple small steps, keep intermediate observations explicit, and synthesize only after validating retrieved evidence."
        }
    };

    let prompt = format!(
        "{}\nOutput ONLY the rewritten instruction.\n\nCurrent instruction:\n{}",
        bias_instruction, current_system
    );

    let messages = vec![
        OllamaChatMsg::system(
            "You rewrite system prompts for agent optimization. Output only the rewritten instruction."
                .to_string(),
        ),
        OllamaChatMsg::user(prompt),
    ];
    let request = ChatMessageRequest::new(model.to_string(), messages);

    match timeout(Duration::from_secs(30), ollama.send_chat_messages(request)).await {
        Ok(Ok(msg)) => {
            let rewritten = strip_think_tags(&msg.message.content);
            if rewritten.len() > 20 && rewritten.len() < current_system.len() * 4 {
                rewritten
            } else {
                current_system.to_string()
            }
        }
        _ => current_system.to_string(),
    }
}

async fn rewrite_prompt_with_bias(
    ollama: &Ollama,
    model: &str,
    current_prompt: &str,
    bias: PromptRewriteBias,
) -> String {
    let bias_instruction = match bias {
        PromptRewriteBias::NotebookFirst => {
            "Rewrite this prompt so the model is encouraged to test assumptions incrementally, prefer small controlled experiments, and explain validated API understanding before proposing large automation."
        }
        PromptRewriteBias::XmlStructured => {
            "Rewrite this prompt so the output format converges toward concise XML blocks instead of loose prose. Preserve placeholders like {question} and {context} exactly."
        }
        PromptRewriteBias::LateInteraction => {
            "Rewrite this prompt so the model first records observations, then evidence, then synthesis. Preserve placeholders like {question} and {context} exactly."
        }
    };

    let prompt = format!(
        "{}\nOutput ONLY the rewritten prompt template.\n\nCurrent prompt:\n{}",
        bias_instruction, current_prompt
    );

    let messages = vec![
        OllamaChatMsg::system(
            "You rewrite prompt templates for agent optimization. Preserve placeholders exactly and output only the final prompt."
                .to_string(),
        ),
        OllamaChatMsg::user(prompt),
    ];
    let request = ChatMessageRequest::new(model.to_string(), messages);

    match timeout(Duration::from_secs(30), ollama.send_chat_messages(request)).await {
        Ok(Ok(msg)) => {
            let rewritten = strip_think_tags(&msg.message.content);
            if rewritten.contains("{question}")
                && rewritten.contains("{context}")
                && rewritten.len() > 20
                && rewritten.len() < current_prompt.len() * 4
            {
                rewritten
            } else {
                current_prompt.to_string()
            }
        }
        _ => current_prompt.to_string(),
    }
}

/// Run optimization for ALL signatures that have enough traces.
pub async fn optimize_all_signatures(
    ollama: &Ollama,
    model: &str,
    db: &RooDb,
    min_traces: u64,
    max_trials_per_sig: usize,
) -> Vec<OptimizationResult> {
    let sig_names = match db.list_dspy_signature_names().await {
        Ok(n) => n,
        Err(_) => return Vec::new(),
    };

    let mut results = Vec::new();
    for (name, count) in &sig_names {
        if *count < min_traces {
            continue;
        }
        if let Some(tmpl) = get_template_by_name(name) {
            match optimize_signature(ollama, model, db, &tmpl, max_trials_per_sig, None).await {
                Ok(result) => results.push(result),
                Err(_) => {} // skip failures
            }
        }
    }
    results
}

// ─── Signature Template Registry ───

/// Look up a static signature template by name.
/// This bridges the gap between DB-stored names and compile-time templates.
pub fn get_template_by_name(name: &str) -> Option<DspySignature> {
    match name {
        "analyst_stance" => Some(sig_analyst_stance()),
        "analyst_evidence" => Some(sig_analyst_evidence()),
        "analyst_argument" => Some(sig_analyst_argument()),
        "challenger_weak_point" => Some(sig_challenger_weak_point()),
        "challenger_counter_evidence" => Some(sig_challenger_counter_evidence()),
        "challenger_alternative" => Some(sig_challenger_alternative()),
        "rebuttal_valid" => Some(sig_rebuttal_valid()),
        "rebuttal_refute" => Some(sig_rebuttal_refute()),
        "rebuttal_refine" => Some(sig_rebuttal_refine()),
        "chair_winner" => Some(sig_chair_winner()),
        "chair_synthesis" => Some(sig_chair_synthesis()),
        "chair_confidence" => Some(sig_chair_confidence()),
        "simple_answer" => Some(sig_simple_answer()),
        "semantic_repair" => Some(sig_semantic_repair()),
        "chat_draft" => Some(sig_chat_draft()),
        "chat_refine" => Some(sig_chat_refine()),
        _ => None,
    }
}

// ─── Signature Definitions (unchanged) ───

pub fn sig_analyst_stance() -> DspySignature {
    DspySignature {
        name: "analyst_stance",
        system: "You are the Analyst in a formal debate. Output ONLY Claim/Evidence blocks.",
        prompt: "QUESTION: {question}\n\n{context}\n\nTASK: Provide your stance as Claim/Evidence blocks.\nRULES: Cite at least one msg:ID when available; edge IDs optional. If recommending an addition, specify a concrete mechanism (task-router/shared-memory/handoff) and cite msg evidence.\nFORMAT:\nClaim: ...\nEvidence: [msg:ID, edge:ID]\n(repeat)",
        roodb_query: Some(ROODB_AGENT_SNAPSHOT),
        max_output_chars: 1200,
    }
}

pub fn sig_analyst_evidence() -> DspySignature {
    DspySignature {
        name: "analyst_evidence",
        system: "You are the Analyst. Output ONLY Claim/Evidence blocks.",
        prompt: "QUESTION: {question}\n\n{context}\n\nTASK: Provide key evidence as Claim/Evidence blocks.\nRULES: Cite at least one msg:ID when available; edge IDs optional. If recommending an addition, specify a concrete mechanism (task-router/shared-memory/handoff) and cite msg evidence.\nFORMAT:\nClaim: ...\nEvidence: [msg:ID, edge:ID]\n(repeat)",
        roodb_query: Some(ROODB_BELIEFS_TOP),
        max_output_chars: 1400,
    }
}

pub fn sig_analyst_argument() -> DspySignature {
    DspySignature {
        name: "analyst_argument",
        system: "You are the Analyst. Output ONLY Claim/Evidence blocks.",
        prompt: "QUESTION: {question}\n\n{context}\n\nTASK: Provide your strongest argument as Claim/Evidence blocks.\nRULES: Cite at least one msg:ID when available; edge IDs optional. If recommending an addition, specify a concrete mechanism (task-router/shared-memory/handoff) and cite msg evidence.\nFORMAT:\nClaim: ...\nEvidence: [msg:ID, edge:ID]\n(repeat)",
        roodb_query: None,
        max_output_chars: 1400,
    }
}

pub fn sig_challenger_weak_point() -> DspySignature {
    DspySignature {
        name: "challenger_weak_point",
        system: "You are the Challenger. Output ONLY Claim/Evidence blocks.",
        prompt: "QUESTION: {question}\n\n{context}\n\nTASK: Identify the weakest point as Claim/Evidence blocks.\nRULES: Cite at least one msg:ID when available; edge IDs optional. If recommending an addition, specify a concrete mechanism (task-router/shared-memory/handoff) and cite msg evidence.\nFORMAT:\nClaim: ...\nEvidence: [msg:ID, edge:ID]\n(repeat)",
        roodb_query: None,
        max_output_chars: 1200,
    }
}

pub fn sig_challenger_counter_evidence() -> DspySignature {
    DspySignature {
        name: "challenger_counter_evidence",
        system: "You are the Challenger. Output ONLY Claim/Evidence blocks.",
        prompt: "QUESTION: {question}\n\n{context}\n\nTASK: Provide counter-evidence as Claim/Evidence blocks.\nRULES: Cite at least one msg:ID when available; edge IDs optional. If recommending an addition, specify a concrete mechanism (task-router/shared-memory/handoff) and cite msg evidence.\nFORMAT:\nClaim: ...\nEvidence: [msg:ID, edge:ID]\n(repeat)",
        roodb_query: Some(ROODB_BELIEFS_TOP),
        max_output_chars: 1400,
    }
}

pub fn sig_challenger_alternative() -> DspySignature {
    DspySignature {
        name: "challenger_alternative",
        system: "You are the Challenger. Output ONLY Claim/Evidence blocks.",
        prompt: "QUESTION: {question}\n\n{context}\n\nTASK: Provide your alternative position as Claim/Evidence blocks.\nRULES: Cite at least one msg:ID when available; edge IDs optional. If recommending an addition, specify a concrete mechanism (task-router/shared-memory/handoff) and cite msg evidence.\nFORMAT:\nClaim: ...\nEvidence: [msg:ID, edge:ID]\n(repeat)",
        roodb_query: None,
        max_output_chars: 1400,
    }
}

pub fn sig_rebuttal_valid() -> DspySignature {
    DspySignature {
        name: "rebuttal_valid",
        system: "You are the Analyst rebutting. Output ONLY Claim/Evidence blocks.",
        prompt: "QUESTION: {question}\n\n{context}\n\nTASK: Acknowledge valid critiques as Claim/Evidence blocks.\nRULES: Cite at least one msg:ID when available; edge IDs optional. If recommending an addition, specify a concrete mechanism (task-router/shared-memory/handoff) and cite msg evidence.\nFORMAT:\nClaim: ...\nEvidence: [msg:ID, edge:ID]\n(repeat)",
        roodb_query: None,
        max_output_chars: 1200,
    }
}

pub fn sig_rebuttal_refute() -> DspySignature {
    DspySignature {
        name: "rebuttal_refute",
        system: "You are the Analyst rebutting. Output ONLY Claim/Evidence blocks.",
        prompt: "QUESTION: {question}\n\n{context}\n\nTASK: Refute weak critiques as Claim/Evidence blocks.\nRULES: Cite at least one msg:ID when available; edge IDs optional. If recommending an addition, specify a concrete mechanism (task-router/shared-memory/handoff) and cite msg evidence.\nFORMAT:\nClaim: ...\nEvidence: [msg:ID, edge:ID]\n(repeat)",
        roodb_query: None,
        max_output_chars: 1200,
    }
}

pub fn sig_rebuttal_refine() -> DspySignature {
    DspySignature {
        name: "rebuttal_refine",
        system: "You are the Analyst rebutting. Output ONLY Claim/Evidence blocks.",
        prompt: "QUESTION: {question}\n\n{context}\n\nTASK: Provide your refined position as Claim/Evidence blocks.\nRULES: Cite at least one msg:ID when available; edge IDs optional. If recommending an addition, specify a concrete mechanism (task-router/shared-memory/handoff) and cite msg evidence.\nFORMAT:\nClaim: ...\nEvidence: [msg:ID, edge:ID]\n(repeat)",
        roodb_query: None,
        max_output_chars: 1400,
    }
}

pub fn sig_chair_winner() -> DspySignature {
    DspySignature {
        name: "chair_winner",
        system: "You are the Chair. Decide the winning argument and why (3–5 sentences).",
        prompt: "QUESTION: {question}\n\n{context}\n\nTASK: Decide the winning argument and why.",
        roodb_query: None,
        max_output_chars: 1400,
    }
}

pub fn sig_chair_synthesis() -> DspySignature {
    DspySignature {
        name: "chair_synthesis",
        system: "You are the Chair. Output ONLY Claim/Evidence blocks. Do not add prose outside that format.",
        prompt: "QUESTION: {question}\n\n{context}\n\nTASK: Produce a synthesis answer as Claim/Evidence blocks.\nRULES: Cite at least one msg:ID when available; edge IDs optional. If recommending an addition, specify a concrete mechanism (task-router/shared-memory/handoff) and cite msg evidence.\nFORMAT:\nClaim: ...\nEvidence: [msg:ID, edge:ID]\n(repeat)",
        roodb_query: Some(ROODB_BELIEFS_TOP),
        max_output_chars: 1800,
    }
}

pub fn sig_chair_confidence() -> DspySignature {
    DspySignature {
        name: "chair_confidence",
        system: "You are the Chair. State confidence and remaining uncertainty (2–4 sentences).",
        prompt: "QUESTION: {question}\n\n{context}\n\nTASK: State confidence and uncertainty.",
        roodb_query: None,
        max_output_chars: 900,
    }
}

pub fn sig_simple_answer() -> DspySignature {
    DspySignature {
        name: "simple_answer",
        system: "You are a concise expert advisor. Output ONLY Claim/Evidence blocks. Do not add prose outside that format.",
        prompt: "QUESTION: {question}\n\n{context}\n\nTASK: Provide a direct answer as Claim/Evidence blocks.\nRULES: Cite at least one msg:ID when available; edge IDs optional. If recommending an addition, specify a concrete mechanism (task-router/shared-memory/handoff) and cite msg evidence.\nFORMAT:\nClaim: ...\nEvidence: [msg:ID, edge:ID]\n(repeat)",
        roodb_query: Some(ROODB_BELIEFS_TOP),
        max_output_chars: 2000,
    }
}

pub fn sig_semantic_repair() -> DspySignature {
    DspySignature {
        name: "semantic_repair",
        system: "You are a verifier. Rewrite ONLY in Claim/Evidence blocks using provided evidence IDs. No extra text.",
        prompt: "QUESTION: {question}\n\n{context}\n\nPRIOR OUTPUT:\n{prior}\n\nTASK: Rewrite as valid Claim/Evidence blocks referencing only the listed msg:/edge: IDs.\nRULES: Cite at least one msg:ID when available; edge IDs optional. If recommending an addition, specify a concrete mechanism (task-router/shared-memory/handoff) and cite msg evidence.\nFORMAT:\nClaim: ...\nEvidence: [msg:ID, edge:ID]\n(repeat)",
        roodb_query: None,
        max_output_chars: 2000,
    }
}

pub fn sig_chat_draft() -> DspySignature {
    DspySignature {
        name: "chat_draft",
        system: "You are a concise, practical assistant. Output ONLY Claim/Evidence blocks.",
        prompt: "QUESTION: {question}\n\n{context}\n\nTASK: Draft a helpful response as Claim/Evidence blocks.\nRULES: Cite at least one msg:ID when available; edge IDs optional. If recommending an addition, specify a concrete mechanism (task-router/shared-memory/handoff) and cite msg evidence.\nFORMAT:\nClaim: ...\nEvidence: [msg:ID, edge:ID]\n(repeat)",
        roodb_query: Some(ROODB_BELIEFS_TOP),
        max_output_chars: 2200,
    }
}

pub fn sig_chat_refine() -> DspySignature {
    DspySignature {
        name: "chat_refine",
        system: "You are a meticulous editor. Output ONLY Claim/Evidence blocks.",
        prompt: "QUESTION: {question}\n\n{context}\n\nTASK: Refine and improve the response as Claim/Evidence blocks.\nRULES: Cite at least one msg:ID when available; edge IDs optional. If recommending an addition, specify a concrete mechanism (task-router/shared-memory/handoff) and cite msg evidence.\nFORMAT:\nClaim: ...\nEvidence: [msg:ID, edge:ID]\n(repeat)",
        roodb_query: None,
        max_output_chars: 2600,
    }
}

/// Strip `Claim: ... / Evidence: [...]` formatting from DSPy output,
/// returning only the claim text as natural prose for user-facing display.
pub fn strip_claim_evidence_format(raw: &str) -> String {
    let mut claims: Vec<&str> = Vec::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("Claim:") {
            let claim = rest.trim();
            if !claim.is_empty() {
                claims.push(claim);
            }
        }
        // Skip "Evidence:" lines entirely
    }
    if claims.is_empty() {
        // No Claim: lines found — return the original text as-is
        raw.to_string()
    } else {
        claims.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_claim_evidence_format() {
        // Basic Claim/Evidence stripping
        let input = "Claim: The collaboration module is available.\nEvidence: [msg:msg_0_1773260333]";
        let result = strip_claim_evidence_format(input);
        assert_eq!(result, "The collaboration module is available.");

        // Multiple Claim/Evidence blocks
        let input2 = "Claim: First point.\nEvidence: [msg:1]\nClaim: Second point.\nEvidence: [msg:2]";
        let result2 = strip_claim_evidence_format(input2);
        assert_eq!(result2, "First point.\nSecond point.");

        // No Claim/Evidence - return as-is
        let input3 = "This is just regular text.";
        let result3 = strip_claim_evidence_format(input3);
        assert_eq!(result3, "This is just regular text.");

        // Empty input
        let input4 = "";
        let result4 = strip_claim_evidence_format(input4);
        assert_eq!(result4, "");
    }

    #[test]
    fn score_output_rewards_xml_and_stepwise_structure() {
        let plain = "Claim: A\nEvidence: [msg:1]";
        let structured = "<observation>API checked</observation><claim>A</claim><evidence>msg:1</evidence><validation>ok</validation>";
        let reference = "Claim: A\nEvidence: [msg:1]";

        assert!(score_output(structured, reference) > score_output(plain, reference));
    }

    #[test]
    fn mutation_style_cycles_across_new_biases() {
        use DspyMutationStyle::*;

        assert!(matches!(DspyMutationStyle::from_trial(0), DemoSubset));
        assert!(matches!(DspyMutationStyle::from_trial(3), NotebookFirst));
        assert!(matches!(DspyMutationStyle::from_trial(4), XmlConverged));
        assert!(matches!(DspyMutationStyle::from_trial(5), LateInteraction));
    }
}
