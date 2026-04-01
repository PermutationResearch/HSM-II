#!/usr/bin/env bash
# Demo: (1) council offline + optional live LLM  (2) A2A delegation routing in dry-run (no Hermes).
#
# Usage from repo root:
#   ./scripts/demo_multi_agent.sh
#   LIVE=1 ./scripts/demo_multi_agent.sh              # LLM: 2 workers
#   LIVE=1 COMPLEX=1 ./scripts/demo_multi_agent.sh      # LLM: 4 specialists + synthesizer + extra offline Debate/stigmergic
#
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "=== Part A: hsm-council-demo (offline council; LIVE=1 for LLM; COMPLEX=1 for bigger demo) ==="
ARGS=()
[[ "${COMPLEX:-}" == "1" ]] && ARGS+=(--complex)
if [[ "${LIVE:-}" == "1" ]]; then
  ARGS+=(--live)
fi
if [[ ${#ARGS[@]} -gt 0 ]]; then
  cargo run -q -p hyper-stigmergy --bin hsm-council-demo -- "${ARGS[@]}"
else
  cargo run -q -p hyper-stigmergy --bin hsm-council-demo
fi

echo ""
echo "=== Part B: hsm_a2a_adapter heartbeat_tick (dry-run — selects delegatee, does not run hermes) ==="
PORT="${DEMO_A2A_PORT:-9799}"
STATE="${ROOT}/target/hsm_demo_a2a_state"
rm -rf "$STATE"
mkdir -p "$STATE"

# Use `cargo run` so the binary path matches the active target dir (works in sandboxes too).
cargo run -q -p hyper-stigmergy --bin hsm_a2a_adapter -- \
  --bind "127.0.0.1:${PORT}" --state-dir "$STATE" --dry-run &
PID=$!

# First run can compile for a while; wait until TCP accepts.
READY=0
for _ in $(seq 1 120); do
  if nc -z 127.0.0.1 "${PORT}" 2>/dev/null; then
    READY=1
    break
  fi
  sleep 1
done
if [[ "$READY" != "1" ]]; then
  echo "Adapter did not listen on 127.0.0.1:${PORT} in time (pid $PID)." >&2
  kill "$PID" 2>/dev/null || true
  exit 1
fi

REQ=$(mktemp)
cat >"$REQ" <<JSON
{"jsonrpc":"2.0","id":1,"method":"heartbeat_tick","params":{
  "trace_id":"demo_trace",
  "from_agent":"orchestrator.local",
  "dry_run":true,
  "ticket":{
    "task_id":"demo_task",
    "objective":"Draft a short checklist for adding health and readiness endpoints to an axum service.",
    "required_capabilities":["code.api.rest"],
    "domain":"software_engineering"
  }
}}
JSON

echo "POST http://127.0.0.1:${PORT}/rpc"
if curl -sS -f -X POST "http://127.0.0.1:${PORT}/rpc" \
  -H 'Content-Type: application/json' \
  -d @"$REQ" | python3 -m json.tool 2>/dev/null; then
  :
else
  echo "(curl failed — is the adapter listening? Raw request was:)"
  cat "$REQ"
fi

kill "$PID" 2>/dev/null || true
wait "$PID" 2>/dev/null || true
rm -f "$REQ"

echo ""
echo "To run real Hermes delegation: start adapter without --dry-run, install Hermes CLI as 'hermes', same curl with dry_run false."
echo "State dir was: $STATE"
