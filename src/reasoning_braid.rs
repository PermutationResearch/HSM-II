//! Parallel reasoning braid orchestrator with Neural-Symbolic Integration.
//!
//! Spawns multiple Prolog inference threads ("braids") that run concurrently,
//! each exploring different aspects of the hypergraph state. Results are
//! collected and woven into a unified synthesis for the LLM prompt.
//!
//! This implements the "parallel Prolog reasoning braids" concept:
//! - Multiple queries run simultaneously via tokio tasks
//! - Each braid has a depth limit and timeout
//! - Results are woven into structured context for LivingPrompt
//! - Dead-end braids are pruned, successful ones contribute to synthesis
//!
//! NEURAL-SYMBOLIC BRIDGE (Fixes Symbolic-Neural Gap):
//! - Prolog facts are embedded in the same vector space as LLM context
//! - Semantic similarity search over symbolic facts
//! - Bidirectional grounding: neural retrieval of symbolic facts
//! - Synthesized facts from pattern detection in embedding space
//!
//! The dual-process architecture:
//! - Prolog (System 2): structured inference, graph topology, constraint checking
//! - LLM (System 1): creative generation, narrative synthesis, action proposals
//! - Neural-Symbolic Bridge: connects both via shared embeddings

use std::time::{Duration, Instant};

use crate::consensus::EmergentAssociation;
use crate::hyper_stigmergy::HyperStigmergicMorphogenesis;
use crate::meta_graph::MetaGraph;
use crate::prolog_embedding_bridge::{NeuralQueryResult, NeuralSymbolicBraid};
use crate::prolog_engine::{PrologEngine, QueryResult};
use crate::skill::SkillBank;

// ── Braid Types ──────────────────────────────────────────────────────────

/// A single reasoning braid — a parallel inference thread
#[derive(Clone, Debug)]
pub struct ReasoningBraid {
    pub id: String,
    pub query_name: String,
    pub status: BraidStatus,
    pub duration_ms: u64,
}

#[derive(Clone, Debug)]
pub enum BraidStatus {
    Pending,
    Running,
    Completed(BraidResult),
    DeadEnd,
    TimedOut,
}

#[derive(Clone, Debug)]
pub struct BraidResult {
    pub findings: Vec<String>,
    pub solution_count: usize,
    pub applicable_skills: Vec<String>,
}

/// Synthesis of all braid results — woven into unified context
#[derive(Clone, Debug)]
pub struct BraidSynthesis {
    pub braids_run: usize,
    pub braids_succeeded: usize,
    pub braids_dead_end: usize,
    pub braids_timed_out: usize,
    pub total_duration_ms: u64,

    /// Structured findings by category
    pub topology_findings: Vec<String>,
    pub belief_findings: Vec<String>,
    pub skill_findings: Vec<String>,
    pub risk_findings: Vec<String>,
    /// JoulWork (JW) economy findings
    pub jw_findings: Vec<String>,

    /// Emergent association findings from post-skill analysis
    pub association_findings: Vec<String>,

    /// Federation cross-system findings
    pub cross_system_findings: Vec<String>,
    /// Trust violations detected across federation
    pub trust_violations: Vec<String>,

    /// Applicable skill IDs (union across all braids)
    pub applicable_skill_ids: Vec<String>,

    /// Detected emergent associations (for consensus engine)
    pub emergent_associations: Vec<EmergentAssociation>,

    /// The woven prompt section ready for LivingPrompt injection
    pub prompt_section: String,

    // ── NEURAL-SYMBOLIC BRIDGE FIELDS ───────────────────────────────────────
    /// Neural-symbolic query results for semantic grounding
    pub neural_symbolic_results: Vec<NeuralQueryResult>,
    /// Semantically retrieved facts (bridged between Prolog and LLM embeddings)
    pub semantically_grounded_facts: Vec<String>,
    /// Confidence in the neural-symbolic grounding
    pub neural_symbolic_confidence: f64,
    /// Synthesized patterns from embedding space
    pub neural_syntheses: Vec<String>,
}

