#!/bin/bash
# HSM-II — Hyper-Stigmergic Morphogenesis II
DIR="$(cd "$(dirname "$0")" && pwd)"
exec /opt/homebrew/bin/wezterm start --cwd "$DIR" -- bash -c '
DIR="'"$DIR"'"

BOLD="\033[1m"
DIM="\033[2m"
CYAN="\033[38;5;51m"
VIOLET="\033[38;5;135m"
GOLD="\033[38;5;220m"
RESET="\033[0m"

export HSM_EMBED_MODEL="nomic-embed-text"

clear

echo ""
echo -e "${VIOLET}${BOLD}"
cat "$DIR/banner.txt"
echo -e "${RESET}"
echo ""

echo -e "${CYAN}  ⟳  building...${RESET}"
echo ""

# Ensure RooDB is running (default 127.0.0.1:3307)
ROODB_HOST="127.0.0.1"
ROODB_PORT="3307"
ROODB_VENDOR_DIR="/Users/cno/personal-ai-node/vendor/roodb"
ROODB_CERT_DIR="${ROODB_VENDOR_DIR}/certs"
ROODB_DATA_DIR="${ROODB_VENDOR_DIR}/data"
ROODB_BIN="${ROODB_VENDOR_DIR}/target/release/roodb"
ROODB_INIT_BIN="${ROODB_VENDOR_DIR}/target/release/roodb_init"
ROODB_ROOT_PASSWORD="${ROODB_ROOT_PASSWORD:-hsm-secret}"
ROODB_STARTED=0

cleanup_roodb() {
    if [ "$ROODB_STARTED" -eq 1 ] && [ -n "${ROODB_PID:-}" ]; then
        kill "${ROODB_PID}" >/dev/null 2>&1 || true
    fi
}
trap cleanup_roodb EXIT

build_roodb() {
    if [ ! -x "${ROODB_BIN}" ] || [ ! -x "${ROODB_INIT_BIN}" ]; then
        echo -e "${DIM}  ↳ building RooDB release (this might take a moment)...${RESET}"
        (
            cd "${ROODB_VENDOR_DIR}"
            cargo build --release >/dev/null
        )
    fi
}

generate_rts_cert() {
    mkdir -p "${ROODB_CERT_DIR}"
    if [ ! -f "${ROODB_CERT_DIR}/server.crt" ] || [ ! -f "${ROODB_CERT_DIR}/server.key" ]; then
        openssl req -x509 -newkey rsa:4096 \
            -keyout "${ROODB_CERT_DIR}/server.key" \
            -out "${ROODB_CERT_DIR}/server.crt" \
            -days 365 -nodes \
            -subj "/CN=localhost" >/dev/null 2>&1
    fi
}

init_roodb_data() {
    mkdir -p "${ROODB_DATA_DIR}"
    if [ ! -f "${ROODB_DATA_DIR}/.initialized" ]; then
        echo -e "${DIM}  ↳ running roodb_init${RESET}"
        ROODB_ROOT_PASSWORD="${ROODB_ROOT_PASSWORD}" \
            "${ROODB_INIT_BIN}" --data-dir "${ROODB_DATA_DIR}" >/dev/null
        touch "${ROODB_DATA_DIR}/.initialized"
    fi
}

start_vendor_roodb() {
    build_roodb
    generate_rts_cert
    init_roodb_data
    echo -e "${GOLD}  ⚠ RooDB not detected on ${ROODB_HOST}:${ROODB_PORT} — launching embedded instance...${RESET}"
    "${ROODB_BIN}" \
        --port "${ROODB_PORT}" \
        --data-dir "${ROODB_DATA_DIR}" \
        --cert-path "${ROODB_CERT_DIR}/server.crt" \
        --key-path "${ROODB_CERT_DIR}/server.key" &
    ROODB_PID=$!
    ROODB_STARTED=1
}

if ! lsof -Pi :${ROODB_PORT} -sTCP:LISTEN -t >/dev/null 2>&1; then
    if [ -n "${ROODB_START_CMD:-}" ]; then
        echo -e "${GOLD}  ⚠ RooDB not detected on ${ROODB_HOST}:${ROODB_PORT} — using ROODB_START_CMD...${RESET}"
        eval "${ROODB_START_CMD}" &
    else
        start_vendor_roodb
    fi
    # wait up to 5s for RooDB
    for i in $(seq 1 5); do
        if lsof -Pi :${ROODB_PORT} -sTCP:LISTEN -t >/dev/null 2>&1; then
            echo -e "${CYAN}  ✓ RooDB is up on ${ROODB_HOST}:${ROODB_PORT}${RESET}"
            break
        fi
        sleep 1
    done
fi

cargo build --release 2>&1
if [ $? -ne 0 ]; then
    echo ""
    echo -e "${BOLD}\033[38;5;196m  ✗  build failed${RESET}"
    read -p "  Press Enter to close..."
    exit 1
fi

echo ""
echo -e "${CYAN}  ✓  build successful — starting monolith backend${RESET}"
echo -e "${DIM}  ─────────────────────────────────────────────────────────────────────────${RESET}"
echo ""

export HSM_API_HOST=127.0.0.1
export HSM_API_PORT=9000
echo -e "${CYAN}  ✓  API target: http://127.0.0.1:9000${RESET}"
echo -e "${DIM}  Start UI proxy with: open-hypergraphd.command${RESET}"
echo -e "${DIM}  Start personal agent with: run-personal-agent.command${RESET}"
echo ""

cargo run --release -- --headless --roodb 127.0.0.1:3307

echo ""
echo -e "${DIM}  session ended.${RESET}"
read -p "  Press Enter to close..."
'
