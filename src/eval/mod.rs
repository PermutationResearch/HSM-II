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

pub mod metrics;
pub mod runner;
pub mod tasks;

pub use metrics::{compare, print_report, ComparisonReport, RunnerMetrics, TurnMetrics};
pub use runner::{BaselineRunner, HsmRunner};
pub use tasks::{
    load_eval_suite,
    suite_council_vs_single,
    suite_memory_retrieval,
    suite_tool_routing,
    EvalTask,
};
