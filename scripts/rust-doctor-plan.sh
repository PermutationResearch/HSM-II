#!/usr/bin/env bash
# Run rust-doctor with a TTY so output is line-buffered (fixes "hang" when piped to head).
# Writes a copy to target/rust-doctor-plan.txt for logs/CI.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
mkdir -p target
OUT="${RUST_DOCTOR_PLAN_OUT:-$ROOT/target/rust-doctor-plan.txt}"
echo "rust-doctor: writing plan to $OUT (this can take several minutes)…" >&2
if [[ "$(uname -s)" == "Darwin" ]] && command -v script >/dev/null 2>&1; then
  # macOS: allocate a pseudo-tty so rust-doctor stderr/stdout are line-buffered.
  script -q /dev/null rust-doctor --offline --plan "$@" 2>&1 | tee "$OUT"
else
  rust-doctor --offline --plan "$@" 2>&1 | tee "$OUT"
fi
echo "Done. Plan: $OUT" >&2
