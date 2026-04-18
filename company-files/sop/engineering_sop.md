# Engineering SOP

*Last updated:* 2026-04-18  
*Loop: Signal → Frame → Execute → Gate → Compound*

---

## Signal — Triage

1. Triage incoming GitHub issues and task requests via `issue-triage`.
2. Every task requires a `project_id` and a `dri_agent_ref` before it moves forward.
   A task without an owner is blocked at Signal.
3. Prioritize by impact score and client SLA.
4. Assign `complexity: low | medium | high` based on scope.

---

## Frame — Challenge before coding

For `complexity: medium` or `complexity: high`:

1. Load task context (`GET .../tasks/{task_id}/llm-context`) — do not start from memory.
2. Run Council deliberation:
   - **What problem is actually being solved?** (Not what the issue title says — what the underlying need is.)
   - **What are the real constraints?** (Performance, compatibility, architecture bounds.)
   - **Is there a simpler version?** (Can this be solved in half the code?)
   - **What does verified completion look like?** (Define it before touching a file.)
3. Write framing note to company memory (`kind: framing`, `scope: local`).
4. Only proceed to Execute once the *what and why* are stable.

High-complexity tasks that skip Frame and fail Gate must go back to Frame, not straight
to re-execution.

---

## Execute — Implementation

1. Check out a clean git worktree for isolation.
2. Pull task context at the start of every session — context is rebuilt from durable
   storage, not assumed to persist.
3. Implement. Run CI. Track spend against the configured budget.
4. `outcome: failure` on an agent run triggers Repair — do not manually mark the task complete.

---

## Gate — Sovereign verification

Gate is not owned by the executor. Verification must come from a separate agent or
a human approval event.

1. Log a `task_verified` governance event from the verifier's `agent_ref`.
2. Resolve all open approvals. `task.status` cannot advance to `completed` with
   `approvals_pending > 0`.
3. Confirm the verification artifact is present (test run result, review note, or
   explicit sign-off in task notes).
4. Rejected approval sends the task back to Execute with `repair: true` and the
   rejection reason logged.

---

## Compound — Promote and close

1. Promote successful execution traces: `POST .../promote/roodb-skills`.
2. Merge via `pr-review`.
3. Update `release_notes.md` after deployment.
4. If the pattern changes how engineering work should be done, broadcast to shared
   memory and update the relevant project playbook.

---

## Repair

| Failure | Response |
|---|---|
| Execute outcome = failure | Re-queue with `repair: true`; context re-hydrates from Postgres |
| Gate approval rejected | Return to Execute; rejection reason in `governance_events` |
| Frame council stalled | Escalate via `approvals` with `escalation_reason: frame_stalled` |
| Two consecutive repair cycles | Auto-escalate to human |

All failures appear in `GET .../ops/overview` under `audit.failures`.

See also: [`company-files/sop/task_lifecycle_sop.md`](./task_lifecycle_sop.md) for the full
API-level procedure.
