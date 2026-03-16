//! HSM-II Onboarding, Belief Extraction, and Document Ingestion
//!
//! Three-layer system for teaching HSM-II about a business:
//! 1. Guided onboarding questionnaire → instant beliefs
//! 2. Post-chat belief extraction → learns over time
//! 3. Document ingestion → bulk knowledge import

use crate::hyper_stigmergy::{BeliefSource, HyperStigmergicMorphogenesis};
use crate::ollama_client::OllamaClient;
use crate::rlm::LivingPrompt;
use anyhow::Result;
use serde::{Deserialize, Serialize};

// ─────────────────────────────────────────────────────────────────────────────
// Layer 1: Structured Onboarding Questionnaire
// ─────────────────────────────────────────────────────────────────────────────

/// Questions for the onboarding flow
const ONBOARD_QUESTIONS: &[(&str, &str, f64)] = &[
    (
        "What does your company or product do? (one sentence)",
        "product",
        0.95,
    ),
    (
        "Who are your target customers? (size, industry, role)",
        "market",
        0.90,
    ),
    (
        "What's your current revenue or stage? (e.g. $12K MRR, pre-seed, 50 customers)",
        "financial",
        0.90,
    ),
    (
        "Who are your main competitors?",
        "competitive",
        0.85,
    ),
    (
        "What's your competitive advantage? (why do customers pick you?)",
        "competitive",
        0.90,
    ),
    (
        "What's your monthly budget or team size?",
        "operational",
        0.85,
    ),
    (
        "What topics do you use this assistant for most? (coding, marketing, strategy, etc.)",
        "usage",
        0.80,
    ),
    (
        "Anything the assistant should NEVER suggest? (e.g. tools you can't afford, strategies that don't fit)",
        "avoid",
        0.95,
    ),
];

/// Result of the onboarding process
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnboardResult {
    pub beliefs_created: usize,
    pub avoid_patterns_added: usize,
    pub categories: Vec<String>,
}

/// Run interactive onboarding, creating beliefs from each answer.
/// Returns the number of beliefs created.
pub async fn run_onboard_interactive(
    world: &mut HyperStigmergicMorphogenesis,
    living_prompt: &mut LivingPrompt,
) -> Result<OnboardResult> {
    use tokio::io::{stdin, AsyncBufReadExt, BufReader};

    let stdin_reader = BufReader::new(stdin());
    let mut lines = stdin_reader.lines();

    println!("\n🏢 HSM-II Business Onboarding");
    println!("{}\n", "─".repeat(50));
    println!("Answer a few questions so HSM-II can give you better advice.");
    println!("Press Enter to skip any question.\n");

    let mut beliefs_created = 0usize;
    let mut avoid_patterns_added = 0usize;
    let mut categories = Vec::new();

    for (question, category, confidence) in ONBOARD_QUESTIONS {
        println!("  {} ", question);
        print!("  > ");
        // flush stdout so the prompt appears before readline
        use std::io::Write;
        std::io::stdout().flush().ok();

        let answer = lines.next_line().await?.unwrap_or_default();
        let answer = answer.trim().to_string();

        if answer.is_empty() {
            println!("  (skipped)\n");
            continue;
        }

        if *category == "avoid" {
            // Parse comma-separated avoid patterns
            let patterns: Vec<&str> = answer.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
            for pattern in &patterns {
                living_prompt.add_avoid_pattern(pattern.to_string());
                avoid_patterns_added += 1;
            }
            println!("  ✓ Added {} avoid pattern(s)\n", patterns.len());
        } else {
            // Create beliefs from the answer
            let new_beliefs = answer_to_beliefs(&answer, category, *confidence);
            for (content, conf) in &new_beliefs {
                world.add_belief(content, *conf, BeliefSource::UserProvided);
                beliefs_created += 1;
            }
            if !categories.contains(&category.to_string()) {
                categories.push(category.to_string());
            }
            println!("  ✓ Created {} belief(s) [{}]\n", new_beliefs.len(), category);
        }
    }

    println!("{}", "─".repeat(50));
    println!(
        "✨ Onboarding complete: {} beliefs, {} avoid patterns across {:?}",
        beliefs_created, avoid_patterns_added, categories
    );

    Ok(OnboardResult {
        beliefs_created,
        avoid_patterns_added,
        categories,
    })
}