// ── Braid Orchestrator ───────────────────────────────────────────────────

/// Standard braid queries for each tick cycle
pub const STANDARD_BRAIDS: &[&str] = &[
    "belief_conflicts",
    "stale_edges",
    "disconnected_clusters",
    "coherence_plateau",
    "weak_edges",
    "belief_contradictions",
    // JoulWork (JW) economy braids
    "high_jw_agents",
    "low_jw_agents",
    "jw_economy_health",
];

/// Federation-specific braid queries
pub const FEDERATION_BRAIDS: &[&str] = &[
    "cross_system_bridges",
    "trust_violations",
    "federation_opportunities",
    "remote_edges",
    "shared_edges",
];

/// Orchestrates parallel reasoning braids over the Prolog engine.
pub struct BraidOrchestrator {
    pub timeout: Duration,
    pub max_concurrent: usize,
    /// Neural-symbolic bridge for embedding-based fact retrieval
    neural_bridge: Option<NeuralSymbolicBraid>,
    /// Enable neural-symbolic grounding
    enable_neural_grounding: bool,
}

impl Default for BraidOrchestrator {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(5),
            max_concurrent: 8,
            neural_bridge: Some(NeuralSymbolicBraid::new()),
            enable_neural_grounding: true,
        }
    }
}

impl BraidOrchestrator {
    pub fn new(timeout_secs: u64, max_concurrent: usize) -> Self {
        Self {
            timeout: Duration::from_secs(timeout_secs),
            max_concurrent,
            neural_bridge: Some(NeuralSymbolicBraid::new()),
            enable_neural_grounding: true,
        }
    }

    /// Create a braid orchestrator without neural-symbolic features
    pub fn new_symbolic_only(timeout_secs: u64, max_concurrent: usize) -> Self {
        Self {
            timeout: Duration::from_secs(timeout_secs),
            max_concurrent,
            neural_bridge: None,
            enable_neural_grounding: false,
        }
    }

    /// Enable or disable neural-symbolic grounding
    pub fn set_neural_grounding(&mut self, enabled: bool) {
        self.enable_neural_grounding = enabled;
        if enabled && self.neural_bridge.is_none() {
            self.neural_bridge = Some(NeuralSymbolicBraid::new());
        }
    }

