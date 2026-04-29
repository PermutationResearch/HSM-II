#!/usr/bin/env bash
set -euo pipefail

BASE="${HSM_CONSOLE_URL:-http://127.0.0.1:3847}"
BASE="${BASE%/}"
WEB_BASE="${HSM_COMPANY_CONSOLE_URL:-http://127.0.0.1:3050}"
WEB_BASE="${WEB_BASE%/}"
WEB_WAIT_SECS="${HSM_E2E_WEB_WAIT_SECS:-30}"
API_WAIT_SECS="${HSM_E2E_API_WAIT_SECS:-30}"
STREAM_TIMEOUT_SECS="${HSM_E2E_STREAM_TIMEOUT_SECS:-180}"
REPORT_PATH="${HSM_E2E_CAPABILITY_REPORT_PATH:-/tmp/agent-chat-capability-matrix.json}"

require_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "error: required command '$1' not found" >&2
    exit 1
  fi
}

wait_for_url() {
  local url="$1"
  local max_secs="$2"
  local i
  for i in $(seq 1 "$max_secs"); do
    if curl -sfS -m 3 "$url" >/dev/null 2>&1; then
      return 0
    fi
    sleep 1
  done
  return 1
}

require_cmd curl
require_cmd python3

echo "[1/4] Verifying required services..."
wait_for_url "$WEB_BASE" "$WEB_WAIT_SECS" || {
  echo "error: Company Console is not live at $WEB_BASE" >&2
  exit 1
}
wait_for_url "$BASE/api/company/health" "$API_WAIT_SECS" || {
  echo "error: Company OS API is not healthy at $BASE/api/company/health" >&2
  exit 1
}
echo "ok: service checks passed"

echo "[2/4] Resolving company/task context..."
read -r COMPANY_ID TASK_ID PERSONA <<EOF
$(BASE="$BASE" HSM_COMPANY_ID="${HSM_COMPANY_ID:-}" HSM_TASK_ID="${HSM_TASK_ID:-}" HSM_PERSONA="${HSM_PERSONA:-}" python3 - <<'PY'
import json, os, urllib.request
base = os.environ["BASE"]
company_id = (os.environ.get("HSM_COMPANY_ID") or "").strip()
task_id = (os.environ.get("HSM_TASK_ID") or "").strip()
persona = (os.environ.get("HSM_PERSONA") or "operator").strip() or "operator"
if not company_id:
    with urllib.request.urlopen(base + "/api/company/companies", timeout=15) as r:
        companies = json.load(r).get("companies", [])
    if not companies:
        print("", "", persona)
        raise SystemExit(0)
    company_id = companies[0]["id"]
if not task_id:
    with urllib.request.urlopen(f"{base}/api/company/companies/{company_id}/tasks", timeout=15) as r:
        task_payload = json.load(r)
    tasks = task_payload if isinstance(task_payload, list) else task_payload.get("tasks", task_payload.get("items", []))
    if tasks:
        task = tasks[0]
        task_id = task["id"]
        if "HSM_PERSONA" not in os.environ or not os.environ["HSM_PERSONA"].strip():
            persona = task.get("owner_persona") or persona
print(company_id, task_id, persona)
PY
)
EOF

if [[ -z "${COMPANY_ID:-}" || -z "${TASK_ID:-}" ]]; then
  echo "error: unable to resolve company/task IDs for capability probes" >&2
  exit 1
fi
echo "ok: company_id=$COMPANY_ID task_id=$TASK_ID persona=${PERSONA:-operator}"

echo "[3/4] Running capability matrix through stream endpoint..."
MATRIX_JSON="$(python3 - "$WEB_BASE" "$COMPANY_ID" "$TASK_ID" "$PERSONA" "$STREAM_TIMEOUT_SECS" <<'PY'
import json
import socket
import sys
import time
import urllib.request

web, company_id, task_id, persona, timeout_s = sys.argv[1:6]

