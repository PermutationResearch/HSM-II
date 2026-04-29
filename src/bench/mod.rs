//! Benchmark suite for HSM-II subsystems.
//!
//! Provides a lightweight, zero-dependency (beyond serde/serde_json) benchmark
//! harness for measuring performance of core subsystems: memory persistence,
//! context ranking, multi-perspective council simulation, and serialization cost.
//!
//! Usage:
//! ```no_run
//! use hyper_stigmergy::bench::BenchSuite;
//!
//! let mut suite = BenchSuite::new();
//! suite.bench("example/noop", 1000, || {
//!     std::hint::black_box(42);
//! });
//! suite.print_report();
//! ```

use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

// ── BenchResult ────────────────────────────────────────────────────────────

/// Result of a single benchmark run, capturing timing statistics and throughput.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BenchResult {
    /// Human-readable name (e.g. "memory/belief_insert")
    pub name: String,
    /// Number of timed iterations (excludes warmup)
    pub iterations: u64,
    /// Wall-clock sum of all timed iterations
    pub total_duration: Duration,
    /// Arithmetic mean of per-iteration durations
    pub avg_duration: Duration,
    /// Fastest single iteration
    pub min_duration: Duration,
    /// Slowest single iteration
    pub max_duration: Duration,
    /// Throughput: iterations / total_seconds
    pub ops_per_sec: f64,
    /// Arbitrary key-value metadata attached by benchmark functions
    pub metadata: HashMap<String, String>,
}

// ── BenchSuite ─────────────────────────────────────────────────────────────

/// Benchmark suite runner that collects results and produces reports.
pub struct BenchSuite {
    pub results: Vec<BenchResult>,
}

impl Default for BenchSuite {
    fn default() -> Self {
        Self::new()
    }
}

impl BenchSuite {
    pub fn new() -> Self {
        Self {
            results: Vec::new(),
        }
    }

    /// Run a synchronous benchmark.
    ///
    /// Performs a warmup phase of 10% of `iterations` (minimum 1), then times
    /// each of the `iterations` individually. Uses `std::hint::black_box`
    /// internally where appropriate to discourage the compiler from eliding
    /// the closure.
    pub fn bench<F: FnMut()>(&mut self, name: &str, iterations: u64, mut f: F) -> &BenchResult {
        assert!(iterations > 0, "iterations must be > 0");

        let warmup = (iterations / 10).max(1);
        for _ in 0..warmup {
            f();
        }

        let mut durations = Vec::with_capacity(iterations as usize);
        for _ in 0..iterations {
            let start = Instant::now();
            f();
            durations.push(start.elapsed());
        }

        let result = Self::compute_result(name, iterations, &durations);
        self.results.push(result);
        self.results.last().unwrap()
    }

    /// Run an async benchmark.
    ///
    /// Same warmup/timing strategy as [`bench`], but awaits a `Future<Output = ()>`
    /// produced by the closure on each iteration.
    pub async fn bench_async<F, Fut>(
        &mut self,
        name: &str,
        iterations: u64,
        mut f: F,
    ) -> &BenchResult
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = ()>,
    {
        assert!(iterations > 0, "iterations must be > 0");

        let warmup = (iterations / 10).max(1);
        for _ in 0..warmup {
            f().await;
        }

        let mut durations = Vec::with_capacity(iterations as usize);
        for _ in 0..iterations {
            let start = Instant::now();
            f().await;
            durations.push(start.elapsed());
        }

        let result = Self::compute_result(name, iterations, &durations);
        self.results.push(result);
        self.results.last().unwrap()
    }

