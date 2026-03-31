//! Comparative evaluation harness for HSM-II vs baseline LLM.
//!
//! Runs 20 multi-session tasks through both a vanilla LLM (no memory) and
//! the full HSM-II pipeline (persistent memory + context ranking + reputation
//! routing), then produces a comparative report proving (or disproving)
//! the 30%+ improvement claim.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │                   Eval Task Suite (20 tasks)            │
//! │   SE×5  ·  DS×5  ·  Biz×5  ·  Research×2  ·  Stress×2 │
//! └───────────────────┬─────────────────────────────────────┘
//!                     │
//!          ┌──────────┴──────────┐
//!          ▼                     ▼
//!   ┌─────────────┐      ┌─────────────┐
//!   │  Baseline    │      │  HSM-II     │
//!   │  Runner      │      │  Runner     │
//!   │              │      │             │
//!   │ • No memory  │      │ • Beliefs   │
//!   │ • No ranking │      │ • Ranking   │
//!   │ • No skills  │      │ • Skills    │
//!   │ • Session-   │      │ • Cross-    │
//!   │   scoped     │      │   session   │
//!   │   history    │      │   memory    │
//!   └──────┬───────┘      └──────┬──────┘
//!          │                     │
//!          ▼                     ▼
//!   ┌─────────────┐      ┌─────────────┐
//!   │  Turn        │      │  Turn       │
//!   │  Metrics     │      │  Metrics    │
//!   └──────┬───────┘      └──────┬──────┘
//!          │                     │
//!          └──────────┬──────────┘
//!                     ▼
//!          ┌─────────────────┐
//!          │  Comparison     │
//!          │  Report         │
//!          │                 │
//!          │ • Quality Δ     │
//!          │ • Recall Δ      │
//!          │ • Token Δ       │
//!          │ • Latency Δ     │
//!          │ • Domain split  │
//!          │ • Verdict       │
//!          └─────────────────┘
//! ```

pub mod artifacts;
pub mod calibration;
pub mod external;
pub mod judges;
pub mod metrics;
pub mod proposer;
pub mod runner;
pub mod run_store;
pub mod suites;
pub mod tasks;
pub mod trace;

pub use judges::{
    evaluate_turn_rubric, grounding_metrics, llm_judge_enabled, llm_judge_turn, parse_tool_json,
    tool_metrics, RubricExtras,
};
pub use metrics::{
    compare, print_report, ComparisonReport, ImprovementMetrics, RunnerMetrics, RunnerSummary,
    TurnMetrics, turn_rubric_composite,
};
pub use artifacts::{
    append_runs_index, default_runs_index, try_git_head, write_jsonl, write_manifest,
    write_turn_metrics_jsonl, ArtifactPaths, RunManifest,
};
pub use calibration::{calibration_report, load_gold_labels, CalibrationReport, GoldFile, GoldTurnLabel};
pub use external::{
    run_external_batch_sync, run_external_sync, ExternalBenchmarkBatch,
    ExternalBenchmarkBatchResult, ExternalBenchmarkResult, ExternalBenchmarkSpec,
};
pub use proposer::{build_proposer_context, discover_harness_rust_sources, ProposerContext, ProposerOptions};
pub use run_store::{
    clear_runs, hash_index_line, ingest_jsonl, insert_index_jsonl_line, open_run_store,
    query_best_objective, query_by_harness, query_by_run_dir_contains, query_recent, rebuild_fts,
    row_count, search_fts, sync_index_line_to_sqlite, RunRow,
};
pub use runner::{BaselineRunner, HarnessPolicy, HsmRunner, HsmRunnerConfig};
pub use suites::{eval_tasks_for_suite, parse_weighted_suites, filter_tasks, WeightedEvalSuite};
pub use tasks::{
    load_eval_suite,
    suite_council_vs_single,
    suite_memory_retrieval,
    suite_tool_routing,
    EvalTask,
};
pub use trace::{BeliefRankEntry, HsmTurnTrace, RankedContextResult};
