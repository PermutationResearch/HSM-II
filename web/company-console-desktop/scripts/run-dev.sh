#!/usr/bin/env bash
# Run from anywhere: finds repo root (parent of web/) and starts the desktop dev flow.
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
# scripts → company-console-desktop → web → repo root
REPO_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"
if [[ ! -f "$REPO_ROOT/Cargo.toml" ]]; then
  echo "error: could not find Cargo.toml — expected repo root at: $REPO_ROOT" >&2
  exit 1
fi
cd "$REPO_ROOT"
echo "Repo root: $REPO_ROOT"

# Export repo-root .env for child processes (Electron -> hsm_console/Next).
# This makes DB config available even if the spawned binary doesn't auto-load .env.
if [[ -f "$REPO_ROOT/.env" ]]; then
  set -a
  # shellcheck disable=SC1091
  . "$REPO_ROOT/.env"
  set +a
fi

if [[ -z "${HSM_COMPANY_OS_DATABASE_URL:-}" ]]; then
  echo "warning: HSM_COMPANY_OS_DATABASE_URL is empty; Company OS Postgres features will be disabled."
  echo "         set it in $REPO_ROOT/.env and rerun this script."
fi

DESKTOP="$REPO_ROOT/web/company-console-desktop"
CC="$REPO_ROOT/web/company-console"
if [[ ! -d "$CC" ]]; then
  echo "error: missing $CC" >&2
  exit 1
fi
# Build Next standalone on every launcher run by default so desktop always picks up
# latest app/api changes without requiring manual rebuild commands.
# Set HSM_DESKTOP_SKIP_CC_BUILD=1 to skip when iterating only on Electron shell.
if [[ "${HSM_DESKTOP_SKIP_CC_BUILD:-0}" != "1" ]]; then
  echo "Building company-console…"
  (cd "$CC" && npm install && npm run build)
elif [[ ! -f "$CC/.next/standalone/server.js" ]]; then
  echo "company-console standalone missing; building once…"
  (cd "$CC" && npm install && npm run build)
fi
cd "$DESKTOP"
npm install
npm run dev
