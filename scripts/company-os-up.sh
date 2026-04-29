#!/usr/bin/env bash
# One command: Company OS Postgres (Docker) when needed, hsm_console, company-console (Next dev),
# and optionally the TypeScript claude-harness + executor services.
#
# Usage (from repo root):
#   bash scripts/company-os-up.sh
#
# Env (optional):
#   HSM_COMPANY_OS_DATABASE_URL — if already set (non-empty), Docker Postgres is NOT started.
#   HSM_CONSOLE_PORT              — default 3847 (hsm_console)
#   HSM_COMPANY_CONSOLE_PORT      — default 3050 (next dev)
#   HSM_COMPANY_OS_SKIP_DOCKER=1  — skip Docker Postgres and prefer local Postgres bootstrap
#   HSM_COMPANY_OS_SKIP_CONSOLE=1 — do not start hsm_console (Next only); API must already listen on HSM_CONSOLE_PORT
#   HSM_OPERATOR_CHAT_WORKER_FIRST=1 — force worker-first routing (optional; default is semantic/chat-first)
#   HSM_COMPANY_OS_RELEASE=1      — run `cargo build --release` then target/release/hsm_console
#   HSM_SKILL_EXTERNAL_DIRS       — optional extra SKILL.md trees; this script prepends <repo>/skills when absent
#   HSM_AGENT_CHAT_PROVIDER       — default `openrouter`; set `ollama` to use local Ollama chat
#   HSM_OPENROUTER_FREE_MODEL       — when `HSM_EXECUTION_BACKEND=openrouter`, default for `DEFAULT_LLM_MODEL` /
#                                   `HSM_AGENT_CHAT_MODEL` / uniform `HSM_MODEL_ROUTING_JSON` if those are unset
#   HSM_MODEL_ROUTING_JSON / DEFAULT_LLM_MODEL / HSM_AGENT_CHAT_MODEL — optional overrides (not overwritten when set)
#   HSM_SRT / HSM_TOOL_SANDBOX    — default on (`HSM_SRT=1`, `HSM_TOOL_SANDBOX=srt`) for host-tool sandboxing
#   HSM_DOCKER_BASH               — default `0` for Company OS up (avoid docker-wrapped bash in worker runs)
#                                 To turn off: set `HSM_SRT=0` and `HSM_TOOL_SANDBOX` to something other than `srt`
#                                 (e.g. `docker`); an empty value is treated as unset and defaults to `srt`.
#   OLLAMA_URL                    — default http://127.0.0.1:11434
#   OLLAMA_MODEL / HSM_AGENT_CHAT_MODEL — local model tag to use for company-console agent chat
#
# ── Claude harness (native claude -p mode) ────────────────────────────────────
#   HSM_CLAUDE_HARNESS=1          — start the TypeScript claude-harness service (port 3848)
#                                   execute-worker will route tasks to claude -p --output-format stream-json
#   HSM_CLAUDE_HARNESS_PORT       — default 3848 (claude-harness listen port)
#   HSM_CLAUDE_MAX_TURNS          — default 30 (max agent turns for harness runs)
#
# ── Executor service (pi-executor code-execution model) ───────────────────────
#   HSM_EXECUTOR=1                — start the executor service alongside the harness
#                                   Provides execute(code)/resume(id) tools to the agent.
#                                   Requires HSM_CLAUDE_HARNESS=1.
#   HSM_EXECUTOR_PORT             — default 3849
#   HSM_EXECUTOR_CWD              — working directory for executor sandbox (default: repo root)
#   HSM_ELICITATION_CALLBACK_URL  — set automatically to harness elicit-notify endpoint
#
# Host bash/argv tools default to Anthropic `srt` when `HSM_SRT=1` (see repo `src/harness/srt_sandbox.rs`).
# Stop: Ctrl+C (stops Next + hsm_console). Postgres container is left running (data kept).
#       To stop DB:  bash scripts/company_os_postgres.sh down
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

if [[ -f "$ROOT/.env" ]]; then
  set -a
  # shellcheck disable=SC1091
  . "$ROOT/.env"
  set +a
fi

