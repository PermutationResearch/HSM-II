#!/usr/bin/env bash
# Start/stop local Postgres for Company OS (matches .env.example URL on port 55432).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
COMPOSE="$ROOT/compose/company-os-postgres.yml"
URL="postgres://hsm:hsm@127.0.0.1:55432/hsm_company_os"

if ! command -v docker &>/dev/null; then
  echo "docker: command not found." >&2
  echo "" >&2
  echo "Either install Docker Desktop and re-run this script, or use a local Postgres (no Docker):" >&2
  echo "  brew install postgresql@16 && brew services start postgresql@16" >&2
  echo "  export PATH=\"/opt/homebrew/opt/postgresql@16/bin:\$PATH\"   # Apple Silicon" >&2
  echo "  $ROOT/scripts/company_os_postgres_local.sh" >&2
  exit 1
fi

cmd="${1:-up}"
case "$cmd" in
  up)
    docker compose -f "$COMPOSE" up -d
    echo "Waiting for Postgres (healthcheck)…"
    for i in $(seq 1 60); do
      if docker compose -f "$COMPOSE" exec -T company-os-postgres pg_isready -U hsm -d hsm_company_os &>/dev/null; then
        break
      fi
      sleep 1
    done
    echo ""
    echo "Postgres is up. Add this to repo-root .env (or export in your shell), then restart hsm_console:"
    echo ""
    echo "  HSM_COMPANY_OS_DATABASE_URL=$URL"
    echo ""
    ;;
  down)
    docker compose -f "$COMPOSE" down
    ;;
  logs)
    docker compose -f "$COMPOSE" logs -f
    ;;
  *)
    echo "usage: $0 up | down | logs" >&2
    exit 1
    ;;
esac