    /// Run a benchmark that returns a value (prevents optimization via black_box).
    pub fn bench_with_result<F, R>(&mut self, name: &str, iterations: u64, mut f: F) -> &BenchResult
    where
        F: FnMut() -> R,
    {
        assert!(iterations > 0, "iterations must be > 0");

        let warmup = (iterations / 10).max(1);
        for _ in 0..warmup {
            std::hint::black_box(f());
        }

        let mut durations = Vec::with_capacity(iterations as usize);
        for _ in 0..iterations {
            let start = Instant::now();
            let r = f();
            durations.push(start.elapsed());
            std::hint::black_box(r);
        }

        let result = Self::compute_result(name, iterations, &durations);
        self.results.push(result);
        self.results.last().unwrap()
    }

    /// Print results as a formatted table to stdout.
    pub fn print_report(&self) {
        println!();
        println!("{:=<90}", "");
        println!("  HSM-II Benchmark Report");
        println!("{:=<90}", "");
        println!(
            "{:<40} {:>10} {:>12} {:>12} {:>12}",
            "Benchmark", "Iterations", "Avg (us)", "Min (us)", "ops/sec"
        );
        println!("{:-<90}", "");
        for r in &self.results {
            println!(
                "{:<40} {:>10} {:>12.1} {:>12.1} {:>12.0}",
                r.name,
                r.iterations,
                r.avg_duration.as_nanos() as f64 / 1_000.0,
                r.min_duration.as_nanos() as f64 / 1_000.0,
                r.ops_per_sec,
            );
        }
        println!("{:=<90}", "");
        println!();
    }

    /// Export all results as pretty-printed JSON.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(&self.results).unwrap_or_else(|_| "[]".to_string())
    }

    /// Compute aggregate statistics from raw per-iteration durations.
    fn compute_result(name: &str, iterations: u64, durations: &[Duration]) -> BenchResult {
        let total: Duration = durations.iter().sum();
        let avg = total / iterations as u32;
        let min = durations.iter().copied().min().unwrap_or(Duration::ZERO);
        let max = durations.iter().copied().max().unwrap_or(Duration::ZERO);
        let total_secs = total.as_secs_f64();
        let ops = if total_secs > 0.0 {
            iterations as f64 / total_secs
        } else {
            f64::INFINITY
        };

        BenchResult {
            name: name.to_string(),
            iterations,
            total_duration: total,
            avg_duration: avg,
            min_duration: min,
            max_duration: max,
            ops_per_sec: ops,
            metadata: HashMap::new(),
        }
    }
}

// ── Specific Benchmark Functions ───────────────────────────────────────────

/// Benchmark: Memory subsystem -- belief serialization and in-memory CRUD.
///
/// Since the persistence layer (HsmSqliteStore / email storage) requires runtime
/// setup, this benchmark focuses on the serializable Belief struct operations
/// that dominate the hot path: construction, cloning, and serde round-trips.
pub fn bench_memory_persistence(suite: &mut BenchSuite) {
    use crate::hyper_stigmergy::{Belief, BeliefSource};

    let template_belief = Belief {
        id: 1,
        content: "Test belief for benchmarking memory persistence subsystem".to_string(),
        confidence: 0.85,
        source: BeliefSource::Observation,
        supporting_evidence: vec!["evidence_alpha".into(), "evidence_beta".into()],
        contradicting_evidence: vec![],
        created_at: 1_700_000_000,
        updated_at: 1_700_000_000,
        update_count: 0,
        abstract_l0: None,
        overview_l1: None,
        owner_namespace: None,
        supersedes_belief_id: None,
        evidence_belief_ids: Vec::new(),
        human_committed: false,
    };

    // Benchmark belief construction
    suite.bench("memory/belief_construct", 5000, || {
        let belief = Belief {
            id: 42,
            content: "Dynamically constructed belief".to_string(),
            confidence: 0.90,
            source: BeliefSource::Reflection,
            supporting_evidence: vec!["ev1".into()],
            contradicting_evidence: vec![],
            created_at: 1_700_000_000,
            updated_at: 1_700_000_000,
            update_count: 0,
            abstract_l0: Some("Short abstract".into()),
            overview_l1: None,
            owner_namespace: None,
            supersedes_belief_id: None,
            evidence_belief_ids: Vec::new(),
            human_committed: false,
        };
        std::hint::black_box(&belief);
    });

    // Benchmark belief cloning (simulates read from cache)
    suite.bench("memory/belief_clone", 5000, || {
        let cloned = template_belief.clone();
        std::hint::black_box(&cloned);
    });

    // Benchmark JSON round-trip (simulates disk persistence path)
    suite.bench("memory/belief_json_roundtrip", 2000, || {
        let json = serde_json::to_string(&template_belief).unwrap();
        let restored: Belief = serde_json::from_str(&json).unwrap();
        std::hint::black_box(&restored);
    });

    // Benchmark bincode round-trip (simulates compact persistence path)
    suite.bench("memory/belief_bincode_roundtrip", 2000, || {
        let encoded = bincode::serialize(&template_belief).unwrap();
        let restored: Belief = bincode::deserialize(&encoded).unwrap();
        std::hint::black_box(&restored);
    });

    // Benchmark HashMap-based belief store insert + lookup
    suite.bench("memory/hashmap_insert_lookup", 1000, || {
        let mut store: HashMap<usize, Belief> = HashMap::with_capacity(128);
        for i in 0..100 {
            let mut b = template_belief.clone();
            b.id = i;
            b.confidence = (i as f64) / 100.0;
            store.insert(i, b);
        }
        for i in 0..100 {
            let fetched = store.get(&i);
            std::hint::black_box(&fetched);
        }
    });
}