# Repo-local Agent Skills: <repo>/skills/**/SKILL.md (e.g. predict-rlm) for hsm_console + import-paperclip-home.
# Prepend once so `skills_list` / `skill_md_read` see them even when HSMII_HOME is ~/.hsmii.
if [[ -d "$ROOT/skills" ]]; then
  _repo_skills="$(cd "$ROOT/skills" && pwd)"
  _skill_dirs="${HSM_SKILL_EXTERNAL_DIRS:-}"
  if [[ -z "$_skill_dirs" ]]; then
    export HSM_SKILL_EXTERNAL_DIRS="$_repo_skills"
  elif [[ ",${_skill_dirs}," != *",${_repo_skills},"* ]]; then
    export HSM_SKILL_EXTERNAL_DIRS="${_repo_skills},${_skill_dirs}"
  fi
  unset _repo_skills _skill_dirs
fi

HSM_CONSOLE_PORT="${HSM_CONSOLE_PORT:-3847}"
HSM_COMPANY_CONSOLE_PORT="${HSM_COMPANY_CONSOLE_PORT:-3050}"
COMPOSE="$ROOT/compose/company-os-postgres.yml"
DOCKER_DB_URL="postgres://hsm:hsm@127.0.0.1:55432/hsm_company_os"
CONSOLE_ORIGIN="http://127.0.0.1:${HSM_CONSOLE_PORT}"
HEALTH_URL="${CONSOLE_ORIGIN}/api/company/health"
export HSM_AGENT_CHAT_PROVIDER="${HSM_AGENT_CHAT_PROVIDER:-openrouter}"
export HSM_OPERATOR_CHAT_WORKER_FIRST="${HSM_OPERATOR_CHAT_WORKER_FIRST:-0}"
export HSM_SRT="${HSM_SRT:-1}"
export HSM_TOOL_SANDBOX="${HSM_TOOL_SANDBOX:-srt}"
# Execution backend:
#   openrouter (default): native execute-worker loop using OpenRouter/free models
#   claude: TypeScript claude-harness + executor + local Claude CLI path
export HSM_EXECUTION_BACKEND="${HSM_EXECUTION_BACKEND:-openrouter}"
if [[ "${HSM_EXECUTION_BACKEND}" == "claude" ]]; then
  export HSM_OPERATOR_CLAUDE_CODE_MODE=1
  export HSM_CLAUDE_HARNESS=1
  export HSM_CLAUDE_HARNESS_REQUIRED=1
  export HSM_EXECUTOR=1
  export HSM_CLAUDE_CLI_REQUIRE_LOCAL=1
else
  # Force OpenRouter execution lane; ignore stale shell exports from prior Claude sessions.
  _free_model="${HSM_OPENROUTER_FREE_MODEL:-qwen/qwen3-32b:free}"
  export HSM_OPERATOR_CLAUDE_CODE_MODE=0
  export HSM_CLAUDE_HARNESS=0
  export HSM_CLAUDE_HARNESS_REQUIRED=0
  export HSM_EXECUTOR=0
  export HSM_CLAUDE_CLI_REQUIRE_LOCAL=0
  export HSM_CLAUDE_HARNESS_URL=""
  export HSM_LLM_PROVIDER_ORDER="openrouter"
  # Respect explicit model env (e.g. CLI or .env) when set; otherwise default all bands to $_free_model.
  export DEFAULT_LLM_MODEL="${DEFAULT_LLM_MODEL:-$_free_model}"
  export HSM_AGENT_CHAT_MODEL="${HSM_AGENT_CHAT_MODEL:-$_free_model}"
  if [[ -z "${HSM_MODEL_ROUTING_JSON:-}" ]]; then
    export HSM_MODEL_ROUTING_JSON="{\"low\":\"$_free_model\",\"medium\":\"$_free_model\",\"high\":\"$_free_model\"}"
  fi
  export HSM_HERMES_MAX_TURNS="${HSM_HERMES_MAX_TURNS:-8}"
  unset _free_model
fi
if [[ -z "${HSM_CLAUDE_CLI_PATH:-}" ]]; then
  _claude_local="$ROOT/external/claude-code-from-npm/package/cli.js"
  if [[ -f "$_claude_local" ]]; then
    export HSM_CLAUDE_CLI_PATH="$_claude_local"
  fi
  unset _claude_local
