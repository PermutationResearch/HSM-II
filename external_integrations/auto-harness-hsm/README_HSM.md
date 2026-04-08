# auto-harness — YC-bench / HSM (Apex) fork

Vendored [NeoSigma auto-harness](https://github.com/neosigmaai/auto-harness) with **`benchmark_backend: yc_hsm`** wired in `gating.py`, `benchmark.py`, and `prepare.py`.

## Prereqs

- Local [yc-bench](https://github.com/) checkout (`yc_bench_root` in `experiment_config.yaml`).
- `uv` on `PATH`.
- API key for your model (default: `OPENROUTER_API_KEY`).

## Refresh all seeds (then aggregate in harness)

`yc_rollout_mode: aggregate_existing` merges every matching `results/yc_bench_result_<config>_<seed>_<model>.json` under `yc_bench_root`. To **re-run yc-bench for many seeds** in one go:

```bash
export OPENROUTER_API_KEY=sk-or-v1-...
export YC_BENCH_ROOT=/Users/cno/yc-bench   # match experiment_config.yaml
cd /Users/cno/hyper-stigmergic-morphogenesisII/external_integrations/auto-harness-hsm
./run_yc_bench_all_seeds.sh    # default seeds: 1 2 3 4 5 6 7 8 9
```

- **`YC_BENCH_SEEDS="6 7 10"`** — only those seeds.
- **`YC_BENCH_SEEDS=discover`** — re-run every seed that **already has** a matching JSON (same config + model slug).
- **`YC_BENCH_PAUSE_BETWEEN_SEEDS_SEC=45`** — sleep between seeds (helps with OpenRouter **`:free`** upstream 429s when running many long sims back-to-back).
- **`YC_BENCH_CONTINUE_ON_FAIL=1`** — exit 0 if some seeds fail; fix failed seeds and re-run, or delete bad `results/*.json` before aggregating.

**Reading logs:** `Agent output length: 44, commands: 1` on **`sim resume`** is normal. `Turn attempt N failed: RateLimitError 429` means the **free** model slot was throttled; yc-bench retries a few times then may end with `terminal_reason=error` (e.g. seed 9) — that rollout is still written; remove it or re-run that seed after switching to a **non-`:free`** model or adding pause.

Then **`python3 prepare.py --force-baseline`** if the aggregate bar should move, and **`python3 benchmark.py --split train && python3 gating.py`**.

**One-liner** (fresh rollouts for seeds 1–9, then train + gate):

```bash
export OPENROUTER_API_KEY='YOUR_KEY' && export YC_BENCH_ROOT=/Users/cno/yc-bench && /Users/cno/hyper-stigmergic-morphogenesisII/external_integrations/auto-harness-hsm/run_yc_bench_all_seeds.sh && cd /Users/cno/hyper-stigmergic-morphogenesisII/external_integrations/auto-harness-hsm && python3 benchmark.py --split train && python3 gating.py
```

## One-time setup

```bash
cd /Users/cno/hyper-stigmergic-morphogenesisII/external_integrations/auto-harness-hsm
# Edit experiment_config.yaml — set yc_bench_root, yc_model, yc_seed, yc_config if needed
export OPENROUTER_API_KEY=sk-or-v1-...
python3 prepare.py
```

`prepare.py` initializes `workspace/` and runs a **baseline** full YC-bench gate split (slow). If `workspace/results.tsv` already has rows, baseline is **skipped** unless you run **`python3 prepare.py --force-baseline`** (backs up the TSV first) — do that after changing `yc_model` or `yc_rollout_mode` so the gate compares against the right policy.

## Optimization loop (same idea as upstream `PROGRAM.md`)

1. **Train benchmark** (writes `workspace/train_results.json`):

   ```bash
   python3 benchmark.py --split train
   ```

2. **Analyze failures** using train output / yc-bench logs; update `workspace/learnings.md`.

3. **Edit only** `agent/agent.py` (upstream rule).  
   Note: YC-bench uses its own tools inside the sim; this file is still the harness the *coding agent* improves for tau-style workflows — for pure YC-bench, improvements may instead live in prompts you mirror from here into HSM. Adjust expectations to your setup.

4. **Gate** (regression suite + full eval + promotion):

   ```bash
   python3 gating.py
   ```

5. On success: commit + `python3 record.py --val-score … --evals-passed … --evals-total …`.

6. Repeat.

## All marketplace companies (portfolio view)

`experiment_config.yaml` pins **one** `yc_config` (e.g. `hsm_market_apex-systems`) for the NeoSigma gate. Your HSM external specs (`config/external_yc_bench_seed*.json`) run **many** `hsm_market_<company>` configs; their rollouts all land under the same yc-bench `results/` tree.

To score **every company** that has matching JSONs for a given model — same per-seed / aggregate rules as `aggregate_existing`, plus a **portfolio** line (mean of per-company aggregates):

```bash
cd /Users/cno/hyper-stigmergic-morphogenesisII/external_integrations/auto-harness-hsm
YC_BENCH_ROOT=/Users/cno/yc-bench python3 summarize_yc_bench_market_grid.py --json-out workspace/all_companies_summary.json
```

That does **not** replace `gating.py` (still one config per workspace). Use it for **dashboards / regression triage** across the full grid; keep a dedicated harness folder per company if you want a full gate per pack.

### Worst turns + transcript snippets

After `python3 benchmark.py --split train`, inspect low `turn_*` scores and optional lines from a single-seed rollout JSON:

```bash
python3 list_worst_turns.py --top 20 --below 0.5
python3 list_worst_turns.py --discover-rollout          # all seeds: snippet from worst-scoring seed per turn
python3 list_worst_turns.py --discover-rollout --seed 7 # single-seed snippets only
python3 list_worst_turns.py --rollout /Users/cno/yc-bench/results/yc_bench_result_....json
```

Use that to drive pack / prompt fixes, then refresh rollouts and re-run benchmark + gate.

## Apex reliability (scores vs noise)

What actually moves `val_score` is the **LLM inside yc-bench** (OpenRouter by default), not `agent/agent.py` unless your fork wires the harness into the sim.

1. **429 / flaky runs:** OpenRouter **`:free`** endpoints are rate-limited. Prefer a **non-free** `yc_model` in `experiment_config.yaml` when you care about stable gates.
2. **Retries:** `yc_bench_max_retries` and `yc_bench_retry_base_seconds` make `ychsm_benchmark_runner.py` re-run `yc-bench` when stderr/stdout looks like rate limits, 502/503, or timeouts (bounded exponential backoff).
3. **Cheap iteration on scoring code only:** `yc_rollout_mode: aggregate_existing` reuses saved `results/yc_bench_result_*.json` for full runs (see below) so you avoid new API calls while editing the normalizer or gate — not a substitute for a paid model when you need fresh rollouts.

## How scoring works

`ychsm_benchmark_runner.py` runs `uv run yc-bench run …`, then reads `results/yc_bench_result_<yc_config>_<seed>_<model>.json` and converts it to flat `turn_*` scores plus `_run_terminal` (see HSM `normalize_yc_hsm_results.py`). `val_score` is the mean over keys that do **not** start with `_`.

By default only **one** seed is used (`yc_seed` in `experiment_config.yaml`) so each gate compares a single rollout to the `prepare.py` baseline (one number per row in `workspace/results.tsv`). The runner did not originally glob older seeds.

Optional **`yc_rollout_mode: aggregate_existing`**: if matching files exist under `yc_bench_root/results/`, **full** runs load every `yc_bench_result_<yc_config>_<seed>_<model>.json`, set **`val_score`** to the **mean of each file’s per-seed `val_score`**, and merge per-turn rewards by averaging across seeds — **no** `yc-bench` subprocess. **Suite / gate step 3** requests specific task IDs: upstream **`yc-bench run` has no `--task-ids`**, so the runner serves those IDs by **slicing the merged aggregate** (still no API calls). If no rollouts match, it runs a **full** `yc-bench` and subsets locally. After enabling this, re-run **`prepare.py --force-baseline`** so `results.tsv` matches the policy.

## Copy upstream updates

This tree was cloned from `neosigmaai/auto-harness`. To refresh, re-clone upstream and re-apply HSM patches (or merge manually).
