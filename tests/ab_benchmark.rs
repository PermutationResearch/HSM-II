//! A/B Benchmark: Plain LLM vs HSM-II Enriched LLM
//!
//! Sends the same 10 prompts through two arms:
//!   Arm A (baseline): bare system prompt → LLM
//!   Arm B (enriched): full HSM-II context (beliefs, skills, playbooks, hints, living prompt) → LLM
//!
//! Uses an LLM judge to blind-score both responses, then prints a comparison table.
//!
//! Run:
//!   ANTHROPIC_API_KEY="sk-ant-..." cargo test --test ab_benchmark -- --nocapture
//!   OPENAI_API_KEY="sk-..."       cargo test --test ab_benchmark -- --nocapture
//!   OLLAMA_MODEL="qwen3-coder:480b-cloud" cargo test --test ab_benchmark -- --nocapture
//!
//! Results saved to ~/.hsmii/benchmarks/

use ::hyper_stigmergy::autocontext::{Hint, Playbook, Step};
use ::hyper_stigmergy::consensus::{BayesianConfidence, SkillStatus};
use ::hyper_stigmergy::hyper_stigmergy::{Belief, BeliefSource};
use ::hyper_stigmergy::llm::client::{LlmClient, LlmRequest, Message};
use ::hyper_stigmergy::rlm::LivingPrompt;
use ::hyper_stigmergy::skill::{
    ApplicabilityCondition, Skill, SkillCuration, SkillLevel, SkillScope,
    SkillSource, TrajectoryType,
};

use serde::{Deserialize, Serialize};
use std::time::Instant;