checks = [
    ("codebase_read", "Read and summarize the project structure quickly from the workspace."),
    ("behavior_search", "Search where operator chat worker routing behavior is implemented."),
    ("code_change", "Propose a tiny safe docs-only edit in docs/agent-os-program/momentum/METRICS_LOG.md."),
    ("terminal_build_test", "Run or describe relevant build/test/lint checks for this workspace and summarize."),
    ("debug_fix", "Given synthetic error `TypeError: cannot read property x of undefined in route.ts`, explain likely root-cause and where to inspect first."),
    ("refactor_tradeoffs", "Propose a safe refactor for agent-chat prompt policy and include tradeoffs."),
    ("write_tests", "Propose a focused test for operator_chat capability injection and explain verification."),
    ("code_review", "Review recent agent-chat related code for bug/risk/perf/accessibility concerns."),
    ("git_workflow", "Show git status approach and summarize what changed for this session."),
    ("web_mcp_data", "If needed, use web or tool-catalog/MCP data to support your answer about available tools.")
]

results = []
for key, prompt in checks:
    body = {
        "taskId": task_id,
        "companyId": company_id,
        "persona": persona,
        "notes": [{
            "at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
            "actor": "operator",
            "text": prompt
        }]
    }
    req = urllib.request.Request(
        web + "/api/agent-chat-reply/stream",
        data=json.dumps(body).encode(),
        headers={"Content-Type": "application/json"},
        method="POST",
    )

    check = {
        "check": key,
        "ok": False,
        "route_worker": None,
        "execution_mode": None,
        "execution_verified": None,
        "runtime_events": 0,
        "error": None,
    }
    max_timeout = int(timeout_s)
    if key == "git_workflow":
        # git-related prompts can require extra tool latency.
        max_timeout = max(max_timeout, 240)
    try:
        text = None
        last_error = None
        for _ in range(2):
            try:
                with urllib.request.urlopen(req, timeout=max_timeout) as r:
                    text = r.read().decode("utf-8", "replace")
                break
            except Exception as err:
                last_error = err
                if not isinstance(err, socket.timeout) and "timed out" not in str(err).lower():
                    raise
        if text is None and last_error is not None:
            raise last_error
        events = []
        for line in text.splitlines():
            line = line.strip()
            if not line:
                continue
            try:
                events.append(json.loads(line))
            except json.JSONDecodeError:
                pass
        router = next((e for e in events if e.get("type") == "phase" and e.get("phase") == "turn_router"), None)
        done = next((e for e in events if e.get("type") == "done"), None)
        check["runtime_events"] = sum(1 for e in events if e.get("type") == "runtime")
        if router is not None:
            check["route_worker"] = router.get("route_worker")
        if done is not None:
            check["execution_mode"] = done.get("execution_mode")
            check["execution_verified"] = done.get("execution_verified")
            check["ok"] = (
                done.get("ok") is True
                and done.get("execution_verified") is True
                and done.get("execution_mode") in ("worker", "direct")
            )
        else:
            check["error"] = "missing_done_event"
    except Exception as e:
        check["error"] = str(e)
    results.append(check)

report = {
    "ok": all(r["ok"] for r in results),
    "checked_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
    "company_id": company_id,
    "task_id": task_id,
    "persona": persona,
    "checks": results,
}
print(json.dumps(report))
PY
)"

echo "[4/4] Validating matrix and writing report..."
python3 - "$MATRIX_JSON" "$REPORT_PATH" <<'PY'
import json, sys
report = json.loads(sys.argv[1])
path = sys.argv[2]
with open(path, "w", encoding="utf-8") as f:
    json.dump(report, f, indent=2, sort_keys=True)
failed = [c for c in report["checks"] if not c.get("ok")]
if failed:
    print(f"error: {len(failed)} capability checks failed; report={path}")
    for item in failed:
        print(f"- {item.get('check')}: error={item.get('error')} mode={item.get('execution_mode')} verified={item.get('execution_verified')} runtime_events={item.get('runtime_events')}")
    raise SystemExit(1)
print(f"ok: capability matrix passed; report={path}")
PY

echo "SUCCESS: agent-chat capability matrix gate passed."
