//! DreamAdvisor — Bridge from Dream Patterns to Task Routing
//!
//! Converts CrystallizedPatterns (from the dream engine) and campaign feedback
//! patterns into per-role, per-domain routing adjustments. The advisor pre-computes
//! a lookup table so that `advise()` is O(1) during task routing.
//!
//! # Feedback Loop
//!
//! ```text
//! Campaign metrics → extract_dream_patterns() → DreamAdvisor.ingest()
//!                                                      ↓
//! CrystallizedPatterns → DreamAdvisor.ingest_crystallized()
//!                                                      ↓
//!                                             pre-computed lookup
//!                                             (BusinessRole, domain) → f64
//!                                                      ↓
//! route_task() → bid_with_context(desc, Some(&advisor))
//! ```

use crate::autonomous_team::BusinessRole;
use crate::dream::CrystallizedPattern;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

// ═══════════════════════════════════════════════════════════════════
// Section 1: DreamAdvisor
// ═══════════════════════════════════════════════════════════════════

/// Pre-computed routing adjustments derived from dream patterns.
///
/// Stored per tenant alongside TeamOrchestrator state.
/// The core data structure is a `HashMap<(BusinessRole, String), f64>`
/// mapping (role, domain_key) to a routing adjustment in [-1.0, 1.0].
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DreamAdvisor {
    /// (BusinessRole, domain_key) → adjustment in [-1.0, 1.0].
    /// Positive = boost this role for this domain, negative = penalty.
    ///
    /// Serialized as `HashMap<String, f64>` with keys like "Ceo:domain_key"
    /// because JSON doesn't support tuple keys.
    #[serde(
        serialize_with = "serialize_role_domain_map",
        deserialize_with = "deserialize_role_domain_map"
    )]
    role_domain_adjustments: HashMap<(BusinessRole, String), f64>,

    /// Per-role aggregate adjustment (fallback when no domain key matches).
    role_adjustments: HashMap<BusinessRole, f64>,

    /// Dream-learned keywords per role, supplementing static activation_keywords.
    /// Populated from ProtoSkill.associated_task_keys and CrystallizedPattern motifs.
    pub expanded_keywords: HashMap<BusinessRole, Vec<String>>,

    /// Monotonically increasing generation counter. Incremented on each refresh.
    pub generation: u64,
}

// ── Custom serde for tuple-keyed HashMap ────────────────────────────

fn serialize_role_domain_map<S>(
    map: &HashMap<(BusinessRole, String), f64>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    use serde::ser::SerializeMap;
    let mut ser_map = serializer.serialize_map(Some(map.len()))?;
    for ((role, domain), adj) in map {
        let key = format!("{:?}:{}", role, domain);
        ser_map.serialize_entry(&key, adj)?;
    }
    ser_map.end()
}

fn deserialize_role_domain_map<'de, D>(
    deserializer: D,
) -> Result<HashMap<(BusinessRole, String), f64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let string_map: HashMap<String, f64> = HashMap::deserialize(deserializer)?;
    let mut result = HashMap::new();
    for (key, adj) in string_map {
        if let Some(colon_pos) = key.find(':') {
            let role_str = &key[..colon_pos];
            let domain = key[colon_pos + 1..].to_string();
            if let Some(role) = parse_business_role(role_str) {
                result.insert((role, domain), adj);
            }
        }
    }
    Ok(result)
}

fn parse_business_role(s: &str) -> Option<BusinessRole> {
    match s {
        "Ceo" => Some(BusinessRole::Ceo),
        "Cto" => Some(BusinessRole::Cto),
        "Cfo" => Some(BusinessRole::Cfo),
        "Cmo" => Some(BusinessRole::Cmo),
        "Coo" => Some(BusinessRole::Coo),
        "Developer" => Some(BusinessRole::Developer),
        "Designer" => Some(BusinessRole::Designer),
        "Marketer" => Some(BusinessRole::Marketer),
        "Analyst" => Some(BusinessRole::Analyst),
        "Writer" => Some(BusinessRole::Writer),
        "Support" => Some(BusinessRole::Support),
        "Hr" => Some(BusinessRole::Hr),
        "Sales" => Some(BusinessRole::Sales),
        "Legal" => Some(BusinessRole::Legal),
        _ => None,
    }
}

impl DreamAdvisor {
    pub fn new() -> Self {
        Self::default()
    }

    // ── Campaign Pattern Ingestion ──────────────────────────────────

