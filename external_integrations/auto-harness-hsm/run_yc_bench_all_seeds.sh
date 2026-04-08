#!/usr/bin/env bash
# Run yc-bench once per seed so aggregate_existing can merge results/*.json.
#
# Usage:
#   export YC_BENCH_ROOT=/Users/cno/yc-bench
#   export OPENROUTER_API_KEY=...
#   ./run_yc_bench_all_seeds.sh
#
# Seeds (space-separated):
#   YC_BENCH_SEEDS="1 2 3 4 5 6 7 8 9"   default when unset
#   YC_BENCH_SEEDS=discover               use seeds already present under results/ for this config+model
# Optional: YC_BENCH_UV YC_BENCH_MODEL YC_BENCH_CONFIG YC_BENCH_NO_LIVE (same semantics as run_yc_bench_apex.sh)
#   YC_BENCH_PAUSE_BETWEEN_SEEDS_SEC=45   sleep between seeds (reduces OpenRouter :free 429 bursts)
#   YC_BENCH_CONTINUE_ON_FAIL=1           exit 0 even if some seeds fail (partial results still on disk)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
: "${YC_BENCH_ROOT:?Set YC_BENCH_ROOT to your yc-bench checkout}"
: "${OPENROUTER_API_KEY:?Set OPENROUTER_API_KEY for model calls}"

UV="${YC_BENCH_UV:-uv}"
MODEL="${YC_BENCH_MODEL:-openrouter/qwen/qwen3.6-plus:free}"
CONFIG="${YC_BENCH_CONFIG:-hsm_market_minimax-studio}"
SLUG="${MODEL//\//_}"
SLUG_ALT="${SLUG//:/_}"

discover_seeds() {
  export YC_BENCH_CONFIG="$CONFIG"
  export SLUG SLUG_ALT
  python3 <<'PY'
import os, re
from pathlib import Path
root = Path(os.environ["YC_BENCH_ROOT"]) / "results"
cfg = os.environ["YC_BENCH_CONFIG"]
slug = os.environ["SLUG"]
slug_alt = os.environ["SLUG_ALT"]
prefix = f"yc_bench_result_{cfg}_"
pat = re.compile("^" + re.escape(prefix) + r"(\d+)_(.+)\.json$")
seeds = []
for p in root.iterdir():
    if not p.is_file():
        continue
    m = pat.match(p.name)
    if not m:
        continue
    rest = m.group(2)
    if rest not in (slug, slug_alt):
        continue
    seeds.append(int(m.group(1)))
seeds = sorted(set(seeds))
if not seeds:
    raise SystemExit("discover: no matching yc_bench_result_*.json under results/")
print(" ".join(str(s) for s in seeds))
PY
}

if [[ "${YC_BENCH_SEEDS:-}" == "discover" ]]; then
  export SLUG SLUG_ALT
  SEEDS="$(discover_seeds)"
  echo "[run_yc_bench_all_seeds] discover -> seeds: $SEEDS"
else
  SEEDS="${YC_BENCH_SEEDS:-1 2 3 4 5 6 7 8 9}"
fi

cd "$YC_BENCH_ROOT"
PAUSE="${YC_BENCH_PAUSE_BETWEEN_SEEDS_SEC:-0}"
any_fail=0
first=1
for seed in $SEEDS; do
  if [[ "$first" -eq 0 && "$PAUSE" -gt 0 ]]; then
    echo "[run_yc_bench_all_seeds] pause ${PAUSE}s (YC_BENCH_PAUSE_BETWEEN_SEEDS_SEC - eases :free tier 429s)" >&2
    sleep "$PAUSE"
  fi
  first=0
  echo "========== seed=$seed =========="
  set -- run --model "$MODEL" --seed "$seed" --config "$CONFIG"
  if [[ "${YC_BENCH_NO_LIVE:-1}" != "0" ]]; then
    set -- "$@" --no-live
  fi
  if ! "$UV" run yc-bench "$@"; then
    echo "[run_yc_bench_all_seeds] seed $seed FAILED (often 429 on :free - retry seed alone or use paid model)" >&2
    any_fail=1
  fi
done

if [[ "$any_fail" -ne 0 ]]; then
  echo "[run_yc_bench_all_seeds] one or more seeds failed" >&2
  if [[ "${YC_BENCH_CONTINUE_ON_FAIL:-0}" == "1" ]]; then
    echo "[run_yc_bench_all_seeds] YC_BENCH_CONTINUE_ON_FAIL=1 - exiting 0; re-run failed seeds only" >&2
    exit 0
  fi
  exit 1
fi
echo "[run_yc_bench_all_seeds] done - run from auto-harness-hsm: python3 benchmark.py --split train && python3 gating.py"
