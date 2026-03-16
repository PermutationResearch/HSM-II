//! StigmergicDreamEngine — orchestrates the full 6-phase dream cycle.
//!
//! Phase 1: Trajectory Assembly (trajectory.rs)
//! Phase 2: Temporal Motif Detection (motif.rs)
//! Phase 3: Pattern Crystallization (crystallize.rs)
//! Phase 4: Stigmergic Deposition + Temporal Credit (deposit.rs)
//! Phase 5: DKS Survival Pressure (survival.rs)
//! Phase 6: Proto-Skill Generation (crystallize.rs)
//!
//! The engine maintains its own state (crystallized patterns, generation counter)
//! and operates on borrowed world + memory references during each dream cycle.

use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use crate::hyper_stigmergy::HyperStigmergicMorphogenesis;
use crate::rlm::LivingPrompt;

use super::crystallize::{crystallize, generate_proto_skills};
use super::deposit::{deposit, DepositionResult};
use super::motif::detect_motifs;
use super::survival::{apply_survival_pressure, SurvivalResult};
use super::trajectory::assemble_trajectory;
use super::{
    CrystallizedPattern, DreamConfig, DreamCycleResult, ProtoSkill,
};

/// The Stigmergic Dream Consolidation Engine.
///
/// Maintains crystallized patterns across dream cycles and
/// orchestrates the full 6-phase dream pipeline.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StigmergicDreamEngine {
    pub config: DreamConfig,
    /// All crystallized patterns (the "dream memory")
    pub patterns: Vec<CrystallizedPattern>,
    /// Current dream generation counter
    pub dream_generation: u64,
    /// History of dream cycle results (last N)
    pub history: Vec<DreamCycleResult>,
    /// Maximum history entries to keep
    pub max_history: usize,
}

impl Default for StigmergicDreamEngine {
    fn default() -> Self {
        Self {
            config: DreamConfig::default(),
            patterns: Vec::new(),
            dream_generation: 0,
            history: Vec::new(),
            max_history: 50,
        }
    }
}

impl StigmergicDreamEngine {
    pub fn new(config: DreamConfig) -> Self {
        Self {
            config,
            ..Self::default()
        }
    }

    /// Check if it's time to dream based on tick count.
    pub fn should_dream(&self, current_tick: u64) -> bool {
        current_tick > 0 && current_tick % self.config.dream_interval == 0
    }