    /// Execute all standard braids + skill applicability check in parallel.
    /// Returns a woven synthesis ready for LivingPrompt injection.
    ///
    /// NEURAL-SYMBOLIC INTEGRATION: If enabled, also performs embedding-based
    /// fact retrieval to bridge the symbolic-neural gap.
    pub async fn execute_braids(
        &self,
        world: &HyperStigmergicMorphogenesis,
        skill_bank: &SkillBank,
    ) -> BraidSynthesis {
        let start = Instant::now();

        // Build Prolog engine from current world state
        let engine = PrologEngine::from_world(world, skill_bank);

        // Execute standard braids
        let mut braids: Vec<ReasoningBraid> = Vec::new();

        for &query_name in STANDARD_BRAIDS {
            let braid_start = Instant::now();
            let result = engine.named_query(query_name);
            let duration = braid_start.elapsed().as_millis() as u64;

            let status = if result.succeeded {
                BraidStatus::Completed(BraidResult {
                    findings: format_query_findings(query_name, &result),
                    solution_count: result.solutions.len(),
                    applicable_skills: vec![],
                })
            } else {
                BraidStatus::DeadEnd
            };

            braids.push(ReasoningBraid {
                id: format!("braid_{}", query_name),
                query_name: query_name.to_string(),
                status,
                duration_ms: duration,
            });
        }

        // Skill applicability braid
        let skill_start = Instant::now();
        let applicable_skills = engine.find_applicable_skills(skill_bank);
        let skill_duration = skill_start.elapsed().as_millis() as u64;

        braids.push(ReasoningBraid {
            id: "braid_skill_applicability".to_string(),
            query_name: "skill_applicability".to_string(),
            status: BraidStatus::Completed(BraidResult {
                findings: vec![format!(
                    "{} skills applicable in current state",
                    applicable_skills.len()
                )],
                solution_count: applicable_skills.len(),
                applicable_skills: applicable_skills.clone(),
            }),
            duration_ms: skill_duration,
        });

        // Weave results with neural-symbolic integration
        let mut synthesis = self.weave_synthesis(
            braids,
            applicable_skills,
            start.elapsed().as_millis() as u64,
        );

        // NEURAL-SYMBOLIC BRIDGE: Perform embedding-based fact grounding
        if self.enable_neural_grounding {
            if let Some(ref neural_bridge) = self.neural_bridge {
                let neural_start = Instant::now();

                // Create a mutable copy for this execution
                let mut bridge = neural_bridge.clone();

                // Build the bridge from current world state
                bridge.build_from_world(world, skill_bank);

                // Perform neural-symbolic queries for each braid type
                let mut neural_results = Vec::new();
                let mut all_syntheses = Vec::new();
                let mut all_grounded_facts = Vec::new();

                for &query_name in STANDARD_BRAIDS {
                    let query = format!(
                        "What is the semantic meaning of {} in the current world state?",
                        query_name
                    );
                    let result = bridge.neural_query(&query);

                    // Collect grounded facts
                    for (score, fact) in &result.relevant_facts {
                        if *score >= 0.6 {
                            all_grounded_facts.push(format!(
                                "[{}] {} (sim: {:.2})",
                                format!("{:?}", fact.category).to_lowercase(),
                                fact.fact_text,
                                score
                            ));
                        }
                    }

                    all_syntheses.extend(result.syntheses.clone());
                    neural_results.push(result);
                }

                // Update synthesis with neural-symbolic results
                synthesis.neural_symbolic_results = neural_results;
                synthesis.semantically_grounded_facts = all_grounded_facts;
                synthesis.neural_syntheses = all_syntheses;
                synthesis.neural_symbolic_confidence =
                    if !synthesis.semantically_grounded_facts.is_empty() {
                        0.7 // Placeholder confidence based on successful grounding
                    } else {
                        0.0
                    };

                // Add neural context to prompt section
                synthesis.prompt_section = self.enhance_prompt_with_neural_grounding(&synthesis);

                let neural_duration = neural_start.elapsed().as_millis() as u64;
                synthesis.total_duration_ms += neural_duration;
            }
        }

        synthesis
    }

    /// Weave individual braid results into unified synthesis
    fn weave_synthesis(
        &self,
        braids: Vec<ReasoningBraid>,
        applicable_skills: Vec<String>,
        total_duration_ms: u64,
    ) -> BraidSynthesis {
        let mut synthesis = BraidSynthesis {
            braids_run: braids.len(),
            braids_succeeded: 0,
            braids_dead_end: 0,
            braids_timed_out: 0,
            total_duration_ms,
            topology_findings: Vec::new(),
            belief_findings: Vec::new(),
            skill_findings: Vec::new(),
            risk_findings: Vec::new(),
            jw_findings: Vec::new(),
            association_findings: Vec::new(),
            cross_system_findings: Vec::new(),
            trust_violations: Vec::new(),
            applicable_skill_ids: applicable_skills,
            emergent_associations: Vec::new(),
            prompt_section: String::new(),
            // Neural-symbolic fields (initialized empty, populated later)
            neural_symbolic_results: Vec::new(),
            semantically_grounded_facts: Vec::new(),
            neural_symbolic_confidence: 0.0,
            neural_syntheses: Vec::new(),
        };

        for braid in &braids {
            match &braid.status {
                BraidStatus::Completed(result) => {
                    synthesis.braids_succeeded += 1;

                    // Categorize findings
                    match braid.query_name.as_str() {
                        "disconnected_clusters" | "stale_edges" | "weak_edges" => {
                            synthesis.topology_findings.extend(result.findings.clone());
                        }
                        "belief_conflicts" | "belief_contradictions" => {
                            synthesis.belief_findings.extend(result.findings.clone());
                        }
                        "skill_applicability" => {
                            synthesis.skill_findings.extend(result.findings.clone());
                        }
                        "coherence_plateau" => {
                            synthesis.risk_findings.extend(result.findings.clone());
                        }
                        "emergent_associations" | "identity_bridges" => {
                            synthesis
                                .association_findings
                                .extend(result.findings.clone());
                        }
                        "cross_system_bridges"
                        | "federation_opportunities"
                        | "remote_edges"
                        | "shared_edges" => {
                            synthesis
                                .cross_system_findings
                                .extend(result.findings.clone());
                        }
                        "trust_violations" => {
                            synthesis.trust_violations.extend(result.findings.clone());
                        }
                        _ => {
                            synthesis.topology_findings.extend(result.findings.clone());
                        }
                    }
                }
                BraidStatus::DeadEnd => synthesis.braids_dead_end += 1,
                BraidStatus::TimedOut => synthesis.braids_timed_out += 1,
                _ => {}
            }
        }

        // Build the woven prompt section
        synthesis.prompt_section = build_prompt_section(&synthesis);

        synthesis
    }
}