/// Benchmark: Context ranking via cosine similarity over skill embeddings.
///
/// Simulates the CASS semantic search hot path: given a query embedding,
/// rank a bank of skills by cosine similarity and return the top-K.
pub fn bench_context_ranking(suite: &mut BenchSuite) {
    use crate::consensus::{BayesianConfidence, SkillStatus};
    use crate::skill::{Skill, SkillCuration, SkillLevel, SkillScope, SkillSource, TrajectoryType};

    let embedding_dim: usize = 32;

    // Build a bank of 500 skills with deterministic embeddings
    let skills: Vec<Skill> = (0..500)
        .map(|i| {
            // Deterministic pseudo-embedding: vary components by index
            let emb: Vec<f32> = (0..embedding_dim)
                .map(|d| ((i * 7 + d * 13) % 100) as f32 / 100.0)
                .collect();
            Skill {
                id: format!("skill-{}", i),
                title: format!("Skill #{}", i),
                principle: format!("Do thing #{} well", i),
                when_to_apply: vec![],
                level: SkillLevel::General,
                source: SkillSource::Distilled {
                    from_experience_ids: vec![i],
                    trajectory_type: TrajectoryType::Success,
                },
                confidence: (i as f64 % 100.0) / 100.0,
                usage_count: i as u64,
                success_count: (i / 2) as u64,
                failure_count: (i / 4) as u64,
                embedding: Some(emb),
                created_at: 1_700_000_000,
                last_evolved: 1_700_000_000,
                status: SkillStatus::Active,
                bayesian: BayesianConfidence::default(),
                credit_ema: 0.5,
                credit_count: 0,
                last_credit_tick: 0,
                curation: SkillCuration::default(),
                scope: SkillScope::default(),
                delegation_ema: 0.0,
                delegation_count: 0,
                hired_count: 0,
            }
        })
        .collect();

    // Pre-compute a query embedding
    let query_emb: Vec<f32> = (0..embedding_dim)
        .map(|d| (d as f32 + 1.0) / 32.0)
        .collect();

    // Benchmark: cosine similarity ranking over 500 skills
    suite.bench("context/cosine_rank_500_skills", 500, || {
        let mut scored: Vec<(usize, f64)> = skills
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let emb = s.embedding.as_ref().unwrap();
                let dot: f64 = query_emb
                    .iter()
                    .zip(emb.iter())
                    .map(|(a, b)| (*a as f64) * (*b as f64))
                    .sum();
                let norm_q: f64 = query_emb
                    .iter()
                    .map(|x| (*x as f64).powi(2))
                    .sum::<f64>()
                    .sqrt();
                let norm_s: f64 = emb.iter().map(|x| (*x as f64).powi(2)).sum::<f64>().sqrt();
                let cosine = if norm_q * norm_s > 0.0 {
                    dot / (norm_q * norm_s)
                } else {
                    0.0
                };
                (i, cosine)
            })
            .collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        std::hint::black_box(&scored);
    });

    // Benchmark: top-K extraction (K=10) with partial sort
    suite.bench("context/top10_partial_sort_500", 500, || {
        let mut scored: Vec<(usize, f64)> = skills
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let emb = s.embedding.as_ref().unwrap();
                let dot: f64 = query_emb
                    .iter()
                    .zip(emb.iter())
                    .map(|(a, b)| (*a as f64) * (*b as f64))
                    .sum();
                let norm_q: f64 = query_emb
                    .iter()
                    .map(|x| (*x as f64).powi(2))
                    .sum::<f64>()
                    .sqrt();
                let norm_s: f64 = emb.iter().map(|x| (*x as f64).powi(2)).sum::<f64>().sqrt();
                let cosine = if norm_q * norm_s > 0.0 {
                    dot / (norm_q * norm_s)
                } else {
                    0.0
                };
                (i, cosine)
            })
            .collect();
        // Partial sort: only need top 10
        let nth = 9.min(scored.len().saturating_sub(1));
        scored.select_nth_unstable_by(nth, |a, b| {
            b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal)
        });
        let top10: Vec<_> = scored[..10.min(scored.len())].to_vec();
        std::hint::black_box(&top10);
    });

    // Benchmark: 768-dimensional embeddings (production size)
    let big_dim = 768;
    let big_embeddings: Vec<Vec<f32>> = (0..500)
        .map(|i| {
            (0..big_dim)
                .map(|d| ((i * 7 + d * 13) % 1000) as f32 / 1000.0)
                .collect()
        })
        .collect();
    let big_query: Vec<f32> = (0..big_dim)
        .map(|d| (d as f32 + 1.0) / big_dim as f32)
        .collect();

    suite.bench("context/cosine_rank_500_dim768", 200, || {
        let mut scored: Vec<(usize, f64)> = big_embeddings
            .iter()
            .enumerate()
            .map(|(i, emb)| {
                let dot: f64 = big_query
                    .iter()
                    .zip(emb.iter())
                    .map(|(a, b)| (*a as f64) * (*b as f64))
                    .sum();
                let norm_q: f64 = big_query
                    .iter()
                    .map(|x| (*x as f64).powi(2))
                    .sum::<f64>()
                    .sqrt();
                let norm_s: f64 = emb.iter().map(|x| (*x as f64).powi(2)).sum::<f64>().sqrt();
                let cosine = if norm_q * norm_s > 0.0 {
                    dot / (norm_q * norm_s)
                } else {
                    0.0
                };
                (i, cosine)
            })
            .collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        std::hint::black_box(&scored);
    });
}

