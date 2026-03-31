# A2A Message Contracts (Minimal v1)

This guide defines a minimal JSON-RPC contract for multi-agent org execution in HSM-II:

- each engineer agent runs its own runtime
- sidecars expose JSON-RPC for inter-agent communication
- Knowledge Broker handles capability discovery/routing
- CEO/PM delegates, engineers execute, results flow back as structured artifacts

## Goals

- Keep contracts small and implementation-friendly
- Make delegation observable, retryable, and auditable
- Support direct machine routing (no human babysitting)

## Transport and Envelope

- Protocol: JSON-RPC 2.0
- Transport: HTTP or local IPC socket
- Content type: `application/json`
- Correlation: every request includes `trace_id` and `task_id`

Standard envelope fields inside params:

- `trace_id`: run-level correlation ID
- `task_id`: stable work item ID
- `from_agent`: sender agent ID
- `to_agent`: receiver agent ID (if point-to-point)
- `timestamp_unix`: unix seconds

## Canonical IDs

- Agent IDs: `role.namespace.instance` (example: `eng.backend.alpha`)
- Capability IDs: reverse-domain style (example: `code.api.rest`)
- Artifact IDs: `artifact_<uuid>`
- Delegation IDs: `deleg_<uuid>`

## Core Methods

### 1) `discover_capabilities`

Used by CEO/PM or peers to ask the Knowledge Broker who can do what.

Request:

```json
{
  "jsonrpc": "2.0",
  "id": "1",
  "method": "discover_capabilities",
  "params": {
    "trace_id": "trace_abc",
    "task_id": "task_123",
    "from_agent": "pm.core.1",
    "query": {
      "capabilities_any": ["code.api.rest", "test.integration"],
      "domain": "software_engineering",
      "max_candidates": 5
    }
  }
}
```

Response:

```json
{
  "jsonrpc": "2.0",
  "id": "1",
  "result": {
    "candidates": [
      {
        "agent_id": "eng.backend.alpha",
        "capabilities": ["code.api.rest", "test.integration"],
        "load": 0.35,
        "reputation": 0.82,
        "availability": "online"
      }
    ]
  }
}
```

### 2) `delegate_task`

Creates an executable delegation to an engineer agent.

Request:

```json
{
  "jsonrpc": "2.0",
  "id": "2",
  "method": "delegate_task",
  "params": {
    "trace_id": "trace_abc",
    "task_id": "task_123",
    "delegation_id": "deleg_001",
    "from_agent": "pm.core.1",
    "to_agent": "eng.backend.alpha",
    "objective": "Add webhook endpoint with auth and tests",
    "acceptance_criteria": [
      "OpenAPI updated",
      "Integration tests pass",
      "No new warnings in touched files"
    ],
    "constraints": {
      "deadline_unix": 1760000000,
      "max_tokens_budget": 12000,
      "allowed_tools": ["shell", "edit", "tests"]
    },
    "inputs": {
      "repo_ref": "main",
      "files": ["src/api", "tests/api"],
      "context_artifact_ids": ["artifact_spec_12"]
    }
  }
}
```

Response:

```json
{
  "jsonrpc": "2.0",
  "id": "2",
  "result": {
    "accepted": true,
    "delegation_id": "deleg_001",
    "eta_seconds": 900
  }
}
```

### 3) `status_update`

Progress heartbeat from engineer to PM/CEO (or broker relay).

Request:

```json
{
  "jsonrpc": "2.0",
  "method": "status_update",
  "params": {
    "trace_id": "trace_abc",
    "task_id": "task_123",
    "delegation_id": "deleg_001",
    "from_agent": "eng.backend.alpha",
    "state": "in_progress",
    "percent": 60,
    "message": "Endpoint and auth middleware done; writing tests now",
    "blocked": false,
    "timestamp_unix": 1759999999
  }
}
```

### 4) `handoff_artifact`

Returns outputs as typed artifacts (diffs, test results, logs, docs).

Request:

```json
{
  "jsonrpc": "2.0",
  "id": "3",
  "method": "handoff_artifact",
  "params": {
    "trace_id": "trace_abc",
    "task_id": "task_123",
    "delegation_id": "deleg_001",
    "from_agent": "eng.backend.alpha",
    "to_agent": "pm.core.1",
    "artifact": {
      "artifact_id": "artifact_777",
      "type": "code_patch",
      "uri": "file://runs/task_123/patch.diff",
      "checksum_sha256": "abc123",
      "metadata": {
        "files_touched": ["src/api/webhooks.rs", "tests/webhooks.rs"],
        "tests_passed": ["cargo test webhooks"],
        "warnings_introduced": 0
      }
    }
  }
}
```

