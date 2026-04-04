#!/usr/bin/env bash
# Regenerate ARCHITECTURE.generated.md from embedded architecture/hsm-ii-blueprint.ron.
# Run from anywhere; script cd's to repo root.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
cargo run -q --bin hsm_archviz -- markdown > ARCHITECTURE.generated.md
echo "Wrote $ROOT/ARCHITECTURE.generated.md ($(wc -l < ARCHITECTURE.generated.md | tr -d ' ') lines)"