    /// Run a full dream consolidation cycle.
    ///
    /// This is the main entry point, called from the Conductor after
    /// Phase 6 (skill distillation / consensus) when should_dream() is true.
    ///
    /// # Arguments
    /// * `world` — mutable reference to the HSM world (includes stigmergic_memory, skill_bank, etc.)
    /// * `living_prompt` — mutable reference (for injecting consolidated insights)
    pub fn dream(
        &mut self,
        world: &mut HyperStigmergicMorphogenesis,
        living_prompt: &mut LivingPrompt,
    ) -> DreamCycleResult {
        let start = std::time::Instant::now();
        self.dream_generation += 1;
        let gen = self.dream_generation;

        info!(
            "Dream cycle {} starting (replay_horizon={}, patterns={})",
            gen,
            self.config.replay_horizon,
            self.patterns.len()
        );

        // ── Phase 1: Trajectory Assembly ───────────────────────────────
        let trajectory = assemble_trajectory(
            &self.config,
            &world.stigmergic_memory,
            &world.experiences,
            &world.beliefs,
        );
        let traces_replayed = trajectory.len();

        if trajectory.is_empty() {
            debug!("Dream cycle {}: no trajectory data, skipping", gen);
            let result = DreamCycleResult {
                dream_generation: gen,
                traces_replayed: 0,
                dream_duration_ms: start.elapsed().as_millis() as u64,
                total_patterns_alive: self.patterns.len(),
                ..DreamCycleResult::default()
            };
            self.push_history(result.clone());
            return result;
        }

        debug!(
            "Dream cycle {}: assembled {} trajectory elements",
            gen, traces_replayed
        );

        // ── Phase 2: Temporal Motif Detection ──────────────────────────
        let motifs = detect_motifs(&self.config, &trajectory);
        let motifs_detected = motifs.len();

        debug!(
            "Dream cycle {}: detected {} motifs (min_obs={})",
            gen,
            motifs_detected,
            self.config.min_observations
        );

        // ── Phase 3: Pattern Crystallization ───────────────────────────
        let (new_patterns, reinforced) =
            crystallize(&self.config, &mut self.patterns, &motifs, gen);
        let patterns_crystallized = new_patterns.len();
        let patterns_reinforced = reinforced;

        debug!(
            "Dream cycle {}: crystallized {} new, reinforced {}",
            gen, patterns_crystallized, patterns_reinforced
        );

        // ── Phase 4: Stigmergic Deposition + Temporal Credit ──────────
        // Only deposit NEW patterns and strongly reinforced existing ones
        let patterns_to_deposit: Vec<CrystallizedPattern> = new_patterns
            .iter()
            .cloned()
            .chain(
                self.patterns
                    .iter()
                    .filter(|p| p.last_reinforced_generation == gen && p.confidence > 0.6)
                    .cloned(),
            )
            .collect();

        let dep_result: DepositionResult = deposit(
            &self.config,
            &patterns_to_deposit,
            world,
        );

        debug!(
            "Dream cycle {}: deposited {} trails, boosted {} traces, weakened {}",
            gen, dep_result.dream_trails_deposited, dep_result.traces_boosted, dep_result.traces_weakened
        );

        // ── Phase 5: DKS Survival Pressure ─────────────────────────────
        let surv_result: SurvivalResult =
            apply_survival_pressure(&self.config, &mut self.patterns, gen);

        debug!(
            "Dream cycle {}: survival — {} decayed, {} pruned, {} alive",
            gen, surv_result.patterns_decayed, surv_result.patterns_pruned, surv_result.patterns_after
        );

        // ── Phase 6: Proto-Skill Generation ────────────────────────────
        let proto_skills = generate_proto_skills(&self.config, &self.patterns);
        let proto_skills_generated = proto_skills.len();

        // Inject proto-skills into SkillBank via living prompt insights
        // (actual SkillBank injection should be done by the Conductor,
        //  here we just inject the guidance into the living prompt)
        if !proto_skills.is_empty() {
            inject_dream_insights(living_prompt, &proto_skills, &self.patterns, gen);
        }

        // Also inject consolidated wisdom from patterns
        if !self.patterns.is_empty() {
            inject_pattern_wisdom(living_prompt, &self.patterns);
        }

        let coherence = world.global_coherence();

        let result = DreamCycleResult {
            dream_generation: gen,
            traces_replayed,
            motifs_detected,
            patterns_crystallized,
            patterns_reinforced,
            patterns_decayed: surv_result.patterns_decayed,
            traces_boosted: dep_result.traces_boosted,
            traces_weakened: dep_result.traces_weakened,
            dream_trails_deposited: dep_result.dream_trails_deposited,
            proto_skills_generated,
            dream_duration_ms: start.elapsed().as_millis() as u64,
            coherence_at_dream: coherence,
            total_patterns_alive: self.patterns.len(),
        };

        info!(
            "Dream cycle {} complete: {} motifs → {} new patterns, {} reinforced, {} alive ({} ms)",
            gen,
            motifs_detected,
            patterns_crystallized,
            patterns_reinforced,
            self.patterns.len(),
            result.dream_duration_ms
        );

        self.push_history(result.clone());
        result
    }

    /// Retrieve patterns matching a given scenario/context for prompt injection.
    pub fn retrieve_patterns(&self, context: &str, top_k: usize) -> Vec<&CrystallizedPattern> {
        let context_lower = context.to_lowercase();

        let mut scored: Vec<(&CrystallizedPattern, f64)> = self
            .patterns
            .iter()
            .map(|p| {
                let mut score = 0.0;
                // Score based on task key overlap
                for key in &p.motif.associated_task_keys {
                    if context_lower.contains(&key.to_lowercase()) {
                        score += 0.5;
                    }
                }
                // Score based on narrative keyword overlap
                let narrative_lower = p.narrative.to_lowercase();
                let words: Vec<&str> = context_lower.split_whitespace().collect();
                for word in &words {
                    if word.len() > 3 && narrative_lower.contains(word) {
                        score += 0.2;
                    }
                }
                // Boost by confidence and persistence
                score *= p.confidence * p.persistence_score.max(0.1);
                (p, score)
            })
            .filter(|(_, s)| *s > 0.0)
            .collect();

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(top_k);
        scored.into_iter().map(|(p, _)| p).collect()
    }