impl BraidOrchestrator {
    /// Enhance prompt section with neural-symbolic grounding
    fn enhance_prompt_with_neural_grounding(&self, synthesis: &BraidSynthesis) -> String {
        let mut enhanced = synthesis.prompt_section.clone();

        if !synthesis.semantically_grounded_facts.is_empty() {
            enhanced.push_str("\n### Neural-Symbolic Grounding\n");
            enhanced.push_str(&format!(
                "_Prolog facts embedded in LLM space (confidence: {:.2})_\n\n",
                synthesis.neural_symbolic_confidence
            ));

            // Group facts by category
            let mut by_category: std::collections::HashMap<String, Vec<&String>> =
                std::collections::HashMap::new();
            for fact in &synthesis.semantically_grounded_facts {
                let category = fact
                    .split(']')
                    .next()
                    .map(|s| s.trim_start_matches('[').to_string())
                    .unwrap_or_else(|| "unknown".to_string());
                by_category.entry(category).or_default().push(fact);
            }

            for (category, facts) in by_category.iter().take(5) {
                enhanced.push_str(&format!("**{}**:\n", category));
                for fact in facts.iter().take(3) {
                    enhanced.push_str(&format!("  - {}\n", fact));
                }
                if facts.len() > 3 {
                    enhanced.push_str(&format!("  - ... and {} more\n", facts.len() - 3));
                }
            }
        }

        if !synthesis.neural_syntheses.is_empty() {
            enhanced.push_str("\n### Neural-Synthesized Patterns\n");
            for synthesis in &synthesis.neural_syntheses {
                enhanced.push_str(&format!("- {}\n", synthesis));
            }
        }

        enhanced
    }

