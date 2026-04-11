#!/usr/bin/env bash
# Agent OS program — M1 smoke: prove local control plane responds (no secrets).
# Usage: from repo root: bash scripts/agent-os-milestone1-smoke.sh
# Optional: HSM_CONSOLE_URL=http://127.0.0.1:3847 (default)

set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

BASE="${HSM_CONSOLE_URL:-http://127.0.0.1:3847}"
echo "[agent-os M1 smoke] HSM_CONSOLE_URL base: ${BASE}"

code="$(curl -sS -o /tmp/hsm_health.json -w "%{http_code}" "${BASE}/api/company/health" || true)"
if [[ "$code" != "200" ]]; then
  echo "[agent-os M1 smoke] FAIL: GET ${BASE}/api/company/health -> HTTP ${code}" >&2
  echo "  Start hsm_console or set HSM_CONSOLE_URL." >&2
  exit 1
fi

echo "[agent-os M1 smoke] OK: health HTTP ${code}"
if command -v jq >/dev/null 2>&1; then
  jq -c . /tmp/hsm_health.json 2>/dev/null || cat /tmp/hsm_health.json
else
  head -c 400 /tmp/hsm_health.json; echo
fi

echo "[agent-os M1 smoke] Next: complete docs/agent-os-program/verification/MILESTONE_1_CHECKLIST.md with a real company_id + task path."
