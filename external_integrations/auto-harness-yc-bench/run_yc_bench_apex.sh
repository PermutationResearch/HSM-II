#!/usr/bin/env bash
# Run yc-bench the same way as HSM external specs (config/external_yc_bench_seed*.json),
# then normalize bench output -> workspace/yc_hsm_results.json for YcHsmBenchRunner / auto-harness.
#
# Usage:
#   export YC_BENCH_ROOT=/path/to/yc-bench
#   export OPENROUTER_API_KEY=...   # required for real LLM runs
#   Optional:
#     YC_BENCH_UV=uv
#     YC_BENCH_MODEL=openrouter/qwen/qwen3.6-plus:free
#     YC_BENCH_SEED=6
#     YC_BENCH_CONFIG=hsm_market_apex-systems
#     YC_BENCH_NO_LIVE=0            # omit --no-live (default: add --no-live)
#     YCHSM_WORKSPACE=/path         # default: ./workspace next to this script
#     YC_BENCH_RAW_JSON=path        # override input JSON (otherwise we use results/yc_bench_result_*.json)
#     YC_BENCH_STDOUT_JSONL=1       # yc-bench prints JSONL lines only on stdout
#     YC_BENCH_SKIP_RUN=1           # do not invoke yc-bench; only normalize from RAW_JSON or results/*.json
#   chmod +x run_yc_bench_apex.sh && ./run_yc_bench_apex.sh
#
# After a normal run, yc-bench writes:
#   $YC_BENCH_ROOT/results/yc_bench_result_${CONFIG}_${SEED}_<model_with_slashes_to_underscores>.json
# This script maps that file with normalize_yc_hsm_results.py --format yc_rollout.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
WORKSPACE="${YCHSM_WORKSPACE:-$ROOT/workspace}"
mkdir -p "$WORKSPACE"

: "${YC_BENCH_ROOT:?Set YC_BENCH_ROOT to your yc-bench checkout}"
UV="${YC_BENCH_UV:-uv}"
MODEL="${YC_BENCH_MODEL:-openrouter/qwen/qwen3.6-plus:free}"
SEED="${YC_BENCH_SEED:-6}"
CONFIG="${YC_BENCH_CONFIG:-hsm_market_apex-systems}"
SLUG="${MODEL//\//_}"
DEFAULT_RESULT="$YC_BENCH_ROOT/results/yc_bench_result_${CONFIG}_${SEED}_${SLUG}.json"

OUT_LOG="$WORKSPACE/yc_bench_last.out.log"
ERR_LOG="$WORKSPACE/yc_bench_last.err.log"
RESULT_JSON="$WORKSPACE/yc_hsm_results.json"
NORM="$ROOT/normalize_yc_hsm_results.py"

RC=0
if [[ "${YC_BENCH_SKIP_RUN:-0}" != "1" ]]; then
  cd "$YC_BENCH_ROOT"
  set -- run --model "$MODEL" --seed "$SEED" --config "$CONFIG"
  if [[ "${YC_BENCH_NO_LIVE:-1}" != "0" ]]; then
    set -- "$@" --no-live
  fi
  set +e
  "$UV" run yc-bench "$@" >"$OUT_LOG" 2>"$ERR_LOG"
  RC=$?
  set -e
  echo "yc-bench exit=$RC (stdout $OUT_LOG stderr $ERR_LOG)"
fi

if [[ -n "${YC_BENCH_RAW_JSON:-}" ]]; then
  if [[ ! -f "$YC_BENCH_RAW_JSON" ]]; then
    echo "YC_BENCH_RAW_JSON not found: $YC_BENCH_RAW_JSON" >&2
    exit 1
  fi
  python3 "$NORM" -i "$YC_BENCH_RAW_JSON" -o "$RESULT_JSON"
elif [[ -f "$DEFAULT_RESULT" ]]; then
  echo "Normalizing $DEFAULT_RESULT -> $RESULT_JSON"
  python3 "$NORM" -i "$DEFAULT_RESULT" --format yc_rollout -o "$RESULT_JSON" --fail-empty
elif [[ -n "${YC_BENCH_STDOUT_JSONL:-}" ]]; then
  python3 "$NORM" -i "$OUT_LOG" --format jsonl -o "$RESULT_JSON" --fail-empty
else
  if ! python3 "$NORM" -i "$OUT_LOG" -o "$RESULT_JSON" --fail-empty; then
    echo "" >&2
    echo "No result file at: $DEFAULT_RESULT" >&2
    echo "Set YC_BENCH_RAW_JSON, run yc-bench first, or use YC_BENCH_SKIP_RUN=1 with an existing results JSON." >&2
    exit "${RC:-1}"
  fi
fi

echo "Normalized -> $RESULT_JSON"
exit "$RC"