    /// Ingest campaign-derived dream patterns.
    ///
    /// Takes the `(domain, narrative, valence)` tuples from
    /// `CampaignStore::extract_dream_patterns()` and converts them into
    /// per-role routing adjustments using keyword relevance matching.
    pub fn ingest_campaign_patterns(&mut self, patterns: &[(String, String, f64)]) {
        for (domain, narrative, valence) in patterns {
            let narrative_lower = narrative.to_lowercase();

            for role in BusinessRole::all() {
                let relevance = Self::compute_role_relevance(*role, &narrative_lower);
                if relevance > 0.0 {
                    let adjustment = valence * relevance * 0.5; // Dampen to avoid volatile swings
                    let key = (*role, domain.clone());
                    let entry = self.role_domain_adjustments.entry(key).or_insert(0.0);
                    // Exponential moving average: old * 0.7 + new * 0.3
                    *entry = (*entry * 0.7 + adjustment * 0.3).clamp(-1.0, 1.0);
                }
            }
        }

        self.recompute_role_aggregates();
        self.generation += 1;
    }

    // ── Crystallized Pattern Ingestion ──────────────────────────────

    /// Ingest crystallized patterns directly from the dream engine.
    ///
    /// Uses `role_affinity: HashMap<Role, f64>` (council roles) and converts
    /// to BusinessRole affinities via the `to_council_role()` reverse mapping.
    /// Filters out low-quality patterns (confidence < 0.3 or persistence < 0.1).
    pub fn ingest_crystallized_patterns(&mut self, patterns: &[CrystallizedPattern]) {
        for pattern in patterns {
            if pattern.confidence < 0.3 || pattern.persistence_score < 0.1 {
                continue;
            }

            let task_keys = &pattern.motif.associated_task_keys;

            for role in BusinessRole::all() {
                let council_role = role.to_council_role();

                let affinity = pattern
                    .role_affinity
                    .get(&council_role)
                    .copied()
                    .unwrap_or(0.0);

                if affinity.abs() < 0.01 {
                    continue;
                }

                // Weighted: affinity × valence × confidence × persistence
                let adjustment =
                    affinity * pattern.valence * pattern.confidence * pattern.persistence_score.min(1.0);

                for key in task_keys {
                    let entry = self
                        .role_domain_adjustments
                        .entry((*role, key.clone()))
                        .or_insert(0.0);
                    *entry = (*entry * 0.7 + adjustment * 0.3).clamp(-1.0, 1.0);
                }

                // Expand keyword vocabulary for this role from task_keys
                let expanded = self.expanded_keywords.entry(*role).or_default();
                for key in task_keys {
                    if !expanded.contains(key) {
                        expanded.push(key.clone());
                    }
                }
            }
        }

        self.recompute_role_aggregates();
        self.generation += 1;
    }

    // ── Query Interface ─────────────────────────────────────────────

    /// Core query: what routing adjustment should this role receive
    /// for a task matching these domain keys?
    ///
    /// Returns a value in [-1.0, 1.0]. Callers multiply by the dream
    /// weight (W_dream) in the bid formula.
    ///
    /// Complexity: O(task_keys.len()) HashMap lookups — effectively O(1).
    pub fn advise(&self, role: BusinessRole, task_keys: &[&str]) -> f64 {
        if self.role_domain_adjustments.is_empty() {
            return 0.0;
        }

        // Check specific domain matches first
        let mut total = 0.0;
        let mut matches = 0;

        for key in task_keys {
            if let Some(&adj) = self.role_domain_adjustments.get(&(role, key.to_string())) {
                total += adj;
                matches += 1;
            }
        }

        if matches > 0 {
            (total / matches as f64).clamp(-1.0, 1.0)
        } else {
            // Fall back to aggregate role adjustment
            self.role_adjustments.get(&role).copied().unwrap_or(0.0)
        }
    }

    /// Count additional keyword hits from dream-learned vocabulary.
    ///
    /// These supplement the static `BusinessRole::activation_keywords()`.
    pub fn expanded_keyword_hits(&self, role: BusinessRole, task_description: &str) -> usize {
        let lower = task_description.to_lowercase();
        self.expanded_keywords
            .get(&role)
            .map(|keys| {
                keys.iter()
                    .filter(|k| lower.contains(&k.to_lowercase()))
                    .count()
            })
            .unwrap_or(0)
    }

    /// Whether this advisor has any learned data.
    pub fn is_empty(&self) -> bool {
        self.role_domain_adjustments.is_empty()
    }

    // ── Persistence ─────────────────────────────────────────────────