fi
# Company OS operator turns expect Claude Code-style local shell tools; avoid docker-run wrapper by default.
export HSM_DOCKER_BASH="${HSM_DOCKER_BASH:-0}"
export OLLAMA_URL="${OLLAMA_URL:-http://127.0.0.1:11434}"
OLLAMA_BASE="${OLLAMA_URL%/}"
OLLAMA_HEALTH_URL="${OLLAMA_BASE}/api/tags"
OLLAMA_PID=""
OLLAMA_STARTED_BY_SCRIPT=0

usage() {
  sed -n '1,25p' "$0" | tail -n +2
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

ensure_docker_postgres() {
  if ! command -v docker &>/dev/null; then
    echo "error: Docker not found and no reachable Company OS Postgres endpoint is available." >&2
    echo "  Install Docker Desktop, or set HSM_COMPANY_OS_DATABASE_URL to a reachable DB and use HSM_COMPANY_OS_SKIP_DOCKER=1," >&2
    echo "  or run:  bash scripts/company_os_postgres_local.sh  (Homebrew Postgres)" >&2
    exit 1
  fi
  docker compose -f "$COMPOSE" up -d
  echo "Waiting for Postgres (healthcheck)…"
  local i
  for i in $(seq 1 90); do
    if docker compose -f "$COMPOSE" exec -T company-os-postgres pg_isready -U hsm -d hsm_company_os &>/dev/null; then
      return 0
    fi
    sleep 1
  done
  echo "error: Postgres did not become ready in time." >&2
  exit 1
}

try_local_postgres_bootstrap() {
  if ! command -v psql &>/dev/null; then
    return 1
  fi
  if ! "$ROOT/scripts/company_os_postgres_local.sh" >/tmp/hsm-company-os-local-postgres.log 2>&1; then
    return 1
  fi
  local local_db_port
  local_db_port="${PGPORT:-5432}"
  export HSM_COMPANY_OS_DATABASE_URL="postgres://hsm:hsm@127.0.0.1:${local_db_port}/hsm_company_os"
  echo "Using local Postgres bootstrap at ${HSM_COMPANY_OS_DATABASE_URL}"
  return 0
}

ensure_company_os_database_url() {
  # Prefer explicit URL when reachable.
  if [[ -n "${HSM_COMPANY_OS_DATABASE_URL:-}" ]]; then
    _db_url="${HSM_COMPANY_OS_DATABASE_URL}"
    _db_probe="unknown"
    if postgres_endpoint_reachable "$_db_url"; then
      _db_probe="reachable"
    else
      case "$?" in
        1) _db_probe="unreachable" ;;
        *) _db_probe="unknown" ;;
      esac
    fi

    if [[ "$_db_probe" == "reachable" ]]; then
      echo "Using existing HSM_COMPANY_OS_DATABASE_URL (Postgres endpoint reachable)."
      return 0
    fi
    if [[ "$_db_probe" == "unknown" ]]; then
      echo "Using existing HSM_COMPANY_OS_DATABASE_URL (endpoint preflight unavailable)."
      return 0
    fi
    echo "warning: existing HSM_COMPANY_OS_DATABASE_URL appears unreachable: ${_db_url}" >&2
  fi

  # Next prefer Docker unless explicitly skipped.
  if [[ "${HSM_COMPANY_OS_SKIP_DOCKER:-0}" != "1" ]]; then
    if command -v docker &>/dev/null; then
      ensure_docker_postgres
      export HSM_COMPANY_OS_DATABASE_URL="$DOCKER_DB_URL"
      echo "Exported HSM_COMPANY_OS_DATABASE_URL for this session: $DOCKER_DB_URL"
      return 0
    fi
    echo "warning: Docker not found; trying local Postgres bootstrap." >&2
  fi

  # Last fallback: local (Homebrew) Postgres bootstrap.
  if try_local_postgres_bootstrap; then
    return 0
  fi

  echo "error: no reachable Company OS Postgres endpoint is available." >&2
  echo "  Tried: existing HSM_COMPANY_OS_DATABASE_URL, Docker Postgres, local Postgres bootstrap." >&2
  echo "  Fix by doing one of:" >&2
  echo "    1) Start Docker Desktop and rerun." >&2
  echo "    2) Start local Postgres (brew services start postgresql@16) and rerun." >&2
  echo "    3) Set HSM_COMPANY_OS_DATABASE_URL to a reachable remote Postgres URL." >&2
  exit 1
}

