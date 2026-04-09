#!/usr/bin/env bash
set -euo pipefail

# Portable single-line base64 encoder (macOS/Linux/Nix shells).
# Usage: scripts/portable_base64.sh /path/to/file

if [[ $# -ne 1 ]]; then
  echo "usage: $0 <file>" >&2
  exit 1
fi

FILE="$1"
if [[ ! -f "$FILE" ]]; then
  echo "file not found: $FILE" >&2
  exit 1
fi

base64 < "$FILE" | tr -d '\n'
