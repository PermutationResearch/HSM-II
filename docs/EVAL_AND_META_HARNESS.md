# Eval, meta-harness, and outer loop

This document is the **canonical** guide for when to use each tool, **where artifacts land**, and the **contract** between promoted harness JSON and the rest of HSM-II.

For native SMB-style benchmarks (different harness), see [`HSM_NATIVE_BENCH.md`](./HSM_NATIVE_BENCH.md).

---

## Which tool when

| Tool | Role | Use it when |
|------|------|-------------|
| **`hsm-eval`** | **Inner eval runtime** — runs the benchmark suite with `HsmRunner` vs a vanilla LLM baseline, writes metrics and optional JSONL traces. | You want a **single** measurement pass, a fixed `HsmRunnerConfig`, or to **validate** a JSON harness file (e.g. after meta-harness) with `--hsm-config`. |
| **`hsm_meta_harness`** | **Outer search (phase 1)** — samples or loads **many** `HsmRunnerConfig` candidates, compares each to baseline, ranks by objective, exports **Pareto frontier**, optionally writes **`best_config.json`** and can **promote** a file to `config/hsm_harness.default.json`. | You want to **search** harness knobs (memory injection, budgets, thresholds, etc.) against the same verifiable tasks, not tweak weights. |
| **`hsm_outer_loop`** | **Outer-loop infrastructure** — compile gate, **SQLite** over `runs/runs_index.jsonl`, **queries**, **proposer** context for agents, **external** Rust benchmark batches (e.g. YC-bench, side repos). | You need to **index/query** past runs, feed a coding agent **context** from history, or run **non-in-tree** harnesses from JSON specs. |

**Mental model:** `hsm-eval` = one experiment; `hsm_meta_harness` = many candidates + leaderboard; `hsm_outer_loop` = archive/DB/tooling around runs (and external benchmarks), not the core HSM-vs-baseline eval loop itself.

---

## Contract: promoted config vs production runtime

- **`HsmRunnerConfig`** is the tunable policy object used by **`HsmRunner`** inside **`hsm-eval`**, **`hsm_meta_harness`**, and related eval binaries (`hsm_native_eval`, `hsm_longmemeval`, etc.).
- **`personal_agent`**, **`hsm_console`**, and the main Telegram/API agent stack **do not** automatically load `config/hsm_harness.default.json` or `HSM_META_HARNESS_CONFIG`. Their memory and tool behavior use **different** configuration paths.
- Therefore: **meta-harness and `hsm-eval` are eval-side tooling.** A promoted **`best_config.json`** (or copied **`config/hsm_harness.default.json`**) is **not** wired into the live bot until an explicit integration maps those fields into the runtime you run in production.

**Practical implication:** Treat **`best_config.json`** as the artifact you **re-test** with `hsm-eval --hsm-config path/to/best_config.json` and, if you change product behavior manually, as documentation of what worked on benchmarks — not as a switch that flips `personal_agent` today.

---

## Where artifacts go

Default layout uses a **`runs/`** directory at the repo root (override with `--out-dir` / `--artifacts` where supported).

### `hsm-eval`

- With **`--artifacts <dir>`**: writes **`manifest.json`**, comparison outputs, and per-suite dirs with **`turns_hsm.jsonl`**, **`turns_baseline.jsonl`**, optional **`hsm_trace.jsonl`**, paths recorded under **`artifact_paths`** in the manifest.
- May append one line to **`runs/runs_index.jsonl`** (unless disabled) for outer-loop ingestion.

### `hsm_meta_harness`

- Default run directory: **`runs/run_<unix_timestamp>/`** (or **`--out-dir`**).
- Per run:
  - **`baseline_by_suite.json`** (and **`baseline_metrics.json`** if a single suite).
  - Under **`cand_*/`**: **`candidate_result.json`**, **`per_suite.json`**, per-suite subdirs with **`hsm_metrics.json`**, **`comparison_report.json`**, **`turns_hsm.jsonl`**, **`turns_baseline.jsonl`**, optional **`hsm_trace.jsonl`**.
  - **`leaderboard.json`**, **`pareto_frontier.json`**, **`manifest.json`**.
  - If the confidence gate passes: **`best_config.json`** (full **`HsmRunnerConfig`** JSON).
- **`promote` subcommand** copies a harness JSON to **`config/hsm_harness.default.json`** by default (see `hsm_meta_harness promote --help`).

### `hsm_outer_loop`

- **`external-batch`** / **`external`**: write results under paths given in the spec (e.g. **`runs/external_batch_<timestamp>.json`**).
- **`index-db`**: builds **`runs/runs.sqlite`** from **`runs/runs_index.jsonl`** (paths configurable).
- **`propose`**: emits **ProposerContext** JSON (e.g. for agent workflows).

Environment variables (see **`.env.example`**): `HSM_RUNS_SQLITE`, `HSM_PARENT_RUN_ID`, `HSM_META_HARNESS_CONFIG`, eval thresholds, etc.