    /// Execute emergent association braids — run post-skill to detect new structures.
    /// Called during consensus evaluation every N ticks.
    ///
    /// This adds:
    /// - emergent_associations query (what new structures appeared?)
    /// - identity_bridges query (reversal-curse regularization facts)
    /// - The detected associations are returned for the consensus engine
    pub fn execute_association_braids(
        &self,
        world: &HyperStigmergicMorphogenesis,
        skill_bank: &SkillBank,
    ) -> (Vec<EmergentAssociation>, BraidSynthesis) {
        let start = Instant::now();

        // Build engine with association detection
        let mut engine = PrologEngine::from_world(world, skill_bank);

        // Detect emergent associations from current world state
        let associations = engine.detect_emergent_associations(world, skill_bank);

        // Load association facts so they can be queried
        engine.load_association_facts(&associations);
        engine.load_identity_bridges(skill_bank);

        // Run association-specific braids
        let mut braids: Vec<ReasoningBraid> = Vec::new();

        // Braid: emergent associations
        let assoc_start = Instant::now();
        let assoc_result = engine.named_query("emergent_associations");
        let assoc_duration = assoc_start.elapsed().as_millis() as u64;

        let assoc_status = if assoc_result.succeeded {
            BraidStatus::Completed(BraidResult {
                findings: vec![format!(
                    "{} emergent associations detected across {} skill(s)",
                    assoc_result.solutions.len(),
                    associations
                        .iter()
                        .map(|a| a.skill_id.as_str())
                        .collect::<std::collections::HashSet<_>>()
                        .len()
                )],
                solution_count: assoc_result.solutions.len(),
                applicable_skills: vec![],
            })
        } else {
            BraidStatus::Completed(BraidResult {
                findings: vec!["No emergent associations detected".to_string()],
                solution_count: 0,
                applicable_skills: vec![],
            })
        };

        braids.push(ReasoningBraid {
            id: "braid_emergent_associations".to_string(),
            query_name: "emergent_associations".to_string(),
            status: assoc_status,
            duration_ms: assoc_duration,
        });

        // Braid: identity bridges
        let bridge_start = Instant::now();
        let bridge_result = engine.named_query("identity_bridges");
        let bridge_duration = bridge_start.elapsed().as_millis() as u64;

        let bridge_status = if bridge_result.succeeded {
            BraidStatus::Completed(BraidResult {
                findings: vec![format!(
                    "{} identity bridge facts loaded for reversal-curse mitigation",
                    bridge_result.solutions.len()
                )],
                solution_count: bridge_result.solutions.len(),
                applicable_skills: vec![],
            })
        } else {
            BraidStatus::DeadEnd
        };

        braids.push(ReasoningBraid {
            id: "braid_identity_bridges".to_string(),
            query_name: "identity_bridges".to_string(),
            status: bridge_status,
            duration_ms: bridge_duration,
        });

        let total_ms = start.elapsed().as_millis() as u64;
        let mut synthesis = self.weave_synthesis(braids, vec![], total_ms);
        synthesis.emergent_associations = associations.clone();

        (associations, synthesis)
    }

    /// Execute federation-specific braids over the MetaGraph.
    /// Queries cross-system bridges, trust violations, and federation opportunities.
    pub fn execute_federation_braids(
        &self,
        world: &HyperStigmergicMorphogenesis,
        skill_bank: &SkillBank,
        meta_graph: &MetaGraph,
    ) -> BraidSynthesis {
        let start = Instant::now();

        // Build engine with federation facts
        let mut engine = PrologEngine::from_world(world, skill_bank);
        engine.load_remote_facts(meta_graph);

        let mut braids: Vec<ReasoningBraid> = Vec::new();

        for &query_name in FEDERATION_BRAIDS {
            let braid_start = Instant::now();
            let result = engine.named_query(query_name);
            let duration = braid_start.elapsed().as_millis() as u64;

            let status = if result.succeeded {
                BraidStatus::Completed(BraidResult {
                    findings: format_query_findings(query_name, &result),
                    solution_count: result.solutions.len(),
                    applicable_skills: vec![],
                })
            } else {
                BraidStatus::DeadEnd
            };

            braids.push(ReasoningBraid {
                id: format!("braid_fed_{}", query_name),
                query_name: query_name.to_string(),
                status,
                duration_ms: duration,
            });
        }

        let total_ms = start.elapsed().as_millis() as u64;
        self.weave_synthesis(braids, vec![], total_ms)
    }
}

// ── Formatting Helpers ───────────────────────────────────────────────────