/// Run non-interactive onboarding from a pre-filled answers map.
/// Keys: "product", "market", "financial", "competitive", "advantage",
///        "operational", "usage", "avoid"
pub fn run_onboard_batch(
    world: &mut HyperStigmergicMorphogenesis,
    living_prompt: &mut LivingPrompt,
    answers: &std::collections::HashMap<String, String>,
) -> OnboardResult {
    let mut beliefs_created = 0usize;
    let mut avoid_patterns_added = 0usize;
    let mut categories = Vec::new();

    // Map answer keys to categories and confidence
    let mappings: &[(&str, &str, f64)] = &[
        ("product", "product", 0.95),
        ("market", "market", 0.90),
        ("financial", "financial", 0.90),
        ("competitors", "competitive", 0.85),
        ("advantage", "competitive", 0.90),
        ("operational", "operational", 0.85),
        ("usage", "usage", 0.80),
    ];

    for (key, category, confidence) in mappings {
        if let Some(answer) = answers.get(*key) {
            let answer = answer.trim();
            if answer.is_empty() {
                continue;
            }
            let new_beliefs = answer_to_beliefs(answer, category, *confidence);
            for (content, conf) in &new_beliefs {
                world.add_belief(content, *conf, BeliefSource::UserProvided);
                beliefs_created += 1;
            }
            if !categories.contains(&category.to_string()) {
                categories.push(category.to_string());
            }
        }
    }

    // Handle avoid patterns separately
    if let Some(avoid_answer) = answers.get("avoid") {
        let patterns: Vec<&str> = avoid_answer
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();
        for pattern in &patterns {
            living_prompt.add_avoid_pattern(pattern.to_string());
            avoid_patterns_added += 1;
        }
    }

    OnboardResult {
        beliefs_created,
        avoid_patterns_added,
        categories,
    }
}

/// Convert a free-text answer into one or more beliefs.
/// Handles multi-part answers (comma/semicolon separated).
fn answer_to_beliefs(answer: &str, category: &str, base_confidence: f64) -> Vec<(String, f64)> {
    let mut beliefs = Vec::new();

    // If the answer contains commas or semicolons, split into multiple beliefs
    let parts: Vec<&str> = answer
        .split(&[',', ';'][..])
        .map(|s| s.trim())
        .filter(|s| s.len() > 3)
        .collect();

    if parts.len() > 1 {
        // Multiple distinct facts
        for part in &parts {
            let content = format_belief(part, category);
            beliefs.push((content, base_confidence));
        }
    } else {
        // Single belief from the full answer
        let content = format_belief(answer, category);
        beliefs.push((content, base_confidence));
    }

    beliefs
}

