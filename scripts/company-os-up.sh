#!/usr/bin/env bash
# One command: Company OS Postgres (Docker) when needed, hsm_console, company-console (Next dev).
#
# Usage (from repo root):
#   bash scripts/company-os-up.sh
#
# Env (optional):
#   HSM_COMPANY_OS_DATABASE_URL — if already set (non-empty), Docker Postgres is NOT started.
#   HSM_CONSOLE_PORT              — default 3847 (hsm_console)
#   HSM_COMPANY_CONSOLE_PORT      — default 3050 (next dev)
#   HSM_COMPANY_OS_SKIP_DOCKER=1  — never start Docker; you must set HSM_COMPANY_OS_DATABASE_URL
#   HSM_COMPANY_OS_RELEASE=1      — run `cargo build --release` then target/release/hsm_console
#
# Honest limits: worker tools run as the same OS user as hsm_console (no default sandbox).
# Stop: Ctrl+C (stops Next + hsm_console). Postgres container is left running (data kept).
#       To stop DB:  bash scripts/company_os_postgres.sh down
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

if [[ -f "$ROOT/.env" ]]; then
  set -a
  # shellcheck disable=SC1091
  . "$ROOT/.env"
  set +a
fi

HSM_CONSOLE_PORT="${HSM_CONSOLE_PORT:-3847}"
HSM_COMPANY_CONSOLE_PORT="${HSM_COMPANY_CONSOLE_PORT:-3050}"
COMPOSE="$ROOT/compose/company-os-postgres.yml"
DOCKER_DB_URL="postgres://hsm:hsm@127.0.0.1:55432/hsm_company_os"
CONSOLE_ORIGIN="http://127.0.0.1:${HSM_CONSOLE_PORT}"
HEALTH_URL="${CONSOLE_ORIGIN}/api/company/health"

usage() {
  sed -n '1,25p' "$0" | tail -n +2
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

ensure_docker_postgres() {
  if ! command -v docker &>/dev/null; then
    echo "error: Docker not found and HSM_COMPANY_OS_DATABASE_URL is unset." >&2
    echo "  Install Docker Desktop, or set HSM_COMPANY_OS_DATABASE_URL and use HSM_COMPANY_OS_SKIP_DOCKER=1," >&2
    echo "  or run:  bash scripts/company_os_postgres_local.sh  (Homebrew Postgres)" >&2
    exit 1
  fi
  docker compose -f "$COMPOSE" up -d
  echo "Waiting for Postgres (healthcheck)…"
  local i
  for i in $(seq 1 90); do
    if docker compose -f "$COMPOSE" exec -T company-os-postgres pg_isready -U hsm -d hsm_company_os &>/dev/null; then
      return 0
    fi
    sleep 1
  done
  echo "error: Postgres did not become ready in time." >&2
  exit 1
}

if [[ -n "${HSM_COMPANY_OS_DATABASE_URL:-}" ]]; then
  echo "Using existing HSM_COMPANY_OS_DATABASE_URL (Docker Postgres will not be started by this script)."
else
  if [[ "${HSM_COMPANY_OS_SKIP_DOCKER:-0}" == "1" ]]; then
    echo "error: HSM_COMPANY_OS_DATABASE_URL is empty and HSM_COMPANY_OS_SKIP_DOCKER=1." >&2
    exit 1
  fi
  ensure_docker_postgres
  export HSM_COMPANY_OS_DATABASE_URL="$DOCKER_DB_URL"
  echo "Exported HSM_COMPANY_OS_DATABASE_URL for this session (add to repo .env to persist)."
fi

if command -v curl &>/dev/null && curl -sfS -m 1 "$HEALTH_URL" &>/dev/null; then
  echo "warning: ${HEALTH_URL} already responds — another hsm_console may be running. Continuing anyway." >&2
fi

export HSM_CONSOLE_URL="${HSM_CONSOLE_URL:-$CONSOLE_ORIGIN}"
export NEXT_PUBLIC_API_BASE="${NEXT_PUBLIC_API_BASE:-$CONSOLE_ORIGIN}"

CC="$ROOT/web/company-console"
if [[ ! -d "$CC" ]]; then
  echo "error: missing $CC" >&2
  exit 1
fi

if [[ ! -d "$CC/node_modules" ]]; then
  echo "Installing company-console npm dependencies…"
  (cd "$CC" && npm install)
fi

HSM_PID=""
stop_console() {
  if [[ -n "${HSM_PID:-}" ]] && kill -0 "$HSM_PID" 2>/dev/null; then
    kill "$HSM_PID" 2>/dev/null || true
    wait "$HSM_PID" 2>/dev/null || true
  fi
}
trap stop_console EXIT

if [[ "${HSM_COMPANY_OS_RELEASE:-0}" == "1" ]]; then
  echo "Building hsm_console (release)…"
  cargo build --release --bin hsm_console
  echo "Starting hsm_console (release) on ${CONSOLE_ORIGIN}…"
  "$ROOT/target/release/hsm_console" --port "$HSM_CONSOLE_PORT" &
  HSM_PID=$!
else
  echo "Building hsm_console (debug; first compile may take a while)…"
  cargo build --bin hsm_console
  echo "Starting hsm_console (debug) on ${CONSOLE_ORIGIN}…"
  "$ROOT/target/debug/hsm_console" --port "$HSM_CONSOLE_PORT" &
  HSM_PID=$!
fi

echo "Waiting for hsm_console health…"
for _ in $(seq 1 120); do
  if command -v curl &>/dev/null; then
    if curl -sfS -m 2 "$HEALTH_URL" &>/dev/null; then
      break
    fi
  elif command -v nc &>/dev/null; then
    if nc -z 127.0.0.1 "$HSM_CONSOLE_PORT" 2>/dev/null; then
      break
    fi
  else
    sleep 2
    break
  fi
  sleep 0.5
done

if command -v curl &>/dev/null && ! curl -sfS -m 2 "$HEALTH_URL" &>/dev/null; then
  echo "error: hsm_console did not respond at $HEALTH_URL (see logs above)." >&2
  exit 1
fi

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo " Company OS is up"
echo "   • API / graph:     ${CONSOLE_ORIGIN}/api/company/*"
echo "   • Health:          ${HEALTH_URL}"
echo "   • UI (Next dev):   http://127.0.0.1:${HSM_COMPANY_CONSOLE_PORT}"
echo "   • DB URL (session): ${HSM_COMPANY_OS_DATABASE_URL}"
echo " Ctrl+C stops Next + hsm_console. Postgres container keeps running."
echo "   Stop DB: bash scripts/company_os_postgres.sh down"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

cd "$CC"
# HSM_CONSOLE_URL / NEXT_PUBLIC_API_BASE already exported for this shell + child.
# Run Next in the background so this shell stays in the foreground process group and
# receives Ctrl+C; then we stop Next and hsm_console reliably.
bash scripts/run-next-with-path.sh dev -H 127.0.0.1 -p "$HSM_COMPANY_CONSOLE_PORT" &
NEXT_PID=$!
trap 'kill "$NEXT_PID" 2>/dev/null || true; wait "$NEXT_PID" 2>/dev/null || true; stop_console; exit 130' INT
trap 'kill "$NEXT_PID" 2>/dev/null || true; wait "$NEXT_PID" 2>/dev/null || true; stop_console; exit 143' TERM
wait "$NEXT_PID" || true