// ── Seed Data ────────────────────────────────────────────────────────────────

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn seed_beliefs() -> Vec<Belief> {
    let now = now_secs();
    vec![
        Belief {
            id: 0,
            content: "The user prefers concise, structured answers over long verbose explanations"
                .into(),
            confidence: 0.92,
            source: BeliefSource::Observation,
            supporting_evidence: vec![
                "User asked for bullet points 3 times".into(),
                "User interrupted long explanation twice".into(),
            ],
            contradicting_evidence: vec![],
            created_at: now,
            updated_at: now,
            update_count: 5,
            abstract_l0: Some("User prefers concise answers".into()),
            overview_l1: None,
        },
        Belief {
            id: 1,
            content: "Rust async code in this project uses tokio runtime with multi-threaded executor".into(),
            confidence: 0.95,
            source: BeliefSource::Inference,
            supporting_evidence: vec!["Cargo.toml has tokio full features".into()],
            contradicting_evidence: vec![],
            created_at: now,
            updated_at: now,
            update_count: 2,
            abstract_l0: Some("Project uses tokio multi-threaded".into()),
            overview_l1: None,
        },
        Belief {
            id: 2,
            content: "Cost optimization matters more than raw performance for this project — the team operates on a fixed cloud budget".into(),
            confidence: 0.78,
            source: BeliefSource::UserProvided,
            supporting_evidence: vec!["User stated budget constraints".into()],
            contradicting_evidence: vec!["Some endpoints need <100ms latency".into()],
            created_at: now,
            updated_at: now,
            update_count: 1,
            abstract_l0: Some("Budget > raw performance".into()),
            overview_l1: None,
        },
        Belief {
            id: 3,
            content: "All public API endpoints must validate input with serde and return structured error responses".into(),
            confidence: 0.88,
            source: BeliefSource::Observation,
            supporting_evidence: vec!["Existing handlers all use serde validation".into()],
            contradicting_evidence: vec![],
            created_at: now,
            updated_at: now,
            update_count: 3,
            abstract_l0: Some("APIs use serde validation".into()),
            overview_l1: None,
        },
        Belief {
            id: 4,
            content: "Tests should use real data structures, not mocks, except for external HTTP calls".into(),
            confidence: 0.85,
            source: BeliefSource::UserProvided,
            supporting_evidence: vec!["User: 'I hate mocks, use real structs'".into()],
            contradicting_evidence: vec![],
            created_at: now,
            updated_at: now,
            update_count: 2,
            abstract_l0: Some("Real data in tests, minimal mocks".into()),
            overview_l1: None,
        },
        Belief {
            id: 5,
            content: "Documentation should include runnable examples and explain trade-offs, not just API signatures".into(),
            confidence: 0.82,
            source: BeliefSource::Reflection,
            supporting_evidence: vec!["Best docs in codebase all have examples".into()],
            contradicting_evidence: vec![],
            created_at: now,
            updated_at: now,
            update_count: 1,
            abstract_l0: Some("Docs need examples + trade-offs".into()),
            overview_l1: None,
        },
        Belief {
            id: 6,
            content: "Security: never log sensitive data (API keys, tokens, passwords) even at debug level".into(),
            confidence: 0.97,
            source: BeliefSource::UserProvided,
            supporting_evidence: vec!["Security audit finding #12".into()],
            contradicting_evidence: vec![],
            created_at: now,
            updated_at: now,
            update_count: 0,
            abstract_l0: Some("Never log secrets".into()),
            overview_l1: None,
        },
        Belief {
            id: 7,
            content: "The team follows trunk-based development: short-lived feature branches, small PRs, CI must pass before merge".into(),
            confidence: 0.90,
            source: BeliefSource::Observation,
            supporting_evidence: vec!["All merged PRs are <500 lines".into()],
            contradicting_evidence: vec![],
            created_at: now,
            updated_at: now,
            update_count: 4,
            abstract_l0: Some("Trunk-based dev, small PRs".into()),
            overview_l1: None,
        },
        Belief {
            id: 8,
            content: "Error handling should use anyhow for applications and thiserror for libraries, with context on every ?".into(),
            confidence: 0.91,
            source: BeliefSource::Inference,
            supporting_evidence: vec![
                "Cargo.toml depends on both".into(),
                "All existing code follows this pattern".into(),
            ],
            contradicting_evidence: vec![],
            created_at: now,
            updated_at: now,
            update_count: 3,
            abstract_l0: Some("anyhow for apps, thiserror for libs".into()),
            overview_l1: None,
        },
        Belief {
            id: 9,
            content: "Concurrent request handling should use tokio::spawn with bounded channels for backpressure, never unbounded".into(),
            confidence: 0.86,
            source: BeliefSource::Reflection,
            supporting_evidence: vec!["Unbounded channel caused OOM in prod last month".into()],
            contradicting_evidence: vec![],
            created_at: now,
            updated_at: now,
            update_count: 1,
            abstract_l0: Some("Bounded channels, never unbounded".into()),
            overview_l1: None,
        },
        // ── Business Beliefs ──
        Belief {
            id: 10,
            content: "Our product is an AI-powered code review platform for enterprise dev teams. Revenue model is per-seat SaaS ($49/seat/month). Current MRR: $12K with 15 paying customers.".into(),
            confidence: 0.98,
            source: BeliefSource::UserProvided,
            supporting_evidence: vec!["User stated product details directly".into()],
            contradicting_evidence: vec![],
            created_at: now,
            updated_at: now,
            update_count: 0,
            abstract_l0: Some("AI code review SaaS, $49/seat, $12K MRR".into()),
            overview_l1: None,
        },
        Belief {
            id: 11,
            content: "Our target market is mid-size engineering teams (20-200 devs) at companies using GitHub or GitLab. Enterprise sales cycle is 2-6 months.".into(),
            confidence: 0.90,
            source: BeliefSource::Observation,
            supporting_evidence: vec!["All 15 customers fit this profile".into(), "Lost 3 deals to long procurement cycles".into()],
            contradicting_evidence: vec!["One customer is a 5-person startup".into()],
            created_at: now,
            updated_at: now,
            update_count: 2,
            abstract_l0: Some("Target: mid-size eng teams, GitHub/GitLab".into()),
            overview_l1: None,
        },
        Belief {
            id: 12,
            content: "Developer-focused content marketing (technical blog posts, open-source tools) drives 70% of our qualified leads. Paid ads have negative ROI so far.".into(),
            confidence: 0.85,
            source: BeliefSource::Observation,
            supporting_evidence: vec!["Analytics show blog → signup funnel".into(), "Ad spend of $3K produced 0 conversions".into()],
            contradicting_evidence: vec![],
            created_at: now,
            updated_at: now,
            update_count: 3,
            abstract_l0: Some("Content marketing works, paid ads don't".into()),
            overview_l1: None,
        },
        Belief {
            id: 13,
            content: "Our competitive advantage is speed (reviews in <30 seconds) and accuracy (92% agreement with human reviewers). Main competitors are CodeRabbit and Sourcery.".into(),
            confidence: 0.88,
            source: BeliefSource::Inference,
            supporting_evidence: vec!["Benchmark against competitors done Q4".into()],
            contradicting_evidence: vec!["CodeRabbit recently improved their speed".into()],
            created_at: now,
            updated_at: now,
            update_count: 1,
            abstract_l0: Some("Speed + accuracy advantage vs CodeRabbit/Sourcery".into()),
            overview_l1: None,
        },
        Belief {
            id: 14,
            content: "Monthly cloud infrastructure costs are $15K. The biggest cost driver is LLM API calls (60% of infra spend). Running our own fine-tuned model could cut costs 40%.".into(),
            confidence: 0.82,
            source: BeliefSource::Reflection,
            supporting_evidence: vec!["AWS billing analysis Q1".into()],
            contradicting_evidence: vec!["Self-hosted model needs GPU infra investment".into()],
            created_at: now,
            updated_at: now,
            update_count: 1,
            abstract_l0: Some("$15K/mo infra, 60% is LLM API costs".into()),
            overview_l1: None,
        },
    ]
}

