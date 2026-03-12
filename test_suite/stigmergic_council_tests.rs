//! Stigmergic Council Integration Tests
//!
//! Tests for trace summarization, evidence logging, and graph query fusion
//! in council deliberation.

use hyper_stigmergy::council::{
    CouncilDecisionMetadata, CouncilEvidence, CouncilEvidenceKind,
    CouncilMember, CouncilMode, DebateCouncil, Proposal, TraceSummarizer, TraceSummary,
};
use hyper_stigmergy::agent::{AgentId, Role};
use hyper_stigmergy::social_memory::{DataSensitivity, PromiseStatus, SocialMemory};
use hyper_stigmergy::stigmergic_policy::{StigmergicMemory, StigmergicTrace, TraceKind};

/// Test that trace summary enriches proposal with context
#[tokio::test]
async fn test_trace_summary_enriches_proposal() {
    // Setup stigmergic memory with traces
    let mut stigmergic_memory = StigmergicMemory::default();
    stigmergic_memory.traces.push(StigmergicTrace {
        id: "trace-1".to_string(),
        agent_id: 1 as AgentId,
        model_id: "test".to_string(),
        task_key: Some("test-task".to_string()),
        kind: TraceKind::PromiseMade,
        summary: "Agent 1 promised to handle test-task".to_string(),
        success: Some(true),
        outcome_score: Some(0.9),
        sensitivity: DataSensitivity::Internal,
        planned_tool: None,
        recorded_at: 100,
        tick: 100,
        metadata: Default::default(),
    });

    // Setup social memory with reputation
    let mut social_memory = SocialMemory::default();
    social_memory.record_promise(
        1 as AgentId,
        None,
        "test-task",
        "Test promise",
        DataSensitivity::Internal,
        100,
        Some(200),
    );

    // Create proposal
    let mut proposal = Proposal::new("test-1", "Test Proposal", "Test description", 0 as AgentId);
    proposal.task_key = Some("test-task".to_string());

    // Create debate council and enrich proposal
    let members = vec![
        CouncilMember { agent_id: 1 as AgentId, role: Role::Architect, expertise_score: 0.9, participation_weight: 1.0 },
    ];
    let debate = DebateCouncil::new(hyper_stigmergy::council::CouncilId::new_v4(), members);
    
    debate.enrich_with_trace_summary(
        &mut proposal,
        &stigmergic_memory,
        Some(&social_memory),
        500,
    );

    // Verify proposal was enriched
    assert!(proposal.stigmergic_context.is_some());
    let context = proposal.stigmergic_context.unwrap();
    assert!(!context.graph_snapshot_bullets.is_empty());
    
    // Verify bullets contain relevant information
    let bullets_text = context.graph_snapshot_bullets.join(" ");
    assert!(
        bullets_text.contains("trace") || bullets_text.contains("Agent"),
        "Trace summary should contain relevant context"
    );
}

/// Test that council decision logs influencing evidence
#[tokio::test]
async fn test_council_decision_logs_evidence() {
    // Create proposal with stigmergic context
    let mut proposal = Proposal::new("test-2", "Test Proposal", "Test description", 0 as AgentId);
    proposal.stigmergic_context = Some(hyper_stigmergy::council::StigmergicCouncilContext {
        preferred_agent: Some(1 as AgentId),
        preferred_tool: None,
        confidence: 0.8,
        require_council_review: true,
        rationale: "Test rationale".to_string(),
        evidence: vec![
            CouncilEvidence {
                id: "evidence-1".to_string(),
                kind: CouncilEvidenceKind::Trace,
                summary: "Test trace evidence".to_string(),
            },
            CouncilEvidence {
                id: "evidence-2".to_string(),
                kind: CouncilEvidenceKind::Directive,
                summary: "Test directive evidence".to_string(),
            },
        ],
        graph_snapshot_bullets: vec!["Test bullet".to_string()],
        graph_queries: vec![],
    });

    // Create council and run deliberation
    let members = vec![
        CouncilMember { agent_id: 1 as AgentId, role: Role::Architect, expertise_score: 0.9, participation_weight: 1.0 },
        CouncilMember { agent_id: 2 as AgentId, role: Role::Critic, expertise_score: 0.8, participation_weight: 1.0 },
    ];
    
    let mut debate = DebateCouncil::new(hyper_stigmergy::council::CouncilId::new_v4(), members);
    let decision = debate.evaluate(&proposal, CouncilMode::Debate).await.unwrap();

    // Verify decision metadata contains evidence
    assert!(!decision.metadata.evidence_ids.is_empty());
    assert!(decision.metadata.trace_ids.contains(&"evidence-1".to_string()));
    assert!(decision.metadata.directive_ids.contains(&"evidence-2".to_string()));
}

