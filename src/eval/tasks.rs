//! Evaluation task suite — 20 multi-session tasks across domains.
//!
//! Each task has multiple turns that simulate a real multi-session workflow.
//! Tasks are designed so persistent memory, context ranking, and reputation
//! routing should measurably help.

use serde::{Deserialize, Serialize};

/// A single conversational turn within a task
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Turn {
    /// The user message for this turn
    pub user: String,
    /// Keywords that a good answer should contain (for automated scoring)
    pub expected_keywords: Vec<String>,
    /// Session number — turns with different session IDs simulate process restarts
    pub session: u32,
    /// Whether this turn requires recall of earlier sessions
    pub requires_recall: bool,
    /// Optional hint about which domain this belongs to (for context ranking)
    pub domain: Option<String>,
    /// If set, response should contain a JSON tool call with this `tool` name (see `judges::parse_tool_json`).
    pub expected_tool: Option<String>,
    /// Required keys in the `parameters` object of the tool JSON (subset check).
    pub expected_arg_keys: Vec<String>,
}

/// A complete evaluation task
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EvalTask {
    /// Unique task identifier
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// Domain category
    pub domain: String,
    /// Difficulty (0-1)
    pub difficulty: f64,
    /// The ordered turns
    pub turns: Vec<Turn>,
    /// What this task tests (for reporting)
    pub tests: Vec<String>,
}