postgres_url_host_port() {
  local url="$1"
  local authority host port
  authority="${url#*://}"
  authority="${authority#*@}"
  authority="${authority%%/*}"
  authority="${authority%%\?*}"
  if [[ -z "$authority" ]]; then
    return 1
  fi

  if [[ "$authority" == \[*\] ]]; then
    host="${authority#\[}"
    host="${host%\]}"
    port="5432"
  elif [[ "$authority" == \[*\]:* ]]; then
    host="${authority#\[}"
    host="${host%%\]*}"
    port="${authority##*:}"
  elif [[ "$authority" == *:* ]]; then
    host="${authority%:*}"
    port="${authority##*:}"
  else
    host="$authority"
    port="5432"
  fi

  [[ -n "$host" && -n "$port" ]] || return 1
  printf '%s %s\n' "$host" "$port"
}

postgres_endpoint_reachable() {
  local url="$1"
  local parsed host port
  parsed="$(postgres_url_host_port "$url" 2>/dev/null || true)"
  [[ -n "$parsed" ]] || return 2
  host="${parsed% *}"
  port="${parsed##* }"
  if command -v nc &>/dev/null; then
    nc -z -w 1 "$host" "$port" >/dev/null 2>&1
    return $?
  fi
  # Without `nc` we cannot probe reliably; do not block startup.
  return 2
}

ollama_model_exists() {
  local target="$1"
  [[ -n "$target" ]] || return 1
  ollama list 2>/dev/null | awk 'NR>1 {print $1}' | grep -Fx -- "$target" >/dev/null 2>&1
}

detect_first_ollama_model() {
  ollama list 2>/dev/null | awk 'NR>1 && $1 != "" {print $1; exit}'
}

ensure_ollama_chat_backend() {
  if [[ "${HSM_AGENT_CHAT_PROVIDER}" != "ollama" ]]; then
    return 0
  fi
  if ! command -v ollama &>/dev/null; then
    echo "error: HSM_AGENT_CHAT_PROVIDER=ollama but the \`ollama\` CLI is not installed." >&2
    exit 1
  fi
  if ! command -v curl &>/dev/null || ! curl -sfS -m 2 "$OLLAMA_HEALTH_URL" >/dev/null; then
    echo "Starting ollama serve on ${OLLAMA_URL}…"
    ollama serve >/tmp/hsm-company-os-ollama.log 2>&1 &
    OLLAMA_PID=$!
    OLLAMA_STARTED_BY_SCRIPT=1
    local i
    for i in $(seq 1 40); do
      if curl -sfS -m 2 "$OLLAMA_HEALTH_URL" >/dev/null; then
        break
      fi
      sleep 0.5
    done
  fi
  if ! curl -sfS -m 2 "$OLLAMA_HEALTH_URL" >/dev/null; then
    echo "error: Ollama did not respond at ${OLLAMA_HEALTH_URL}." >&2
    echo "  See /tmp/hsm-company-os-ollama.log if this script started it." >&2
    exit 1
  fi

  local resolved_model=""
  if [[ -n "${HSM_AGENT_CHAT_MODEL:-}" ]]; then
    resolved_model="${HSM_AGENT_CHAT_MODEL}"
  elif [[ -n "${OLLAMA_MODEL:-}" ]]; then
    resolved_model="${OLLAMA_MODEL}"
  else
    resolved_model="$(detect_first_ollama_model)"
  fi

  if [[ -z "$resolved_model" ]]; then
    echo "error: Ollama is running but no local models are installed." >&2
    echo "  Pull one first, for example:  ollama pull llama3.2" >&2
    exit 1
  fi
  if ! ollama_model_exists "$resolved_model"; then
    echo "error: Ollama model '$resolved_model' is not installed locally." >&2
    echo "  Install it with:  ollama pull $resolved_model" >&2
    exit 1
  fi

  export OLLAMA_MODEL="${OLLAMA_MODEL:-$resolved_model}"
  export HSM_AGENT_CHAT_MODEL="${HSM_AGENT_CHAT_MODEL:-$resolved_model}"
}

# True when Rust `srt_sandbox_enabled()` would be on: HSM_SRT truthy OR HSM_TOOL_SANDBOX=srt.
srt_sandbox_wanted() {
  local srt_raw="${HSM_SRT:-0}"
  local srt_lc
  srt_lc="$(printf '%s' "$srt_raw" | tr '[:upper:]' '[:lower:]')"
  case "$srt_lc" in
    1|true|yes|on) return 0 ;;
  esac
  local tool_lc
  tool_lc="$(printf '%s' "${HSM_TOOL_SANDBOX:-}" | tr '[:upper:]' '[:lower:]')"
  [[ "$tool_lc" == "srt" ]] && return 0
  return 1
}

