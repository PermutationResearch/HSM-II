# Company Ops Overview API

`GET /api/company/companies/:company_id/ops/overview`

This endpoint is the first unified operations surface for the Company OS. It is
meant to close the gap between the existing low-level pieces:

- Company OS tasks / goals / agents / governance / spend in Postgres
- `operations.yaml` under each company `hsmii_home`
- task-trail telemetry under `memory/task_trail.jsonl`
- the company console operator UI

## What it returns

- `company`
  Basic company record from Postgres.
- `ops_config`
  Whether `config/operations.yaml` was found and validated for this company.
- `overview`
  Counts for goals, tasks, human-escalation load, agents, and spend.
- `budgets`
  Configured monthly budgets with current usage when computable.
- `heartbeats`
  Heartbeat schedule from `operations.yaml` plus persisted runtime state from
  `memory/heartbeat_state.json`.
- `tickets`
  Ticket list from `operations.yaml`.
- `ticket_sync`
  Result of mirroring configured `operations.yaml` tickets into Company OS tasks.
- `org`
  Org chart / role model from `operations.yaml`.
- `governance_recent`
  Recent governance events from Postgres.
- `spend`
  Spend totals grouped by kind and by `agent_ref`.
- `audit`
  Aggregated task-trail telemetry from `memory/task_trail.jsonl`.
- `integration_status`
  Simple booleans summarizing which orchestration features are configured or
  already implemented.

## Context resilience

The overview endpoint is also the primary **context recovery surface**. When an agent
loses session state — crash, restart, context window flush — it re-enters the system
by calling this endpoint before doing anything else.

`GET .../ops/overview` returns:

- `audit.task_trail` — recent task-level execution history so a re-entering agent can
  reconstruct what was done and what was not
- `governance_recent` — open flags and unresolved approvals the agent must be aware of
- `heartbeats` — current heartbeat state so the agent knows whether scheduled work is
  overdue
- `overview.tasks` — counts by status so the agent can triage before acting

Context is **not** stored in agent memory or session. It lives in Postgres. Every agent
entry should start with this call, not with assumptions about what was previously loaded.
This is the answer to context decay: durable storage queried on demand, not fragile
in-process state.

## Repair visibility

Repair events — failures at any loop phase — appear in two places in the overview
response:

- `audit.failures` — task trail entries with `outcome: failure` or `event_type: phase_failure`
- `governance_recent` — governance events of type `phase_failure` or `escalation`

Repair events are not filtered out of the overview. An operator or agent reading the
overview always sees the current failure state. Suppressing failures is not an option
the API offers.

When `audit.failures` is non-empty, the standard repair path is:

1. Read the failure entry to identify which phase failed and why
2. Re-hydrate task context: `GET .../tasks/{task_id}/llm-context`
3. Re-enter the failed phase with `repair: true` on the task
4. If two consecutive cycles fail, the task auto-escalates to `approvals` with
   `escalation_reason` set; this appears in `governance_recent`

## Current limitations

- Hard-stop budgets are now enforced on task checkout.
- Role-scoped budget usage is only available when `spend_events.agent_ref`
  matches the configured role id used at checkout/runtime.
- Heartbeats are surfaced from config and runtime exists separately, but this
  endpoint only reflects lightweight persisted status, not full job telemetry.
- Ticket mirroring currently happens when this overview endpoint is fetched; it
  upserts Company OS tasks using `capability_refs: [{kind: "ticket", ref: ...}]`.

## Why this exists

The repo already had partial implementations of:

- budgets
- heartbeats
- tasks / handoffs
- org chart / workforce
- governance log
- spend ledger
- multi-company isolation
- operator UI
- bundle import/export
- audit trail telemetry

But they were spread across separate APIs and config files. This endpoint gives
the UI and future orchestration code one integration point to build on.
