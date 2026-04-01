//! Runnable demo: multi-agent **council** (Simple + Orchestrate [+ optional Debate / stigmergic])
//! and optional live LLM orchestration (2 workers, or **4 workers + synthesizer**).
//!
//! ```bash
//! cargo run -p hyper-stigmergy --bin hsm-council-demo
//! cargo run -p hyper-stigmergy --bin hsm-council-demo -- --live
//! cargo run -p hyper-stigmergy --bin hsm-council-demo -- --live --complex
//! ```
//!
//! For delegation over JSON-RPC (Hermes optional), see `scripts/demo_multi_agent.sh`.

use clap::Parser;
use hyper_stigmergy::agent::Role;
use hyper_stigmergy::council::{
    Council, CouncilDecision, CouncilEvidence, CouncilEvidenceKind, CouncilMember, CouncilMode,
    Decision, Proposal, StigmergicCouncilContext,
};
use hyper_stigmergy::graph_runtime::GraphToolKind;
use hyper_stigmergy::llm::client::{LlmClient, LlmRequest, Message};

#[derive(Parser, Debug)]
#[command(name = "hsm-council-demo")]
#[command(about = "Show multi-agent council + optional live LLM orchestration (no Hermes required)")]
struct Cli {
    /// Call the configured LLM: orchestrator decomposes task, then parallel workers (+ synth if --complex).
    #[arg(long, default_value_t = false)]
    live: bool,

    /// Extra offline scenarios (stigmergic proposal + Debate council) and a larger live pipeline (4 specialists + merge).
    #[arg(long, default_value_t = false)]
    complex: bool,
}

fn demo_members() -> Vec<CouncilMember> {
    vec![
        CouncilMember {
            agent_id: 1,
            role: Role::Architect,
            expertise_score: 0.92,
            participation_weight: 1.0,
        },
        CouncilMember {
            agent_id: 2,
            role: Role::Critic,
            expertise_score: 0.88,
            participation_weight: 1.0,
        },
        CouncilMember {
            agent_id: 3,
            role: Role::Explorer,
            expertise_score: 0.82,
            participation_weight: 1.0,
        },
        CouncilMember {
            agent_id: 4,
            role: Role::Catalyst,
            expertise_score: 0.78,
            participation_weight: 0.9,
        },
        CouncilMember {
            agent_id: 5,
            role: Role::Chronicler,
            expertise_score: 0.8,
            participation_weight: 0.85,
        },
        CouncilMember {
            agent_id: 6,
            role: Role::Coder,
            expertise_score: 0.87,
            participation_weight: 1.0,
        },
    ]
}

fn print_banner(title: &str) {
    println!("\n{}", "═".repeat(72));
    println!("  {title}");
    println!("{}\n", "═".repeat(72));
}

fn print_decision(label: &str, d: &CouncilDecision) {
    println!("{label}");
    println!("  proposal_id:     {}", d.proposal_id);
    println!("  mode_used:       {:?}", d.mode_used);
    println!("  decision:        {:?}", d.decision);
    println!("  confidence:      {:.2}", d.confidence);
    println!("  participants:    {:?}", d.participating_agents);
    if let Some(plan) = &d.execution_plan {
        println!(
            "  execution_plan ({} steps, ~{} ms est.):",
            plan.steps.len(),
            plan.estimated_duration_ms
        );
        for s in &plan.steps {
            let who = s
                .assigned_agent
                .map(|a| format!("agent {a}"))
                .unwrap_or_else(|| "unassigned".into());
            let deps = if s.dependencies.is_empty() {
                String::new()
            } else {
                format!(" (after step {:?})", s.dependencies)
            };
            println!("    {}. [{}] {}{}", s.sequence, who, s.description, deps);
        }
        if let Some(rb) = &plan.rollback_strategy {
            println!("  rollback:        {rb}");
        }
    } else {
        println!("  execution_plan:  (none)");
    }
    println!();
}

