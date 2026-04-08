#!/usr/bin/env bash
# Turbopack's Rust worker pool spawns `node` by basename. Preview/sandbox environments
# often run with a minimal PATH where `node` is not found. Export a sane PATH *before*
# exec'ing Next so worker children inherit resolvable `node` (e.g. under /usr/local/bin).
#
# For `next dev`, pass `-H 127.0.0.1` (see package.json): otherwise Next may call
# `os.networkInterfaces()` to print the LAN URL and some environments throw
# (uv_interface_addresses / ERR_SYSTEM_ERROR).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$ROOT"

_std_path="/usr/local/bin:/opt/homebrew/bin:/usr/bin:/bin"
export PATH="${_std_path}:${PATH:-}"

NEXT_CLI="$ROOT/node_modules/next/dist/bin/next"
if [[ ! -f "$NEXT_CLI" ]]; then
  echo "run-next-with-path.sh: missing $NEXT_CLI — run npm install in web/company-console" >&2
  exit 1
fi

exec node "$NEXT_CLI" "$@"
