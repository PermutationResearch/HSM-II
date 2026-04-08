# Benchmark Stack

This repo should carry three benchmark tracks:

1. `LongMemEval` for memory fidelity
2. `YC-Bench` for long-horizon outcomes
3. `HSM-native` for stigmergic memory, belief revision, and agent handoff

The goal is not to force one benchmark to prove everything. Each track should measure one thing well.

## 1. LongMemEval

Purpose:
- Verify that HSM-II preserves exact cross-session memory without lossy abstraction.

Current entrypoint:
- `src/bin/hsm_longmemeval.rs`

Files to add or maintain:
- `src/bin/hsm_longmemeval.rs`
- `src/eval/runner.rs`
- `src/eval/trace.rs`
- `src/eval/metrics.rs`
- `scripts/run_longmemeval.sh`
- `scripts/eval_longmemeval.sh`
- `docs/LONGMEMEVAL.md`

Required modes:
- `baseline-direct`
  Raw timestamped history, no HSM retrieval layer.
- `hsm-fullhistory-plus-memory`
  Raw timestamped history plus HSM memory augmentation.
- `hsm-retrieval-only`
  Retrieved evidence without full history. This is an ablation, not the default benchmark mode.

Required commands:
```bash
cargo run --bin hsm-longmemeval -- \
  --input external/LongMemEval/data/longmemeval_oracle.json \
  --output runs/longmemeval/baseline_oracle.jsonl \
  --mode baseline

cargo run --bin hsm-longmemeval -- \
  --input external/LongMemEval/data/longmemeval_oracle.json \
  --output runs/longmemeval/hsm_oracle.jsonl \
  --mode hsm
```

Required output files:
- `*.jsonl` predictions with:
```json
{"question_id":"...", "hypothesis":"..."}
```
- `*.trace.jsonl` with retrieval/debug metadata
- `*.eval.json` aggregate summary

Required summary schema:
```json
{
  "benchmark": "longmemeval",
  "mode": "baseline-direct",
  "dataset": "oracle",
  "model": "openai/gpt-5.4",
  "n_questions": 500,
  "qa_accuracy": 0.0,
  "abstention_accuracy": 0.0,
  "retrieval_turn_recall_at_k": 0.0,
  "retrieval_session_recall_at_k": 0.0,
  "notes": []
}
```

Success criteria:
- `hsm-fullhistory-plus-memory` should stay near `baseline-direct` on oracle mode.
- `hsm-retrieval-only` is allowed to underperform, but should improve as retrieval quality improves.

## 2. YC-Bench

Purpose:
- Measure whether memory improves long-horizon company performance under compounding consequences.

Current integration points:
- `external_integrations/auto-harness-yc-bench/`
- `config/external_yc_bench_seed*.json`
- `web/company-console/app/api/companies-sh/yc-bench/`

Files to add or maintain:
- `scripts/run_ycbench_grid.sh`
- `scripts/aggregate_ycbench_hsm.py`
- `scripts/compare_ycbench_ablations.py`
- `docs/YC_BENCH.md`
- `config/ycbench_hsm_baseline.json`
- `config/ycbench_hsm_nomemory.json`
- `config/ycbench_hsm_scratchpad_only.json`
- `config/ycbench_hsm_full.json`

Required ablations:
- `baseline-no-memory`
- `scratchpad-only`
- `hsm-memory-no-belief-update`
- `hsm-full`

Required outputs per run:
```json
{
  "benchmark": "yc-bench",
  "company": "apex-systems",
  "seed": 1,
  "model": "openrouter/qwen/qwen3.6-plus:free",
  "variant": "hsm-full",
  "final_funds_ratio": 0.0,
  "terminal_reason": "horizon_end",
  "turns_completed": 0,
  "total_cost_usd": 0.0
}
```

Required aggregate schema:
```json
{
  "benchmark": "yc-bench",
  "variant": "hsm-full",
  "val_score": 0.0,
  "pass_rate": 0.0,
  "pass_count": 0,
  "n_companies": 0,
  "avg_seeds": 0.0,
  "companies": {},
  "tier_distribution": {},
  "behavioral_metrics": {
    "rat_accept_rate": 0.0,
    "trust_task_ratio": 0.0,
    "policy_flip_rate": 0.0,
    "bankruptcy_rate": 0.0
  }
}
```

Success criteria:
- `hsm-full` should beat weaker-memory variants on funds ratio and behavioral stability.
- Behavioral metrics should explain the outcome, not just the final dollars.

## 3. HSM-Native

Purpose:
- Measure what HSM-II is actually designed for: stigmergic coordination, belief revision, agent handoff, and cross-session synthesis.

New files to add:
- `src/eval/hsm_native_tasks.rs`
- `src/bin/hsm_native_eval.rs`
- `src/eval/hsm_native_metrics.rs`
- `docs/HSM_NATIVE_BENCH.md`
- `scripts/run_hsm_native.sh`

Suites to add:
- `cross_session_synthesis`
- `belief_revision`
- `agent_handoff`
- `policy_persistence`
- `conflict_resolution`

Task schema:
```json
{
  "id": "handoff-001",
  "suite": "agent_handoff",
  "sessions": [
    {
      "session_id": 1,
      "agent": "researcher",
      "turns": []
    }
  ],
  "question": "What should the finisher do next?",
  "gold": {
    "answer": "...",
    "required_facts": [],
    "forbidden_stale_facts": []
  }
}
```

Required metrics schema:
```json
{
  "benchmark": "hsm-native",
  "suite": "belief_revision",
  "variant": "hsm-full",
  "n_tasks": 0,
  "answer_accuracy": 0.0,
  "required_fact_recall": 0.0,
  "stale_fact_suppression": 0.0,
  "handoff_success": 0.0,
  "policy_consistency": 0.0,
  "explanation_grounding": 0.0
}
```

Success criteria:
- HSM should clearly outperform plain chat-history baselines on belief revision and handoff tasks.

## Implementation Order

1. Stabilize `LongMemEval` as a regression suite.
2. Add `YC-Bench` ablation variants and behavioral aggregates.
3. Build `HSM-native` benchmark once the first two tracks are reproducible.

## Short Task List

### LongMemEval
- Add direct/baseline/hsm mode naming cleanup in `src/bin/hsm_longmemeval.rs`
- Export retrieval metrics from traces
- Add `scripts/run_longmemeval.sh`
- Add `scripts/eval_longmemeval.sh`
- Add JSON summary writer

### YC-Bench
- Add named ablation configs under `config/`
- Add run matrix script under `scripts/`
- Add behavior aggregator for RAT avoidance, trust specialization, policy stability
- Store aggregate outputs under `runs/ycbench/`

### HSM-Native
- Add task format under `src/eval/`
- Add runner bin under `src/bin/`
- Add scorers under `src/eval/`
- Add seedable JSONL artifact output

## Repository Conventions

Suggested run artifact layout:
```text
runs/
  longmemeval/
  ycbench/
  hsm_native/
```

Suggested docs:
```text
docs/
  LONGMEMEVAL.md
  YC_BENCH.md
  HSM_NATIVE_BENCH.md
  BENCHMARK_STACK.md
```
