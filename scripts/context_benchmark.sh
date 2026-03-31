#!/usr/bin/env bash
# Context assembly benchmark gate:
# - runs small eval slice
# - checks retrieval/context quality deltas vs baseline
# - fails on significant regression
#
# Usage:
#   scripts/context_benchmark.sh
#
# Optional env thresholds:
#   CONTEXT_SUITE=memory
#   CONTEXT_LIMIT=3
#   CONTEXT_MIN_KEYWORD_DELTA=0.00
#   CONTEXT_MIN_RECALL_DELTA=0.00
#   CONTEXT_MIN_RUBRIC_DELTA=0.00
#   CONTEXT_MAX_LATENCY_RATIO=1.80
#
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

SUITE="${CONTEXT_SUITE:-memory}"
LIMIT="${CONTEXT_LIMIT:-3}"
OUT_DIR="${ROOT}/target/context_benchmark"
OUT_JSON="${OUT_DIR}/report.json"
SUITE_JSON="${OUT_DIR}/report_${SUITE}.json"

MIN_KEYWORD_DELTA="${CONTEXT_MIN_KEYWORD_DELTA:-0.00}"
MIN_RECALL_DELTA="${CONTEXT_MIN_RECALL_DELTA:-0.00}"
MIN_RUBRIC_DELTA="${CONTEXT_MIN_RUBRIC_DELTA:-0.00}"
MAX_LATENCY_RATIO="${CONTEXT_MAX_LATENCY_RATIO:-1.80}"

mkdir -p "$OUT_DIR"

echo "== context benchmark: suite=${SUITE} limit=${LIMIT}"
cargo run -q -p hyper-stigmergy --bin hsm-eval -- \
  --suite "$SUITE" \
  --limit "$LIMIT" \
  --json "$OUT_JSON" \
  >/dev/null

if [[ ! -s "$SUITE_JSON" ]]; then
  echo "context_benchmark: missing expected suite report: $SUITE_JSON" >&2
  exit 1
fi

python3 - "$SUITE_JSON" "$MIN_KEYWORD_DELTA" "$MIN_RECALL_DELTA" "$MIN_RUBRIC_DELTA" "$MAX_LATENCY_RATIO" <<'PY'
import json, sys

path, min_kw, min_rec, min_rub, max_lat_ratio = sys.argv[1:]
min_kw = float(min_kw)
min_rec = float(min_rec)
min_rub = float(min_rub)
max_lat_ratio = float(max_lat_ratio)

with open(path, "r", encoding="utf-8") as f:
    report = json.load(f)

imp = report["improvement"]
kw = float(imp["keyword_score_delta"])
rec = float(imp["recall_score_delta"])
rub = float(imp["rubric_composite_delta"])
base_lat = float(report["baseline"]["avg_latency_ms"]) or 1.0
hsm_lat = float(report["hsm"]["avg_latency_ms"])
lat_ratio = hsm_lat / base_lat

print(f"keyword_delta={kw:+.4f} recall_delta={rec:+.4f} rubric_delta={rub:+.4f} latency_ratio={lat_ratio:.3f}")

errors = []
if kw < min_kw:
    errors.append(f"keyword delta {kw:+.4f} < min {min_kw:+.4f}")
if rec < min_rec:
    errors.append(f"recall delta {rec:+.4f} < min {min_rec:+.4f}")
if rub < min_rub:
    errors.append(f"rubric delta {rub:+.4f} < min {min_rub:+.4f}")
if lat_ratio > max_lat_ratio:
    errors.append(f"latency ratio {lat_ratio:.3f} > max {max_lat_ratio:.3f}")

if errors:
    print("context_benchmark FAILED:")
    for e in errors:
        print(f"  - {e}")
    sys.exit(1)

print("context_benchmark OK")
PY

