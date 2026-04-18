# Task Lifecycle SOP

*Loop: Signal → Frame → Execute → Gate → Compound*  
*Full loop definition: [`docs/company-os/operating-loop.md`](../../docs/company-os/operating-loop.md)*

---

## Creating a task (Signal)

```bash
POST /api/company/companies/{id}/tasks
{
  "title": "...",
  "project_id": "...",       # required — no orphan tasks
  "dri_agent_ref": "...",    # required — no owner = blocked at Signal
  "complexity": "low|medium|high",
  "capability_refs": [...]
}
```

A task without `project_id` or `dri_agent_ref` is rejected at creation.

---

## Before writing any code (Frame)

For `complexity: medium` or `complexity: high`, a framing artifact is required
before implementation begins.

**Steps:**
1. Load task context: `GET .../tasks/{task_id}/llm-context`
2. Run Council deliberation — challenge the problem statement:
   - Is this the right problem?
   - What are the real constraints?
   - Is there a simpler version?
   - What does success look like concretely?
3. Write framing note to company memory:

```bash
POST /api/company/companies/{id}/memory
{
  "task_id": "...",
  "scope": "local",
  "kind": "framing",
  "content": "...",
  "agent_ref": "..."
}
```

High-complexity tasks cannot advance to Execute without this entry.

---

## Running the work (Execute)

1. Check out a clean git worktree for isolation
2. Pull task context at start — do not rely on in-memory state from previous sessions
3. Implement using available tools and skills
4. All LLM calls are logged as spend events automatically — check budget ceiling before
   starting large work: `GET .../spend/summary`
5. On completion, set outcome on the agent run:

```bash
PATCH /api/company/companies/{id}/agent-runs/{run_id}
{
  "outcome": "success|failure|partial",
  "notes": "..."
}
```

If `outcome: failure` — Repair activates automatically. Do not mark the task complete.

---

## Verifying the work (Gate)

Gate is mandatory. The agent or human who verifies must not be the same as the one
who executed.

**Steps:**
1. Log a governance event:

```bash
POST /api/company/companies/{id}/governance/events
{
  "task_id": "...",
  "event_type": "task_verified",
  "agent_ref": "...",       # verifier, not executor
  "notes": "..."
}
```

2. Resolve any open approvals:

```bash
PATCH /api/company/companies/{id}/approvals/{approval_id}
{
  "status": "approved|rejected",
  "resolved_by": "..."
}
```

3. Check Gate is clear before advancing: `GET .../tasks/{task_id}` — confirm
   `approvals_pending = 0` and at least one `task_verified` governance event exists.

Rejected approvals send the task back to Execute with `repair: true`.

---

## Promoting knowledge (Compound)

After Gate passes and `task.status = completed`:

1. Promote successful execution traces to durable memory:

```bash
POST /api/company/companies/{id}/promote/roodb-skills
{ "task_id": "..." }
```

2. If the pattern is useful across the company, broadcast it:

```bash
POST /api/company/companies/{id}/memory
{
  "scope": "shared",
  "kind": "broadcast",
  "content": "...",
  "tags": [...]
}
```

3. Update project playbook if this task changed how work should be done.

---

## When a phase fails (Repair)

| What happened | What to do |
|---|---|
| DRI not found | Re-assign via `PATCH .../tasks/{id}` with new `dri_agent_ref` |
| Frame council stalled | Escalate: `POST .../approvals` with `escalation_reason: frame_stalled` |
| Execute outcome = failure | Task re-queues with `repair: true`; context re-hydrates from memory |
| Gate approval rejected | Returns to Execute; rejection reason logged in `governance_events` |
| Compound promotion fails | Check `store_promotions` for conflict; resolve hash mismatch manually |
| Two consecutive Repair cycles | Auto-escalates to human via `approvals` table |

All Repair events appear in `GET .../ops/overview` under `audit.failures`. They are
not suppressed and must be resolved or explicitly closed.

---

## Quick status check

```bash
# Current loop state for a company
GET /api/company/companies/{id}/ops/overview

# Specific task phase
GET /api/company/companies/{id}/tasks/{task_id}

# Open approvals blocking Gate
GET /api/company/companies/{id}/approvals?status=pending

# Recent Gate events
GET /api/company/companies/{id}/governance/events?limit=20
```