/// Format a raw answer fragment into a clear belief statement
fn format_belief(raw: &str, category: &str) -> String {
    let raw = raw.trim();
    // If it already reads like a statement, use as-is
    if raw.len() > 20 {
        return raw.to_string();
    }
    // Otherwise, add category context
    match category {
        "product" => format!("Our product: {}", raw),
        "market" => format!("Target market: {}", raw),
        "financial" => format!("Financial status: {}", raw),
        "competitive" => format!("Competitive landscape: {}", raw),
        "operational" => format!("Operations: {}", raw),
        "usage" => format!("Primary use cases: {}", raw),
        _ => raw.to_string(),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Layer 2: Post-Chat Belief Extraction
// ─────────────────────────────────────────────────────────────────────────────

/// The extraction prompt sent to the LLM after a conversation exchange.
const EXTRACTION_SYSTEM_PROMPT: &str = r#"You extract business facts from conversations.
Given a user message and assistant response, extract any NEW factual information about:
- The user's product, company, or service
- Their customers, market, or industry
- Revenue, pricing, budget, or financial details
- Competitors or competitive dynamics
- Team size, tools, or operational details
- Preferences, constraints, or things to avoid

Rules:
- Only extract FACTS, not opinions or questions
- Each fact should be a single clear sentence
- Skip generic statements that any business would say
- Skip facts that are obviously temporary (e.g. "I'm debugging X right now")
- If no new business facts are present, return an empty array

Respond ONLY with a JSON array of objects:
[{"content": "fact statement", "confidence": 0.7-0.95, "category": "product|market|financial|competitive|operational|preference"}]

If nothing to extract, respond: []"#;

/// Extract beliefs from a conversation exchange.
/// Returns parsed beliefs ready for storage.
pub async fn extract_beliefs_from_chat(
    llm: &OllamaClient,
    user_message: &str,
    assistant_response: &str,
) -> Vec<ExtractedBelief> {
    // Skip extraction for very short exchanges or obvious non-business chat
    if user_message.len() < 15 || assistant_response.len() < 30 {
        return Vec::new();
    }

    let extraction_input = format!(
        "User: {}\n\nAssistant: {}",
        &user_message[..user_message.len().min(1500)],
        &assistant_response[..assistant_response.len().min(1500)]
    );

    let result = llm
        .chat(EXTRACTION_SYSTEM_PROMPT, &extraction_input, &[])
        .await;

    if result.timed_out || result.text.is_empty() {
        return Vec::new();
    }

    parse_extracted_beliefs(&result.text)
}

/// A belief extracted from conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedBelief {
    pub content: String,
    pub confidence: f64,
    pub category: String,
}

/// Parse the LLM's JSON response into extracted beliefs
fn parse_extracted_beliefs(response: &str) -> Vec<ExtractedBelief> {
    // Find the JSON array in the response (may have surrounding text)
    let text = response.trim();

    // Try to find JSON array bounds
    let start = text.find('[');
    let end = text.rfind(']');

    if let (Some(s), Some(e)) = (start, end) {
        if e > s {
            let json_str = &text[s..=e];
            if let Ok(beliefs) = serde_json::from_str::<Vec<ExtractedBelief>>(json_str) {
                return beliefs
                    .into_iter()
                    .filter(|b| {
                        !b.content.is_empty()
                            && b.confidence >= 0.5
                            && b.confidence <= 1.0
                            && b.content.len() > 10
                    })
                    .collect();
            }
        }
    }

    Vec::new()
}

// ─────────────────────────────────────────────────────────────────────────────
// Layer 3: Extraction → add_belief() Wiring
// ─────────────────────────────────────────────────────────────────────────────

/// Store extracted beliefs into the world's belief system.
/// Returns the number of new beliefs added (vs updated existing ones).
pub fn store_extracted_beliefs(
    world: &mut HyperStigmergicMorphogenesis,
    living_prompt: &mut LivingPrompt,
    beliefs: &[ExtractedBelief],
) -> usize {
    let existing_count = world.beliefs.len();

    for belief in beliefs {
        let source = match belief.category.as_str() {
            "preference" => {
                // Preferences become avoid patterns if they sound negative
                if belief.content.to_lowercase().contains("don't")
                    || belief.content.to_lowercase().contains("avoid")
                    || belief.content.to_lowercase().contains("never")
                    || belief.content.to_lowercase().contains("not ")
                {
                    living_prompt.add_avoid_pattern(belief.content.clone());
                    continue;
                }
                BeliefSource::Observation
            }
            _ => BeliefSource::Observation,
        };

        world.add_belief(&belief.content, belief.confidence, source);
    }

    // Count how many were genuinely new (not updates to existing)
    world.beliefs.len() - existing_count
}

/// Full post-chat extraction pipeline: extract + store.
/// Call this after each chat exchange. Lightweight — skips if nothing to extract.
pub async fn post_chat_extract_and_store(
    llm: &OllamaClient,
    world: &mut HyperStigmergicMorphogenesis,
    living_prompt: &mut LivingPrompt,
    user_message: &str,
    assistant_response: &str,
) -> usize {
    let extracted = extract_beliefs_from_chat(llm, user_message, assistant_response).await;
    if extracted.is_empty() {
        return 0;
    }
    store_extracted_beliefs(world, living_prompt, &extracted)
}

// ─────────────────────────────────────────────────────────────────────────────
// Layer 4: Document Ingestion
// ─────────────────────────────────────────────────────────────────────────────

/// Configuration for document ingestion
#[derive(Debug, Clone)]
pub struct IngestConfig {
    /// Maximum chunk size in characters
    pub chunk_size: usize,
    /// Overlap between chunks
    pub chunk_overlap: usize,
    /// Maximum beliefs to extract per chunk
    pub max_beliefs_per_chunk: usize,
    /// Minimum confidence threshold
    pub min_confidence: f64,
}

impl Default for IngestConfig {
    fn default() -> Self {
        Self {
            chunk_size: 2000,
            chunk_overlap: 200,
            max_beliefs_per_chunk: 5,
            min_confidence: 0.6,
        }
    }
}

/// Result of document ingestion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestResult {
    pub file_path: String,
    pub chunks_processed: usize,
    pub beliefs_created: usize,
    pub avoid_patterns_added: usize,
    pub categories: std::collections::HashMap<String, usize>,
    pub errors: Vec<String>,
}

