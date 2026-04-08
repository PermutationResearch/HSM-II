# NeoSigma auto-harness × YC-bench × HSM (apex-systems)

This folder helps you run [NeoSigma’s auto-harness](https://github.com/neosigmaai/auto-harness) self-improvement loop ([blog: self-improving agentic systems](https://www.neosigma.ai/blog/self-improving-agentic-systems)) **against your YC-bench setup** for the **apex-systems** marketplace pack (`hsm_market_apex-systems`), the same labeling pattern used in `config/external_yc_bench_seed*.json` in this repo.

## What you get

- **Outer loop (auto-harness):** mine failures → propose harness changes → **gate** (regression suite + val score) → record → repeat, as described in upstream `PROGRAM.md`.
- **Benchmark (YC-bench):** your existing `uv run yc-bench run ... --config hsm_market_apex-systems` command becomes the “reward” source instead of Tau2.
- **HSM-II:** continue to use `cargo run --bin hsm_outer_loop -- external-batch --spec config/external_yc_bench_seed*.json` for batch scoring and the company console YC-bench aggregator (`README.md` in repo root).

### Fix “Usage error” / `ok:false` on `yc-bench` commands (LLM tool-use)

Rollouts often fail because the **in-sim agent** chains shell (`&&`, `| head`), uses non-existent subcommands (`yc-bench team`), or omits required flags (`task cancel` without `--reason`). This repo ships:

- **`snippets/yc_bench_sim_cli_rules.txt`** — short mechanical rules to append to `[agent] system_prompt`.
- **`patch_hsm_market_system_prompts.py`** — idempotently appends that block to every `hsm_market_*.toml` under your yc-bench checkout.

```bash
python3 external_integrations/auto-harness-yc-bench/patch_hsm_market_system_prompts.py "$YC_BENCH_ROOT"
```

Re-run benchmarks after patching; commit the updated `.toml` files **in your yc-bench repo** (they are not stored inside HSM-II by default).

`agent/agent.py` in auto-harness is still the file the coding agent edits. For HSM, treat that module as the **apex harness** (prompts, tool wiring, state) that mirrors what you want in company OS / pack skills—then **promote** stable improvements into your repo’s `business/` pack or `SKILL.md` files (see “Closing the loop back into HSM” below).

## Run it on your machine

**A — Re-normalize an existing yc-bench result (no API calls, instant)**  
Uses `results/yc_bench_result_<config>_<seed>_<model>.json` under your yc-bench checkout.

```bash
export YC_BENCH_ROOT=/path/to/yc-bench   # e.g. ~/yc-bench
export YC_BENCH_SKIP_RUN=1
./external_integrations/auto-harness-yc-bench/run_yc_bench_apex.sh
```

Output: `external_integrations/auto-harness-yc-bench/workspace/yc_hsm_results.json` (per-turn scores + `_run_terminal`).

**B — Full Apex benchmark run (calls your LLM; can take a long time)**  
Requires `OPENROUTER_API_KEY` (or whatever your yc-bench model provider needs) in the environment.

```bash
export YC_BENCH_ROOT=/path/to/yc-bench
export OPENROUTER_API_KEY=sk-or-v1-...
./external_integrations/auto-harness-yc-bench/run_yc_bench_apex.sh
```

Optional: `YC_BENCH_SEED=7`, `YC_BENCH_MODEL=...`, `YC_BENCH_CONFIG=hsm_market_apex-systems`.

**C — Normalize one file by hand**

```bash
python3 external_integrations/auto-harness-yc-bench/normalize_yc_hsm_results.py \
  -i /path/to/yc_bench_result_....json --format yc_rollout \
  -o external_integrations/auto-harness-yc-bench/workspace/yc_hsm_results.json
```

## Vendored fork (fastest path)

This repo includes a **pre-wired** copy: [`../auto-harness-hsm/`](../auto-harness-hsm/) (see `README_HSM.md` there). Set `yc_bench_root` + API keys, then `python3 prepare.py`, `python3 benchmark.py --split train`, edit `agent/agent.py`, `python3 gating.py`.

## Quick integration steps (upstream clone)

1. Clone auto-harness and copy the runner into it:

   ```bash
   git clone https://github.com/neosigmaai/auto-harness
   cd auto-harness
   cp /path/to/hyper-stigmergic-morphogenesisII/external_integrations/auto-harness-yc-bench/ychsm_benchmark_runner.py .
   cp /path/to/hyper-stigmergic-morphogenesisII/external_integrations/auto-harness-yc-bench/experiment_config.yc_hsm_apex.example.yaml experiment_config.yaml
   ```

2. Edit `experiment_config.yaml` (paths, model, seed, `hsm_market_*` config name).

3. Point `gating.py` at the new runner (replace `TauBenchRunner` imports and constructors):

   ```python
   from ychsm_benchmark_runner import YcHsmBenchRunner

   train_runner = YcHsmBenchRunner(split="train")
   gate_runner = YcHsmBenchRunner(split="test")
   ```

   If YC-bench only exposes a **single** preset (no train/test split), use the same runner for both until your bench supports splits.

4. **Per-task scores JSON (required for auto-harness):** after each `yc-bench run`, there must be a flat map `task_id -> float` (0–1). This repo ships helpers in this folder:

   - **`normalize_yc_hsm_results.py`** — converts common JSON / JSONL shapes into `workspace/yc_hsm_results.json` (or `-o` / `YCHSM_OUT`). Examples:
     ```bash
     # From a summary file your yc-bench already writes:
     python3 normalize_yc_hsm_results.py -i /path/to/bench_output.json -o workspace/yc_hsm_results.json

     # From JSONL (one object per line with task_id + score / passed / success):
     python3 normalize_yc_hsm_results.py -i traces.jsonl --format jsonl -o workspace/yc_hsm_results.json
     ```
     Run `python3 normalize_yc_hsm_results.py -h` for `--id-key` / `--score-key` overrides.

   - **`run_yc_bench_apex.sh`** — runs `uv run yc-bench run` with the same defaults as `config/external_yc_bench_seed*.json` for Apex (`hsm_market_apex-systems`), then calls the normalizer. Set `YC_BENCH_ROOT`, then either:
     - **`YC_BENCH_RAW_JSON`** — path to the JSON artifact your bench produces (recommended), or
     - **`YC_BENCH_STDOUT_JSONL=1`** — if the bench prints only JSONL lines on stdout.

   Target shape:

   ```json
   { "0": 1.0, "1": 0.0, "task_slug": 0.5 }
   ```

5. Run upstream `prepare.py`, then `benchmark.py`, then follow `PROGRAM.md` with your coding agent.

## Does this alone run auto-harness and “make Apex better”?

**Not by itself.** A stable `yc_hsm_results.json` is a **prerequisite**: NeoSigma’s loop needs numeric rewards per task to gate harness edits. You still must:

1. Copy **`ychsm_benchmark_runner.py`** into the auto-harness tree and wire **`gating.py`** (see above).
2. Tune **`experiment_config.yaml`** and train/val splits (or use one split twice until the bench supports two).
3. Run auto-harness’s **prepare / benchmark / PROGRAM** flow so the coding agent can change **`agent/agent.py`** (or your harness) and re-score.

**“Better Apex”** then means: **higher scores on your YC-bench tasks** under that harness, after edits pass regression + validation gates—not automatic promotion into HSM company OS packs; you still **merge or port** winning changes into skills/packs in git (see “Closing the loop” below).

## Closing the loop back into HSM (skills / agents)

Aligned with the blog’s “failures → eval cases → harness changes” narrative:

1. **Failures as signal:** use HSM `trace2skill` / eval artifacts from `runs/external_batch_*.json` to tag low-scoring apex-systems runs.
2. **Regression suite:** keep `workspace/suite.json` in auto-harness in sync with the task IDs you care about on the apex pack.
3. **Promote:** when a gated change improves val score, copy prompt / tool policy deltas from `agent/agent.py` (or extracted modules) into your apex company pack or shared skills under the repo’s business pack layout—same idea as NeoSigma’s durable harness evolution, but versioned in git.

## References

- [NeoSigma: Self-Improving Agentic Systems](https://www.neosigma.ai/blog/self-improving-agentic-systems)
- [auto-harness README](https://github.com/neosigmaai/auto-harness) (Tau2 default; plug-in benchmark via `BenchmarkRunner`)
- HSM external batch: `config/external_yc_bench.example.json` and `config/external_yc_bench_seed*.json` (`company_pack`: `apex-systems`)
