use clap::Parser;
use hyper_stigmergy::agent::Role;
use hyper_stigmergy::council::CouncilMember;
use hyper_stigmergy::ouroboros_compat::phase1_policy::{
    ConstitutionConfig, PolicyContext, PolicyDecision, PolicyEngine, ReleaseState,
};
use hyper_stigmergy::ouroboros_compat::phase2_risk_gate::{RiskGate, RiskGateConfig};
use hyper_stigmergy::ouroboros_compat::phase3_council_bridge::{
    CouncilBridge, CouncilBridgeConfig,
};
use hyper_stigmergy::ouroboros_compat::phase4_evidence_contract::{
    EvidenceBundle, EvidenceContract, EvidenceRequirements,
};
use hyper_stigmergy::ouroboros_compat::phase5_ops_memory::{
    evaluate_runtime_slos, RuntimeSnapshot, RuntimeThresholds,
};
use hyper_stigmergy::ouroboros_compat::ProposedAction;
use serde_json::json;

#[derive(Parser, Debug)]
#[command(
    name = "ouroboros_gate",
    about = "Phase 1-5 gate for Ouroboros -> HSM-II migration"
)]
struct Args {
    /// Proposed action JSON blob.
    /// Example: {"id":"a1","title":"edit","description":"modify file","actor_id":"1","kind":"SelfModification","target_path":"src/main.rs","target_peer":null,"metadata":{}}
    #[arg(long)]
    action_json: String,

    /// Optional evidence bundle JSON.
    #[arg(long)]
    evidence_json: Option<String>,

    /// Simulated council confidence score for final approval check.
    #[arg(long, default_value_t = 0.70)]
    council_confidence: f64,

    /// Optional release version.
    #[arg(long)]
    version: Option<String>,
    /// Optional git tag.
    #[arg(long)]
    git_tag: Option<String>,
    /// Optional README version.
    #[arg(long)]
    readme_version: Option<String>,

    /// Runtime snapshot fields for SLO checks.
    #[arg(long, default_value_t = 0.72)]
    coherence: f64,
    #[arg(long, default_value_t = 0.30)]
    stability: f64,
    #[arg(long, default_value_t = 0.70)]
    mean_trust: f64,
}

fn default_evidence() -> EvidenceBundle {
    EvidenceBundle {
        investigation_session_id: Some("bootstrap-session".to_string()),
        tool_calls: vec![],
        evidence_chain_count: 1,
        claim_count: 1,
        evidence_count: 1,
        coverage: 1.0,
    }
}

fn default_members() -> Vec<CouncilMember> {
    vec![
        CouncilMember {
            agent_id: 1,
            role: Role::Architect,
            expertise_score: 0.9,
            participation_weight: 1.0,
        },
        CouncilMember {
            agent_id: 2,
            role: Role::Critic,
            expertise_score: 0.8,
            participation_weight: 1.0,
        },
        CouncilMember {
            agent_id: 3,
            role: Role::Explorer,
            expertise_score: 0.7,
            participation_weight: 1.0,
        },
        CouncilMember {
            agent_id: 4,
            role: Role::Chronicler,
            expertise_score: 0.8,
            participation_weight: 1.0,
        },
    ]
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let action: ProposedAction = serde_json::from_str(&args.action_json)?;
    let evidence: EvidenceBundle = match &args.evidence_json {
        Some(raw) => serde_json::from_str(raw)?,
        None => default_evidence(),
    };

    let policy_engine = PolicyEngine::new(ConstitutionConfig::default());
    let policy = policy_engine.evaluate(
        &action,
        &PolicyContext {
            requested_by: action.actor_id.clone(),
            release_state: ReleaseState {
                version: args.version.clone(),
                git_tag: args.git_tag.clone(),
                readme_version: args.readme_version.clone(),
            },
        },
    );

    let risk = RiskGate::new(RiskGateConfig::default()).assess(&action, &policy);

    let bridge = CouncilBridge::new(CouncilBridgeConfig::default());
    let council_plan = bridge.plan(&action, &risk, &default_members());

    let evidence_validation =
        EvidenceContract::new(EvidenceRequirements::default()).validate(&evidence);

    let slo_report = evaluate_runtime_slos(
        &RuntimeSnapshot {
            coherence: args.coherence,
            stability: args.stability,
            mean_trust: args.mean_trust,
            council_confidence: Some(args.council_confidence),
            evidence_coverage: Some(evidence.coverage),
        },
        &RuntimeThresholds::default(),
    );

    let policy_allows_execution = !matches!(policy.decision, PolicyDecision::Deny);
    let approved = bridge.should_approve(
        args.council_confidence,
        evidence.coverage,
        policy_allows_execution && evidence_validation.ok && slo_report.healthy,
    );

    let output = json!({
        "approved": approved,
        "policy": policy,
        "risk": risk,
        "council_plan": council_plan,
        "evidence_validation": evidence_validation,
        "slo_report": slo_report
    });
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}
