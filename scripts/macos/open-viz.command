#!/bin/bash
# Open the Hyper-Stigmergy Studio UI in the browser.
# Ensures full stack: monolith API (:9000) + hypergraphd proxy (:8787).

set -euo pipefail

DIR="$(cd "$(dirname "$0")" && pwd)"
MONO_PORT=9000
PROXY_PORT=8787
PROXY_LOG="/tmp/hypergraphd.log"

echo "========================================="
echo "  HSM-II Studio Launcher"
echo "========================================="
echo ""

# Make sure hypergraphd knows how to reach RooDB created by scripts/macos/run-hyper-stigmergy-II.command.
export HSM_ROODB_URL="127.0.0.1:3307"
export HSM_ROODB="127.0.0.1:3307"

if ! lsof -Pi :"${MONO_PORT}" -sTCP:LISTEN -t >/dev/null 2>&1; then
    echo "❌ Monolith API is not running on :${MONO_PORT}."
    echo "   First run: run-hyper-stigmergy-II.command"
    echo ""
    read -p "Press Enter to close..."
    exit 1
fi
echo "✅ Monolith API detected on :${MONO_PORT}"

if ! lsof -Pi :"${PROXY_PORT}" -sTCP:LISTEN -t >/dev/null 2>&1; then
    echo "⏳ Starting hypergraphd on :${PROXY_PORT}..."
    (
      cd "$DIR"
      nohup cargo run --bin hypergraphd >"${PROXY_LOG}" 2>&1 &
    )
fi

echo "⏳ Waiting for proxy /api/health..."
for i in $(seq 1 20); do
    if curl -sf "http://127.0.0.1:${PROXY_PORT}/api/health" -o /dev/null 2>/dev/null; then
        echo "✅ Proxy is ready on :${PROXY_PORT}"
        break
    fi
    sleep 0.5
done

if ! curl -sf "http://127.0.0.1:${PROXY_PORT}/api/health" -o /dev/null 2>/dev/null; then
    echo "❌ hypergraphd failed to start."
    echo "   Check log: ${PROXY_LOG}"
    echo ""
    read -p "Press Enter to close..."
    exit 1
fi

echo ""
echo "🚀 Opening Studio UI with forced reload..."
open "http://localhost:${PROXY_PORT}?forceReload=true"
echo "✅ Done."