/// Load the full evaluation suite (20 tasks)
pub fn load_eval_suite() -> Vec<EvalTask> {
    vec![
        // ═══════════════════════════════════════════════════════════════════
        // DOMAIN 1: Software Engineering (5 tasks)
        // ═══════════════════════════════════════════════════════════════════
        EvalTask {
            id: "se-01".into(),
            name: "Iterative API Design".into(),
            domain: "software_engineering".into(),
            difficulty: 0.6,
            tests: vec!["memory_recall".into(), "context_ranking".into(), "tool_routing".into()],
            turns: vec![
                Turn {
                    user: "Design a REST API for a task management system. I need endpoints for tasks, projects, and users. Include authentication.".into(),
                    expected_keywords: vec!["POST".into(), "GET".into(), "JWT".into(), "bearer".into(), "task".into(), "project".into()],
                    session: 1, requires_recall: false, domain: Some("api_design".into()), expected_tool: None, expected_arg_keys: vec![],
                },
                Turn {
                    user: "Add pagination and filtering to the task list endpoint. Also add rate limiting.".into(),
                    expected_keywords: vec!["offset".into(), "limit".into(), "filter".into(), "rate".into(), "429".into()],
                    session: 1, requires_recall: true, domain: Some("api_design".into()), expected_tool: None, expected_arg_keys: vec![],
                },
                Turn {
                    user: "I'm back. What API did we design last time? I need to add webhook support for task state changes.".into(),
                    expected_keywords: vec!["webhook".into(), "callback".into(), "task".into(), "event".into()],
                    session: 2, requires_recall: true, domain: Some("api_design".into()), expected_tool: None, expected_arg_keys: vec![],
                },
                Turn {
                    user: "Now write the OpenAPI spec for the endpoints we discussed across both sessions.".into(),
                    expected_keywords: vec!["openapi".into(), "paths".into(), "schema".into(), "webhook".into(), "pagination".into()],
                    session: 2, requires_recall: true, domain: Some("api_design".into()), expected_tool: None, expected_arg_keys: vec![],
                },
            ],
        },
        EvalTask {
            id: "se-02".into(),
            name: "Debug Across Sessions".into(),
            domain: "software_engineering".into(),
            difficulty: 0.7,
            tests: vec!["memory_recall".into(), "reasoning".into()],
            turns: vec![
                Turn {
                    user: "My Python web app crashes intermittently with 'ConnectionResetError' when handling concurrent requests. I'm using Flask with SQLAlchemy and PostgreSQL. About 50 req/sec.".into(),
                    expected_keywords: vec!["connection pool".into(), "thread".into(), "SQLAlchemy".into()],
                    session: 1, requires_recall: false, domain: Some("debugging".into()), expected_tool: None, expected_arg_keys: vec![],
                },
                Turn {
                    user: "I tried increasing pool_size to 20 but now I get 'too many connections'. My PostgreSQL max_connections is 100.".into(),
                    expected_keywords: vec!["pgbouncer".into(), "pool".into(), "overflow".into(), "connection".into()],
                    session: 1, requires_recall: true, domain: Some("debugging".into()), expected_tool: None, expected_arg_keys: vec![],
                },
                Turn {
                    user: "Picking up from last session — the connection pooling issue. I switched to pgbouncer but now queries are slower. What's happening?".into(),
                    expected_keywords: vec!["transaction".into(), "prepared".into(), "statement".into(), "mode".into()],
                    session: 2, requires_recall: true, domain: Some("debugging".into()), expected_tool: None, expected_arg_keys: vec![],
                },
            ],
        },
        EvalTask {
            id: "se-03".into(),
            name: "Architecture Evolution".into(),
            domain: "software_engineering".into(),
            difficulty: 0.8,
            tests: vec!["memory_recall".into(), "multi_perspective".into()],
            turns: vec![
                Turn {
                    user: "I'm building a real-time collaborative document editor. Currently monolithic Django app. 1000 concurrent users. Should I go microservices?".into(),
                    expected_keywords: vec!["websocket".into(), "CRDT".into(), "operational transform".into()],
                    session: 1, requires_recall: false, domain: Some("architecture".into()), expected_tool: None, expected_arg_keys: vec![],
                },
                Turn {
                    user: "Good points on CRDTs. But my team is 3 people. Is the complexity worth it vs just using OT?".into(),
                    expected_keywords: vec!["team size".into(), "complexity".into(), "trade".into()],
                    session: 1, requires_recall: true, domain: Some("architecture".into()), expected_tool: None, expected_arg_keys: vec![],
                },
                Turn {
                    user: "We went with CRDTs as you suggested. Now we need offline support. How does that change the architecture we discussed?".into(),
                    expected_keywords: vec!["offline".into(), "sync".into(), "merge".into(), "conflict".into()],
                    session: 2, requires_recall: true, domain: Some("architecture".into()), expected_tool: None, expected_arg_keys: vec![],
                },
                Turn {
                    user: "Summarize our full architecture decisions across both sessions and the trade-offs we considered.".into(),
                    expected_keywords: vec!["CRDT".into(), "offline".into(), "monolith".into(), "trade".into()],
                    session: 2, requires_recall: true, domain: Some("architecture".into()), expected_tool: None, expected_arg_keys: vec![],
                },
            ],
        },
        EvalTask {
            id: "se-04".into(),
            name: "Refactoring Plan".into(),
            domain: "software_engineering".into(),
            difficulty: 0.5,
            tests: vec!["memory_recall".into(), "context_ranking".into()],
            turns: vec![
                Turn {
                    user: "I have a 5000-line God class in Java called OrderProcessor. It handles validation, pricing, inventory, shipping, and notifications. How should I break it up?".into(),
                    expected_keywords: vec!["single responsibility".into(), "extract".into(), "class".into(), "service".into()],
                    session: 1, requires_recall: false, domain: Some("refactoring".into()), expected_tool: None, expected_arg_keys: vec![],
                },
                Turn {
                    user: "Continuing our refactoring of OrderProcessor — I've extracted ValidationService and PricingService. The remaining 3000 lines handle inventory with complex warehouse logic. Next steps?".into(),
                    expected_keywords: vec!["inventory".into(), "warehouse".into(), "strategy".into(), "pattern".into()],
                    session: 2, requires_recall: true, domain: Some("refactoring".into()), expected_tool: None, expected_arg_keys: vec![],
                },
            ],
        },
        EvalTask {
            id: "se-05".into(),
            name: "Test Strategy Across Sessions".into(),
            domain: "software_engineering".into(),
            difficulty: 0.6,
            tests: vec!["memory_recall".into()],
            turns: vec![
                Turn {
                    user: "I'm building a payment processing system in Node.js. What's a good testing strategy? I need unit, integration, and E2E coverage.".into(),
                    expected_keywords: vec!["mock".into(), "stripe".into(), "integration".into(), "idempotent".into()],
                    session: 1, requires_recall: false, domain: Some("testing".into()), expected_tool: None, expected_arg_keys: vec![],
                },
                Turn {
                    user: "For the payment system's test strategy we discussed — I've implemented unit tests. Now I need to set up the integration tests with a Stripe test environment. How?".into(),
                    expected_keywords: vec!["test key".into(), "webhook".into(), "sandbox".into(), "fixture".into()],
                    session: 2, requires_recall: true, domain: Some("testing".into()), expected_tool: None, expected_arg_keys: vec![],
                },
                Turn {
                    user: "Great. Now review our complete test strategy from both sessions and identify any gaps in coverage. On the last line output ONLY JSON: {\"tool\":\"run_tests\",\"parameters\":{\"scope\":\"integration\"}}".into(),
                    expected_keywords: vec!["gap".into(), "coverage".into(), "edge case".into(), "payment".into()],
                    session: 2, requires_recall: true, domain: Some("testing".into()),
                    expected_tool: Some("run_tests".into()),
                    expected_arg_keys: vec!["scope".into()],
                },
            ],
        },

        // ═══════════════════════════════════════════════════════════════════
        // DOMAIN 2: Data Science / ML (5 tasks)
        // ═══════════════════════════════════════════════════════════════════
        EvalTask {
            id: "ds-01".into(),
            name: "ML Pipeline Iteration".into(),
            domain: "data_science".into(),
            difficulty: 0.7,
            tests: vec!["memory_recall".into(), "context_ranking".into()],
            turns: vec![
                Turn {
                    user: "I'm building a churn prediction model for a SaaS product. 50K users, 200 features, 5% churn rate. Where do I start?".into(),
                    expected_keywords: vec!["imbalanced".into(), "SMOTE".into(), "feature".into(), "baseline".into()],
                    session: 1, requires_recall: false, domain: Some("ml_pipeline".into()), expected_tool: None, expected_arg_keys: vec![],
                },
                Turn {
                    user: "I tried logistic regression as baseline — 85% accuracy but only 20% recall on churners. The class imbalance is killing me.".into(),
                    expected_keywords: vec!["recall".into(), "precision".into(), "threshold".into(), "weight".into(), "F1".into()],
                    session: 1, requires_recall: true, domain: Some("ml_pipeline".into()), expected_tool: None, expected_arg_keys: vec![],
                },
                Turn {
                    user: "Back again. Last session we discussed the churn model with class imbalance issues. I tried SMOTE and got recall to 60%. But precision dropped to 40%. What now?".into(),
                    expected_keywords: vec!["ensemble".into(), "XGBoost".into(), "calibrat".into(), "threshold".into()],
                    session: 2, requires_recall: true, domain: Some("ml_pipeline".into()), expected_tool: None, expected_arg_keys: vec![],
                },
            ],
        },
        EvalTask {
            id: "ds-02".into(),
            name: "Feature Engineering Across Sessions".into(),
            domain: "data_science".into(),
            difficulty: 0.6,
            tests: vec!["memory_recall".into()],
            turns: vec![
                Turn {
                    user: "I have user clickstream data — page views, time on page, click sequences. How should I engineer features for a recommendation model?".into(),
                    expected_keywords: vec!["session".into(), "sequence".into(), "embedding".into(), "temporal".into()],
                    session: 1, requires_recall: false, domain: Some("feature_engineering".into()), expected_tool: None, expected_arg_keys: vec![],
                },
                Turn {
                    user: "Continuing our feature engineering work on clickstream data — I've built session-level aggregates. Now I need to capture sequential patterns. Best approach?".into(),
                    expected_keywords: vec!["n-gram".into(), "RNN".into(), "attention".into(), "window".into()],
                    session: 2, requires_recall: true, domain: Some("feature_engineering".into()), expected_tool: None, expected_arg_keys: vec![],
                },
            ],
        },
        EvalTask {
            id: "ds-03".into(),
            name: "Model Deployment Strategy".into(),
            domain: "data_science".into(),
            difficulty: 0.7,
            tests: vec!["memory_recall".into(), "multi_perspective".into()],
            turns: vec![
                Turn {
                    user: "I have a trained XGBoost model for fraud detection (99.5% accuracy, 80% recall). How should I deploy it? Needs <100ms latency, 10K req/sec.".into(),
                    expected_keywords: vec!["serving".into(), "batch".into(), "real-time".into(), "container".into()],
                    session: 1, requires_recall: false, domain: Some("deployment".into()), expected_tool: None, expected_arg_keys: vec![],
                },
                Turn {
                    user: "Following up on our fraud model deployment discussion. I've containerized it with FastAPI. But latency is 200ms. How to get under 100ms?".into(),
                    expected_keywords: vec!["ONNX".into(), "cache".into(), "precompute".into(), "async".into()],
                    session: 2, requires_recall: true, domain: Some("deployment".into()), expected_tool: None, expected_arg_keys: vec![],
                },
                Turn {
                    user: "Got latency to 50ms with ONNX as you suggested. Now I need A/B testing and model monitoring. How does this fit with what we've built?".into(),
                    expected_keywords: vec!["shadow".into(), "canary".into(), "drift".into(), "monitor".into()],
                    session: 2, requires_recall: true, domain: Some("deployment".into()), expected_tool: None, expected_arg_keys: vec![],
                },
            ],
        },
        EvalTask {
            id: "ds-04".into(),
            name: "Data Pipeline Design".into(),
            domain: "data_science".into(),
            difficulty: 0.5,
            tests: vec!["memory_recall".into(), "context_ranking".into()],
            turns: vec![
                Turn {
                    user: "I need to build an ETL pipeline that ingests 10M events/day from Kafka, transforms them, and loads into a data warehouse. What architecture?".into(),
                    expected_keywords: vec!["Spark".into(), "Flink".into(), "warehouse".into(), "partition".into()],
                    session: 1, requires_recall: false, domain: Some("data_pipeline".into()), expected_tool: None, expected_arg_keys: vec![],
                },
                Turn {
                    user: "For our ETL pipeline discussion — I chose Spark Structured Streaming. Now I need exactly-once semantics and late data handling. How?".into(),
                    expected_keywords: vec!["checkpoint".into(), "watermark".into(), "exactly-once".into(), "idempotent".into()],
                    session: 2, requires_recall: true, domain: Some("data_pipeline".into()), expected_tool: None, expected_arg_keys: vec![],
                },
            ],
        },
        EvalTask {
            id: "ds-05".into(),
            name: "Experiment Tracking".into(),
            domain: "data_science".into(),
            difficulty: 0.4,
            tests: vec!["memory_recall".into()],
            turns: vec![
                Turn {
                    user: "I'm running 50+ ML experiments weekly and losing track. How should I organize experiment tracking for a team of 5 data scientists?".into(),
                    expected_keywords: vec!["MLflow".into(), "version".into(), "metric".into(), "artifact".into()],
                    session: 1, requires_recall: false, domain: Some("experiment_tracking".into()), expected_tool: None, expected_arg_keys: vec![],
                },
                Turn {
                    user: "We set up MLflow as you recommended. Now we need to track data versions alongside model versions. Our earlier setup only tracks model artifacts.".into(),
                    expected_keywords: vec!["DVC".into(), "data version".into(), "lineage".into(), "reproducib".into()],
                    session: 2, requires_recall: true, domain: Some("experiment_tracking".into()), expected_tool: None, expected_arg_keys: vec![],
                },
            ],
        },

        // ═══════════════════════════════════════════════════════════════════
        // DOMAIN 3: Business Analysis (5 tasks)
        // ═══════════════════════════════════════════════════════════════════
        EvalTask {
            id: "biz-01".into(),
            name: "Market Entry Strategy".into(),
            domain: "business".into(),
            difficulty: 0.7,
            tests: vec!["multi_perspective".into(), "memory_recall".into()],
            turns: vec![
                Turn {
                    user: "We're a B2B SaaS startup ($2M ARR) considering expanding from the US into the European market. What should we think about?".into(),
                    expected_keywords: vec!["GDPR".into(), "localization".into(), "pricing".into(), "partner".into()],
                    session: 1, requires_recall: false, domain: Some("strategy".into()), expected_tool: None, expected_arg_keys: vec![],
                },
                Turn {
                    user: "Good analysis. Our product is a developer tool. GDPR compliance is handled. Main question: should we start with UK, Germany, or Nordics?".into(),
                    expected_keywords: vec!["developer".into(), "English".into(), "market size".into(), "adoption".into()],
                    session: 1, requires_recall: true, domain: Some("strategy".into()), expected_tool: None, expected_arg_keys: vec![],
                },
                Turn {
                    user: "Following up on our EU expansion discussion. We chose UK as entry market. Now I need a go-to-market plan. Budget is $500K for year one.".into(),
                    expected_keywords: vec!["content".into(), "conference".into(), "hire".into(), "channel".into(), "metric".into()],
                    session: 2, requires_recall: true, domain: Some("strategy".into()), expected_tool: None, expected_arg_keys: vec![],
                },
            ],
        },
        EvalTask {
            id: "biz-02".into(),
            name: "Pricing Model Evolution".into(),
            domain: "business".into(),
            difficulty: 0.6,
            tests: vec!["memory_recall".into(), "multi_perspective".into()],
            turns: vec![
                Turn {
                    user: "We sell an API product. Currently flat $99/mo. Usage varies 100x between customers. Some use 100 calls/day, others 100K. How should we restructure pricing?".into(),
                    expected_keywords: vec!["usage-based".into(), "tier".into(), "metered".into(), "overage".into()],
                    session: 1, requires_recall: false, domain: Some("pricing".into()), expected_tool: None, expected_arg_keys: vec![],
                },
                Turn {
                    user: "Revisiting our API pricing discussion. We've drafted 3 tiers but enterprise customers want committed-use discounts. How do we structure that without cannibalizing?".into(),
                    expected_keywords: vec!["commit".into(), "discount".into(), "annual".into(), "minimum".into()],
                    session: 2, requires_recall: true, domain: Some("pricing".into()), expected_tool: None, expected_arg_keys: vec![],
                },
            ],
        },
        EvalTask {
            id: "biz-03".into(),
            name: "Competitive Analysis".into(),
            domain: "business".into(),
            difficulty: 0.5,
            tests: vec!["memory_recall".into(), "context_ranking".into()],
            turns: vec![
                Turn {
                    user: "My product is a project management tool. Main competitors are Linear, Jira, and Asana. How do I differentiate?".into(),
                    expected_keywords: vec!["niche".into(), "workflow".into(), "integration".into(), "pain point".into()],
                    session: 1, requires_recall: false, domain: Some("competitive".into()), expected_tool: None, expected_arg_keys: vec![],
                },
                Turn {
                    user: "Building on our competitive analysis — we identified developer workflows as our niche. Now I need a feature comparison matrix and messaging framework.".into(),
                    expected_keywords: vec!["matrix".into(), "message".into(), "developer".into(), "position".into()],
                    session: 2, requires_recall: true, domain: Some("competitive".into()), expected_tool: None, expected_arg_keys: vec![],
                },
            ],
        },
        EvalTask {
            id: "biz-04".into(),
            name: "Hiring Plan".into(),
            domain: "business".into(),
            difficulty: 0.5,
            tests: vec!["memory_recall".into()],
            turns: vec![
                Turn {
                    user: "We just raised Series A ($5M). Team is 8 people (4 eng, 1 design, 1 PM, 2 founders). We need to grow to 20 in 12 months. Hiring priorities?".into(),
                    expected_keywords: vec!["senior".into(), "sales".into(), "marketing".into(), "runway".into()],
                    session: 1, requires_recall: false, domain: Some("hiring".into()), expected_tool: None, expected_arg_keys: vec![],
                },
                Turn {
                    user: "Following our hiring plan discussion — Q1 hires are done (2 engineers, 1 sales). Now planning Q2. Our pipeline conversion from MQL to customer is only 3%. Do we still hire more sales or fix the funnel first?".into(),
                    expected_keywords: vec!["conversion".into(), "funnel".into(), "qualify".into(), "enable".into()],
                    session: 2, requires_recall: true, domain: Some("hiring".into()), expected_tool: None, expected_arg_keys: vec![],
                },
            ],
        },
        EvalTask {
            id: "biz-05".into(),
            name: "Product Roadmap Alignment".into(),
            domain: "business".into(),
            difficulty: 0.6,
            tests: vec!["memory_recall".into(), "multi_perspective".into()],
            turns: vec![
                Turn {
                    user: "My CEO wants us to build an AI feature. My top customers want better reporting. My engineers want to pay down tech debt. I'm the PM — how do I prioritize?".into(),
                    expected_keywords: vec!["framework".into(), "impact".into(), "effort".into(), "stakeholder".into()],
                    session: 1, requires_recall: false, domain: Some("product".into()), expected_tool: None, expected_arg_keys: vec![],
                },
                Turn {
                    user: "Based on our prioritization discussion, I chose RICE and the AI feature won. But 2 engineers threatened to quit if we don't address tech debt. New information — what now?".into(),
                    expected_keywords: vec!["allocat".into(), "split".into(), "retention".into(), "debt".into(), "parallel".into()],
                    session: 2, requires_recall: true, domain: Some("product".into()), expected_tool: None, expected_arg_keys: vec![],
                },
            ],
        },

        // ═══════════════════════════════════════════════════════════════════
        // DOMAIN 4: Research & Writing (3 tasks)
        // ═══════════════════════════════════════════════════════════════════
        EvalTask {
            id: "rw-01".into(),
            name: "Research Paper Iteration".into(),
            domain: "research".into(),
            difficulty: 0.8,
            tests: vec!["memory_recall".into(), "context_ranking".into()],
            turns: vec![
                Turn {
                    user: "I'm writing a paper on the effectiveness of retrieval-augmented generation. I need a literature review covering key papers from 2023-2024.".into(),
                    expected_keywords: vec!["RAG".into(), "retrieval".into(), "chunk".into(), "benchmark".into()],
                    session: 1, requires_recall: false, domain: Some("research".into()), expected_tool: None, expected_arg_keys: vec![],
                },
                Turn {
                    user: "Good literature review. Now I need to design experiments comparing RAG vs fine-tuning vs long-context models on factual QA. What methodology?".into(),
                    expected_keywords: vec!["baseline".into(), "metric".into(), "dataset".into(), "statistical".into()],
                    session: 1, requires_recall: true, domain: Some("research".into()), expected_tool: None, expected_arg_keys: vec![],
                },
                Turn {
                    user: "Continuing our RAG paper — experiments are done. RAG wins on factual accuracy (82% vs 71% fine-tuning) but loses on fluency. Help me write the discussion section incorporating our earlier lit review.".into(),
                    expected_keywords: vec!["trade-off".into(), "accuracy".into(), "fluency".into(), "limitation".into()],
                    session: 2, requires_recall: true, domain: Some("research".into()), expected_tool: None, expected_arg_keys: vec![],
                },
            ],
        },
        EvalTask {
            id: "rw-02".into(),
            name: "Technical Blog Series".into(),
            domain: "research".into(),
            difficulty: 0.5,
            tests: vec!["memory_recall".into()],
            turns: vec![
                Turn {
                    user: "I want to write a 3-part blog series on building production ML systems. Part 1 should cover data pipelines. Outline it.".into(),
                    expected_keywords: vec!["pipeline".into(), "quality".into(), "monitor".into(), "section".into()],
                    session: 1, requires_recall: false, domain: Some("writing".into()), expected_tool: None, expected_arg_keys: vec![],
                },
                Turn {
                    user: "Part 1 is published. Now outline Part 2 on model serving. It should reference concepts from Part 1 (data pipelines, quality checks).".into(),
                    expected_keywords: vec!["serving".into(), "pipeline".into(), "latency".into(), "Part 1".into()],
                    session: 2, requires_recall: true, domain: Some("writing".into()), expected_tool: None, expected_arg_keys: vec![],
                },
            ],
        },

        // ═══════════════════════════════════════════════════════════════════
        // DOMAIN 5: Cross-Domain Stress Tests (2 tasks)
        // ═══════════════════════════════════════════════════════════════════
        EvalTask {
            id: "stress-01".into(),
            name: "Long-Context Recall".into(),
            domain: "stress_test".into(),
            difficulty: 0.9,
            tests: vec!["memory_recall".into(), "context_ranking".into(), "multi_perspective".into(), "council_comparison".into()],
            turns: vec![
                Turn {
                    user: "I'm building a healthcare appointment scheduling system. Requirements: HIPAA compliance, multi-timezone, 50 clinics, SMS reminders, insurance verification.".into(),
                    expected_keywords: vec!["HIPAA".into(), "encrypt".into(), "timezone".into(), "remind".into()],
                    session: 1, requires_recall: false, domain: Some("healthcare_tech".into()), expected_tool: None, expected_arg_keys: vec![],
                },
                Turn {
                    user: "Good start. Add: each clinic has different operating hours, some providers work across clinics, patients can have multiple insurance plans. Update the system design.".into(),
                    expected_keywords: vec!["schedule".into(), "provider".into(), "insurance".into(), "conflict".into()],
                    session: 1, requires_recall: true, domain: Some("healthcare_tech".into()), expected_tool: None, expected_arg_keys: vec![],
                },
                Turn {
                    user: "Session 3 now. Recall the healthcare scheduling system with HIPAA, multi-timezone, multi-clinic providers, and multiple insurance plans. I need the database schema.".into(),
                    expected_keywords: vec!["patient".into(), "provider".into(), "clinic".into(), "appointment".into(), "insurance".into()],
                    session: 3, requires_recall: true, domain: Some("healthcare_tech".into()), expected_tool: None, expected_arg_keys: vec![],
                },
                Turn {
                    user: "Based on everything we've discussed across all sessions, write the API contract for the booking endpoint. Include all constraints we identified.".into(),
                    expected_keywords: vec!["HIPAA".into(), "timezone".into(), "insurance".into(), "conflict".into(), "endpoint".into()],
                    session: 3, requires_recall: true, domain: Some("healthcare_tech".into()), expected_tool: None, expected_arg_keys: vec![],
                },
            ],
        },
        EvalTask {
            id: "stress-02".into(),
            name: "Evolving Requirements".into(),
            domain: "stress_test".into(),
            difficulty: 0.8,
            tests: vec!["memory_recall".into(), "reasoning".into(), "council_comparison".into()],
            turns: vec![
                Turn {
                    user: "Build me a notification system. Start simple: email notifications when a user gets a new message.".into(),
                    expected_keywords: vec!["email".into(), "queue".into(), "template".into(), "async".into()],
                    session: 1, requires_recall: false, domain: Some("notification".into()), expected_tool: None, expected_arg_keys: vec![],
                },
                Turn {
                    user: "Add push notifications (iOS + Android) and SMS. Keep backward compatibility with what we designed for email.".into(),
                    expected_keywords: vec!["channel".into(), "provider".into(), "abstract".into(), "preference".into()],
                    session: 1, requires_recall: true, domain: Some("notification".into()), expected_tool: None, expected_arg_keys: vec![],
                },
                Turn {
                    user: "New session. The notification system we built now needs: batching (digest mode), quiet hours, channel preferences per user, and priority overrides. How does this extend our existing design?".into(),
                    expected_keywords: vec!["digest".into(), "quiet".into(), "preference".into(), "priority".into(), "override".into()],
                    session: 2, requires_recall: true, domain: Some("notification".into()), expected_tool: None, expected_arg_keys: vec![],
                },
                Turn {
                    user: "Final requirement: international users need locale-aware templates and timezone-aware quiet hours. Summarize the complete system from session 1 through now.".into(),
                    expected_keywords: vec!["locale".into(), "timezone".into(), "template".into(), "channel".into(), "digest".into(), "email".into()],
                    session: 3, requires_recall: true, domain: Some("notification".into()), expected_tool: None, expected_arg_keys: vec![],
                },
            ],
        },
    ]
}

