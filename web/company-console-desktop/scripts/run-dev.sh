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
DESKTOP="$REPO_ROOT/web/company-console-desktop"
CC="$REPO_ROOT/web/company-console"
if [[ ! -d "$CC" ]]; then
  echo "error: missing $CC" >&2
  exit 1
fi
if [[ ! -f "$CC/.next/standalone/server.js" ]]; then
  echo "Building company-console (first time or after clean)…"
  (cd "$CC" && npm install && npm run build)
fi
cd "$DESKTOP"
npm install
npm run dev