/// Benchmark: Multi-perspective council simulation.
///
/// Since the real `SimpleCouncil` requires async evaluation and `CouncilMember`
/// construction, this benchmark measures the core voting/tallying algorithm
/// in isolation: weighted vote aggregation across N agents.
pub fn bench_multi_perspective(suite: &mut BenchSuite) {
    use crate::council::simple::VoteValue;

    /// Inline vote struct for benchmark isolation (avoids async council setup).
    #[derive(Clone)]
    #[allow(dead_code)]
    struct BenchVote {
        agent_id: u64,
        value: VoteValue,
        weight: f64,
        reasoning: String,
    }

    /// Tally benchmark votes using the same weighted algorithm as SimpleCouncil.
    fn tally_votes(votes: &[BenchVote]) -> (bool, f64) {
        let mut approve_weight = 0.0_f64;
        let mut _reject_weight = 0.0_f64;
        let mut total_weight = 0.0_f64;

        for vote in votes {
            total_weight += vote.weight;
            match vote.value {
                VoteValue::Approve => approve_weight += vote.weight,
                VoteValue::Reject => _reject_weight += vote.weight,
                VoteValue::Abstain => {}
            }
        }

        if total_weight == 0.0 {
            return (false, 0.0);
        }

        let approval_ratio = approve_weight / total_weight;
        let approved = approval_ratio > 0.5;
        let confidence = if approved {
            approval_ratio
        } else {
            1.0 - approval_ratio
        };
        (approved, confidence)
    }

    for n in [5u64, 10, 25, 50, 100] {
        let votes: Vec<BenchVote> = (0..n)
            .map(|i| BenchVote {
                agent_id: i,
                value: if i % 3 != 0 {
                    VoteValue::Approve
                } else {
                    VoteValue::Reject
                },
                weight: 1.0 + (i as f64 * 0.01),
                reasoning: format!("Agent {} rationale", i),
            })
            .collect();

        suite.bench(&format!("council/tally_{}_agents", n), 500, || {
            let result = tally_votes(&votes);
            std::hint::black_box(&result);
        });
    }

    // Benchmark: confidence-weighted voting with varied weights
    let weighted_votes: Vec<BenchVote> = (0..50)
        .map(|i| BenchVote {
            agent_id: i,
            value: if i % 5 < 3 {
                VoteValue::Approve
            } else if i % 5 == 3 {
                VoteValue::Reject
            } else {
                VoteValue::Abstain
            },
            weight: 0.5 + (i as f64 * 0.03),
            reasoning: format!("Weighted agent {} rationale", i),
        })
        .collect();

    suite.bench("council/weighted_tally_50_agents", 500, || {
        let result = tally_votes(&weighted_votes);
        std::hint::black_box(&result);
    });

    // Benchmark: vote construction overhead (measures allocation cost)
    suite.bench("council/vote_construction_100", 500, || {
        let votes: Vec<BenchVote> = (0..100)
            .map(|i| BenchVote {
                agent_id: i,
                value: VoteValue::Approve,
                weight: 1.0,
                reasoning: format!("Constructed vote {}", i),
            })
            .collect();
        std::hint::black_box(&votes);
    });
}