    /// Get the most recent dream cycle result.
    pub fn last_result(&self) -> Option<&DreamCycleResult> {
        self.history.last()
    }

    /// Get total pattern count.
    pub fn pattern_count(&self) -> usize {
        self.patterns.len()
    }

    /// Get patterns by valence (positive/negative).
    pub fn positive_patterns(&self) -> Vec<&CrystallizedPattern> {
        self.patterns.iter().filter(|p| p.valence > 0.0).collect()
    }

    pub fn negative_patterns(&self) -> Vec<&CrystallizedPattern> {
        self.patterns.iter().filter(|p| p.valence < 0.0).collect()
    }

    fn push_history(&mut self, result: DreamCycleResult) {
        self.history.push(result);
        if self.history.len() > self.max_history {
            self.history.remove(0);
        }
    }
}

/// Inject proto-skill guidance and dream insights into the living prompt.
fn inject_dream_insights(
    living_prompt: &mut LivingPrompt,
    proto_skills: &[ProtoSkill],
    _patterns: &[CrystallizedPattern],
    generation: u64,
) {
    if !proto_skills.is_empty() {
        let skill_names: Vec<&str> = proto_skills.iter().map(|s| s.name.as_str()).collect();
        living_prompt.add_insight(format!(
            "[Dream Gen {}] {} proto-skills crystallized: {}",
            generation,
            proto_skills.len(),
            skill_names.join(", ")
        ));
    }
}