Response:

```json
{
  "jsonrpc": "2.0",
  "id": "3",
  "result": {
    "received": true,
    "artifact_id": "artifact_777"
  }
}
```

### 5) `close_task`

PM/CEO (or QA gate) closes the delegation with a final outcome.

States:

- `accepted`
- `rejected`
- `needs_revision`
- `cancelled`

## Error Contract

Use JSON-RPC error object with stable codes:

- `1001` unknown_capability
- `1002` agent_unavailable
- `1003` invalid_contract
- `1004` budget_exceeded
- `1005` policy_blocked
- `1500` internal_error

Example:

```json
{
  "jsonrpc": "2.0",
  "id": "2",
  "error": {
    "code": 1002,
    "message": "target agent unavailable",
    "data": {
      "agent_id": "eng.backend.alpha"
    }
  }
}
```

## Minimal Orchestration Flow

1. PM calls `discover_capabilities` on broker
2. PM sends `delegate_task` to selected engineer
3. Engineer emits periodic `status_update`
4. Engineer sends `handoff_artifact`
5. PM validates and sends `close_task`
6. Trace is logged for eval and Trace2Skill ingestion

## Integration with HSM-II Eval Loop

For each completed delegation, write:

- turn-level artifacts (`turns_hsm.jsonl`)
- optional retrieval trace (`hsm_trace.jsonl`)
- handoff artifact metadata
- outcome label (`accepted`/`needs_revision`)

Then run:

- `hsm-eval` for behavioral scoring
- `hsm_trace2skill import-eval` to ingest artifacts
- `hsm_trace2skill merge` and `apply` to update skills

This keeps org coordination and model improvement connected through one artifact format.

## Recommended First Implementation

- Build only the 4 methods above plus `close_task`
- Use one broker + one PM + two engineer agents
- Enforce strict JSON schema validation at sidecar boundary
- Persist every message with `trace_id` for replay/debug
- Add timeout + retry policy in PM for `delegate_task` and `handoff_artifact`

## JSON Schemas

Machine-usable method `params` schemas are available at:

- `documentation/references/a2a_schemas/discover_capabilities.params.schema.json`
- `documentation/references/a2a_schemas/delegate_task.params.schema.json`
- `documentation/references/a2a_schemas/status_update.params.schema.json`
- `documentation/references/a2a_schemas/handoff_artifact.params.schema.json`
- `documentation/references/a2a_schemas/close_task.params.schema.json`

## Runnable Sidecar (MVP)

This repo now includes a minimal sidecar binary:

- `hsm_a2a_adapter`

Run it:

```bash
cargo run -p hyper-stigmergy --bin hsm_a2a_adapter -- --bind 127.0.0.1:9797
# or start adapter in global dry-run mode:
cargo run -p hyper-stigmergy --bin hsm_a2a_adapter -- --bind 127.0.0.1:9797 --dry-run
```

Call endpoint:

- `POST /rpc` with JSON-RPC 2.0 payloads

What it does today:

- serves `discover_capabilities`, `delegate_task`, `status_update`, `handoff_artifact`, `close_task`
- serves `heartbeat_tick` to convert one Paperclip heartbeat ticket into discovery + delegation
- executes Hermes CLI on `delegate_task` via:
  - `hermes --single-query "<objective>" [--resume "<session_id>"]`
- persists transcripts/outputs/delegation records in `.hsmii/a2a_adapter/`
- maintains per-task resume map in `.hsmii/a2a_adapter/sessions.json`

`heartbeat_tick` request example:

```json
{
  "jsonrpc": "2.0",
  "id": "hb1",
  "method": "heartbeat_tick",
  "params": {
    "trace_id": "trace_heartbeat_1",
    "from_agent": "pm.core.1",
    "dry_run": true,
    "ticket": {
      "task_id": "ticket_42",
      "objective": "Implement webhook retries with tests",
      "domain": "software_engineering",
      "required_capabilities": ["code.api.rest", "test.integration"],
      "acceptance_criteria": ["tests pass", "docs updated"],
      "constraints": {
        "deadline_unix": 1760000000,
        "max_tokens_budget": 12000,
        "allowed_tools": ["shell", "edit", "tests"]
      },
      "inputs": {
        "repo_ref": "main",
        "files": ["src/api", "tests/api"]
      }
    }
  }
}
```

When `dry_run` is enabled (either CLI `--dry-run` or request `params.dry_run=true`), the adapter:

- performs capability discovery and assignee selection
- returns the planned `delegate_task` payload
- skips Hermes execution and side-effectful delegation artifacts
