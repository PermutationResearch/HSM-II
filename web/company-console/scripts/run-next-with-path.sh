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

# Monorepo root (…/web/company-console → repo root). Next's own `.env*` is not always enough;
# merge OpenRouter + optional `HSM_CONSOLE_URL` from repo + app dotenv (Next proxies `/api/company/*` there).
REPO_ROOT="$(cd "$ROOT/../.." && pwd)"
strip_env_val() {
  local s="${1//$'\r'/}"
  if [[ "${#s}" -ge 2 && "${s:0:1}" == '"' && "${s: -1}" == '"' ]]; then s="${s:1:${#s}-2}"; fi
  if [[ "${#s}" -ge 2 && "${s:0:1}" == "'" && "${s: -1}" == "'" ]]; then s="${s:1:${#s}-2}"; fi
  printf '%s' "$s"
}
load_openrouter_from_dotenv_files() {
  local f line v
  for f in "$REPO_ROOT/.env" "$REPO_ROOT/.env.local" "$ROOT/.env" "$ROOT/.env.local"; do
    [[ -f "$f" ]] || continue
    while IFS= read -r line || [[ -n "$line" ]]; do
      [[ "$line" =~ ^[[:space:]]*# ]] && continue
      [[ -z "${line//[:space:]}" ]] && continue
      if [[ "$line" =~ ^[[:space:]]*export[[:space:]]+(.*)$ ]]; then
        line="${BASH_REMATCH[1]}"
      fi
      if [[ "$line" =~ ^OPENROUTER_API_KEY=(.*)$ ]]; then
        v="$(strip_env_val "${BASH_REMATCH[1]}")"
        [[ -n "$v" ]] && export OPENROUTER_API_KEY="$v"
      elif [[ "$line" =~ ^HSM_OPENROUTER_API_KEY=(.*)$ ]]; then
        v="$(strip_env_val "${BASH_REMATCH[1]}")"
        [[ -n "$v" ]] && export OPENROUTER_API_KEY="$v"
      elif [[ "$line" =~ ^OPENROUTER_API_BASE=(.*)$ ]]; then
        v="$(strip_env_val "${BASH_REMATCH[1]}")"
        [[ -n "$v" ]] && export OPENROUTER_API_BASE="$v"
      elif [[ "$line" =~ ^HSM_CONSOLE_URL=(.*)$ ]]; then
        v="$(strip_env_val "${BASH_REMATCH[1]}")"
        [[ -n "$v" ]] && export HSM_CONSOLE_URL="$v"
      fi
    done <"$f"
  done
}
load_openrouter_from_dotenv_files

_std_path="/usr/local/bin:/opt/homebrew/bin:/usr/bin:/bin"
export PATH="${_std_path}:${PATH:-}"

NEXT_CLI="$ROOT/node_modules/next/dist/bin/next"
if [[ ! -f "$NEXT_CLI" ]]; then
  echo "run-next-with-path.sh: missing $NEXT_CLI — run npm install in web/company-console" >&2
  exit 1
fi

exec node "$NEXT_CLI" "$@"
