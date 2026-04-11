# Implementation contract — HSM-II agent operating system (program)

## Mission

Grow this repository into a **durable, observable, self-improving** agentic stack for computer-based work: same codebase must support **immediate answers**, **bounded tasks**, and **long-horizon company/research operations**, with **verification**, **governance**, and **measured** learning — not a chat-only demo.

## Runtime profile

| Attribute | Choice |
|-----------|--------|
| Mode | **Native runtime mode** (Rust agent core + Postgres Company OS + Next console), with **harness-wrapper** patterns for external agents (Hermes, Codex, etc.) via tools/adapters. |
| Topology | **Hub-like** `hsm_console` + DB; workers/implementations via agent runs and harness; scale-out path documented, not assumed. |
| Canonical project state | **Postgres** for operational graph; **markdown pack** under `docs/agent-os-program/` for **program** continuity and human legibility. |
| Milestone focus | **Coding + company ops first** (already present); browser/desktop expansion **explicit** when harness + eval exist. |

## First milestone (M1) — definition of done

1. **Goal intake**: Represented as an existing **task** (or goal linked to tasks) in Company OS *or* documented manual step to create one via API/console.
2. **Task graph**: At least one task with **visible** state transitions in DB/UI.
3. **Execution**: At least one **agent run** or documented worker path that touches the task.
4. **Verification**: `verification/MILESTONE_1_CHECKLIST.md` completed for a smoke run (human or scripted checks).
5. **Memory / trace**: Run feedback or `context_notes` / `company_memory_entries` updated OR explicitly N/A with reason recorded in `FAILURE.md` / checklist.
6. **Visibility**: Entry visible in **company-console** (tasks or agent runs) without raw SQL.
7. **Learning**: At least **one** item under `momentum/QUEUES.md` → **improve** queue with a concrete follow-up (eval gap, wiring gap, or doc fix).

**Non-goals for M1:** New multi-agent orchestration, full Temporal-style saga engine, greenfield second control plane.

## Constraints

- Do not fork “truth” outside Postgres for company-scoped work (see world-model doc).
- Respect `.env.example` and security tool gates.
- Prefer **one bounded change** per self-improvement PR.

## Safety posture

- Deny-first on destructive tools; approvals where product already has them.
- Secrets: never commit; use env / company credentials patterns already in console.

## Proof-of-progress metrics (initial set)

Track in `momentum/METRICS_LOG.md` (append-only) as events occur:

- Tasks completed / verified (manual counts OK until automated).
- Agent runs terminal success vs error.
- Smoke script pass/fail.
- Intervention count (human approvals, restarts).

## Verification strategy

- **Automated where cheap:** `scripts/agent-os-milestone1-smoke.sh` (health, optional API).
- **Manual where authoritative:** checklist + console inspection.
- **Eval slice:** `cargo run --bin hsm-eval -- --suite memory --limit 2` when LLM path configured (optional gate, not blocking M1 doc completion).
