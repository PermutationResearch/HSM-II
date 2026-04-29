#!/usr/bin/env bash
set -euo pipefail

BASE="${HSM_CONSOLE_URL:-http://127.0.0.1:3847}"
BASE="${BASE%/}"
WEB_BASE="${HSM_COMPANY_CONSOLE_URL:-http://127.0.0.1:3050}"
WEB_BASE="${WEB_BASE%/}"
WEB_WAIT_SECS="${HSM_E2E_WEB_WAIT_SECS:-30}"
API_WAIT_SECS="${HSM_E2E_API_WAIT_SECS:-30}"
STREAM_TIMEOUT_SECS="${HSM_E2E_STREAM_TIMEOUT_SECS:-180}"
REPLY_TIMEOUT_SECS="${HSM_E2E_REPLY_TIMEOUT_SECS:-120}"
REPLY_RETRIES="${HSM_E2E_REPLY_RETRIES:-2}"
REPLY_RETRY_DELAY_SECS="${HSM_E2E_REPLY_RETRY_DELAY_SECS:-2}"
REPORT_PATH="${HSM_E2E_REPORT_PATH:-/tmp/agent-chat-endpoint-check.json}"

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

echo "[1/6] Verifying Company Console base URL..."
if ! wait_for_url "$WEB_BASE" "$WEB_WAIT_SECS"; then
  echo "error: Company Console is not live at $WEB_BASE" >&2
  echo "hint: start it with npm --prefix web/company-console run dev" >&2
  exit 1
fi
echo "ok: $WEB_BASE is reachable"

echo "[2/6] Verifying Company OS API health..."
if ! wait_for_url "$BASE/api/company/health" "$API_WAIT_SECS"; then
  echo "error: Company OS API is not healthy at $BASE/api/company/health" >&2
  echo "hint: start it with bash scripts/company-os-up.sh" >&2
  exit 1
fi
echo "ok: $BASE/api/company/health is reachable"

echo "[3/6] Resolving company/task context..."
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
  echo "error: unable to resolve company/task IDs for agent-chat probes" >&2
  exit 1
fi
echo "ok: company_id=$COMPANY_ID task_id=$TASK_ID persona=${PERSONA:-operator}"

echo "[4/6] Calling /api/agent-chat-reply with assertions..."
REPLY_JSON="$(python3 - "$WEB_BASE" "$COMPANY_ID" "$TASK_ID" "$PERSONA" "$REPLY_TIMEOUT_SECS" "$REPLY_RETRIES" "$REPLY_RETRY_DELAY_SECS" <<'PY'
import json, socket, sys, time, urllib.request
from urllib.error import URLError

web, company_id, task_id, persona, timeout_s, retries_s, delay_s = sys.argv[1:8]
timeout_s = max(1, int(timeout_s))
retries = max(1, int(retries_s))
delay_s = max(0, int(delay_s))
body = {
    "taskId": task_id,
    "companyId": company_id,
    "persona": persona,
    "notes": [{
        "at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "actor": "operator",
        "text": "Say hello in one sentence."
    }]
}
req = urllib.request.Request(
    web + "/api/agent-chat-reply",
    data=json.dumps(body).encode(),
    headers={"Content-Type": "application/json"},
    method="POST",
)
last_err = None
for attempt in range(1, retries + 1):
    try:
        with urllib.request.urlopen(req, timeout=timeout_s) as r:
            print(r.read().decode("utf-8", "replace"))
        break
    except (socket.timeout, TimeoutError, URLError) as e:
        last_err = e
        if attempt >= retries:
            raise
        time.sleep(delay_s * attempt)
if last_err and retries <= 0:
    raise last_err
PY
)"

echo "[5/6] Calling /api/agent-chat-reply/stream with assertions..."
STREAM_OUT="$(python3 - "$WEB_BASE" "$COMPANY_ID" "$TASK_ID" "$PERSONA" "$STREAM_TIMEOUT_SECS" <<'PY'
import json, sys, time, urllib.request
web, company_id, task_id, persona, timeout_s = sys.argv[1:6]
body = {
    "taskId": task_id,
    "companyId": company_id,
    "persona": persona,
    "notes": [{
        "at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "actor": "operator",
        "text": "run pwd and then explain what directory you are in"
    }]
}
req = urllib.request.Request(
    web + "/api/agent-chat-reply/stream",
    data=json.dumps(body).encode(),
    headers={"Content-Type": "application/json"},
    method="POST",
)
with urllib.request.urlopen(req, timeout=int(timeout_s)) as r:
    print(r.read().decode("utf-8", "replace"))
PY
)"

echo "[6/6] Validating payloads and writing report..."
python3 - "$REPLY_JSON" "$STREAM_OUT" "$REPORT_PATH" "$BASE" "$WEB_BASE" "$COMPANY_ID" "$TASK_ID" "$PERSONA" <<'PY'
import json, sys, time
reply_raw, stream_raw, report_path, base, web, company_id, task_id, persona = sys.argv[1:9]
reply = json.loads(reply_raw)
events = []
for line in stream_raw.splitlines():
    line = line.strip()
    if not line:
        continue
    try:
        events.append(json.loads(line))
    except json.JSONDecodeError:
        pass
assert events, "stream produced no JSON events"
done = next((e for e in events if e.get("type") == "done"), None)
assert reply.get("ok") is True, "reply.ok must be true"
assert reply.get("execution_mode") in ("worker", "direct"), "reply.execution_mode missing/invalid"
assert reply.get("execution_verified") is True, "reply.execution_verified must be true"
assert isinstance(reply.get("run_id"), str) and reply.get("run_id"), "reply.run_id must be present"
assert done is not None, "missing done event"
assert done.get("ok") is True, "done.ok must be true"
assert done.get("execution_mode") in ("worker", "direct"), "done.execution_mode missing/invalid"
assert done.get("execution_verified") is True, "done.execution_verified must be true"
has_router = any(e.get("type") == "phase" and e.get("phase") == "turn_router" for e in events)
has_runtime = any(e.get("type") == "runtime" for e in events)
has_harness_meta = any(
    isinstance(e.get("harness_state"), str) or isinstance(e.get("interaction_kind"), str)
    for e in events
)
assert has_router, "missing turn_router phase event"
assert (has_runtime or has_harness_meta), "missing runtime/harness metadata events"
report = {
    "ok": True,
    "checked_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
    "base": base,
    "web_base": web,
    "company_id": company_id,
    "task_id": task_id,
    "persona": persona,
    "reply": {
        "run_id": reply.get("run_id"),
        "execution_mode": reply.get("execution_mode"),
        "execution_verified": reply.get("execution_verified"),
        "status": reply.get("status"),
        "worker_evidence": reply.get("worker_evidence"),
    },
    "stream": {
        "event_count": len(events),
        "has_turn_router": has_router,
        "has_runtime": has_runtime,
        "has_harness_meta": has_harness_meta,
        "done_execution_mode": done.get("execution_mode"),
        "done_execution_verified": done.get("execution_verified"),
        "done_ok": done.get("ok"),
    },
}
with open(report_path, "w", encoding="utf-8") as f:
    json.dump(report, f, indent=2, sort_keys=True)
print(f"ok: assertions passed; report={report_path}")
PY

echo "SUCCESS: agent-chat endpoint gate passed."
