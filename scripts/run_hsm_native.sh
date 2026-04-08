#!/usr/bin/env bash
# Stigmergic Memory Benchmark — persists JSON under runs/hsm_native/ unless you pass --json / --jsonl yourself.
set -euo pipefail

cd "$(dirname "$0")/.."

# Default model id (OpenAI API). With OPENROUTER_API_KEY, OpenRouter still expects the same
# style: openai/<model> — NOT openrouter/openai/... (that string is invalid on OpenRouter).
export DEFAULT_LLM_MODEL="${DEFAULT_LLM_MODEL:-openai/gpt-5.4}"

has_json=false
has_jsonl=false
for a in "$@"; do
  [[ "$a" == --json ]] && has_json=true
  [[ "$a" == --jsonl ]] && has_jsonl=true
done

OUT_DIR="runs/hsm_native"
mkdir -p "$OUT_DIR"
extra=()
if ! $has_json; then
  extra+=(--json "$OUT_DIR/report.json")
fi
if ! $has_jsonl; then
  extra+=(--jsonl "$OUT_DIR/tasks.jsonl")
fi

cargo run --bin hsm-native-eval -- "$@" "${extra[@]}"
echo ""
echo "SMB artifacts: $OUT_DIR/report.json  $OUT_DIR/tasks.jsonl  (runs/ is gitignored — copy or commit a snapshot if you want it in git)"

