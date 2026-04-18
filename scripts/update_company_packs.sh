#!/usr/bin/env bash
# Thin wrapper — delegates to update_company_packs.py
exec python3 "$(dirname "$0")/update_company_packs.py" "$@"