/// Test trace summarizer produces correct bullet points
#[test]
fn test_trace_summarizer_produces_bullets() {
    let summarizer = TraceSummarizer::default();
    
    // Create social memory with agent reputations
    let mut social_memory = SocialMemory::default();
    let agent_id: AgentId = 1;
    
    // Record some deliveries to build reputation
    for _ in 0..5 {
        social_memory.record_delivery(
            agent_id,
            "test-task",
            true,
            0.9,
            true,
            true,
            100,
            &[],
        );
    }

    // Create stigmergic memory with traces
    let mut stigmergic_memory = StigmergicMemory::default();
    stigmergic_memory.traces.push(StigmergicTrace {
        id: "trace-1".to_string(),
        agent_id,
        model_id: "test".to_string(),
        task_key: Some("test-task".to_string()),
        kind: TraceKind::DeliveryRecorded,
        summary: "Successfully completed test-task".to_string(),
        success: Some(true),
        outcome_score: Some(0.95),
        sensitivity: DataSensitivity::Internal,
        planned_tool: None,
        recorded_at: 100,
        tick: 100,
        metadata: Default::default(),
    });

    // Generate summary
    let summary = summarizer.summarize_for_council(
        &stigmergic_memory,
        Some(&social_memory),
        500,
        Some("test-task"),
    );

    // Format as bullets
    let bullets = summarizer.to_bullet_points(&summary);

    // Verify output contains expected sections
    assert!(
        bullets.contains("Trusted Agents") || bullets.contains("Stigmergic Traces"),
        "Bullet points should contain relevant sections"
    );
}

/// Test that evidence deduplication works in decision metadata
#[test]
fn test_evidence_deduplication() {
    let mut metadata = CouncilDecisionMetadata::default();
    
    // Record same evidence multiple times
    let evidence = CouncilEvidence {
        id: "duplicate-evidence".to_string(),
        kind: CouncilEvidenceKind::Trace,
        summary: "Test".to_string(),
    };
    
    metadata.record_evidence(&evidence);
    metadata.record_evidence(&evidence);
    metadata.record_evidence(&evidence);
    
    // Dedupe
    metadata.dedupe();
    
    // Should only have one copy
    assert_eq!(metadata.evidence_ids.len(), 1);
    assert_eq!(metadata.trace_ids.len(), 1);
}

/// Test trace summarizer with empty memories
#[test]
fn test_trace_summarizer_empty_memories() {
    let summarizer = TraceSummarizer::default();
    let stigmergic_memory = StigmergicMemory::default();
    
    let summary = summarizer.summarize_for_council(
        &stigmergic_memory,
        None,
        100,
        None,
    );
    
    let bullets = summarizer.to_bullet_points(&summary);
    assert_eq!(bullets, "No significant stigmergic context available.");
}

/// Test one-liner summary format
#[test]
fn test_trace_summarizer_one_liner() {
    let summarizer = TraceSummarizer::default();
    
    let summary = TraceSummary {
        trusted_agents: vec![(1 as AgentId, 0.9)],
        recent_promise_outcomes: vec![],
        restricted_shares: vec![],
        active_directives: vec![],
        recent_policy_shifts: vec![],
        relevant_traces: vec![],
    };
    
    let one_liner = summarizer.to_one_liner(&summary);
    assert!(one_liner.contains("1 trusted agents"));
    assert!(one_liner.contains("0 recent promises"));
    assert!(one_liner.contains("0 relevant traces"));
}

/// Test council mode affects decision metadata
#[tokio::test]
async fn test_council_mode_in_metadata() {
    let proposal = Proposal::new("test-3", "Test", "Test", 0 as AgentId);
    let members = vec![
        CouncilMember { agent_id: 1 as AgentId, role: Role::Architect, expertise_score: 0.9, participation_weight: 1.0 },
    ];
    
    let mut debate = DebateCouncil::new(hyper_stigmergy::council::CouncilId::new_v4(), members);
    let decision = debate.evaluate(&proposal, CouncilMode::Debate).await.unwrap();
    
    assert_eq!(decision.mode_used, CouncilMode::Debate);
}
