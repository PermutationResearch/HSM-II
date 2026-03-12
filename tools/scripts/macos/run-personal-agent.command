#!/bin/bash
# HSM-II Personal Agent — Your AI Companion
# Grounded, Hermes-like interface to HSM-II's advanced coordination

DIR="$(cd "$(dirname "$0")" && pwd)"
exec /opt/homebrew/bin/wezterm start --cwd "$DIR" -- bash -c '
DIR="'"$DIR"'"

BOLD="\033[1m"
DIM="\033[2m"
CYAN="\033[38;5;51m"
VIOLET="\033[38;5;135m"
GOLD="\033[38;5;220m"
GREEN="\033[38;5;82m"
RESET="\033[0m"

clear

echo ""
echo -e "${VIOLET}${BOLD}"
cat "$DIR/banner.txt"
echo -e "${RESET}"
echo ""

# Check if initialized
if [ ! -f "$HOME/.hsmii/SOUL.md" ]; then
    echo -e "${GOLD}  ✨ Welcome to HSM-II Personal Agent${RESET}"
    echo ""
    echo -e "${CYAN}  This appears to be your first time.${RESET}"
    echo -e "${CYAN}  Let's set up your AI companion...${RESET}"
    echo ""
    
    cargo run --release --bin personal_agent -- bootstrap
    
    echo ""
    echo -e "${GREEN}  ✓ Setup complete!${RESET}"
    echo ""
fi

echo -e "${CYAN}  🚀 Starting Personal Agent...${RESET}"
echo ""

# Check for API keys
if [ -n "${DISCORD_TOKEN:-}" ]; then
    echo -e "${GREEN}  ✓ Discord integration enabled${RESET}"
fi
if [ -n "${TELEGRAM_TOKEN:-}" ]; then
    echo -e "${GREEN}  ✓ Telegram integration enabled${RESET}"
fi

echo ""
echo -e "${DIM}  Commands:${RESET}"
echo -e "${DIM}    Type your message to chat${RESET}"
echo -e "${DIM}    'exit' to quit${RESET}"
echo -e "${DIM}    'help' for more commands${RESET}"
echo ""
echo -e "${DIM}  ─────────────────────────────────────────────────────────────────────────${RESET}"
echo ""

cargo run --release --bin personal_agent -- start

echo ""
echo -e "${DIM}  session ended.${RESET}"
read -p "  Press Enter to close..."
'
