# Agent OS program — operating summary (re-read on long runs)

**Re-read this file** when context gets long; it replaces re-ingesting the full principal prompt.

## Default architecture (this repo)

| Layer | What exists today |
|-------|-------------------|
| **Control plane** | `hsm_console` (Rust) + Postgres **Company OS** per `company_id`: tasks, goals, `agent_runs`, memory, spend, approvals-shaped states. REST under `/api/company/…`. |
| **Human surface** | `web/company-console` (Next) proxies to Rust; workspace rail, intelligence, agent runs, approvals. |
| **Execution** | Agent runs + task checkout; harness/sandbox in `src/harness/`; tools in `src/tools/`. |
| **Eval / learning (offline)** | `hsm-eval`, `hsm_meta_harness`, `hsm_outer_loop` — **not auto-wired** into live `personal_agent` / `hsm_console` (see `docs/EVAL_AND_META_HARNESS.md`). |
| **Canonical truth** | Postgres graph per `docs/company-os/world-model-and-intelligence.md` — not chat transcripts. |

**Design bet:** Extend this **native runtime + file/docs contracts** rather than introducing a second parallel OS. Add **file-pack** artifacts under `docs/agent-os-program/` for cross-session continuity; mirror critical state in DB where product already does.

## First milestone (provable closed loop)

See `MILESTONES.md` §M1. One sentence: **goal → task → execution artifact → verifier checklist → memory or task note → visible in console/DB → one logged improvement item.**

## Key guardrails

- Transparent state: Postgres + `llm-context`; files for **program** direction (`docs/agent-os-program/`).
- No “done” without **evidence** (run row, diff, checklist, or human approval).
- **Single-agent / single-run baseline** before multi-agent theater.
- Eval promotion: never assume `best_config.json` affects production until explicitly integrated.
- Side effects: respect existing approval / `requires_human` / tool security layers.

## Runtime constraints (infer from tree)

- Rust + Node; Postgres for Company OS in real deployments.
- Local dev: `hsm_console` + Next on documented ports; `HSM_CONSOLE_URL` for proxy.
- Evals may need API keys / Ollama per `.env.example`.

## Next three milestones (headline only)

1. **M1** — Closed-loop smoke + checklist (this pack + script).
2. **M2** — Wire **one** observable metric from runs/tasks into a durable log (SQLite or JSONL) consumable by humans.
3. **M3** — Bridge **one** eval artifact type to a **company task** template (e.g. “eval replay” task spec + verifier field on task).