async fn run_offline_demos(complex: bool) -> anyhow::Result<()> {
    let members = demo_members();
    let proposer = 1_u64;

    print_banner("1) Simple council — role-weighted vote + small execution plan");
    let mut simple_prop = Proposal::new(
        "demo-simple",
        "Add structured logging to HTTP handlers",
        "Standard observability improvement for REST services",
        proposer,
    );
    simple_prop.estimate_complexity();
    let mut simple_council = Council::new(CouncilMode::Simple, simple_prop, members.clone());
    let d1 = simple_council.evaluate().await?;
    print_decision("Result:", &d1);

    print_banner("2) Orchestrate council — commander, subtasks, assigned agents, DAG steps");
    let mut orch_prop = Proposal::new(
        "demo-orch",
        "Ship federated sync for the knowledge graph",
        "Design and implement protocol for hypergraph belief synchronization across nodes with conflict resolution",
        proposer,
    );
    orch_prop.urgency = 0.75;
    orch_prop.estimate_complexity();
    let mut orch_council = Council::new(CouncilMode::Orchestrate, orch_prop, members.clone());
    let d2 = orch_council.evaluate().await?;
    print_decision("Result:", &d2);

    if matches!(d2.decision, Decision::Approve) {
        println!("Orchestrate mode produced a multi-step plan with different roles (Critic → Architect → …).\n");
    }

    if !complex {
        return Ok(());
    }

    print_banner("3) Simple council + stigmergic context (preferred agent, graph evidence)");
    let ctx = StigmergicCouncilContext {
        preferred_agent: Some(3),
        preferred_tool: Some(GraphToolKind::VectorAnn),
        confidence: 0.82,
        require_council_review: true,
        rationale: "Vector retrieval cluster matched Explorer-led spikes for similar federation tasks."
            .into(),
        evidence: vec![CouncilEvidence {
            id: "ev-graph-1".into(),
            kind: CouncilEvidenceKind::GraphQuery,
            summary: "Top beliefs: sync_latency SLO, conflict_merge_policy v2".into(),
        }],
        graph_snapshot_bullets: vec![
            "edge: nodeA --replicates--> nodeB (lag_p95=400ms)".into(),
            "node: policy/conflict_resolution = active".into(),
        ],
        graph_queries: vec![],
    };
    let mut stig_prop = Proposal::new(
        "demo-stig",
        "Adopt OT CRDT for low-latency belief merge",
        "Replace last-writer-wins with CRDT merge for selected belief keys; document structure migration",
        proposer,
    );
    stig_prop.estimate_complexity();
    let stig_prop = stig_prop.with_stigmergic_context(ctx);
    let mut stig_council = Council::new(CouncilMode::Simple, stig_prop, members.clone());
    let d3 = stig_council.evaluate().await?;
    print_decision("Result:", &d3);

    print_banner("4) Debate council — opening / rebuttal / synthesis (structured multi-agent)");
    let mut debate_prop = Proposal::new(
        "demo-debate",
        "Green-light multi-region active-active for the knowledge graph",
        "Weigh operational cost vs read latency; security boundary for cross-region replication; rollback if split-brain",
        proposer,
    );
    debate_prop.complexity = 0.88;
    debate_prop.urgency = 0.55;
    debate_prop.estimate_complexity();
    let mut debate_council = Council::new(CouncilMode::Debate, debate_prop, members);
    let d4 = debate_council.evaluate().await?;
    print_decision("Result:", &d4);

    Ok(())
}

async fn chat_once(
    client: &LlmClient,
    model: &str,
    system: &str,
    user: &str,
    max_tokens: usize,
) -> anyhow::Result<String> {
    let req = LlmRequest {
        model: model.to_string(),
        messages: vec![Message::system(system), Message::user(user)],
        temperature: 0.35,
        max_tokens: Some(max_tokens),
        ..Default::default()
    };
    let resp = client.chat(req).await?;
    Ok(resp.content)
}

fn resolve_model() -> String {
    std::env::var("OLLAMA_MODEL")
        .or_else(|_| std::env::var("DEFAULT_LLM_MODEL"))
        .unwrap_or_else(|_| "gpt-4o-mini".to_string())
}

async fn run_live_simple(client: &LlmClient, model: &str) -> anyhow::Result<()> {
    print_banner("Live — orchestrator + 2 parallel workers");
    let task = "List three concrete steps to add request tracing (trace IDs) to a small Rust axum API.";

    let orch_sys = "You are an orchestrator. Break the user's task into exactly 2 parallel sub-tasks for different specialists.\n\
        Output format (use these exact prefixes):\n\
        TASK_A: <one line>\n\
        TASK_B: <one line>";
    let decomposition = chat_once(client, model, orch_sys, task, 500).await?;
    println!("Orchestrator decomposition:\n{decomposition}\n");

    let mut task_a = String::new();
    let mut task_b = String::new();
    for line in decomposition.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("TASK_A:") {
            task_a = rest.trim().to_string();
        } else if let Some(rest) = t.strip_prefix("TASK_B:") {
            task_b = rest.trim().to_string();
        }
    }
    if task_a.is_empty() {
        task_a = "Summarize tracing approaches for axum.".into();
    }
    if task_b.is_empty() {
        task_b = "List middleware ordering concerns for trace propagation.".into();
    }

    let w_sys = "You are a specialist engineer. Answer in 3–6 short bullet lines. Be specific.";
    let user_a = format!("Sub-task A: {}\n\nOriginal: {}", task_a, task);
    let user_b = format!("Sub-task B: {}\n\nOriginal: {}", task_b, task);
    let (a, b) = tokio::try_join!(
        chat_once(client, model, w_sys, &user_a, 600),
        chat_once(client, model, w_sys, &user_b, 600),
    )?;

    println!("── Worker A (parallel) ──\n{a}\n");
    println!("── Worker B (parallel) ──\n{b}\n");
    Ok(())
}

