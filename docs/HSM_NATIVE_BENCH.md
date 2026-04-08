# Stigmergic Memory Benchmark

`hsm-native-eval` is the runnable harness for the Stigmergic Memory Benchmark.

It targets the cases LongMemEval does not measure well:

- `cross_session_synthesis`
- `belief_revision`
- `agent_handoff`
- `policy_persistence`
- `conflict_resolution`

Short name:

- `SMB`

Internal ids kept for code and files:

- binary: `hsm-native-eval`
- suite family: `hsm-native`

Default behavior:

- `baseline`: current-session-only view
- `hsm-full`: sessions are ingested into HSM memory, then the final question is answered from memory

**LLM setup:** set **`OPENROUTER_API_KEY`** / **`OPENAI_API_KEY`** / **`ANTHROPIC_API_KEY`** as appropriate, and **`DEFAULT_LLM_MODEL`**. On **OpenRouter**, use their catalog id (e.g. `openai/gpt-5.4`, `qwen/qwen3-...`) — **do not** prefix with `openrouter/`; that produces HTTP 400 *invalid model ID*. If you rely on **Ollama only**, set **`OLLAMA_MODEL`** to a pulled tag or use fallback **`llama3.2`**.

Built-in suite size:

- `25` tasks total
- `7` `cross_session_synthesis`
- `5` `belief_revision`
- `5` `agent_handoff`
- `4` `policy_persistence`
- `4` `conflict_resolution`

Run both variants on the built-in SMB suite:

```bash
cargo run --bin hsm-native-eval -- --variant both
```

**Where results go:** by default, `cargo run` only prints JSON to stdout — nothing is saved. Use **`bash scripts/run_hsm_native.sh`** to always write **`runs/hsm_native/report.json`** (two `HsmNativeReport` objects: `baseline` + `hsm-full`) and **`runs/hsm_native/tasks.jsonl`**. That directory is **gitignored**; copy or commit a snapshot under `docs/` if you want history in git.

Write summaries and per-task JSONL manually:

```bash
cargo run --bin hsm-native-eval -- \
  --variant both \
  --json runs/hsm_native/report.json \
  --jsonl runs/hsm_native/tasks.jsonl
```

Run only one suite:

```bash
cargo run --bin hsm-native-eval -- --variant both --suite belief_revision
```

Task format:

```json
[
  {
    "id": "handoff-001",
    "suite": "agent_handoff",
    "sessions": [
      {
        "session_id": 1,
        "agent": "researcher",
        "turns": [
          { "role": "user", "content": "..." }
        ]
      }
    ],
    "question": "What should the finisher do next?",
    "gold": {
      "answer": "...",
      "required_facts": [],
      "forbidden_stale_facts": []
    }
  }
]
```

Current scoring is deterministic and lightweight:

- `answer_accuracy`
- `required_fact_recall`
- `stale_fact_suppression`
- `handoff_success`
- `policy_consistency`
- `explanation_grounding`

Scoring details:

- required-fact matching uses normalized phrase checks plus lightweight token-prefix matching for simple paraphrases like `editable` vs `editing` or `batching` vs `batch`
- stale-fact suppression does not penalize corrected mentions when the answer explicitly marks them as revised / obsolete

Trace output:

```bash
cargo run --bin hsm-native-eval -- \
  --variant both \
  --traces \
  --trace-output runs/hsm_native/report.trace.jsonl
```

Regression stack:

```bash
scripts/run_regression_stack.sh
```

## Saved SMB runs (local, under `runs/`)

The **`runs/`** tree is **gitignored**; benchmark outputs still live on disk for your machine (and in Cursor indexing), but **won’t appear in `git status`**.

**HSM-native SMB (this harness), GPT‑5.4 snapshots:**

| File | Notes |
|------|--------|
| `runs/hsm_native/report_gpt54_v5.json` | Full 25-task suite, **2026-04-06**: `baseline` answer_accuracy **0.56** → **`hsm-full` 1.00**; required_fact_recall **0.7633 → 1.00**; stale_fact_suppression **0.92 → 1.00**; handoff_success **0.96 → 1.00**; policy_consistency **0.9667 → 1.00**; explanation_grounding **0.7633 → 1.00**. |
| `runs/hsm_native/tasks_gpt54_v5.jsonl` | Per-task rows for that run. |
| `runs/hsm_native/report_gpt54_v4.json` | Earlier same-day run: baseline **0.52**, hsm-full **0.92**; largest gap in **cross_session_synthesis** and **policy_persistence** (baseline ~0 on answer_accuracy there). |

**Different harness — `hsm-eval` (keyword/recall “chat eval”), not SMB:**

- `runs/eval_20260331_165239/full/comparison_report.json` — mixed outcome vs keyword metrics.
- `runs/eval_full_fixed/full/comparison_report.json` — treat as **invalid** if both variants show `error_rate: 1.0`.

**LongMemEval raw outputs (predictions + traces, not SMB):**

- e.g. `runs/longmemeval/baseline_oracle_gpt54_limit50.jsonl`, `runs/longmemeval/hsm_oracle_gpt54_limit50_fullhistoryplusmemory.jsonl` — scoring summaries may be missing; aggregates live in your eval pipeline if you add them.

To **preserve** a run in git, copy the JSON/JSONL you care about into **`docs/`** (or a committed `benchmarks/` folder) with a dated name.

Task-trail telemetry:

```bash
python3 scripts/aggregate_task_trail_telemetry.py /path/to/task_trail.jsonl
```

The aggregator reports:

- average `tool_prompt_tokens`
- average `skill_prompt_tokens`
- average exposed tool count
- average hidden tool count

This is enough to make the benchmark runnable now. A later pass can add judge-model grading for richer answer quality.