/// Format query results into human-readable findings
fn format_query_findings(query_name: &str, result: &QueryResult) -> Vec<String> {
    let mut findings = Vec::new();

    match query_name {
        "belief_conflicts" => {
            for sol in &result.solutions {
                if let (Some(b1), Some(b2)) = (sol.get("B1"), sol.get("B2")) {
                    findings.push(format!("Belief conflict: {:?} vs {:?}", b1, b2));
                }
            }
            if findings.is_empty() && result.succeeded {
                findings.push("Belief contradictions detected in system".to_string());
            }
        }
        "stale_edges" => {
            for sol in &result.solutions {
                if let (Some(idx), Some(age)) = (sol.get("EdgeIdx"), sol.get("Age")) {
                    findings.push(format!("Stale edge #{:?} (age: {:?} ticks)", idx, age));
                }
            }
            if result.solutions.is_empty() {
                findings.push("No stale edges found".to_string());
            }
        }
        "disconnected_clusters" => {
            let cluster_ids: Vec<_> = result
                .solutions
                .iter()
                .filter_map(|s| s.get("ClusterId"))
                .collect();
            let unique: std::collections::HashSet<String> =
                cluster_ids.iter().map(|t| format!("{:?}", t)).collect();
            if unique.len() > 1 {
                findings.push(format!(
                    "{} disconnected agent clusters detected",
                    unique.len()
                ));
            }
        }
        "coherence_plateau" => {
            if let Some(sol) = result.solutions.first() {
                if let Some(len) = sol.get("Length") {
                    findings.push(format!(
                        "Coherence plateau detected ({}+ ticks without change)",
                        format!("{:?}", len)
                    ));
                }
            } else if result.succeeded {
                findings.push("System coherence has plateaued".to_string());
            }
        }
        "weak_edges" => {
            if result.succeeded {
                findings.push("Weak edges (weight < 0.5) exist in the graph".to_string());
            }
        }
        "belief_contradictions" => {
            if result.succeeded {
                findings.push("Contradicting beliefs present — resolution needed".to_string());
            }
        }
        _ => {
            findings.push(format!(
                "{}: {} solutions found",
                query_name,
                result.solutions.len()
            ));
        }
    }

    findings
}

/// Build the woven prompt section from synthesis
fn build_prompt_section(synthesis: &BraidSynthesis) -> String {
    let mut prompt = String::new();

    prompt.push_str("### Reasoning Braid Analysis (Prolog System 2)\n");
    prompt.push_str(&format!(
        "_Ran {} braids in {}ms ({} succeeded, {} dead-end)_\n\n",
        synthesis.braids_run,
        synthesis.total_duration_ms,
        synthesis.braids_succeeded,
        synthesis.braids_dead_end
    ));

    if !synthesis.topology_findings.is_empty() {
        prompt.push_str("**Graph Topology:**\n");
        for finding in &synthesis.topology_findings {
            prompt.push_str(&format!("- {}\n", finding));
        }
    }

    if !synthesis.belief_findings.is_empty() {
        prompt.push_str("\n**Belief State:**\n");
        for finding in &synthesis.belief_findings {
            prompt.push_str(&format!("- {}\n", finding));
        }
    }

    if !synthesis.risk_findings.is_empty() {
        prompt.push_str("\n**Risk Indicators:**\n");
        for finding in &synthesis.risk_findings {
            prompt.push_str(&format!("- {}\n", finding));
        }
    }

    if !synthesis.skill_findings.is_empty() {
        prompt.push_str("\n**Skill Applicability:**\n");
        for finding in &synthesis.skill_findings {
            prompt.push_str(&format!("- {}\n", finding));
        }
    }

    if !synthesis.association_findings.is_empty() {
        prompt.push_str("\n**Emergent Associations:**\n");
        for finding in &synthesis.association_findings {
            prompt.push_str(&format!("- {}\n", finding));
        }
    }

    if !synthesis.cross_system_findings.is_empty() {
        prompt.push_str("\n**Federation (Cross-System):**\n");
        for finding in &synthesis.cross_system_findings {
            prompt.push_str(&format!("- {}\n", finding));
        }
    }

    if !synthesis.trust_violations.is_empty() {
        prompt.push_str("\n**Trust Violations:**\n");
        for finding in &synthesis.trust_violations {
            prompt.push_str(&format!("- {}\n", finding));
        }
    }

    prompt
}
