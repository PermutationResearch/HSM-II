#!/usr/bin/env bash
# Per-domain memory ablation: baseline vs HSM with memory off, and HSM top-k sweeps.
# Usage:
#   ./scripts/eval_memory_ablation.sh [OUT_ROOT]
# Env:
#   SUITE=full   (or memory, tool, council)
#   DOMAIN=      (empty = full suite; or software_engineering | data_science | business | research | stress_test)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/runs/memory_ablation_$(date +%Y%m%d_%H%M%S)}"
SUITE="${SUITE:-full}"
DOMAIN="${DOMAIN:-}"

mkdir -p "$OUT"
cd "$ROOT"

run_eval() {
  local label="$1"
  shift
  echo "=== $label ==="
  cargo run -p hyper-stigmergy --bin hsm-eval --quiet -- \
    --suite "$SUITE" \
    ${DOMAIN:+--task-domain "$DOMAIN"} \
    --artifacts "$OUT/$label" \
    --trace \
    "$@"
}

if [[ -n "$DOMAIN" ]]; then
  echo "Domain filter: $DOMAIN"
else
  echo "Full suite (no --task-domain)"
fi

# Each run: vanilla baseline vs HSM-II under the same artifact label (compare reports side-by-side).
run_eval "00_default_hsm_memory_on"
run_eval "01_hsm_memory_off" --hsm-no-memory
run_eval "02_hsm_topk1" --hsm-context-top-k 1
run_eval "03_hsm_topk2" --hsm-context-top-k 2
run_eval "04_hsm_topk3" --hsm-context-top-k 3

echo "Artifacts under $OUT"
