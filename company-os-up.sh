#!/usr/bin/env bash
# Wrapper: run from repo root so you don't have to type scripts/.
exec "$(cd "$(dirname "$0")" && pwd)/scripts/company-os-up.sh" "$@"
