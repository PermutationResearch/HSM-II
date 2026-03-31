#!/usr/bin/env bash
# Offline + optional LLM smoke: build → synthetic eval artifacts → Trace2Skill import/merge
# → optional personal_agent bootstrap + apply. Optionally run hsm-eval and `lbug` compile check.
#
# Usage (from repo root):
#   ./scripts/smoke_e2e.sh
#
# Optional:
#   SMOKE_RUN_LLM=1     Also run hsm-eval (needs OPENAI_API_KEY, ANTHROPIC_API_KEY, or Ollama up)
#   SMOKE_CHAT=1        With SMOKE_RUN_LLM=1, run one `personal_agent chat` turn (uses Ollama client path)
#   SMOKE_CHECK_LBUG=1  Run `cargo check --features lbug` (needs CMake + lbug build deps)
#
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

die() { echo "smoke_e2e: $*" >&2; exit 1; }

echo "== Build core binaries"
cargo build -q -p hyper-stigmergy --bin hsm-eval --bin hsm_trace2skill --bin personal_agent

ART="$ROOT/target/hsm_smoke_artifacts"
STAGE="$ROOT/target/hsm_smoke_home"
rm -rf "$ART"
mkdir -p "$ART/memory"

echo "== Write synthetic eval layout (suite memory/, task se-01 turn 0)"
# Minimal manifest (must use a registered suite name for task_map_for_artifacts).
cat >"$ART/manifest.json" <<'MANIFEST'
{
  "run_id": "smoke",
  "created_unix": 1700000000,
  "git_commit": null,
  "harness": "smoke_e2e",
  "suites": ["memory"],
  "suite_weights": null,
  "tasks_filter": null,
  "task_count": 1,
  "turn_count": 1,
  "parent_run_id": null,
  "artifact_paths": {
    "manifest": "manifest.json",
    "turns_hsm_jsonl": "memory/turns_hsm.jsonl"
  }
}
MANIFEST

# One TurnMetrics line aligned with eval task se-01 / turn 0 (keywords for scoring).
cat >"$ART/memory/turns_hsm.jsonl" <<'JSONL'
{"task_id":"se-01","turn_index":0,"session":1,"requires_recall":false,"response":"We use POST and GET with JWT bearer tokens for tasks, projects, and users per REST practice.","latency_ms":1,"prompt_tokens":10,"completion_tokens":20,"keyword_score":0.85,"llm_calls":1,"error":null,"deterministic_pass":true,"rubric_pass":true,"rubric_composite":0.85,"grounding_applicable":false,"grounding_score":1.0,"grounding_pass":true,"tool_check_applicable":false,"tool_pass":null,"llm_judge_pass":null,"llm_judge_notes":null,"wall_clock_ms":1,"llm_http_requests":1}
JSONL

TRAJ="$ART/traj.jsonl"
MERGED="$ART/merged.json"

echo "== Trace2Skill: import-eval → merge"
cargo run -q -p hyper-stigmergy --bin hsm_trace2skill -- import-eval --artifacts "$ART" --out "$TRAJ"
[[ -s "$TRAJ" ]] || die "empty trajectory JSONL"
cargo run -q -p hyper-stigmergy --bin hsm_trace2skill -- merge --in "$TRAJ" --out "$MERGED"
[[ -s "$MERGED" ]] || die "empty merged.json"

echo "== personal_agent bootstrap + apply (isolated HSMII_HOME + cwd)"
rm -rf "$STAGE"
mkdir -p "$STAGE"
export HSMII_HOME="$STAGE"
(
  cd "$STAGE"
  cargo run -q -p hyper-stigmergy --bin personal_agent -- bootstrap
  cargo run -q -p hyper-stigmergy --bin hsm_trace2skill -- apply --merged "$MERGED"
)
echo "  (world + skill ingest under $STAGE)"

have_llm=0
if [[ -n "${OPENAI_API_KEY:-}" ]] || [[ -n "${ANTHROPIC_API_KEY:-}" ]]; then
  have_llm=1
elif curl -sf "http://127.0.0.1:11434/api/tags" >/dev/null 2>&1; then
  have_llm=1
fi

if [[ "${SMOKE_RUN_LLM:-}" == "1" ]]; then
  if [[ "$have_llm" -ne 1 ]]; then
    die "SMOKE_RUN_LLM=1 but no OPENAI_API_KEY/ANTHROPIC_API_KEY and Ollama not responding on :11434"
  fi
  echo "== hsm-eval (multi-suite slice, artifacts refresh)"
  EVAL_ART="$ROOT/target/hsm_smoke_eval_out"
  rm -rf "$EVAL_ART"
  cargo run -q -p hyper-stigmergy --bin hsm-eval -- --suites "memory:1,tool:1,council:1" --limit 2 --artifacts "$EVAL_ART"
  echo "== Trace2Skill from real eval artifacts"
  TRAJ2="$EVAL_ART/traj_from_eval.jsonl"
  cargo run -q -p hyper-stigmergy --bin hsm_trace2skill -- import-eval --artifacts "$EVAL_ART" --out "$TRAJ2"
  [[ -s "$TRAJ2" ]] || die "import-eval from hsm-eval artifacts produced no rows"
  echo "  wrote $TRAJ2 ($(wc -l < "$TRAJ2") lines)"
  if [[ "${SMOKE_CHAT:-}" == "1" ]]; then
    echo "== personal_agent chat (one message, same HSMII_HOME)"
    (
      cd "$STAGE"
      export HSMII_HOME="$STAGE"
      cargo run -q -p hyper-stigmergy --bin personal_agent -- chat --message "Reply with exactly: smoke_ok"
    )
  fi
else
  echo "== Skip hsm-eval (set SMOKE_RUN_LLM=1 and configure an LLM to include it)"
fi

if [[ "${SMOKE_CHECK_LBUG:-}" == "1" ]]; then
  echo "== cargo check --features lbug (optional native graph; may fail without CMake)"
  cargo check -q -p hyper-stigmergy --no-default-features --features lbug || {
    echo "smoke_e2e: lbug check failed — install CMake / lbug deps or omit SMOKE_CHECK_LBUG=1" >&2
    exit 1
  }
else
  echo "== Skip lbug (set SMOKE_CHECK_LBUG=1 to compile with Ladybug primary feature)"
fi

echo "== Smoke OK"