/// The extraction prompt for document chunks
const INGEST_SYSTEM_PROMPT: &str = r#"You extract business knowledge from documents.
Given a chunk of text from a business document, extract factual claims about:
- Product/service details (features, pricing, positioning)
- Market information (size, segments, trends)
- Customer details (personas, pain points, behavior)
- Financial data (revenue, costs, margins, projections)
- Competitive intelligence (competitors, differentiation)
- Operational facts (team, tools, processes, constraints)
- Strategic insights (goals, priorities, risks)

Rules:
- Extract CONCRETE FACTS, not vague statements
- Each fact should stand alone (no "as mentioned above")
- Include numbers, names, and specifics when present
- Skip boilerplate, legal disclaimers, and formatting artifacts
- Confidence: 0.9 for explicit statements, 0.7 for inferred, 0.5 for implied

Respond ONLY with a JSON array:
[{"content": "specific fact", "confidence": 0.5-0.95, "category": "product|market|financial|competitive|operational|strategic"}]

If nothing useful, respond: []"#;

/// Ingest a file and extract beliefs from it.
/// Supports: .txt, .md, .csv, .json, .pdf (text only)
pub async fn ingest_file(
    llm: &OllamaClient,
    world: &mut HyperStigmergicMorphogenesis,
    living_prompt: &mut LivingPrompt,
    file_path: &str,
    config: &IngestConfig,
) -> Result<IngestResult> {
    let path = std::path::Path::new(file_path);

    if !path.exists() {
        anyhow::bail!("File not found: {}", file_path);
    }

    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    // Read file content based on type
    let content = match extension.as_str() {
        "txt" | "md" | "markdown" | "rst" => std::fs::read_to_string(path)?,
        "csv" | "tsv" => {
            // Read CSV and format as readable text
            let raw = std::fs::read_to_string(path)?;
            csv_to_readable(&raw, &extension)
        }
        "json" => {
            // Pretty-print JSON for readability
            let raw = std::fs::read_to_string(path)?;
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&raw) {
                serde_json::to_string_pretty(&val).unwrap_or(raw)
            } else {
                raw
            }
        }
        "html" | "htm" => {
            // Strip HTML tags, keep text
            let raw = std::fs::read_to_string(path)?;
            strip_html_tags(&raw)
        }
        _ => {
            // Try to read as plain text
            std::fs::read_to_string(path).map_err(|e| {
                anyhow::anyhow!(
                    "Cannot read '{}' (unsupported format or binary file): {}",
                    extension,
                    e
                )
            })?
        }
    };

    if content.trim().is_empty() {
        anyhow::bail!("File is empty: {}", file_path);
    }

    // Chunk the content
    let chunks = chunk_text(&content, config.chunk_size, config.chunk_overlap);

    println!(
        "📄 Ingesting {} ({} chunks, {} chars)",
        path.file_name().unwrap_or_default().to_string_lossy(),
        chunks.len(),
        content.len()
    );

    let mut result = IngestResult {
        file_path: file_path.to_string(),
        chunks_processed: 0,
        beliefs_created: 0,
        avoid_patterns_added: 0,
        categories: std::collections::HashMap::new(),
        errors: Vec::new(),
    };

    for (i, chunk) in chunks.iter().enumerate() {
        print!("  Processing chunk {}/{}... ", i + 1, chunks.len());
        std::io::Write::flush(&mut std::io::stdout()).ok();

        let llm_result = llm
            .chat(INGEST_SYSTEM_PROMPT, chunk, &[])
            .await;

        if llm_result.timed_out || llm_result.text.is_empty() {
            result.errors.push(format!("Chunk {} timed out", i + 1));
            println!("⏰ timeout");
            continue;
        }

        let extracted = parse_extracted_beliefs(&llm_result.text);
        let filtered: Vec<_> = extracted
            .into_iter()
            .filter(|b| b.confidence >= config.min_confidence)
            .take(config.max_beliefs_per_chunk)
            .collect();

        let existing_count = world.beliefs.len();

        for belief in &filtered {
            // Track categories
            *result.categories.entry(belief.category.clone()).or_insert(0) += 1;

            // Check for avoid patterns in strategic/preference
            if belief.category == "preference"
                || (belief.content.to_lowercase().contains("avoid")
                    || belief.content.to_lowercase().contains("don't")
                    || belief.content.to_lowercase().contains("risk"))
                    && belief.category == "strategic"
            {
                living_prompt.add_avoid_pattern(belief.content.clone());
                result.avoid_patterns_added += 1;
                continue;
            }

            world.add_belief(&belief.content, belief.confidence, BeliefSource::Inference);
        }

        let new_beliefs = world.beliefs.len() - existing_count;
        result.beliefs_created += new_beliefs;
        result.chunks_processed += 1;

        println!("✓ {} beliefs", new_beliefs);
    }

    println!(
        "\n📊 Ingestion complete: {} beliefs, {} avoid patterns from {} chunks",
        result.beliefs_created, result.avoid_patterns_added, result.chunks_processed
    );
    if !result.categories.is_empty() {
        println!("   Categories: {:?}", result.categories);
    }
    if !result.errors.is_empty() {
        println!("   ⚠ {} errors: {:?}", result.errors.len(), result.errors);
    }

    Ok(result)
}