/// Inject wisdom from the most confident positive patterns into avoid/insight patterns.
fn inject_pattern_wisdom(
    living_prompt: &mut LivingPrompt,
    patterns: &[CrystallizedPattern],
) {
    // Find the single best positive pattern and inject as insight
    if let Some(best_positive) = patterns
        .iter()
        .filter(|p| p.valence > 0.1 && p.confidence > 0.7)
        .max_by(|a, b| {
            (a.confidence * a.persistence_score)
                .partial_cmp(&(b.confidence * b.persistence_score))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    {
        living_prompt.add_insight(format!(
            "[Dream] Beneficial pattern (conf {:.2}): {}",
            best_positive.confidence,
            &best_positive.narrative[..best_positive.narrative.len().min(200)]
        ));
    }

    // Find the single worst negative pattern and inject as avoid
    if let Some(worst_negative) = patterns
        .iter()
        .filter(|p| p.valence < -0.1 && p.confidence > 0.6)
        .min_by(|a, b| {
            a.valence
                .partial_cmp(&b.valence)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    {
        living_prompt.add_avoid_pattern(format!(
            "[Dream] Harmful pattern (conf {:.2}): {}",
            worst_negative.confidence,
            &worst_negative.narrative[..worst_negative.narrative.len().min(200)]
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use crate::dream::TemporalMotif;
    use crate::stigmergic_policy::TraceKind;

    fn make_test_pattern(
        id: &str,
        valence: f64,
        confidence: f64,
        persistence: f64,
        tasks: Vec<&str>,
    ) -> CrystallizedPattern {
        CrystallizedPattern {
            id: id.into(),
            narrative: format!("Pattern {} with tasks {:?}", id, tasks),
            embedding: vec![0.5; 12],
            motif: TemporalMotif {
                trace_sequence: vec![TraceKind::PromiseMade, TraceKind::PromiseResolved],
                typical_duration_ticks: 5,
                associated_task_keys: tasks.iter().map(|s| s.to_string()).collect(),
                transition_weights: vec![0.9],
                min_match_length: 2,
            },
            valence,
            confidence,
            observation_count: 10,
            role_affinity: HashMap::new(),
            origin_generation: 1,
            last_reinforced_generation: 1,
            temporal_reach: 50,
            persistence_score: persistence,
            created_at: 0,
            last_reinforced_at: 0,
        }
    }

    #[test]
    fn test_engine_default() {
        let engine = StigmergicDreamEngine::default();
        assert_eq!(engine.dream_generation, 0);
        assert!(engine.patterns.is_empty());
        assert!(engine.history.is_empty());
    }

    #[test]
    fn test_should_dream() {
        let engine = StigmergicDreamEngine::new(DreamConfig {
            dream_interval: 50,
            ..DreamConfig::default()
        });

        assert!(!engine.should_dream(0));
        assert!(!engine.should_dream(25));
        assert!(engine.should_dream(50));
        assert!(engine.should_dream(100));
        assert!(!engine.should_dream(73));
    }

    #[test]
    fn test_retrieve_patterns_by_context() {
        let mut engine = StigmergicDreamEngine::default();
        engine.patterns = vec![
            make_test_pattern("search", 0.7, 0.8, 1.0, vec!["search_code"]),
            make_test_pattern("deploy", 0.5, 0.6, 0.8, vec!["deploy_prod"]),
            make_test_pattern("unrelated", 0.3, 0.4, 0.5, vec!["other"]),
        ];

        let results = engine.retrieve_patterns("search_code for patterns", 5);
        assert!(!results.is_empty());
        assert_eq!(results[0].id, "search");
    }

    #[test]
    fn test_retrieve_patterns_empty() {
        let engine = StigmergicDreamEngine::default();
        let results = engine.retrieve_patterns("anything", 5);
        assert!(results.is_empty());
    }

    #[test]
    fn test_positive_negative_patterns() {
        let mut engine = StigmergicDreamEngine::default();
        engine.patterns = vec![
            make_test_pattern("pos1", 0.5, 0.8, 1.0, vec![]),
            make_test_pattern("neg1", -0.3, 0.6, 0.8, vec![]),
            make_test_pattern("pos2", 0.2, 0.7, 0.9, vec![]),
        ];

        assert_eq!(engine.positive_patterns().len(), 2);
        assert_eq!(engine.negative_patterns().len(), 1);
    }

    #[test]
    fn test_history_capped() {
        let mut engine = StigmergicDreamEngine::default();
        engine.max_history = 3;

        for i in 0..5 {
            engine.push_history(DreamCycleResult {
                dream_generation: i,
                ..DreamCycleResult::default()
            });
        }

        assert_eq!(engine.history.len(), 3);
        assert_eq!(engine.history[0].dream_generation, 2);
        assert_eq!(engine.history[2].dream_generation, 4);
    }

    #[test]
    fn test_dream_empty_world() {
        let mut engine = StigmergicDreamEngine::default();
        let mut world = HyperStigmergicMorphogenesis::new(0);
        let mut prompt = LivingPrompt {
            base_prompt: "test".into(),
            accumulated_insights: vec![],
            context_window: vec![],
            max_context: 10,
            evolution_history: vec![],
            mutation_count: 0,
            avoid_patterns: vec![],
        };

        let result = engine.dream(&mut world, &mut prompt);
        assert_eq!(result.dream_generation, 1);
        assert_eq!(result.traces_replayed, 0);
        assert_eq!(result.motifs_detected, 0);
    }

    #[test]
    fn test_engine_serde_roundtrip() {
        let mut engine = StigmergicDreamEngine::new(DreamConfig {
            dream_interval: 25,
            replay_horizon: 100,
            ..DreamConfig::default()
        });
        engine.dream_generation = 5;
        engine.patterns.push(make_test_pattern("test", 0.5, 0.8, 1.0, vec!["task_a"]));

        let json = serde_json::to_string(&engine).unwrap();
        let restored: StigmergicDreamEngine = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.dream_generation, 5);
        assert_eq!(restored.patterns.len(), 1);
        assert_eq!(restored.config.dream_interval, 25);
    }
}
