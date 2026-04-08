#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

MODEL="${DEFAULT_LLM_MODEL:-openai/gpt-5.4}"
OUT_DIR="${1:-runs/regression}"
mkdir -p "$OUT_DIR"

echo "[1/3] HSM-native full"
cargo run --bin hsm-native-eval -- \
  --variant both \
  --json "$OUT_DIR/hsm_native.json" \
  --jsonl "$OUT_DIR/hsm_native.jsonl" \
  --trace-output "$OUT_DIR/hsm_native.trace.jsonl" \
  --traces

echo "[2/3] LongMemEval slice"
cargo run --bin hsm-longmemeval -- \
  --input external/LongMemEval/data/longmemeval_oracle.json \
  --output "$OUT_DIR/longmemeval_hsm.jsonl" \
  --mode hsm \
  --limit 10 \
  --trace-output "$OUT_DIR/longmemeval_hsm.trace.jsonl" \
  --max-attempts 3 \
  --retry-sleep-secs 10

echo "[3/3] YC-Bench aggregate check"
if [ -d /Users/cno/yc-bench ]; then
  (cd /Users/cno/yc-bench && python3 scripts/aggregate.py) | tee "$OUT_DIR/yc_bench_aggregate.txt"
else
  echo "yc-bench checkout not found at /Users/cno/yc-bench" | tee "$OUT_DIR/yc_bench_aggregate.txt"
fi