start_claude_harness() {
  local harness_dir="$ROOT/services/claude-harness"
  if [[ "${HSM_CLAUDE_CLI_REQUIRE_LOCAL:-0}" == "1" ]]; then
    if [[ -z "${HSM_CLAUDE_CLI_PATH:-}" ]] || [[ ! -f "${HSM_CLAUDE_CLI_PATH}" ]]; then
      echo "error: HSM_CLAUDE_CLI_REQUIRE_LOCAL=1 but local Claude CLI path is unavailable." >&2
      echo "  Expected: $ROOT/external/claude-code-from-npm/package/cli.js" >&2
      echo "  Set HSM_CLAUDE_CLI_PATH explicitly if you moved it." >&2
      exit 1
    fi
  fi
  if [[ ! -d "$harness_dir/node_modules" ]]; then
    echo "Installing claude-harness npm dependencies…"
    (cd "$harness_dir" && npm install --silent)
  fi
  local harness_port="${HSM_CLAUDE_HARNESS_PORT:-3848}"
  echo "Starting claude-harness on http://127.0.0.1:${harness_port}…"
  # Run from inside the harness dir so tsx resolves from its own node_modules.
  (cd "$harness_dir" && HSM_CLAUDE_HARNESS_PORT="$harness_port" \
    node_modules/.bin/tsx src/index.ts) \
    >/tmp/hsm-claude-harness.log 2>&1 &
  HARNESS_PID=$!
  # Wait for harness health
  local i
  for i in $(seq 1 20); do
    if command -v curl &>/dev/null && curl -sfS -m 1 "http://127.0.0.1:${harness_port}/health" &>/dev/null; then
      export HSM_CLAUDE_HARNESS_URL="http://127.0.0.1:${harness_port}"
      echo "claude-harness ready at ${HSM_CLAUDE_HARNESS_URL}"
      return 0
    fi
    sleep 0.5
  done
  echo "warning: claude-harness did not respond on port ${harness_port}." >&2
  export HSM_CLAUDE_HARNESS_URL=""
  if [[ "${HSM_CLAUDE_HARNESS_REQUIRED:-0}" == "1" ]]; then
    echo "error: HSM_CLAUDE_HARNESS_REQUIRED=1 and claude-harness is not healthy." >&2
    echo "  Check: /tmp/hsm-claude-harness.log" >&2
    tail -n 80 /tmp/hsm-claude-harness.log >&2 || true
    exit 1
  fi
  return 0  # non-fatal
}

start_executor() {
  local executor_dir="$ROOT/services/executor"
  if [[ ! -d "$executor_dir/node_modules" ]]; then
    echo "Installing executor npm dependencies…"
    (cd "$executor_dir" && npm install --silent)
  fi
  local executor_port="${HSM_EXECUTOR_PORT:-3849}"
  local executor_cwd="${HSM_EXECUTOR_CWD:-$ROOT}"
  local harness_port="${HSM_CLAUDE_HARNESS_PORT:-3848}"
  echo "Starting executor on http://127.0.0.1:${executor_port}…"
  (cd "$executor_dir" && \
    HSM_EXECUTOR_PORT="$executor_port" \
    HSM_EXECUTOR_CWD="$executor_cwd" \
    HSM_ELICITATION_CALLBACK_URL="http://127.0.0.1:${harness_port}/elicit/notify" \
    node_modules/.bin/tsx src/index.ts) \
    >/tmp/hsm-executor.log 2>&1 &
  EXECUTOR_PID=$!
  # Wait for executor health
  local i
  for i in $(seq 1 20); do
    if command -v curl &>/dev/null && curl -sfS -m 1 "http://127.0.0.1:${executor_port}/health" &>/dev/null; then
      export HSM_EXECUTOR_URL="http://127.0.0.1:${executor_port}"
      echo "executor ready at ${HSM_EXECUTOR_URL}"
      return 0
    fi
    sleep 0.5
  done
  echo "warning: executor did not respond on port ${executor_port} — executor mode unavailable." >&2
  export HSM_EXECUTOR_URL=""
  EXECUTOR_PID=""
  return 0  # non-fatal
}