    /// Save advisor state to disk.
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let file = path.join("dream_advisor.json");
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(file, json)?;
        Ok(())
    }

    /// Load advisor state from disk. Returns default if file doesn't exist.
    pub fn load(path: &Path) -> Self {
        let file = path.join("dream_advisor.json");
        if file.exists() {
            if let Ok(data) = std::fs::read_to_string(&file) {
                if let Ok(advisor) = serde_json::from_str(&data) {
                    return advisor;
                }
            }
        }
        Self::default()
    }

    // ── Internal Helpers ────────────────────────────────────────────

    /// Compute how relevant a narrative is to a BusinessRole using
    /// its static activation keywords. Fast substring matching.
    fn compute_role_relevance(role: BusinessRole, narrative_lower: &str) -> f64 {
        let keywords = role.activation_keywords();
        let hits = keywords
            .iter()
            .filter(|kw| narrative_lower.contains(**kw))
            .count();
        (hits as f64 * 0.25).min(1.0)
    }

    /// Recompute aggregate per-role adjustments from domain-specific data.
    fn recompute_role_aggregates(&mut self) {
        self.role_adjustments.clear();
        let mut role_sums: HashMap<BusinessRole, (f64, usize)> = HashMap::new();

        for ((role, _), &adj) in &self.role_domain_adjustments {
            let entry = role_sums.entry(*role).or_insert((0.0, 0));
            entry.0 += adj;
            entry.1 += 1;
        }

        for (role, (sum, count)) in role_sums {
            if count > 0 {
                self.role_adjustments.insert(role, sum / count as f64);
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// Section 2: Tests
// ═══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::Role;
    use crate::dream::TemporalMotif;
    use crate::stigmergic_policy::TraceKind;

    #[test]
    fn test_empty_advisor_returns_zero() {
        let advisor = DreamAdvisor::new();
        assert_eq!(advisor.advise(BusinessRole::Writer, &["blog"]), 0.0);
        assert_eq!(advisor.advise(BusinessRole::Ceo, &[]), 0.0);
        assert!(advisor.is_empty());
    }

    #[test]
    fn test_campaign_pattern_positive_ingestion() {
        let mut advisor = DreamAdvisor::new();
        let patterns = vec![(
            "campaign:blog_push".to_string(),
            "Blog campaign with great content writing and copy growth".to_string(),
            1.0,
        )];
        advisor.ingest_campaign_patterns(&patterns);

        assert!(!advisor.is_empty());
        assert_eq!(advisor.generation, 1);

        // Writer has keywords like "blog", "copy", "docs" — should match
        let writer_signal = advisor.advise(BusinessRole::Writer, &["campaign:blog_push"]);
        assert!(
            writer_signal > 0.0,
            "Writer should benefit from positive blog campaign, got {}",
            writer_signal
        );

        // Developer has no blog keywords — should get zero or very low
        let dev_signal = advisor.advise(BusinessRole::Developer, &["campaign:blog_push"]);
        assert!(
            writer_signal > dev_signal,
            "Writer ({}) should beat Developer ({}) for blog campaign",
            writer_signal,
            dev_signal
        );
    }

    #[test]
    fn test_campaign_pattern_negative_ingestion() {
        let mut advisor = DreamAdvisor::new();
        let patterns = vec![(
            "campaign:reddit_fail".to_string(),
            "Reddit social media campaign with terrible engagement and growth".to_string(),
            -1.0,
        )];
        advisor.ingest_campaign_patterns(&patterns);

        // Marketer has "social", "campaign", "growth" — should take penalty
        let marketer_signal = advisor.advise(BusinessRole::Marketer, &["campaign:reddit_fail"]);
        assert!(
            marketer_signal < 0.0,
            "Negative campaign should penalize Marketer, got {}",
            marketer_signal
        );
    }

    #[test]
    fn test_crystallized_pattern_ingestion() {
        let mut advisor = DreamAdvisor::new();
        let mut role_affinity = HashMap::new();
        // Catalyst maps to CMO, Marketer, Sales
        role_affinity.insert(Role::Catalyst, 0.8);

        let pattern = CrystallizedPattern {
            id: "test_1".into(),
            narrative: "Effective social media pattern".into(),
            embedding: vec![],
            motif: TemporalMotif {
                trace_sequence: vec![TraceKind::PromiseMade, TraceKind::PromiseResolved],
                typical_duration_ticks: 10,
                associated_task_keys: vec!["social_media".into(), "content_strategy".into()],
                transition_weights: vec![1.0],
                min_match_length: 2,
            },
            valence: 0.8,
            confidence: 0.9,
            observation_count: 10,
            role_affinity,
            origin_generation: 1,
            last_reinforced_generation: 3,
            temporal_reach: 50,
            persistence_score: 0.9,
            created_at: 0,
            last_reinforced_at: 0,
        };

        advisor.ingest_crystallized_patterns(&[pattern]);

        // CMO maps to Catalyst — should get boosted
        let cmo_signal = advisor.advise(BusinessRole::Cmo, &["social_media"]);
        assert!(
            cmo_signal > 0.0,
            "CMO (Catalyst) should get boosted for social_media, got {}",
            cmo_signal
        );

        // Expanded keywords should exist for CMO
        let hits =
            advisor.expanded_keyword_hits(BusinessRole::Cmo, "social_media content_strategy work");
        assert!(
            hits > 0,
            "CMO should have expanded keywords from dream patterns"
        );
    }

    #[test]
    fn test_low_quality_patterns_filtered() {
        let mut advisor = DreamAdvisor::new();

        let pattern = CrystallizedPattern {
            id: "low_q".into(),
            narrative: "Low quality".into(),
            embedding: vec![],
            motif: TemporalMotif {
                trace_sequence: vec![],
                typical_duration_ticks: 0,
                associated_task_keys: vec!["test".into()],
                transition_weights: vec![],
                min_match_length: 1,
            },
            valence: 1.0,
            confidence: 0.1, // Too low
            observation_count: 1,
            role_affinity: HashMap::new(),
            origin_generation: 0,
            last_reinforced_generation: 0,
            temporal_reach: 0,
            persistence_score: 0.05, // Also too low
            created_at: 0,
            last_reinforced_at: 0,
        };

        advisor.ingest_crystallized_patterns(&[pattern]);
        // Should remain empty since pattern was filtered
        assert!(advisor.is_empty());
    }

    #[test]
    fn test_aggregate_fallback() {
        let mut advisor = DreamAdvisor::new();
        // Ingest a pattern for a specific domain
        let patterns = vec![(
            "campaign:email_blast".to_string(),
            "Email marketing campaign drove great growth".to_string(),
            1.0,
        )];
        advisor.ingest_campaign_patterns(&patterns);

        // Query with a DIFFERENT domain key — should fall back to aggregate
        let marketer_fallback = advisor.advise(BusinessRole::Marketer, &["unknown_domain"]);
        // The aggregate should be non-zero because we ingested something for Marketer
        let marketer_specific = advisor.advise(BusinessRole::Marketer, &["campaign:email_blast"]);

        // Specific should be >= aggregate (or both positive)
        assert!(
            marketer_specific >= marketer_fallback,
            "Specific ({}) should be >= aggregate ({})",
            marketer_specific,
            marketer_fallback
        );
    }

    #[test]
    fn test_advise_performance() {
        let mut advisor = DreamAdvisor::new();
        // Populate with many patterns
        for i in 0..100 {
            let patterns = vec![(
                format!("domain_{}", i),
                format!("Pattern {} with content and social growth campaign", i),
                if i % 2 == 0 { 1.0 } else { -1.0 },
            )];
            advisor.ingest_campaign_patterns(&patterns);
        }

        let start = std::time::Instant::now();
        for _ in 0..14 {
            // 14 members per route_task
            advisor.advise(BusinessRole::Cmo, &["domain_0", "domain_5"]);
        }
        let elapsed = start.elapsed();
        assert!(
            elapsed.as_millis() < 5,
            "14 advise calls should be fast, took {}ms",
            elapsed.as_millis()
        );
    }

    #[test]
    fn test_serde_roundtrip() {
        let mut advisor = DreamAdvisor::new();
        advisor.ingest_campaign_patterns(&[(
            "test".to_string(),
            "blog content writing growth".to_string(),
            0.5,
        )]);

        let json = serde_json::to_string(&advisor).unwrap();
        let restored: DreamAdvisor = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.generation, advisor.generation);
        assert!(!restored.is_empty());
    }

    #[test]
    fn test_persistence_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let mut advisor = DreamAdvisor::new();
        advisor.ingest_campaign_patterns(&[(
            "persist_test".to_string(),
            "Blog content writing campaign success".to_string(),
            1.0,
        )]);

        advisor.save(tmp.path()).unwrap();

        let loaded = DreamAdvisor::load(tmp.path());
        assert_eq!(loaded.generation, 1);
        assert!(!loaded.is_empty());

        let signal = loaded.advise(BusinessRole::Writer, &["persist_test"]);
        assert!(signal > 0.0);
    }
}
