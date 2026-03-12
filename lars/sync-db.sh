#!/usr/bin/env bash
# =============================================================================
# lars/sync-db.sh — Sync RooDB → LARS DuckDB bridge
# =============================================================================
# Pulls the latest snapshot from RooDB (MySQL/TLS on :3307) into a local
# DuckDB file that LARS can query with semantic SQL operators.
#
# Usage:
#   ./lars/sync-db.sh              # one-shot sync
#   ./lars/sync-db.sh --watch      # sync every 30s (while TUI is running)
#
# The Rust TUI can also call this automatically via the /exportdb command.
# =============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
DUCKDB_PATH="$SCRIPT_DIR/hyper_stigmergy.duckdb"
WATCH_MODE=false
WATCH_INTERVAL=30

for arg in "$@"; do
  case $arg in
    --watch) WATCH_MODE=true ;;
    --interval=*) WATCH_INTERVAL="${arg#*=}" ;;
  esac
done

sync_once() {
  echo "[$(date '+%H:%M:%S')] Syncing RooDB → $DUCKDB_PATH ..."

  python3.12 - <<PYTHON
import sys
import duckdb

try:
    # Connect to DuckDB output file
    out = duckdb.connect("$DUCKDB_PATH")

    # Install + load MySQL scanner
    out.execute("INSTALL mysql;")
    out.execute("LOAD mysql;")

    # Attach RooDB — no SSL option in DuckDB mysql scanner;
    # RooDB must be running with --no-tls OR we use the Rust-exported path.
    # This script tries direct attach first, falls back to a message.
    try:
        out.execute("""
            ATTACH 'host=127.0.0.1 port=3307 database=hyper_stigmergy user=root password=secret'
            AS roodb (TYPE mysql);
        """)
        print("  [OK] Attached RooDB directly (no-TLS mode)")
        source = "roodb"
    except Exception as e:
        print(f"  [WARN] Direct MySQL attach failed (TLS required): {e}")
        print("  [INFO] Use /exportdb in the TUI to write the DuckDB file directly from Rust.")
        out.close()
        sys.exit(1)

    tables = [
        "agents", "hyper_edges", "beliefs", "experiences",
        "improvement_events", "ontology", "system_snapshots"
    ]

    for tbl in tables:
        try:
            out.execute(f"DROP TABLE IF EXISTS {tbl};")
            out.execute(f"CREATE TABLE {tbl} AS SELECT * FROM roodb.hyper_stigmergy.{tbl};")
            count = out.execute(f"SELECT COUNT(*) FROM {tbl}").fetchone()[0]
            print(f"  [OK] {tbl}: {count} rows")
        except Exception as e:
            print(f"  [WARN] {tbl}: {e}")

    out.execute("DETACH roodb;")
    out.close()
    print(f"  [DONE] Bridge file: {('$DUCKDB_PATH')}")

except Exception as e:
    print(f"  [ERROR] {e}", file=sys.stderr)
    sys.exit(1)
PYTHON
}

if $WATCH_MODE; then
  echo "Watch mode: syncing every ${WATCH_INTERVAL}s. Ctrl+C to stop."
  while true; do
    sync_once || true
    sleep "$WATCH_INTERVAL"
  done
else
  sync_once
fi