fn seed_skills() -> Vec<Skill> {
    let now = now_secs();
    let mk = |id: &str, title: &str, principle: &str, conf: f64| -> Skill {
        Skill {
            id: id.into(),
            title: title.into(),
            principle: principle.into(),
            when_to_apply: vec![ApplicabilityCondition {
                predicate: "always".into(),
                args: vec![],
            }],
            level: SkillLevel::General,
            source: SkillSource::Distilled {
                from_experience_ids: vec![0, 1],
                trajectory_type: TrajectoryType::Success,
            },
            confidence: conf,
            usage_count: 10,
            success_count: 8,
            failure_count: 2,
            embedding: None,
            created_at: now,
            last_evolved: now,
            status: SkillStatus::Active,
            bayesian: BayesianConfidence::default(),
            credit_ema: 0.7,
            credit_count: 10,
            last_credit_tick: 0,
            curation: SkillCuration::HumanCurated {
                curator: "benchmark".into(),
                domain: "software-engineering".into(),
                curated_at: now,
            },
            scope: SkillScope::default(),
            delegation_ema: 0.0,
            delegation_count: 0,
            hired_count: 0,
        }
    };

    vec![
        mk(
            "skill-error-handling",
            "Error Handling Best Practices",
            "Always use Result<T,E> with context. Use anyhow::Context for applications, thiserror for library crates. Log errors with tracing at appropriate levels. Never unwrap() in production code — use expect() with descriptive messages only for invariants.",
            0.92,
        ),
        mk(
            "skill-api-design",
            "REST API Design Patterns",
            "Use resource-oriented URLs, proper HTTP methods, and consistent error response format {error, message, details}. Validate all input with serde + custom validators. Version APIs via URL prefix (/v1/). Return appropriate status codes (201 Created, 404 Not Found, 422 Unprocessable).",
            0.88,
        ),
        mk(
            "skill-testing-strategy",
            "Effective Testing Strategy",
            "Write unit tests for pure logic, integration tests for cross-module behavior. Use real data structures, mock only external HTTP. Prefer property-based testing (proptest) for algorithmic code. Test error paths, not just happy paths. Aim for 80% coverage on critical paths.",
            0.85,
        ),
        mk(
            "skill-perf-profiling",
            "Performance Profiling Workflow",
            "Measure before optimizing. Use criterion for microbenchmarks, flamegraph for hotspot detection. Profile with realistic data sizes. Check allocations with DHAT. Optimize the hot path first — 90% of time is spent in 10% of code. Consider algorithmic complexity before micro-optimization.",
            0.80,
        ),
        mk(
            "skill-code-review",
            "Code Review Best Practices",
            "Review in small batches (<400 lines). Check: correctness, edge cases, error handling, naming clarity, test coverage. Ask 'what happens if this fails?' for every external call. Look for missing error context on ?. Verify logging is sufficient for debugging. Check for accidental secret exposure.",
            0.87,
        ),
    ]
}

fn seed_playbooks() -> Vec<Playbook> {
    vec![
        Playbook::new(
            "Debug Systematically",
            "When debugging: 1) Reproduce reliably 2) Isolate to smallest failing case 3) Add tracing/logging at boundaries 4) Form hypothesis 5) Verify fix doesn't break other tests",
            "debug error fix bug wrong result",
        )
        .with_steps(vec![
            Step::llm_step(0, "Reproduce the bug", "Describe minimal reproduction steps", "Bug reproduces consistently"),
            Step::llm_step(1, "Isolate the cause", "Narrow down to specific function/module", "Root cause identified"),
            Step::llm_step(2, "Verify the fix", "Ensure fix doesn't break existing tests", "All tests pass"),
        ]),
        Playbook::new(
            "Refactor Safely",
            "When refactoring: 1) Ensure tests exist for current behavior 2) Make one structural change at a time 3) Run tests after each change 4) Commit after each green state",
            "refactor restructure reorganize clean improve code quality",
        )
        .with_steps(vec![
            Step::llm_step(0, "Verify test coverage", "Check existing tests cover the code to refactor", "Tests exist and pass"),
            Step::llm_step(1, "Extract and simplify", "Make one structural improvement", "Code compiles and tests pass"),
        ]),
        Playbook::new(
            "Optimize Performance",
            "When optimizing: 1) Profile first with criterion/flamegraph 2) Identify hotspot 3) Check algorithmic complexity 4) Benchmark before/after 5) Document the trade-off",
            "performance slow optimize speed latency cache database query",
        )
        .with_steps(vec![
            Step::llm_step(0, "Profile and measure", "Identify the actual bottleneck with data", "Hotspot identified with numbers"),
            Step::llm_step(1, "Implement optimization", "Apply targeted fix to the hotspot", "Benchmark shows improvement"),
        ]),
        Playbook::new(
            "Secure API Endpoints",
            "When securing APIs: 1) Validate all input (serde + custom) 2) Authenticate every request 3) Authorize at resource level 4) Never log secrets 5) Rate limit public endpoints",
            "security secure authentication authorize protect endpoint",
        )
        .with_steps(vec![
            Step::llm_step(0, "Audit input validation", "Check all endpoints validate input", "No raw user input reaches business logic"),
            Step::llm_step(1, "Verify auth + authz", "Ensure auth middleware covers all routes", "All routes require valid credentials"),
        ]),
    ]
}