ensure_srt_runtime() {
  if ! srt_sandbox_wanted; then
    return 0
  fi
  local srt_bin="${HSM_SRT_BIN:-srt}"
  if ! command -v "$srt_bin" &>/dev/null; then
    echo "error: host tool sandbox is on (HSM_SRT / HSM_TOOL_SANDBOX) but '$srt_bin' is not on PATH." >&2
    echo "  Install with: npm install -g @anthropic-ai/sandbox-runtime" >&2
    echo "  Or disable: HSM_SRT=0 and HSM_TOOL_SANDBOX=docker (or any value other than srt)." >&2
    exit 1
  fi
  # Use absolute path so worker subprocesses can always spawn `srt` even with trimmed PATH.
  export HSM_SRT_BIN="$(command -v "$srt_bin")"
  if ! command -v rg &>/dev/null; then
    echo "warning: ripgrep (\`rg\`) not on PATH — upstream \`srt\` may fail its dependency check." >&2
    echo "  Install: brew install ripgrep   (or add rg to PATH)" >&2
  fi
}

ensure_company_os_database_url

# True if something is already accepting connections on the console port (avoids bind(48) + false-positive health).
console_api_reachable() {
  if command -v curl &>/dev/null && curl -sfS -m 1 "$HEALTH_URL" &>/dev/null; then
    return 0
  fi
  if command -v nc &>/dev/null && nc -z 127.0.0.1 "$HSM_CONSOLE_PORT" 2>/dev/null; then
    return 0
  fi
  return 1
}

export HSM_CONSOLE_URL="${HSM_CONSOLE_URL:-$CONSOLE_ORIGIN}"
export NEXT_PUBLIC_API_BASE="${NEXT_PUBLIC_API_BASE:-$CONSOLE_ORIGIN}"
ensure_srt_runtime
ensure_ollama_chat_backend

if [[ "${HSM_EXECUTION_BACKEND}" != "claude" ]]; then
  if [[ -z "${OPENROUTER_API_KEY:-}" && -z "${HSM_OPENROUTER_API_KEY:-}" ]]; then
    echo "warning: HSM_EXECUTION_BACKEND=openrouter but OPENROUTER_API_KEY is not set." >&2
    echo "  execute-worker may fail until you export OPENROUTER_API_KEY." >&2
  fi
fi

# Start claude-harness when HSM_CLAUDE_HARNESS=1
if [[ "${HSM_CLAUDE_HARNESS:-0}" == "1" ]]; then
  start_claude_harness
  # Start executor alongside the harness when HSM_EXECUTOR=1
  if [[ "${HSM_EXECUTOR:-0}" == "1" ]]; then
    start_executor
  fi
fi

if [[ "${HSM_COMPANY_OS_SKIP_CONSOLE:-0}" == "1" ]]; then
  if ! console_api_reachable; then
    echo "error: HSM_COMPANY_OS_SKIP_CONSOLE=1 but nothing responds at ${HEALTH_URL} (port ${HSM_CONSOLE_PORT})." >&2
    echo "  Start hsm_console first, or fix HSM_CONSOLE_PORT / HSM_CONSOLE_URL." >&2
    exit 1
  fi
  echo "HSM_COMPANY_OS_SKIP_CONSOLE=1 — skipping hsm_console start (using existing API on port ${HSM_CONSOLE_PORT})."
elif console_api_reachable; then
  echo "error: Company OS API already reachable at ${HEALTH_URL} — port ${HSM_CONSOLE_PORT} is in use." >&2
  echo "  Stop the existing process, e.g.:  lsof -nP -iTCP:${HSM_CONSOLE_PORT} -sTCP:LISTEN" >&2
  echo "  Or run Next only against that API:  HSM_COMPANY_OS_SKIP_CONSOLE=1 bash scripts/company-os-up.sh" >&2
  echo "  Or pick another port:  HSM_CONSOLE_PORT=3848 bash scripts/company-os-up.sh" >&2
  exit 1
fi

CC="$ROOT/web/company-console"
if [[ ! -d "$CC" ]]; then
  echo "error: missing $CC" >&2
  exit 1
fi

if [[ ! -d "$CC/node_modules" ]]; then
  echo "Installing company-console npm dependencies…"
  (cd "$CC" && npm install)