/// Split text into overlapping chunks
fn chunk_text(text: &str, chunk_size: usize, overlap: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let total = chars.len();

    if total <= chunk_size {
        chunks.push(text.to_string());
        return chunks;
    }

    let mut start = 0;
    while start < total {
        let end = (start + chunk_size).min(total);

        // Try to break at a sentence or paragraph boundary
        let mut break_at = end;
        if end < total {
            // Look back from end for a good break point
            for i in (start + chunk_size / 2..end).rev() {
                if i < chars.len() && (chars[i] == '.' || chars[i] == '\n' || chars[i] == '!' || chars[i] == '?') {
                    break_at = i + 1;
                    break;
                }
            }
        }

        let chunk: String = chars[start..break_at].iter().collect();
        if !chunk.trim().is_empty() {
            chunks.push(chunk);
        }

        // Advance with overlap
        if break_at >= total {
            break;
        }
        start = if break_at > overlap {
            break_at - overlap
        } else {
            break_at
        };
    }

    chunks
}

/// Convert CSV content to readable text for the LLM
fn csv_to_readable(raw: &str, extension: &str) -> String {
    let separator = if extension == "tsv" { '\t' } else { ',' };
    let mut output = String::new();
    let mut lines = raw.lines();

    if let Some(header) = lines.next() {
        let columns: Vec<&str> = header.split(separator).collect();
        output.push_str(&format!("Columns: {}\n\n", columns.join(" | ")));

        for (i, line) in lines.enumerate() {
            if i >= 50 {
                output.push_str(&format!("... ({} more rows)\n", raw.lines().count() - 51));
                break;
            }
            let values: Vec<&str> = line.split(separator).collect();
            for (j, val) in values.iter().enumerate() {
                if j < columns.len() {
                    output.push_str(&format!("{}: {} | ", columns[j], val.trim()));
                }
            }
            output.push('\n');
        }
    }

    output
}

