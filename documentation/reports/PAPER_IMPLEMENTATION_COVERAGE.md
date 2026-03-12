# HSM-II Paper Implementation Coverage

This maps major claims in `paper.tex` to concrete code paths in the current implementation.

## Core Architecture
- World model / hypergraph / agents / drives: `src/hyper_stigmergy.rs`, `src/hypergraph.rs`, `src/agent.rs`
- Main runtime orchestration + UI/API loop: `src/main.rs`
- Memory system + fusion primitives: `src/memory.rs`, `src/lcm/*`
- Federation layer: `src/federation/*`, runtime sync path in `src/main.rs`

## Council (Debate / Orchestrate / Simple / LLM)
- Council module types and factory: `src/council/mod.rs`
- Debate mode: `src/council/debate.rs`
- Orchestrate mode: `src/council/orchestrate.rs`
- Simple mode: `src/council/simple.rs`
- LLM deliberation mode: `src/council/llm_deliberation.rs`
- Automatic mode switcher: `src/council/mode_switcher.rs`
- Runtime wiring (web + TUI): `src/main.rs`

## DKS and CASS
- DKS system + stigmergic entity layer: `src/dks/*`
- CASS semantic skills/context: `src/cass/*`
- Runtime snapshots and API exposure: `src/main.rs` (`/api/components/dks`, `/api/components/cass`)

## OptimizeAnything
- Optimization engine + evaluators: `src/optimize_anything/*`
- REST + WS orchestration: `src/main.rs` (`/api/optimize`)
- UI panel: `viz/index.html` (Optimize tab)

## Federation
- Federation client/server/meta-graph implementation: `src/federation/*`
- Real sync execution in command path: `src/main.rs` (`do_federation_sync`, `/federation sync`)

## Investigation Agent System
- Tools and registry (`web_search`, `fetch_url`, `subtask`, session tooling): `src/investigation_tools.rs`
- Engine/session/delegation flow: `src/investigation_engine.rs`

## Web Interface and Streaming
- HTTP API + websocket streaming: `src/main.rs` (`web_api_server`)
- Chat streaming with thinking-token split (`<think>` parsing): `src/main.rs` (`ThinkingStreamParser`)
- Real-time graph activity feed: `src/main.rs` + `viz/index.html`
- Studio UI tabs and telemetry cards: `viz/index.html`

## Recent Alignment Fixes (completed)
- Council mode now supports `auto` + explicit `llm` in runtime/UI.
- Auto mode now uses `ModeSwitcher` instead of socratic alias fallback behavior.
- Auto-mode now emits per-mode score breakdown + confidence to the Council stream/UI.
- LLM deliberation mode wired into runtime flow (`do_council` -> `council_run_llm_deliberation`).
- Web chat path writes through LCM-aware message path (not legacy-only).
- Grounded context sections now derive from live component state, not synthetic placeholders.
- Telemetry fields that were misleading fixed to explicit unknown/null semantics (`n/a` in UI),
  with LLM cache-hit now computed from live LCM cache-read vs regular token stats.
- Council timeout behavior tightened to avoid prolonged stuck states.

## Known Limits (remaining)
- Some subsystem metrics are capability-gated or currently not instrumented deeply (e.g., true GPU load %, true LLM cache hit tracking), and are intentionally shown as unknown instead of fake values.
- Batch experiment codepaths still expose broader analysis than interactive runtime; this is by design and not a regression.