/// Benchmark: Serialization cost comparison (JSON vs bincode).
///
/// Measures the overhead of serializing/deserializing core domain types
/// to quantify the cost of different persistence and wire formats.
pub fn bench_cost_reduction(suite: &mut BenchSuite) {
    use crate::hyper_stigmergy::{Belief, BeliefSource};

    let belief = Belief {
        id: 42,
        content: "A moderately long belief content string for benchmarking serialization \
                  performance across the system boundary layer"
            .to_string(),
        confidence: 0.92,
        source: BeliefSource::Reflection,
        supporting_evidence: vec!["ev1".into(), "ev2".into(), "ev3".into()],
        contradicting_evidence: vec!["cev1".into()],
        created_at: 1_700_000_000,
        updated_at: 1_700_000_100,
        update_count: 5,
        abstract_l0: Some("Short abstract".into()),
        overview_l1: Some("Medium overview text for drill-down queries".into()),
        owner_namespace: None,
        supersedes_belief_id: None,
        evidence_belief_ids: vec![7, 8],
        human_committed: false,
    };

    // JSON serialization
    suite.bench("cost/json_serialize_belief", 5000, || {
        let s = serde_json::to_string(&belief).unwrap();
        std::hint::black_box(&s);
    });

    // JSON deserialization
    let json_str = serde_json::to_string(&belief).unwrap();
    suite.bench("cost/json_deserialize_belief", 5000, || {
        let b: Belief = serde_json::from_str(&json_str).unwrap();
        std::hint::black_box(&b);
    });

    // Bincode serialization
    suite.bench("cost/bincode_serialize_belief", 5000, || {
        let b = bincode::serialize(&belief).unwrap();
        std::hint::black_box(&b);
    });

    // Bincode deserialization
    let bin_data = bincode::serialize(&belief).unwrap();
    suite.bench("cost/bincode_deserialize_belief", 5000, || {
        let b: Belief = bincode::deserialize(&bin_data).unwrap();
        std::hint::black_box(&b);
    });

    // JSON pretty-print (common for logging/debugging)
    suite.bench("cost/json_pretty_serialize_belief", 2000, || {
        let s = serde_json::to_string_pretty(&belief).unwrap();
        std::hint::black_box(&s);
    });

    // Size comparison metadata
    let json_size = serde_json::to_string(&belief).unwrap().len();
    let bincode_size = bincode::serialize(&belief).unwrap().len();
    let json_pretty_size = serde_json::to_string_pretty(&belief).unwrap().len();

    // Report sizes via a metadata-only entry
    let mut size_result = BenchResult {
        name: "cost/serialized_sizes".to_string(),
        iterations: 1,
        total_duration: Duration::ZERO,
        avg_duration: Duration::ZERO,
        min_duration: Duration::ZERO,
        max_duration: Duration::ZERO,
        ops_per_sec: 0.0,
        metadata: HashMap::new(),
    };
    size_result
        .metadata
        .insert("json_bytes".to_string(), json_size.to_string());
    size_result.metadata.insert(
        "json_pretty_bytes".to_string(),
        json_pretty_size.to_string(),
    );
    size_result
        .metadata
        .insert("bincode_bytes".to_string(), bincode_size.to_string());
    size_result.metadata.insert(
        "compression_ratio".to_string(),
        format!("{:.2}x", json_size as f64 / bincode_size as f64),
    );
    suite.results.push(size_result);
}

