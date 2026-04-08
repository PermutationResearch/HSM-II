#!/usr/bin/env bash
# Company OS Postgres WITHOUT Docker — uses a local server (e.g. Homebrew postgresql@16).
#
# Prereq:
#   brew install postgresql@16
#   brew services start postgresql@16
#   # Ensure `psql` is on PATH, e.g.:
#   #   echo 'export PATH="/opt/homebrew/opt/postgresql@16/bin:$PATH"' >> ~/.zshrc && source ~/.zshrc
#
# Then from repo root:
#   ./scripts/company_os_postgres_local.sh
#
# Add to repo-root `.env` and restart hsm_console:
#   HSM_COMPANY_OS_DATABASE_URL=postgres://hsm:hsm@127.0.0.1:5432/hsm_company_os
#
set -euo pipefail

if ! command -v psql &>/dev/null; then
  echo "psql not found. Install and start Postgres, e.g.:" >&2
  echo "  brew install postgresql@16" >&2
  echo "  brew services start postgresql@16" >&2
  echo "  export PATH=\"/opt/homebrew/opt/postgresql@16/bin:\$PATH\"   # Apple Silicon" >&2
  echo "  export PATH=\"/usr/local/opt/postgresql@16/bin:\$PATH\"      # Intel Mac" >&2
  exit 1
fi

DB_PORT="${PGPORT:-5432}"
export PGPORT="$DB_PORT"

if ! psql -d postgres -c "SELECT 1" &>/dev/null; then
  echo "Cannot connect to Postgres on port $DB_PORT as your OS user (database 'postgres')." >&2
  echo "Start the service: brew services start postgresql@16" >&2
  exit 1
fi

psql -d postgres -v ON_ERROR_STOP=1 <<'SQL'
DO $$
BEGIN
  IF NOT EXISTS (SELECT FROM pg_catalog.pg_roles WHERE rolname = 'hsm') THEN
    CREATE ROLE hsm WITH LOGIN PASSWORD 'hsm';
  ELSE
    ALTER ROLE hsm WITH LOGIN PASSWORD 'hsm';
  END IF;
END
$$;
SQL

if ! psql -d postgres -tAc "SELECT 1 FROM pg_database WHERE datname = 'hsm_company_os'" | grep -q 1; then
  psql -d postgres -v ON_ERROR_STOP=1 -c "CREATE DATABASE hsm_company_os OWNER hsm;"
fi

URL="postgres://hsm:hsm@127.0.0.1:${DB_PORT}/hsm_company_os"
echo ""
echo "Company OS database is ready."
echo "Add to repo-root .env (or export), then restart hsm_console:"
echo ""
echo "  HSM_COMPANY_OS_DATABASE_URL=$URL"
echo ""
