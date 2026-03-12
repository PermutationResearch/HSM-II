use crate::federation::{SystemId, TrustGraph, TrustPolicy};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RuntimeThresholds {
    pub coherence_min: f64,
    pub stability_min: f64,
    pub mean_trust_min: f64,
    pub council_confidence_min: f64,
    pub evidence_coverage_min: f64,
}

impl Default for RuntimeThresholds {
    fn default() -> Self {
        Self {
            coherence_min: 0.70,
            stability_min: 0.28,
            mean_trust_min: 0.65,
            council_confidence_min: 0.65,
            evidence_coverage_min: 1.0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RuntimeSnapshot {
    pub coherence: f64,
    pub stability: f64,
    pub mean_trust: f64,
    pub council_confidence: Option<f64>,
    pub evidence_coverage: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RuntimeSloReport {
    pub healthy: bool,
    pub failed_checks: Vec<String>,
}

pub fn evaluate_runtime_slos(
    snapshot: &RuntimeSnapshot,
    thresholds: &RuntimeThresholds,
) -> RuntimeSloReport {
    let mut failed_checks = Vec::new();

    if snapshot.coherence < thresholds.coherence_min {
        failed_checks.push(format!(
            "coherence below threshold: {:.3} < {:.3}",
            snapshot.coherence, thresholds.coherence_min
        ));
    }
    if snapshot.stability < thresholds.stability_min {
        failed_checks.push(format!(
            "stability below threshold: {:.3} < {:.3}",
            snapshot.stability, thresholds.stability_min
        ));
    }
    if snapshot.mean_trust < thresholds.mean_trust_min {
        failed_checks.push(format!(
            "mean trust below threshold: {:.3} < {:.3}",
            snapshot.mean_trust, thresholds.mean_trust_min
        ));
    }
    if let Some(conf) = snapshot.council_confidence {
        if conf < thresholds.council_confidence_min {
            failed_checks.push(format!(
                "council confidence below threshold: {:.3} < {:.3}",
                conf, thresholds.council_confidence_min
            ));
        }
    }
    if let Some(cov) = snapshot.evidence_coverage {
        if cov < thresholds.evidence_coverage_min {
            failed_checks.push(format!(
                "evidence coverage below threshold: {:.3} < {:.3}",
                cov, thresholds.evidence_coverage_min
            ));
        }
    }

    RuntimeSloReport {
        healthy: failed_checks.is_empty(),
        failed_checks,
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ExportCadence {
    PerHighRiskAction,
    Hourly,
    Daily,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExportScheduler {
    pub cadence: ExportCadence,
    pub last_export_unix_secs: Option<u64>,
}

impl ExportScheduler {
    pub fn should_export(&self, now_unix_secs: u64, high_risk_action_happened: bool) -> bool {
        match self.cadence {
            ExportCadence::PerHighRiskAction => high_risk_action_happened,
            ExportCadence::Hourly => self
                .last_export_unix_secs
                .map(|last| now_unix_secs.saturating_sub(last) >= 3600)
                .unwrap_or(true),
            ExportCadence::Daily => self
                .last_export_unix_secs
                .map(|last| now_unix_secs.saturating_sub(last) >= 86_400)
                .unwrap_or(true),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MeshHealth {
    pub expected_directed_edges: usize,
    pub observed_directed_edges: usize,
    pub missing_edges: Vec<(SystemId, SystemId)>,
    pub complete: bool,
}

pub fn evaluate_full_mesh(
    local_system: &str,
    peers: &[String],
    trust_graph: &TrustGraph,
) -> MeshHealth {
    let mut nodes = Vec::with_capacity(peers.len() + 1);
    nodes.push(local_system.to_string());
    nodes.extend(peers.iter().cloned());
    nodes.sort();
    nodes.dedup();

    let mut missing = Vec::new();
    let mut expected = 0usize;
    let edge_keys: HashSet<(SystemId, SystemId)> = trust_graph.edges.keys().cloned().collect();

    for from in &nodes {
        for to in &nodes {
            if from == to {
                continue;
            }
            expected += 1;
            if !edge_keys.contains(&(from.clone(), to.clone())) {
                missing.push((from.clone(), to.clone()));
            }
        }
    }

    let observed = expected.saturating_sub(missing.len());
    MeshHealth {
        expected_directed_edges: expected,
        observed_directed_edges: observed,
        missing_edges: missing,
        complete: observed == expected,
    }
}

pub fn default_trust_policy() -> TrustPolicy {
    TrustPolicy::default()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum MemoryEventKind {
    BeliefAdded { key: String, confidence: f64 },
    ExperienceRecorded { id: String, positive: bool },
    SkillPromoted { skill_id: String },
    ActionAudited { action_id: String, approved: bool },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MemoryEvent {
    pub id: String,
    pub timestamp: u64,
    pub kind: MemoryEventKind,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct MutableMemoryCache {
    pub beliefs: HashMap<String, f64>,
    pub experience_count: u64,
    pub positive_experience_count: u64,
    pub promoted_skills: HashSet<String>,
    pub audited_actions: HashMap<String, bool>,
    pub last_event_id: Option<String>,
}

impl MutableMemoryCache {
    pub fn apply_event(&mut self, event: &MemoryEvent) {
        match &event.kind {
            MemoryEventKind::BeliefAdded { key, confidence } => {
                self.beliefs.insert(key.clone(), *confidence);
            }
            MemoryEventKind::ExperienceRecorded { positive, .. } => {
                self.experience_count += 1;
                if *positive {
                    self.positive_experience_count += 1;
                }
            }
            MemoryEventKind::SkillPromoted { skill_id } => {
                self.promoted_skills.insert(skill_id.clone());
            }
            MemoryEventKind::ActionAudited {
                action_id,
                approved,
            } => {
                self.audited_actions.insert(action_id.clone(), *approved);
            }
        }
        self.last_event_id = Some(event.id.clone());
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct EventSourcedMemory {
    pub events: Vec<MemoryEvent>,
    pub cache: MutableMemoryCache,
}

impl EventSourcedMemory {
    pub fn append(&mut self, event: MemoryEvent) {
        self.cache.apply_event(&event);
        self.events.push(event);
    }

    pub fn replay(&self) -> MutableMemoryCache {
        let mut cache = MutableMemoryCache::default();
        for event in &self.events {
            cache.apply_event(event);
        }
        cache
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluates_slos_with_failures() {
        let report = evaluate_runtime_slos(
            &RuntimeSnapshot {
                coherence: 0.4,
                stability: 0.1,
                mean_trust: 0.2,
                council_confidence: Some(0.4),
                evidence_coverage: Some(0.8),
            },
            &RuntimeThresholds::default(),
        );
        assert!(!report.healthy);
        assert!(report.failed_checks.len() >= 3);
    }

    #[test]
    fn detects_missing_mesh_edges() {
        let graph = TrustGraph::default();
        let mesh = evaluate_full_mesh("a", &[String::from("b"), String::from("c")], &graph);
        assert!(!mesh.complete);
        assert_eq!(mesh.expected_directed_edges, 6);
    }

    #[test]
    fn replay_matches_mutable_cache() {
        let mut memory = EventSourcedMemory::default();
        memory.append(MemoryEvent {
            id: "e1".to_string(),
            timestamp: 1,
            kind: MemoryEventKind::BeliefAdded {
                key: "k".to_string(),
                confidence: 0.8,
            },
        });
        memory.append(MemoryEvent {
            id: "e2".to_string(),
            timestamp: 2,
            kind: MemoryEventKind::ExperienceRecorded {
                id: "x".to_string(),
                positive: true,
            },
        });
        let replayed = memory.replay();
        assert_eq!(replayed.experience_count, memory.cache.experience_count);
        assert_eq!(
            replayed.positive_experience_count,
            memory.cache.positive_experience_count
        );
        assert_eq!(replayed.beliefs.get("k").copied(), Some(0.8));
    }
}