fn seed_hints() -> Vec<Hint> {
    vec![
        Hint::new(
            "Check for off-by-one errors in loop boundaries — most 'wrong result' bugs in iteration are fencepost errors",
            "loop iteration bug wrong result off by one",
            0.88,
        ),
        Hint::new(
            "When adding caching, always define an invalidation strategy first — cache without invalidation is a bug waiting to happen",
            "cache caching performance optimize speed",
            0.85,
        ),
        Hint::new(
            "For async Rust tests, use #[tokio::test] with a timeout to catch deadlocks: #[tokio::test(flavor = \"multi_thread\")]",
            "test async tokio testing concurrent",
            0.90,
        ),
        Hint::new(
            "Never store secrets in code or config files — use environment variables or a secrets manager. Rotate keys on exposure.",
            "security secret key token password credential",
            0.95,
        ),
        Hint::new(
            "Document the WHY, not the WHAT — code shows what it does, comments should explain why that approach was chosen and what alternatives were rejected",
            "document documentation comment doc explain",
            0.82,
        ),
        Hint::new(
            "When handling concurrent requests, use tokio::sync::Semaphore for rate limiting and bounded channels for backpressure — never unbounded channels in production",
            "concurrent async parallel request handle spawn channel",
            0.87,
        ),
        // ── Business/Marketing Hints ──
        Hint::new(
            "For pricing changes, always grandfather existing customers for at least 6 months — churn from price increases is 3x harder to recover than delayed revenue",
            "pricing price plan tier revenue monetize",
            0.90,
        ),
        Hint::new(
            "Developer marketing: show don't tell. Live demos, open-source side projects, and technical blog posts convert 5x better than feature comparison pages",
            "marketing content blog launch promote growth acquisition",
            0.88,
        ),
        Hint::new(
            "For B2B SaaS: the buying committee has 3-5 people. Write content for each persona: developer (technical depth), engineering manager (team productivity), VP/CTO (ROI/risk)",
            "sales enterprise customer buyer persona B2B",
            0.84,
        ),
        Hint::new(
            "Competitive positioning: don't attack competitors directly. Instead, define the category around your strength (speed+accuracy) so competitors are evaluated by YOUR criteria",
            "competitor competitive positioning differentiation moat",
            0.86,
        ),
    ]
}

fn seed_living_prompt() -> LivingPrompt {
    let mut lp = LivingPrompt::new(
        "You are an HSM-II enhanced agent — a multi-agent system with accumulated knowledge, \
         learned skills, and contextual awareness built from past interactions across coding, \
         business strategy, and marketing domains. \
         Your responses should reflect the specific context, preferences, and patterns \
         you've learned about the user's projects, business, and communication style.",
    );

    // Accumulated insights from past reflections
    lp.add_insight("Users respond better to structured answers with clear headers and bullet points".into());
    lp.add_insight("Code examples should be complete and runnable, not pseudocode fragments".into());
    lp.add_insight("Always mention trade-offs when recommending an approach — the user values honest assessment over cheerleading".into());
    lp.add_insight("When multiple solutions exist, present the recommended one first with brief alternatives".into());
    lp.add_insight("Include error handling in every code example — the user's project treats unwrap() as a code smell".into());
    lp.add_insight("For business advice, ground recommendations in the user's specific constraints: $15K/month cloud budget, 4-person dev team, B2B SaaS product".into());
    lp.add_insight("Marketing content should emphasize technical differentiation — the user's audience is developers and CTOs, not consumers".into());
    lp.add_insight("The user's product is an AI-powered code review platform for enterprise teams — all advice should relate to this context".into());

    // Avoid patterns from past failures (GEPA: negative instructions > positive)
    lp.add_avoid_pattern("Do not give vague answers like 'it depends' without concrete criteria for each case".into());
    lp.add_avoid_pattern("Do not suggest solutions without explaining the trade-offs and failure modes".into());
    lp.add_avoid_pattern("Do not ignore error handling in code examples — always use Result<T,E> not unwrap()".into());
    lp.add_avoid_pattern("Do not recommend enterprise tools (Salesforce, HubSpot Enterprise) — the user is a startup with limited budget".into());
    lp.add_avoid_pattern("Do not suggest 'hire more people' as a solution — the team is deliberately small and wants to stay that way".into());

    lp
}

// ── Prompt Assembly (replicates enhanced_agent.rs:1140-1214) ─────────────