/// Benchmark: Embedding operations that dominate the CASS retrieval hot path.
///
/// Measures raw vector math throughput independent of the skill type overhead.
pub fn bench_embedding_ops(suite: &mut BenchSuite) {
    let dim = 768;

    // Generate deterministic test vectors
    let vec_a: Vec<f32> = (0..dim).map(|i| (i as f32 * 0.001).sin()).collect();
    let vec_b: Vec<f32> = (0..dim).map(|i| (i as f32 * 0.002).cos()).collect();

    // Dot product
    suite.bench("embedding/dot_product_768", 10000, || {
        let dot: f64 = vec_a
            .iter()
            .zip(vec_b.iter())
            .map(|(a, b)| (*a as f64) * (*b as f64))
            .sum();
        std::hint::black_box(dot);
    });

    // L2 norm
    suite.bench("embedding/l2_norm_768", 10000, || {
        let norm: f64 = vec_a
            .iter()
            .map(|x| (*x as f64).powi(2))
            .sum::<f64>()
            .sqrt();
        std::hint::black_box(norm);
    });

    // Full cosine similarity
    suite.bench("embedding/cosine_similarity_768", 10000, || {
        let dot: f64 = vec_a
            .iter()
            .zip(vec_b.iter())
            .map(|(a, b)| (*a as f64) * (*b as f64))
            .sum();
        let norm_a: f64 = vec_a
            .iter()
            .map(|x| (*x as f64).powi(2))
            .sum::<f64>()
            .sqrt();
        let norm_b: f64 = vec_b
            .iter()
            .map(|x| (*x as f64).powi(2))
            .sum::<f64>()
            .sqrt();
        let cosine = if norm_a * norm_b > 0.0 {
            dot / (norm_a * norm_b)
        } else {
            0.0
        };
        std::hint::black_box(cosine);
    });

    // Euclidean distance
    suite.bench("embedding/euclidean_distance_768", 10000, || {
        let dist: f64 = vec_a
            .iter()
            .zip(vec_b.iter())
            .map(|(a, b)| {
                let diff = *a as f64 - *b as f64;
                diff * diff
            })
            .sum::<f64>()
            .sqrt();
        std::hint::black_box(dist);
    });

    // Batch: cosine similarity of 1 query against 100 vectors
    let batch: Vec<Vec<f32>> = (0..100)
        .map(|k| {
            (0..dim)
                .map(|i| ((i + k * 7) as f32 * 0.003).sin())
                .collect()
        })
        .collect();

    suite.bench("embedding/batch_cosine_100x768", 500, || {
        let norm_q: f64 = vec_a
            .iter()
            .map(|x| (*x as f64).powi(2))
            .sum::<f64>()
            .sqrt();
        let mut scores: Vec<f64> = Vec::with_capacity(100);
        for emb in &batch {
            let dot: f64 = vec_a
                .iter()
                .zip(emb.iter())
                .map(|(a, b)| (*a as f64) * (*b as f64))
                .sum();
            let norm_s: f64 = emb.iter().map(|x| (*x as f64).powi(2)).sum::<f64>().sqrt();
            let cosine = if norm_q * norm_s > 0.0 {
                dot / (norm_q * norm_s)
            } else {
                0.0
            };
            scores.push(cosine);
        }
        std::hint::black_box(&scores);
    });
}

