#!/bin/bash
# HSM-II — Hyper-Stigmergic Morphogenesis II  (TUI launcher)
DIR="$(cd "$(dirname "$0")" && pwd)"
exec /opt/homebrew/bin/wezterm start --cwd "$DIR" -- bash -c '
DIR="'"$DIR"'"

BOLD="\033[1m"
DIM="\033[2m"
CYAN="\033[38;5;51m"
VIOLET="\033[38;5;135m"
GOLD="\033[38;5;220m"
RESET="\033[0m"

clear

echo ""
echo -e "${VIOLET}${BOLD}"
cat "$DIR/banner.txt"
echo -e "${RESET}"
echo ""

echo -e "${CYAN}  ⟳  building...${RESET}"
echo ""
cargo build --release 2>&1
if [ $? -ne 0 ]; then
    echo ""
    echo -e "${BOLD}\033[38;5;196m  ✗  build failed${RESET}"
    read -p "  Press Enter to close..."
    exit 1
fi

echo ""
echo -e "${CYAN}  ✓  build successful — initialising system${RESET}"
echo -e "${DIM}  ─────────────────────────────────────────────────────────────────────────${RESET}"
echo ""

cargo run --release

echo ""
echo -e "${DIM}  session ended.${RESET}"
read -p "  Press Enter to close..."
'