/// Simple keyword matching (same logic as enhanced_agent.rs:1352-1368)
fn keyword_match(text: &str, query: &str) -> bool {
    let text_lower = text.to_lowercase();
    let query_words: Vec<&str> = query.to_lowercase().leak().split_whitespace().collect();
    query_words
        .iter()
        .any(|w| w.len() > 3 && text_lower.contains(*w))
}

fn assemble_enriched_prompt(
    living_prompt: &LivingPrompt,
    beliefs: &[Belief],
    skills: &[Skill],
    playbooks: &[Playbook],
    hints: &[Hint],
    query: &str,
) -> String {
    // 1. Render living prompt (base + insights + avoid patterns)
    let base = living_prompt.render();

    // 2. Match beliefs by keyword (same as enhanced_agent.rs:1171-1179)
    let matched_beliefs: Vec<&Belief> = beliefs
        .iter()
        .filter(|b| keyword_match(&b.content, query))
        .take(3)
        .collect();

    let belief_section = if !matched_beliefs.is_empty() {
        let strs: Vec<String> = matched_beliefs
            .iter()
            .map(|b| format!("- [{:.0}%] {}", b.confidence * 100.0, b.content))
            .collect();
        format!("\n\n## Relevant Beliefs\n{}", strs.join("\n"))
    } else {
        String::new()
    };

    // 3. Match skills (simplified keyword match, not full CASS embedding)
    let matched_skill = skills
        .iter()
        .find(|s| keyword_match(&s.title, query) || keyword_match(&s.principle, query));

    let skill_section = if let Some(s) = matched_skill {
        format!(
            "\n\n## Matched Skill: {} (confidence: {:.2})\n{}",
            s.title, s.confidence, s.principle
        )
    } else {
        String::new()
    };

    // 4. Match playbooks + hints (same as autocontext retrieve_context)
    let matched_pbs: Vec<&Playbook> = playbooks
        .iter()
        .filter(|p| p.matches_scenario(query) > 0.0)
        .take(2)
        .collect();

    let matched_hints: Vec<&Hint> = hints
        .iter()
        .filter(|h| h.matches_trigger(query) > 0.0)
        .take(3)
        .collect();

    let ac_section = if !matched_hints.is_empty() || !matched_pbs.is_empty() {
        let hints_text = matched_hints
            .iter()
            .map(|h| format!("- [hint {:.0}%] {}", h.confidence * 100.0, h.content))
            .collect::<Vec<_>>()
            .join("\n");
        let pb_text = matched_pbs
            .iter()
            .map(|p| {
                format!(
                    "- [playbook {:.0}%] {}: {}",
                    p.quality_score * 100.0,
                    p.name,
                    p.description
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let mut s = "\n\n## AutoContext Guidance\n".to_string();
        if !hints_text.is_empty() {
            s.push_str(&hints_text);
        }
        if !pb_text.is_empty() {
            if !hints_text.is_empty() {
                s.push('\n');
            }
            s.push_str(&pb_text);
        }
        s
    } else {
        String::new()
    };

    // 5. Assemble (same format as enhanced_agent.rs:1206-1214, minus tools section)
    format!("{base}{belief_section}{skill_section}{ac_section}")
}

const BASELINE_SYSTEM_PROMPT: &str =
    "You are a helpful AI assistant. Answer the user's question clearly and helpfully.";

// ── Test Prompts ─────────────────────────────────────────────────────────────

fn test_prompts() -> Vec<(&'static str, &'static str)> {
    vec![
        // ── CODING (4 prompts) ──
        (
            "How should I handle errors in my Rust web API?",
            "CODING: error handling skill + anyhow belief",
        ),
        (
            "My loop is producing wrong results, how do I debug it?",
            "CODING: debug playbook + off-by-one hint",
        ),
        (
            "How do I write good tests for async Rust code?",
            "CODING: testing skill + async hint + no-mocks belief",
        ),
        (
            "How do I handle concurrent requests in Rust?",
            "CODING: async belief + bounded channels hint + tokio belief",
        ),
        // ── MARKETING (3 prompts) ──
        (
            "How should we position our product against CodeRabbit and other AI code review tools?",
            "MARKETING: competitive positioning hint + speed/accuracy belief + competitor belief",
        ),
        (
            "What content marketing strategy should we use to reach more developer teams?",
            "MARKETING: content marketing belief + developer marketing hint + blog insight",
        ),
        (
            "We want to launch a Product Hunt campaign. What should our messaging focus on?",
            "MARKETING: product details belief + show-don't-tell hint + developer audience insight",
        ),
        // ── BUSINESS (3 prompts) ──
        (
            "Should we raise our prices or add a new enterprise tier?",
            "BUSINESS: pricing hint + MRR belief + grandfather existing customers hint",
        ),
        (
            "Our LLM API costs are eating our margins. How should we optimize?",
            "BUSINESS: infra cost belief ($15K, 60% LLM) + fine-tuned model belief + budget constraint",
        ),
        (
            "We have 15 customers and $12K MRR. What should we focus on to reach $50K MRR?",
            "BUSINESS: all business beliefs + target market belief + sales cycle belief + B2B hint",
        ),
        // ── GENERAL (2 prompts — controls) ──
        (
            "Explain the difference between TCP and UDP",
            "CONTROL: general knowledge — should be roughly tied",
        ),
        (
            "What is the trolley problem and why does it matter for AI ethics?",
            "CONTROL: philosophy — should be roughly tied",
        ),
    ]
}

// ── LLM Judge ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DimensionScores {
    relevance: f64,
    specificity: f64,
    depth: f64,
    structure: f64,
    completeness: f64,
}

impl DimensionScores {
    fn average(&self) -> f64 {
        (self.relevance + self.specificity + self.depth + self.structure + self.completeness) / 5.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct JudgeResult {
    baseline_scores: DimensionScores,
    enriched_scores: DimensionScores,
    preferred: String,
    reason: String,
    /// Whether baseline was presented as "X" (true) or "Y" (false) — for bias tracking
    baseline_was_x: bool,
}

async fn judge_responses(
    client: &LlmClient,
    model: &str,
    query: &str,
    baseline_response: &str,
    enriched_response: &str,
) -> anyhow::Result<JudgeResult> {
    // Randomly swap order to mitigate position bias
    let baseline_is_x = rand::random::<bool>();
    let (x_response, y_response) = if baseline_is_x {
        (baseline_response, enriched_response)
    } else {
        (enriched_response, baseline_response)
    };

    let judge_prompt = format!(
        r#"You are an expert evaluator. Score two AI responses to the same question.
Do NOT assume either is better. Judge purely on quality of the answer.

**Question**: {query}

**Response X**:
{x_response}

**Response Y**:
{y_response}

Score each response on these dimensions (integer 0-10):
1. **Relevance**: Does it directly address the question?
2. **Specificity**: Does it give concrete, actionable details (not vague)?
3. **Depth**: Does it show expert-level understanding?
4. **Structure**: Is it well-organized and easy to follow?
5. **Completeness**: Does it cover important aspects without major gaps?

IMPORTANT: Respond ONLY with valid JSON, no other text:
{{"x": {{"relevance": N, "specificity": N, "depth": N, "structure": N, "completeness": N}}, "y": {{"relevance": N, "specificity": N, "depth": N, "structure": N, "completeness": N}}, "preferred": "x" or "y" or "tie", "reason": "one sentence"}}"#
    );

    let request = LlmRequest {
        model: model.to_string(),
        messages: vec![Message::user(judge_prompt)],
        temperature: 0.1, // Low temperature for consistent judging
        max_tokens: Some(500),
        ..Default::default()
    };

    let response = client.chat(request).await?;
    let text = response.content.trim();

    // Try to extract JSON from the response (handle potential markdown wrapping)
    let json_str = if let Some(start) = text.find('{') {
        if let Some(end) = text.rfind('}') {
            &text[start..=end]
        } else {
            text
        }
    } else {
        text
    };

    let parsed: serde_json::Value = serde_json::from_str(json_str)
        .map_err(|e| anyhow::anyhow!("Judge JSON parse error: {} — raw: {}", e, text))?;

    let parse_scores = |key: &str| -> DimensionScores {
        let obj = &parsed[key];
        DimensionScores {
            relevance: obj["relevance"].as_f64().unwrap_or(5.0),
            specificity: obj["specificity"].as_f64().unwrap_or(5.0),
            depth: obj["depth"].as_f64().unwrap_or(5.0),
            structure: obj["structure"].as_f64().unwrap_or(5.0),
            completeness: obj["completeness"].as_f64().unwrap_or(5.0),
        }
    };

    let x_scores = parse_scores("x");
    let y_scores = parse_scores("y");
    let raw_preferred = parsed["preferred"]
        .as_str()
        .unwrap_or("tie")
        .to_lowercase();
    let reason = parsed["reason"]
        .as_str()
        .unwrap_or("No reason provided")
        .to_string();

    // Map X/Y back to baseline/enriched
    let (baseline_scores, enriched_scores) = if baseline_is_x {
        (x_scores, y_scores)
    } else {
        (y_scores, x_scores)
    };

    let preferred = match raw_preferred.as_str() {
        "x" => {
            if baseline_is_x {
                "baseline"
            } else {
                "enriched"
            }
        }
        "y" => {
            if baseline_is_x {
                "enriched"
            } else {
                "baseline"
            }
        }
        _ => "tie",
    }
    .to_string();

    Ok(JudgeResult {
        baseline_scores,
        enriched_scores,
        preferred,
        reason,
        baseline_was_x: baseline_is_x,
    })
}

// ── Results ──────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct BenchmarkEntry {
    index: usize,
    query: String,
    hypothesis: String,
    baseline_response: String,
    enriched_response: String,
    enriched_prompt_preview: String,
    judge: JudgeResult,
    baseline_latency_ms: u64,
    enriched_latency_ms: u64,
}

#[derive(Debug, Serialize)]
struct BenchmarkReport {
    timestamp: String,
    model: String,
    provider: String,
    entries: Vec<BenchmarkEntry>,
    aggregate: AggregateStats,
}

#[derive(Debug, Serialize)]
struct AggregateStats {
    baseline_avg_score: f64,
    enriched_avg_score: f64,
    enriched_wins: usize,
    baseline_wins: usize,
    ties: usize,
    delta: f64,
    enriched_better_pct: f64,
}

fn print_results_table(report: &BenchmarkReport) {
    eprintln!("\n  {}", "=".repeat(80));
    eprintln!(
        "  A/B BENCHMARK: Plain vs HSM-II Enriched | Model: {} | Provider: {}",
        report.model, report.provider
    );
    eprintln!("  {}\n", "=".repeat(80));

    eprintln!(
        "  {:<3} {:<45} {:>6} {:>8} {:>8}",
        "#", "Question", "Base", "Enriched", "Winner"
    );
    eprintln!("  {:-<3} {:-<45} {:-<6} {:-<8} {:-<8}", "", "", "", "", "");

    for e in &report.entries {
        let q = if e.query.len() > 43 {
            format!("{}...", &e.query[..40])
        } else {
            e.query.clone()
        };
        let winner_symbol = match e.judge.preferred.as_str() {
            "enriched" => "✅ HSM-II",
            "baseline" => "❌ Base",
            _ => "➖ Tie",
        };
        eprintln!(
            "  {:<3} {:<45} {:>5.1} {:>7.1}  {}",
            e.index + 1,
            q,
            e.judge.baseline_scores.average(),
            e.judge.enriched_scores.average(),
            winner_symbol,
        );
    }

    let agg = &report.aggregate;
    eprintln!("\n  {:-<80}", "");
    eprintln!(
        "  AGGREGATE: Baseline avg {:.1} | Enriched avg {:.1} | Delta: {:+.1}",
        agg.baseline_avg_score, agg.enriched_avg_score, agg.delta,
    );
    eprintln!(
        "  WINS: Enriched {}/{} ({:.0}%) | Baseline {}/{} | Ties {}",
        agg.enriched_wins,
        report.entries.len(),
        agg.enriched_better_pct,
        agg.baseline_wins,
        report.entries.len(),
        agg.ties,
    );
    eprintln!();

    // Print per-question judge reasoning
    eprintln!("  JUDGE REASONING:");
    for e in &report.entries {
        let q = if e.query.len() > 50 {
            format!("{}...", &e.query[..47])
        } else {
            e.query.clone()
        };
        eprintln!("    Q{}: {} → {}", e.index + 1, q, e.judge.reason);
    }
    eprintln!();
}

async fn save_results(report: &BenchmarkReport) {
    let dir = dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".hsmii")
        .join("benchmarks");
    let _ = tokio::fs::create_dir_all(&dir).await;
    let filename = format!("ab_{}.json", report.timestamp.replace([':', ' ', '-'], "_"));
    let path = dir.join(&filename);
    if let Ok(json) = serde_json::to_string_pretty(report) {
        if let Ok(()) = tokio::fs::write(&path, json).await {
            eprintln!("  📁 Full results saved to: {}", path.display());
        }
    }
}

// ── Main Test ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn ab_benchmark_plain_vs_enriched() {
    // Initialize LLM client from env
    let client = match LlmClient::new() {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "\n  ⚠️  Skipping A/B benchmark: no LLM provider configured.\n  \
                 Set ANTHROPIC_API_KEY, OPENAI_API_KEY, or OLLAMA_URL to run.\n  \
                 Error: {}\n",
                e
            );
            return;
        }
    };

    // Determine provider first, then select appropriate model default
    let provider = if std::env::var("ANTHROPIC_API_KEY").is_ok() {
        "anthropic"
    } else if std::env::var("OPENAI_API_KEY").is_ok() {
        "openai"
    } else {
        "ollama"
    };

    let model = std::env::var("DEFAULT_LLM_MODEL").unwrap_or_else(|_| {
        match provider {
            "anthropic" => "claude-sonnet-4-20250514".to_string(),
            "openai" => "gpt-4o-mini".to_string(),
            _ => hyper_stigmergy::ollama_client::resolve_model_from_env("llama3.2"),
        }
    });

    eprintln!(
        "\n  🧪 A/B Benchmark starting | Model: {} | Provider: {}\n",
        model, provider
    );

    // Seed HSM-II context
    let beliefs = seed_beliefs();
    let skills = seed_skills();
    let playbooks = seed_playbooks();
    let hints = seed_hints();
    let living_prompt = seed_living_prompt();

    let prompts = test_prompts();
    let mut entries = Vec::new();

    for (i, (query, hypothesis)) in prompts.iter().enumerate() {
        eprintln!("  [{}/{}] {}", i + 1, prompts.len(), query);

        // ── Arm A: Baseline ──
        let start_a = Instant::now();
        let request_a = LlmRequest {
            model: model.clone(),
            messages: vec![
                Message::system(BASELINE_SYSTEM_PROMPT),
                Message::user(*query),
            ],
            temperature: 0.7,
            max_tokens: Some(1500),
            ..Default::default()
        };
        let response_a = match client.chat(request_a).await {
            Ok(r) => r,
            Err(e) => {
                eprintln!("    ❌ Baseline call failed: {}", e);
                continue;
            }
        };
        let latency_a = start_a.elapsed().as_millis() as u64;

        // ── Arm B: Enriched ──
        let enriched_system =
            assemble_enriched_prompt(&living_prompt, &beliefs, &skills, &playbooks, &hints, query);

        let start_b = Instant::now();
        let request_b = LlmRequest {
            model: model.clone(),
            messages: vec![Message::system(&enriched_system), Message::user(*query)],
            temperature: 0.7,
            max_tokens: Some(1500),
            ..Default::default()
        };
        let response_b = match client.chat(request_b).await {
            Ok(r) => r,
            Err(e) => {
                eprintln!("    ❌ Enriched call failed: {}", e);
                continue;
            }
        };
        let latency_b = start_b.elapsed().as_millis() as u64;

        // ── Judge ──
        let judge = match judge_responses(
            &client,
            &model,
            query,
            &response_a.content,
            &response_b.content,
        )
        .await
        {
            Ok(j) => j,
            Err(e) => {
                eprintln!("    ⚠️  Judge failed: {} — scoring as tie", e);
                JudgeResult {
                    baseline_scores: DimensionScores {
                        relevance: 5.0,
                        specificity: 5.0,
                        depth: 5.0,
                        structure: 5.0,
                        completeness: 5.0,
                    },
                    enriched_scores: DimensionScores {
                        relevance: 5.0,
                        specificity: 5.0,
                        depth: 5.0,
                        structure: 5.0,
                        completeness: 5.0,
                    },
                    preferred: "tie".into(),
                    reason: format!("Judge error: {}", e),
                    baseline_was_x: true,
                }
            }
        };

        let winner_tag = match judge.preferred.as_str() {
            "enriched" => "✅",
            "baseline" => "❌",
            _ => "➖",
        };
        eprintln!(
            "    {} Base: {:.1} | Enriched: {:.1} | {} | Latency: {}ms vs {}ms",
            winner_tag,
            judge.baseline_scores.average(),
            judge.enriched_scores.average(),
            judge.preferred,
            latency_a,
            latency_b,
        );

        let preview = if enriched_system.len() > 500 {
            format!("{}...[{} chars total]", &enriched_system[..500], enriched_system.len())
        } else {
            enriched_system.clone()
        };

        entries.push(BenchmarkEntry {
            index: i,
            query: query.to_string(),
            hypothesis: hypothesis.to_string(),
            baseline_response: response_a.content.clone(),
            enriched_response: response_b.content.clone(),
            enriched_prompt_preview: preview,
            judge,
            baseline_latency_ms: latency_a,
            enriched_latency_ms: latency_b,
        });
    }

    // ── Aggregate ──
    let total = entries.len() as f64;
    let baseline_avg = entries
        .iter()
        .map(|e| e.judge.baseline_scores.average())
        .sum::<f64>()
        / total;
    let enriched_avg = entries
        .iter()
        .map(|e| e.judge.enriched_scores.average())
        .sum::<f64>()
        / total;
    let enriched_wins = entries
        .iter()
        .filter(|e| e.judge.preferred == "enriched")
        .count();
    let baseline_wins = entries
        .iter()
        .filter(|e| e.judge.preferred == "baseline")
        .count();
    let ties = entries
        .iter()
        .filter(|e| e.judge.preferred == "tie")
        .count();

    let report = BenchmarkReport {
        timestamp: chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        model: model.clone(),
        provider: provider.to_string(),
        entries,
        aggregate: AggregateStats {
            baseline_avg_score: baseline_avg,
            enriched_avg_score: enriched_avg,
            enriched_wins,
            baseline_wins,
            ties,
            delta: enriched_avg - baseline_avg,
            enriched_better_pct: (enriched_wins as f64 / total) * 100.0,
        },
    };

    print_results_table(&report);
    save_results(&report).await;

    // Assert enriched doesn't catastrophically fail
    assert!(
        report.aggregate.enriched_avg_score >= 3.0,
        "Enriched responses scored critically low ({:.1}), something is broken",
        report.aggregate.enriched_avg_score,
    );

    eprintln!("  🏁 A/B Benchmark complete.\n");
}
