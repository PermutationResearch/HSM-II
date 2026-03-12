#!/usr/bin/env python3.12
"""
export-to-duckdb.py — Export RooDB (MySQL/TLS) → LARS DuckDB bridge

Called by the Rust TUI via: /exportdb
Also runnable directly:     python3.12 lars/export-to-duckdb.py

Uses mysql-connector-python (supports TLS/SSL) to read from RooDB,
then writes a DuckDB file that LARS can query with semantic operators.
"""

import sys
import os

SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
PROJECT_DIR = os.path.dirname(SCRIPT_DIR)
DUCKDB_PATH = os.path.join(SCRIPT_DIR, "hyper_stigmergy.duckdb")
CERTS_DIR = os.path.join(PROJECT_DIR, "certs")

# RooDB connection settings (match Rust defaults)
ROODB_HOST = os.environ.get("ROODB_HOST", "127.0.0.1")
ROODB_PORT = int(os.environ.get("ROODB_PORT", "3307"))
ROODB_USER = os.environ.get("ROODB_USER", "root")
ROODB_PASS = os.environ.get("ROODB_PASS", "secret")
ROODB_DB   = os.environ.get("ROODB_DB",   "hyper_stigmergy")

TABLES = [
    "system_snapshots",
    "agents",
    "hyper_edges",
    "beliefs",
    "experiences",
    "improvement_events",
    "ontology",
]

def get_mysql_conn():
    """Connect to RooDB with TLS using mysql-connector-python."""
    try:
        import mysql.connector
    except ImportError:
        print("[INFO] Installing mysql-connector-python...")
        import subprocess
        subprocess.check_call([
            sys.executable, "-m", "pip", "install",
            "mysql-connector-python", "--break-system-packages", "-q"
        ])
        import mysql.connector

    ca_cert = os.path.join(CERTS_DIR, "ca.crt")
    ssl_args = {}
    if os.path.exists(ca_cert):
        ssl_args = {
            "ssl_ca": ca_cert,
            "ssl_verify_cert": False,   # self-signed cert
            "ssl_verify_identity": False,
        }

    conn = mysql.connector.connect(
        host=ROODB_HOST,
        port=ROODB_PORT,
        user=ROODB_USER,
        password=ROODB_PASS,
        database=ROODB_DB,
        **ssl_args,
    )
    return conn

def export():
    import duckdb

    print(f"Connecting to RooDB at {ROODB_HOST}:{ROODB_PORT}/{ROODB_DB} ...")
    try:
        mysql_conn = get_mysql_conn()
    except Exception as e:
        print(f"[ERROR] RooDB connection failed: {e}", file=sys.stderr)
        print("  Make sure RooDB is running and the TUI has saved at least once (/save)")
        sys.exit(1)

    cursor = mysql_conn.cursor(dictionary=True)

    print(f"Writing DuckDB bridge → {DUCKDB_PATH}")
    # Remove old file so we get a clean export
    if os.path.exists(DUCKDB_PATH):
        os.remove(DUCKDB_PATH)

    duck = duckdb.connect(DUCKDB_PATH)

    total = 0
    for table in TABLES:
        try:
            # For system_snapshots, skip the heavy BLOB column
            if table == "system_snapshots":
                cursor.execute(
                    "SELECT id, version, saved_at, tick_count FROM system_snapshots "
                    "ORDER BY id DESC LIMIT 50"
                )
            else:
                cursor.execute(f"SELECT * FROM `{table}`")

            rows = cursor.fetchall()
            if not rows:
                print(f"  [SKIP] {table}: empty")
                continue

            cols = list(rows[0].keys())
            col_defs = []
            for col in cols:
                # Infer types from first non-null value
                sample = next((r[col] for r in rows if r[col] is not None), None)
                if isinstance(sample, int):
                    col_defs.append(f'"{col}" BIGINT')
                elif isinstance(sample, float):
                    col_defs.append(f'"{col}" DOUBLE')
                elif isinstance(sample, (bytes, bytearray)):
                    col_defs.append(f'"{col}" BLOB')
                else:
                    col_defs.append(f'"{col}" VARCHAR')

            duck.execute(f"DROP TABLE IF EXISTS {table}")
            duck.execute(f"CREATE TABLE {table} ({', '.join(col_defs)})")

            # Insert rows in batches
            placeholders = ", ".join(["?" for _ in cols])
            insert_sql = f"INSERT INTO {table} VALUES ({placeholders})"
            batch = []
            for row in rows:
                vals = []
                for col in cols:
                    v = row[col]
                    if isinstance(v, (bytes, bytearray)):
                        v = bytes(v)
                    vals.append(v)
                batch.append(vals)
                if len(batch) >= 500:
                    duck.executemany(insert_sql, batch)
                    batch = []
            if batch:
                duck.executemany(insert_sql, batch)

            count = duck.execute(f"SELECT COUNT(*) FROM {table}").fetchone()[0]
            total += count
            print(f"  [OK] {table}: {count} rows")

        except Exception as e:
            print(f"  [WARN] {table}: {e}")

    duck.close()
    cursor.close()
    mysql_conn.close()

    print(f"Export complete: {total} rows → {DUCKDB_PATH}")
    print("LARS can now query hyper_stigmergy.* with semantic operators.")

if __name__ == "__main__":
    export()
