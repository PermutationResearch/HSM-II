# The HSM-II Operating Loop

```
Signal → Frame → Execute → Gate → Compound
                                      ↑
                          Repair ─────┘  (activates on failure at any phase)
```

Every unit of work — a task, a goal, an incoming message, a heartbeat trigger — travels
this path. The loop is not aspirational. Each phase has a hard entry condition and a
hard exit artifact.

---

## Signal

Work enters the system from any surface:

- Telegram message routed to `personal_agent`
- API task creation (`POST /api/company/companies/{id}/tasks`)
- Goal propagation from the Paperclip IntelligenceLayer
- Heartbeat trigger from `operations.yaml`
- External benchmark or harness event

**Entry condition:** any inbound signal  
**Exit artifact:** task record with `project_id`, `dri_agent_ref`, and `capability_refs` set  
**Blocks on:** missing DRI assignment — a task without an owner cannot enter Frame

---

## Frame

Before any implementation, the problem is challenged. Frame is not planning — it is
reframing. The Council asks: Is this the right problem? Are the constraints accurate?
Is there a simpler version? Could this be skipped entirely?

Council mode is selected by task complexity:

| Complexity signal | Council mode |
|---|---|
| Routine, well-scoped | `Simple` — single deliberation pass |
| Competing interpretations | `Debate` — two-perspective challenge |
| Ambiguous or high-stakes | `Ralph` — recursive reframing until stable |
| Cross-agent coordination | `Orchestrate` — routed to sub-agents |

**Entry condition:** task has DRI, project context loaded into `task_llm_context`  
**Exit artifact:** framing note committed to `company_memory_entries` (`scope: local`, `kind: framing`)  
**Blocks on:** tasks with `complexity: high` and no framing artifact cannot advance to Execute

This is the pre-code challenge gate. Nothing ships from Frame until the *what and why*
are stable.

---

## Execute

Workers run in isolated git worktrees. The RLM, tools (62+), and skills handle
implementation. Spend is tracked per agent per task via `spend_events`.

Context is managed through `GET /api/company/companies/{id}/tasks/{task_id}/llm-context`
so every worker starts with the right window — framing artifact, project playbook,
relevant shared memory — rather than a blank slate. Context is **not** assumed to
persist across worker invocations; it is rebuilt deterministically from durable storage
on every entry.

**Entry condition:** framing artifact present; worktree clean  
**Exit artifact:** implementation artifacts + `agent_run` record with `outcome` and `spend_usd`  
**Blocks on:** budget ceiling (hard-stop enforced at task checkout via `spend_events`)

---

## Gate

No task moves to Compound while the Gate is open. Gate checks, in order:

1. **Governance events logged** — at least one `governance_events` entry for this task
2. **Approvals resolved** — `approvals_pending = 0` (enforced by task state machine)
3. **Verification artifact present** — test run, review note, or explicit sign-off in task `notes`
4. **Budget not exceeded** — spend against configured role budget within threshold

Gate is not advisory. `task.status` cannot advance to `completed` with open flags.

The Gate is sovereign. It is not owned by the same agent that executed the work.
Verification must come from a separate agent or a human approval event.

**Entry condition:** `agent_run.outcome` set; implementation artifacts present  
**Exit artifact:** `governance_events` record with `event_type: task_verified`; `task.status = completed`  
**Blocks on:** any open approval, missing verification, exceeded budget

---

## Compound

Completed work is distilled into durable knowledge. The colony gets smarter with
every task that reaches Compound.

Promotion paths:

| Source | Destination | API |
|---|---|---|
| Successful execution trace | RooDB skill experiment | automatic on `agent_run.outcome = success` |
| RooDB skill experiment | Postgres `company_memory_entries` | `POST .../promote/roodb-skills` |
| Ladybug local beliefs | Postgres | `POST .../promote/ladybug-bundle` |
| Patterns worth broadcasting | Shared memory | `scope: shared`, `kind: broadcast` |

`trace2skill` distills execution trajectories into versioned skills, locked in
`skills-lock.json` with content hashes. Skills promoted this way are available to all
agents in the next Signal phase.

**Entry condition:** Gate passed; `task.status = completed`  
**Exit artifact:** memory entries and/or skill promotion record in `store_promotions`

---

## Repair

Repair activates when any phase fails. It is not a fallback — it is a first-class
path with its own artifacts.

**Trigger conditions:**

| Phase | Trigger |
|---|---|
| Signal | DRI assignment fails; project context missing |
| Frame | Council cannot reach stable reframing after N rounds |
| Execute | `agent_run.outcome = failure`; budget exceeded mid-task |
| Gate | Governance flag raised; approval rejected |
| Compound | Promotion fails; skill hash conflict |

**Repair path:**

1. Failure logged as a stigmergic signal to the hypergraph (`event_type: phase_failure`)
2. Task re-enters with `repair: true` flag; context re-hydrated from durable memory
3. `self_improvement.rs` captures the failure pattern for future prevention
4. If repair fails twice, task is escalated to human via `approvals` table with
   `escalation_reason` set

Repair events appear in `/ops/overview` under `audit.failures` and
`governance_recent`. They are not suppressed.

---

## Reading the loop in the console

`GET /api/company/companies/{id}/ops/overview` returns the current loop state:

- `overview.tasks` — tasks by phase (signal/frame/execute/gate/compound)
- `governance_recent` — recent Gate events and open flags
- `audit.failures` — Repair activations
- `spend` — Execute-phase budget consumption
- `integration_status` — which loop phases are fully wired

See [`ops-overview-api.md`](./ops-overview-api.md) for the full response contract.

---

## Loop phases vs. stored artifacts

| Phase | Postgres table | Key field |
|---|---|---|
| Signal | `tasks` | `status = open`, `dri_agent_ref` |
| Frame | `company_memory_entries` | `kind = framing` |
| Execute | `agent_runs`, `spend_events` | `outcome`, `spend_usd` |
| Gate | `governance_events`, `approvals` | `event_type = task_verified` |
| Compound | `store_promotions`, `company_memory_entries` | `source`, `promoted_at` |
| Repair | `governance_events` | `event_type = phase_failure` |