/// Tasks biased toward cross-session memory / recall (all suite tasks include at least one recall turn).
pub fn suite_memory_retrieval() -> Vec<EvalTask> {
    load_eval_suite()
        .into_iter()
        .filter(|t| t.turns.iter().any(|u| u.requires_recall))
        .collect()
}

/// Tasks tagged for tool-use / routing scenarios (`tests` contains `tool_routing`).
pub fn suite_tool_routing() -> Vec<EvalTask> {
    load_eval_suite()
        .into_iter()
        .filter(|t| t.tests.iter().any(|x| x == "tool_routing"))
        .collect()
}

/// Tasks tagged for council vs single-agent style comparison (`tests` contains `council_comparison`).
pub fn suite_council_vs_single() -> Vec<EvalTask> {
    load_eval_suite()
        .into_iter()
        .filter(|t| t.tests.iter().any(|x| x == "council_comparison"))
        .collect()
}

/// Count total turns across all tasks
pub fn total_turns(tasks: &[EvalTask]) -> usize {
    tasks.iter().map(|t| t.turns.len()).sum()
}

/// Count turns that require recall
pub fn recall_turns(tasks: &[EvalTask]) -> usize {
    tasks.iter().flat_map(|t| &t.turns).filter(|t| t.requires_recall).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_suite_has_expected_task_count() {
        let suite = load_eval_suite();
        // Keep this aligned with the canonical suite definition in this file.
        assert_eq!(suite.len(), 19);
    }

    #[test]
    fn test_all_tasks_have_recall_turns() {
        let suite = load_eval_suite();
        for task in &suite {
            assert!(
                task.turns.iter().any(|t| t.requires_recall),
                "Task {} has no recall turns",
                task.id
            );
        }
    }

    #[test]
    fn test_multi_session_tasks_exist() {
        let suite = load_eval_suite();
        let multi_session = suite.iter().filter(|t| {
            let sessions: std::collections::HashSet<u32> = t.turns.iter().map(|turn| turn.session).collect();
            sessions.len() > 1
        }).count();
        assert!(multi_session >= 15, "Expected at least 15 multi-session tasks, got {}", multi_session);
    }

    #[test]
    fn test_pre_registered_suites_non_empty() {
        assert!(!suite_memory_retrieval().is_empty());
        assert!(!suite_tool_routing().is_empty());
        assert!(!suite_council_vs_single().is_empty());
    }
}