fi

HSM_PID=""
HARNESS_PID=""
EXECUTOR_PID=""
HSM_CONSOLE_FALLBACK_MODE="none"
stop_console() {
  if [[ -n "${HSM_PID:-}" ]] && kill -0 "$HSM_PID" 2>/dev/null; then
    kill "$HSM_PID" 2>/dev/null || true
    wait "$HSM_PID" 2>/dev/null || true
  fi
  if [[ -n "${HARNESS_PID:-}" ]] && kill -0 "$HARNESS_PID" 2>/dev/null; then
    kill "$HARNESS_PID" 2>/dev/null || true
    wait "$HARNESS_PID" 2>/dev/null || true
  fi
  if [[ -n "${EXECUTOR_PID:-}" ]] && kill -0 "$EXECUTOR_PID" 2>/dev/null; then
    kill "$EXECUTOR_PID" 2>/dev/null || true
    wait "$EXECUTOR_PID" 2>/dev/null || true
  fi
  if [[ "${OLLAMA_STARTED_BY_SCRIPT:-0}" == "1" ]] && [[ -n "${OLLAMA_PID:-}" ]] && kill -0 "$OLLAMA_PID" 2>/dev/null; then
    kill "$OLLAMA_PID" 2>/dev/null || true
    wait "$OLLAMA_PID" 2>/dev/null || true
  fi
}

trap stop_console EXIT

start_hsm_console() {
  local sandbox_mode="$1" # "default" | "fallback_nosrt"
  if [[ "${HSM_COMPANY_OS_RELEASE:-0}" == "1" ]]; then
    cargo build --release --bin hsm_console
    if [[ "$sandbox_mode" == "fallback_nosrt" ]]; then
      echo "Starting hsm_console (release) on ${CONSOLE_ORIGIN} with fallback sandbox mode (HSM_SRT=0)…"
      env HSM_SRT=0 HSM_TOOL_SANDBOX=docker "$ROOT/target/release/hsm_console" --port "$HSM_CONSOLE_PORT" &
    else
      echo "Starting hsm_console (release) on ${CONSOLE_ORIGIN}…"
      "$ROOT/target/release/hsm_console" --port "$HSM_CONSOLE_PORT" &
    fi
  else
    cargo build --bin hsm_console
    if [[ "$sandbox_mode" == "fallback_nosrt" ]]; then
      echo "Starting hsm_console (debug) on ${CONSOLE_ORIGIN} with fallback sandbox mode (HSM_SRT=0)…"
      env HSM_SRT=0 HSM_TOOL_SANDBOX=docker "$ROOT/target/debug/hsm_console" --port "$HSM_CONSOLE_PORT" &
    else
      echo "Starting hsm_console (debug) on ${CONSOLE_ORIGIN}…"
      "$ROOT/target/debug/hsm_console" --port "$HSM_CONSOLE_PORT" &
    fi
  fi
  HSM_PID=$!
}

wait_console_health() {
  sleep 0.6
  if [[ -n "$HSM_PID" ]] && ! kill -0 "$HSM_PID" 2>/dev/null; then
    wait "$HSM_PID" 2>/dev/null || true
    echo "error: hsm_console exited immediately (port ${HSM_CONSOLE_PORT} likely already in use, or sandbox/network policy blocked startup)." >&2
    echo "  See:  lsof -nP -iTCP:${HSM_CONSOLE_PORT} -sTCP:LISTEN" >&2
    return 1
  fi
  echo "Waiting for hsm_console health…"
  for _ in $(seq 1 120); do
    if command -v curl &>/dev/null; then
      if curl -sfS -m 2 "$HEALTH_URL" &>/dev/null; then
        break
      fi
    elif command -v nc &>/dev/null; then
      if nc -z 127.0.0.1 "$HSM_CONSOLE_PORT" 2>/dev/null; then
        break
      fi
    else
      sleep 2
      break
    fi
    sleep 0.5
  done

  if command -v curl &>/dev/null && ! curl -sfS -m 2 "$HEALTH_URL" &>/dev/null; then
    return 1
  fi
  return 0
}