async fn run_live_complex(client: &LlmClient, model: &str) -> anyhow::Result<()> {
    print_banner("Live (complex) — orchestrator + 4 parallel specialists + synthesizer");
    let task = r#"You are advising a team shipping a multi-tenant B2B SaaS API (Rust axum, Postgres, Redis, S3).

Constraints:
- EU + US data residency (no EU PII in US stores).
- SOC2-style audit: who changed what, when.
- Zero-downtime deploy; feature flags for risky paths.
- Third-party webhooks out + signed inbound webhooks.

Produce a phased delivery plan: security, data model, infra, and observability must all be addressed."#;

    let orch_sys = "You are a principal engineer orchestrating parallel workstreams.\n\
        Break the mission into exactly 4 parallel specialist tasks (different concerns).\n\
        Use EXACTLY these line prefixes (one line each, no markdown headers):\n\
        TASK_A: <one line>\n\
        TASK_B: <one line>\n\
        TASK_C: <one line>\n\
        TASK_D: <one line>";
    let decomposition = chat_once(client, model, orch_sys, task, 700).await?;
    println!("Orchestrator (4-way split):\n{decomposition}\n");

    let prefixes = ["TASK_A:", "TASK_B:", "TASK_C:", "TASK_D:"];
    let mut subs = [
        String::new(),
        String::new(),
        String::new(),
        String::new(),
    ];
    for line in decomposition.lines() {
        let t = line.trim();
        for (i, p) in prefixes.iter().enumerate() {
            if let Some(rest) = t.strip_prefix(p) {
                subs[i] = rest.trim().to_string();
            }
        }
    }
    let fallbacks = [
        "Data residency: schema split, replication, and legal/technical controls.",
        "AuthN/Z and audit trail design for admin + tenant APIs.",
        "Zero-downtime migration + feature-flag rollout strategy.",
        "SLOs, metrics, tracing, and alerting for webhooks and API paths.",
    ];
    for (i, fb) in fallbacks.iter().enumerate() {
        if subs[i].is_empty() {
            subs[i] = (*fb).to_string();
        }
    }

    let labels = [
        "Security / compliance",
        "Data & tenancy",
        "Release engineering",
        "Observability & ops",
    ];
    let w_sys = "You are a senior specialist. Answer in 4–8 tight bullets. Name concrete artifacts (migrations, tables, flags, dashboards) where possible.";
    let u0 = format!(
        "Track: {}\nSub-task: {}\n\nMission:\n{}",
        labels[0], subs[0], task
    );
    let u1 = format!(
        "Track: {}\nSub-task: {}\n\nMission:\n{}",
        labels[1], subs[1], task
    );
    let u2 = format!(
        "Track: {}\nSub-task: {}\n\nMission:\n{}",
        labels[2], subs[2], task
    );
    let u3 = format!(
        "Track: {}\nSub-task: {}\n\nMission:\n{}",
        labels[3], subs[3], task
    );

    let (o0, o1, o2, o3) = tokio::try_join!(
        chat_once(client, model, w_sys, &u0, 900),
        chat_once(client, model, w_sys, &u1, 900),
        chat_once(client, model, w_sys, &u2, 900),
        chat_once(client, model, w_sys, &u3, 900),
    )?;

    println!("── Specialist 1 — {} ──\n{}\n", labels[0], o0);
    println!("── Specialist 2 — {} ──\n{}\n", labels[1], o1);
    println!("── Specialist 3 — {} ──\n{}\n", labels[2], o2);
    println!("── Specialist 4 — {} ──\n{}\n", labels[3], o3);

    let synth_user = format!(
        "You are the tech lead. Merge the four specialist reports into ONE ordered plan:\n\
        (1) phases 0–2 with dependencies\n\
        (2) top 5 risks + mitigations\n\
        (3) minimal MVP slice for week 1\n\n\
        === A ===\n{o0}\n\n=== B ===\n{o1}\n\n=== C ===\n{o2}\n\n=== D ===\n{o3}"
    );
    let merged = chat_once(
        client,
        model,
        "You synthesize parallel engineering work into a single actionable plan. Be concise; use headings.",
        &synth_user,
        1200,
    )
    .await?;
    println!("── Synthesizer (single merged plan) ──\n{merged}\n");
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    println!("hsm-council-demo — multi-agent coordination examples");
    if cli.complex {
        println!("(--complex: extra offline Debate/stigmergic + larger live pipeline when combined with --live)");
    }
    println!("(Council uses in-tree types; sections 1–2 need no API keys; Debate/stigmergic only with --complex.)");

    run_offline_demos(cli.complex).await?;

    if cli.live {
        match LlmClient::new() {
            Ok(client) => {
                let model = resolve_model();
                let r = if cli.complex {
                    run_live_complex(&client, &model).await
                } else {
                    run_live_simple(&client, &model).await
                };
                if let Err(e) = r {
                    eprintln!("Live LLM demo failed: {e}");
                }
            }
            Err(_) => {
                eprintln!("No LLM configured. Set OPENAI_API_KEY, OPENROUTER_API_KEY, ANTHROPIC_API_KEY, or OLLAMA_URL.");
            }
        }
    } else {
        print_banner("Live LLM (skipped)");
        println!(
            "Re-run with --live (and optionally --complex) after configuring an LLM provider.\n"
        );
    }

    println!("Delegation: ./scripts/demo_multi_agent.sh (A2A heartbeat_tick + --dry-run).");
    Ok(())
}