---

## Smoke recipe (copy-paste)

**Prerequisites:** Rust toolchain, repo clone, and **any one** LLM path the rest of the project uses (e.g. Ollama running locally, or `OPENAI_API_KEY` / `ANTHROPIC_API_KEY` set — same as for `hsm-eval`).

Minimal Ollama example:

```bash
cd /path/to/HSM-II
# Ensure your model is available, e.g.:
# ollama pull llama3.2
export OLLAMA_MODEL=llama3.2
```

**1) Single eval (`hsm-eval`) — tiny slice**

```bash
cargo run --bin hsm-eval -- --suite memory --limit 2 --verbose
```

**2) Eval with artifacts (for Trace2Skill / inspection)**

```bash
mkdir -p runs/smoke_eval
cargo run --bin hsm-eval -- --suite memory --limit 2 --artifacts runs/smoke_eval
```

**3) Meta-harness — smoke search (small sample)**

Meta-harness enforces a minimum task count unless you opt out:

```bash
cargo run --bin hsm_meta_harness -- \
  --candidates 2 \
  --bootstrap-runs 1 \
  --suite memory \
  --limit 2 \
  --allow-small-sample \
  --require-positive-ci=false
```

**4) Outer loop — list runs**

After a run that appended **`runs/runs_index.jsonl`**:

```bash
cargo run --bin hsm_outer_loop -- list-runs --index runs/runs_index.jsonl --limit 10
```

**5) Validate a promoted or `best_config.json` with `hsm-eval`**

```bash
cargo run --bin hsm-eval -- \
  --suite memory \
  --limit 2 \
  --hsm-config runs/run_<timestamp>/best_config.json
```

(Use the actual path to your `best_config.json`.)

---

## Company OS agent-chat (Python `scripts/meta-harness`)

This is a **separate** harness from the Rust `hsm_meta_harness` binary: it drives the live **Next.js** Company Console routes `POST /api/agent-chat-reply/stream` and `POST /api/agent-chat-reply`, talking to **`hsm_console`** for tasks and worker execution (`evaluate_turn.py` defaults: Next `http://127.0.0.1:3050`, HSM `http://127.0.0.1:3847`).

**Prerequisites**

- `cargo run -p hyper-stigmergy --bin hsm_console` (or your usual Company OS API) reachable on **3847** (or override in `evaluate_turn.py`).
- `cd web/company-console && npm run dev` (or production build) on **3050** so the stream route exists.
- LLM keys the console expects (e.g. **OpenRouter** / `OPENROUTER_API_KEY` per `web/company-console` docs).
- **Harness data directory:** by default the script uses **`~/.hsm/meta-harness/`** when your home directory is writable. If not (e.g. Cursor sandbox, read-only CI), it automatically falls back to **`<repo>/.meta-harness/`** (gitignored). Override anytime with **`HSM_META_HARNESS_DATA_DIR`**.

**Timeouts**

- Server-side worker telemetry waits are controlled by **`HSM_OPERATOR_CHAT_TELEMETRY_WAIT_EXEC_MS`** / **`HSM_OPERATOR_CHAT_TELEMETRY_WAIT_ANALYSIS_MS`** (see **`.env.example`**). JSON and NDJSON paths share the same caps; build-heavy skills get a longer minimum wait.
- Next route **`maxDuration`** and the browser fallback poll budget are defined in **`web/company-console/app/lib/operator-chat-timeouts.ts`** so hosted and UI behavior stay above those waits.

**Commands**

```bash
cd /path/to/hyper-stigmergic-morphogenesisII
# Quick smoke (one task); optional quality gates exit non-zero if metrics fall below thresholds
./scripts/company-os-agent-chat-meta-harness-smoke.sh

# Full local check (three default engineering tasks)
python3 scripts/meta-harness/meta_harness.py --tasks 3
```

**Belief state (experimental)** — `evaluate_turn.py` also prints a **`belief_state`** object: a Beta-style summary over a single “task success” latent plus a **VoI proxy** for logging, aligned with Papamarkou et al. (*Bayes-consistent decisions*; [HAL hal-05480691](https://hal.science/hal-05480691)). Semantics and limits: **`docs/META_HARNESS_BELIEF_STATE.md`**.

---

## Related references

- **`META_HARNESS_BELIEF_STATE.md`** — Beta / VoI-proxy fields next to **`score`** in **`evaluate_turn.py`**.
- **`GOLDEN_PATH.md`** — Ladybug path; includes quick **`hsm-eval`** suite commands.
- **`documentation/guides/HARNESS_V1_PLAN.md`** — harness hardening plan.
- **`templates/business/starters/online_commerce_squad/knowledge/dspy_gepa_hsm_bridge.md`** — DSPy/GEPA and meta-harness in the “improve over time” story.
- **README** — “Other ways to run” → external harnesses and YC-bench via **`hsm_outer_loop`**.