if [[ "${HSM_COMPANY_OS_SKIP_CONSOLE:-0}" != "1" ]]; then
  if [[ "${HSM_COMPANY_OS_RELEASE:-0}" == "1" ]]; then
    echo "Building hsm_console (release)…"
  else
    echo "Building hsm_console (debug; first compile may take a while)…"
  fi
  start_hsm_console "default"
  if ! wait_console_health; then
    retry_with_fallback=0
    if srt_sandbox_wanted && [[ "${HSM_COMPANY_OS_DISABLE_SANDBOX_RETRY:-0}" != "1" ]]; then
      retry_with_fallback=1
    fi
    if [[ "$retry_with_fallback" == "1" ]]; then
      echo "warning: hsm_console failed under sandboxed settings; retrying once with fallback-nested-sandbox mode (HSM_SRT=0)." >&2
      if [[ -n "${HSM_PID:-}" ]] && kill -0 "$HSM_PID" 2>/dev/null; then
        kill "$HSM_PID" 2>/dev/null || true
        wait "$HSM_PID" 2>/dev/null || true
      fi
      start_hsm_console "fallback_nosrt"
      if ! wait_console_health; then
        echo "error: hsm_console did not respond at $HEALTH_URL (see logs above)." >&2
        exit 1
      fi
      HSM_CONSOLE_FALLBACK_MODE="fallback-nested-sandbox"
    else
      echo "error: hsm_console did not respond at $HEALTH_URL (see logs above)." >&2
      exit 1
    fi
  fi
fi

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo " Company OS is up"
echo "   • API / graph:     ${CONSOLE_ORIGIN}/api/company/*"
echo "   • Health:          ${HEALTH_URL}"
echo "   • UI (Next dev):   http://127.0.0.1:${HSM_COMPANY_CONSOLE_PORT}"
echo "   • Chat backend:    ${HSM_AGENT_CHAT_PROVIDER}"
echo "   • Exec backend:    ${HSM_EXECUTION_BACKEND}"
echo "   • Tool sandbox:    HSM_SRT=${HSM_SRT} (${HSM_TOOL_SANDBOX})"
echo "   • Console launch:  ${HSM_CONSOLE_FALLBACK_MODE}"
if [[ -n "${HSM_CLAUDE_HARNESS_URL:-}" ]]; then
  echo "   • Claude harness:  ${HSM_CLAUDE_HARNESS_URL}  (execute-worker → claude -p stream-json)"
  if [[ -n "${HSM_CLAUDE_CLI_PATH:-}" ]]; then
    echo "   • Claude CLI:      ${HSM_CLAUDE_CLI_PATH}"
  fi
  if [[ -n "${HSM_EXECUTOR_URL:-}" ]]; then
    echo "   • Executor:        ${HSM_EXECUTOR_URL}  (code-execution model — execute/resume tools)"
  else
    echo "   • Executor:        off  (HSM_EXECUTOR=1 to enable code-execution model)"
  fi
else
  echo "   • Claude harness:  off  (native Rust agent)"
fi
if [[ "${HSM_AGENT_CHAT_PROVIDER}" == "ollama" ]]; then
  echo "   • Local model:     ${HSM_AGENT_CHAT_MODEL}"
  echo "   • Ollama:          ${OLLAMA_URL}"
fi
echo "   • DB URL (session): ${HSM_COMPANY_OS_DATABASE_URL}"
if [[ "${HSM_COMPANY_OS_SKIP_CONSOLE:-0}" == "1" ]]; then
  echo " Ctrl+C stops Next (hsm_console was not started by this script)."
else
  echo " Ctrl+C stops Next + hsm_console. Postgres container keeps running."
fi
echo "   Stop DB: bash scripts/company_os_postgres.sh down"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

cd "$CC"
# HSM_CONSOLE_URL / NEXT_PUBLIC_API_BASE already exported for this shell + child.
# Run Next in the background so this shell stays in the foreground process group and
# receives Ctrl+C; then we stop Next and hsm_console reliably.
bash scripts/run-next-with-path.sh dev -H 127.0.0.1 -p "$HSM_COMPANY_CONSOLE_PORT" &
NEXT_PID=$!
trap 'kill "$NEXT_PID" 2>/dev/null || true; wait "$NEXT_PID" 2>/dev/null || true; stop_console; exit 130' INT
trap 'kill "$NEXT_PID" 2>/dev/null || true; wait "$NEXT_PID" 2>/dev/null || true; stop_console; exit 143' TERM
wait "$NEXT_PID" || true
