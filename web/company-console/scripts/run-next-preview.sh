#!/usr/bin/env bash
export PATH="/usr/local/bin:/opt/homebrew/bin:/usr/bin:/bin:${PATH:-}"

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$ROOT"

# Turbopack spawns child "node" processes by basename lookup.
# In sandboxed environments the inherited PATH may be stripped,
# so ensure a node symlink exists inside node_modules/.bin
# which Turbopack always resolves.
NODE_BIN="$(command -v node 2>/dev/null)"
if [ -n "$NODE_BIN" ] && [ ! -e "node_modules/.bin/node" ]; then
  ln -sf "$NODE_BIN" node_modules/.bin/node 2>/dev/null || true
fi

exec node node_modules/next/dist/bin/next "$@"