/// Strip HTML tags from content, keeping text
fn strip_html_tags(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let mut in_tag = false;
    let mut in_script = false;

    for ch in html.chars() {
        match ch {
            '<' => {
                in_tag = true;
                // Check for script/style start
                let rest = &html[html.find('<').unwrap_or(0)..];
                if rest.to_lowercase().starts_with("<script") || rest.to_lowercase().starts_with("<style") {
                    in_script = true;
                }
            }
            '>' => {
                in_tag = false;
                if in_script {
                    let rest = &html[html.find('>').unwrap_or(0)..];
                    if rest.to_lowercase().starts_with("</script") || rest.to_lowercase().starts_with("</style") {
                        in_script = false;
                    }
                }
            }
            _ if !in_tag && !in_script => {
                result.push(ch);
            }
            _ => {}
        }
    }

    // Clean up whitespace
    result
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_answer_to_beliefs_single() {
        let beliefs = answer_to_beliefs("We build AI code review tools for dev teams", "product", 0.95);
        assert_eq!(beliefs.len(), 1);
        assert_eq!(beliefs[0].0, "We build AI code review tools for dev teams");
        assert!((beliefs[0].1 - 0.95).abs() < f64::EPSILON);
    }

    #[test]
    fn test_answer_to_beliefs_multi() {
        let beliefs = answer_to_beliefs("CodeRabbit, Sourcery, GitHub Copilot", "competitive", 0.85);
        assert_eq!(beliefs.len(), 3);
        assert!(beliefs[0].0.contains("CodeRabbit"));
        assert!(beliefs[1].0.contains("Sourcery"));
        assert!(beliefs[2].0.contains("GitHub Copilot"));
    }

    #[test]
    fn test_answer_to_beliefs_short_answer_gets_prefix() {
        let beliefs = answer_to_beliefs("$12K MRR", "financial", 0.90);
        assert_eq!(beliefs.len(), 1);
        assert_eq!(beliefs[0].0, "Financial status: $12K MRR");
    }

    #[test]
    fn test_format_belief_categories() {
        assert_eq!(format_belief("SaaS", "product"), "Our product: SaaS");
        assert_eq!(format_belief("dev teams", "market"), "Target market: dev teams");
        assert_eq!(format_belief("$50K", "financial"), "Financial status: $50K");
    }

    #[test]
    fn test_parse_extracted_beliefs_valid() {
        let json = r#"[
            {"content": "Company has 15 paying customers", "confidence": 0.9, "category": "financial"},
            {"content": "Main competitor is CodeRabbit", "confidence": 0.85, "category": "competitive"}
        ]"#;
        let beliefs = parse_extracted_beliefs(json);
        assert_eq!(beliefs.len(), 2);
        assert_eq!(beliefs[0].content, "Company has 15 paying customers");
        assert!((beliefs[0].confidence - 0.9).abs() < f64::EPSILON);
    }

    #[test]
    fn test_parse_extracted_beliefs_with_surrounding_text() {
        let response = "Here are the extracted facts:\n[{\"content\": \"Revenue is $12K MRR\", \"confidence\": 0.9, \"category\": \"financial\"}]\nDone.";
        let beliefs = parse_extracted_beliefs(response);
        assert_eq!(beliefs.len(), 1);
        assert_eq!(beliefs[0].content, "Revenue is $12K MRR");
    }

    #[test]
    fn test_parse_extracted_beliefs_empty() {
        assert!(parse_extracted_beliefs("[]").is_empty());
        assert!(parse_extracted_beliefs("no json here").is_empty());
        assert!(parse_extracted_beliefs("").is_empty());
    }

    #[test]
    fn test_parse_extracted_beliefs_filters_low_confidence() {
        let json = r#"[
            {"content": "Some vague statement", "confidence": 0.3, "category": "product"},
            {"content": "Solid fact about pricing", "confidence": 0.85, "category": "financial"}
        ]"#;
        let beliefs = parse_extracted_beliefs(json);
        assert_eq!(beliefs.len(), 1);
        assert_eq!(beliefs[0].content, "Solid fact about pricing");
    }

    #[test]
    fn test_parse_extracted_beliefs_filters_short_content() {
        let json = r#"[{"content": "ok", "confidence": 0.9, "category": "product"}]"#;
        let beliefs = parse_extracted_beliefs(json);
        assert!(beliefs.is_empty());
    }

    #[test]
    fn test_chunk_text_small() {
        let chunks = chunk_text("Hello world", 100, 20);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], "Hello world");
    }

    #[test]
    fn test_chunk_text_splits() {
        let text = "A".repeat(500);
        let chunks = chunk_text(&text, 200, 50);
        assert!(chunks.len() >= 3);
    }

    #[test]
    fn test_csv_to_readable() {
        let csv = "Name,Revenue,Stage\nAcme,50K,Series A\nBeta,12K,Seed\n";
        let readable = csv_to_readable(csv, "csv");
        assert!(readable.contains("Name"));
        assert!(readable.contains("Revenue"));
        assert!(readable.contains("Acme"));
        assert!(readable.contains("50K"));
    }

    #[test]
    fn test_strip_html_tags() {
        let html = "<html><body><h1>Title</h1><p>Content here.</p></body></html>";
        let text = strip_html_tags(html);
        assert!(text.contains("Title"));
        assert!(text.contains("Content here."));
        assert!(!text.contains("<h1>"));
    }

    #[test]
    fn test_batch_onboard() {
        let mut world = HyperStigmergicMorphogenesis::new(3);
        let mut living_prompt = LivingPrompt::new("test");

        let mut answers = HashMap::new();
        answers.insert("product".to_string(), "AI code review tool for dev teams".to_string());
        answers.insert("market".to_string(), "20-200 person engineering teams".to_string());
        answers.insert("financial".to_string(), "$12K MRR, 15 customers".to_string());
        answers.insert("competitors".to_string(), "CodeRabbit, Sourcery".to_string());
        answers.insert("advantage".to_string(), "3x faster with higher accuracy".to_string());
        answers.insert("avoid".to_string(), "enterprise tools we can't afford, hiring suggestions".to_string());

        let result = run_onboard_batch(&mut world, &mut living_prompt, &answers);

        assert!(result.beliefs_created >= 5, "Expected at least 5 beliefs, got {}", result.beliefs_created);
        assert_eq!(result.avoid_patterns_added, 2);
        assert!(result.categories.contains(&"product".to_string()));
        assert!(result.categories.contains(&"financial".to_string()));

        // Verify beliefs are actually in the world
        let has_product = world.beliefs.iter().any(|b| b.content.contains("code review"));
        assert!(has_product, "Should have a product belief");

        // Verify avoid patterns are in living prompt
        let rendered = living_prompt.render();
        assert!(rendered.contains("enterprise tools"), "Living prompt should contain avoid pattern");
    }

    #[test]
    fn test_store_extracted_beliefs() {
        let mut world = HyperStigmergicMorphogenesis::new(3);
        let mut living_prompt = LivingPrompt::new("test");

        let extracted = vec![
            ExtractedBelief {
                content: "Company revenue is $12K MRR".to_string(),
                confidence: 0.9,
                category: "financial".to_string(),
            },
            ExtractedBelief {
                content: "Don't recommend enterprise solutions".to_string(),
                confidence: 0.85,
                category: "preference".to_string(),
            },
            ExtractedBelief {
                content: "Target market is B2B SaaS".to_string(),
                confidence: 0.8,
                category: "market".to_string(),
            },
        ];

        let initial_beliefs = world.beliefs.len();
        let new_count = store_extracted_beliefs(&mut world, &mut living_prompt, &extracted);

        // Should add 2 beliefs (the preference one becomes an avoid pattern)
        assert_eq!(new_count, 2, "Expected 2 new beliefs (1 became avoid pattern)");
        assert_eq!(world.beliefs.len(), initial_beliefs + 2);

        // Verify the avoid pattern was added
        let rendered = living_prompt.render();
        assert!(rendered.contains("enterprise solutions"), "Avoid pattern should be in living prompt");
    }

    #[test]
    fn test_ingest_config_defaults() {
        let config = IngestConfig::default();
        assert_eq!(config.chunk_size, 2000);
        assert_eq!(config.chunk_overlap, 200);
        assert_eq!(config.max_beliefs_per_chunk, 5);
        assert!((config.min_confidence - 0.6).abs() < f64::EPSILON);
    }
}
