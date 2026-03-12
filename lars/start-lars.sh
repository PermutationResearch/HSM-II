#!/usr/bin/env bash
# =============================================================================
# lars/start-lars.sh — Start LARS for Hyper-Stigmergic Morphogenesis II
# =============================================================================
#
# Starts:
#   1. LARS SQL server (PostgreSQL wire protocol, port 15432)
#   2. LARS Studio web UI (port 5050)
#   3. Optionally: DB sync watch loop
#
# Usage:
#   ./lars/start-lars.sh              # SQL server + Studio
#   ./lars/start-lars.sh --no-studio  # SQL server only
#   ./lars/start-lars.sh --sync       # Also start background DB sync
#   ./lars/start-lars.sh --ssql "SELECT ..." # Run one semantic query and exit
#
# Connect from any SQL client:
#   psql postgresql://admin:admin@localhost:15432/default
#   DBeaver: PostgreSQL driver → host=localhost port=15432 db=default user=admin
#
# =============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
DUCKDB_PATH="$SCRIPT_DIR/hyper_stigmergy.duckdb"

NO_STUDIO=false
SYNC=false
SSQL=""

for arg in "$@"; do
  case $arg in
    --no-studio)   NO_STUDIO=true ;;
    --sync)        SYNC=true ;;
    --ssql=*)      SSQL="${arg#*=}" ;;
    --ssql)        shift; SSQL="$1" ;;
  esac
done

# ── One-shot semantic SQL query ──────────────────────────────────────────────
if [ -n "$SSQL" ]; then
  echo "Running semantic query..."
  lars ssql "$SSQL"
  exit 0
fi

# ── Check DuckDB bridge exists ───────────────────────────────────────────────
if [ ! -f "$DUCKDB_PATH" ]; then
  echo "⚠  DuckDB bridge not found at $DUCKDB_PATH"
  echo "   Run /exportdb in the TUI first, or run: ./lars/sync-db.sh"
  echo ""
fi

echo "=============================================="
echo "  LARS — Hyper-Stigmergic Morphogenesis II"
echo "=============================================="
echo "  SQL server: postgresql://admin:admin@localhost:15432/default"
echo "  Studio UI:  http://localhost:5050"
echo "  Bridge DB:  $DUCKDB_PATH"
echo "=============================================="
echo ""

# ── Optional background sync ─────────────────────────────────────────────────
if $SYNC; then
  echo "Starting background DB sync (every 30s)..."
  "$SCRIPT_DIR/sync-db.sh" --watch &
  SYNC_PID=$!
  trap "kill $SYNC_PID 2>/dev/null || true" EXIT
fi

# ── Start Studio in background ───────────────────────────────────────────────
if ! $NO_STUDIO; then
  echo "Starting LARS Studio at http://localhost:5050 ..."
  lars serve studio &
  STUDIO_PID=$!
  trap "kill ${STUDIO_PID:-} ${SYNC_PID:-} 2>/dev/null || true" EXIT
  sleep 1
  open "http://localhost:5050" 2>/dev/null || true
fi

# ── Start SQL server (foreground) ────────────────────────────────────────────
echo "Starting LARS SQL server on :15432 ..."
echo "Press Ctrl+C to stop all services."
echo ""
lars serve sql --port 15432