/// Run all benchmarks and return the populated suite.
///
/// Convenience function for binary entry points:
/// ```no_run
/// let suite = hyper_stigmergy::bench::run_all();
/// suite.print_report();
/// ```
pub fn run_all() -> BenchSuite {
    let mut suite = BenchSuite::new();
    bench_memory_persistence(&mut suite);
    bench_context_ranking(&mut suite);
    bench_multi_perspective(&mut suite);
    bench_cost_reduction(&mut suite);
    bench_embedding_ops(&mut suite);
    suite
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bench_suite_captures_results() {
        let mut suite = BenchSuite::new();
        suite.bench("test/noop", 10, || {
            std::hint::black_box(1 + 1);
        });
        assert_eq!(suite.results.len(), 1);
        let r = &suite.results[0];
        assert_eq!(r.name, "test/noop");
        assert_eq!(r.iterations, 10);
        assert!(r.ops_per_sec > 0.0);
        assert!(r.min_duration <= r.avg_duration);
        assert!(r.avg_duration <= r.max_duration);
    }

    #[test]
    fn bench_with_result_prevents_elision() {
        let mut suite = BenchSuite::new();
        suite.bench_with_result("test/sum", 5, || -> u64 { (0..1000u64).sum() });
        assert_eq!(suite.results.len(), 1);
        assert_eq!(suite.results[0].iterations, 5);
    }

    #[test]
    fn bench_suite_json_export() {
        let mut suite = BenchSuite::new();
        suite.bench("test/json", 3, || {});
        let json = suite.to_json();
        assert!(json.contains("test/json"));
        assert!(json.contains("iterations"));
    }

    #[test]
    fn bench_suite_default_trait() {
        let suite = BenchSuite::default();
        assert!(suite.results.is_empty());
    }

    #[tokio::test]
    async fn bench_async_captures_results() {
        let mut suite = BenchSuite::new();
        suite
            .bench_async("test/async_noop", 5, || async {
                std::hint::black_box(42);
            })
            .await;
        assert_eq!(suite.results.len(), 1);
        assert_eq!(suite.results[0].name, "test/async_noop");
    }

    #[test]
    fn compute_result_handles_single_iteration() {
        let durations = vec![Duration::from_micros(100)];
        let r = BenchSuite::compute_result("single", 1, &durations);
        assert_eq!(r.min_duration, r.max_duration);
        assert_eq!(r.min_duration, Duration::from_micros(100));
    }
}
