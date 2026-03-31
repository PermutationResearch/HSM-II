# HarnessV1 Plan (Incremental, File-by-File)

Goal: convert current "partial" layers into a unified, production-style harness runtime without blocking ongoing work.

## Scope

Strengthen these layers first:

1. unified generator query loop + pause/resume
2. centralized retry/error policy
3. formal 3-tier context compression
4. explicit permission/security decision gate
5. background job runtime + broker-driven coordination

## Phase 0 — Non-breaking Foundations

### Add shared runtime models

- Add `src/harness/mod.rs`
- Add `src/harness/types.rs`
  - `HarnessTask`, `HarnessState`, `ResumeToken`, `TaskOutcome`, `ErrorClass`
- Add `src/harness/events.rs`
  - state-transition events and trace envelope (`trace_id`, `task_id`, `agent_id`)

### Wire exports

- Update `src/lib.rs`
  - `pub mod harness;`

Deliverable: compile-only shared types; no behavior change.

## Phase 1 — Unified Query Runtime

### Add runtime state machine

- Add `src/harness/runtime.rs`
  - canonical states: `queued -> running -> waiting_tool -> paused -> resumed -> completed/failed`
  - checkpoint serializer/deserializer

### Add persistence adapter

- Add `src/harness/store.rs`
  - append-only JSONL event log
  - checkpoint snapshots under `runs/harness_state/`

### Integrate first caller (eval runner)

- Update `src/eval/runner.rs`
  - wrap turn execution through `HarnessRuntime::run_step(...)`
  - emit state transitions and checkpoint IDs

Deliverable: eval uses canonical pause/resume semantics.

## Phase 2 — Retry and Error Recovery

### Add policy module

- Add `src/harness/retry.rs`
  - exponential backoff + jitter
  - retry budget
  - provider failover hints

### Add error taxonomy

- Add `src/harness/errors.rs`
  - classify `transient`, `tool`, `policy`, `model`, `fatal`

### Integrate with LLM calls

- Update `src/eval/runner.rs`
- Update `src/llm/client.rs` (adapter wrapper; avoid breaking existing API)

Deliverable: one retry policy path reused everywhere.

## Phase 3 — Context Compression (3-tier)

### Add compressor module

- Add `src/harness/context/mod.rs`
- Add `src/harness/context/tiered.rs`
  - Tier 1: short horizon (latest turns)
  - Tier 2: episodic summaries
  - Tier 3: distilled skills/beliefs

### Add budget allocator

- Add `src/harness/context/budget.rs`
  - enforce token/char budgets per tier
  - deterministic truncation policy

### Integrate in HSM runner

- Update `src/eval/runner.rs`
  - replace ad hoc injection assembly with tiered context builder

Deliverable: formal context pipeline with metrics.

## Phase 4 — Permission/Security Decision Gate

### Add centralized gate

- Add `src/harness/policy.rs`
  - `PolicyDecision { allow, reason, required_approval }`
  - capability-token checks by role/task

### Integrate with tool execution

- Update `src/coder_assistant/tools.rs`
  - enforce gate before execution

### Integrate with A2A delegation

- Update `src/bin/hsm_a2a_adapter.rs`
  - gate `delegate_task` / `heartbeat_tick` before Hermes invocation

Deliverable: one PDP-like gate for side effects.

## Phase 5 — Background Runtime + Broker

### Background job lifecycle manager

- Add `src/harness/background.rs`
  - lease, heartbeat, backpressure, cancellation

### Knowledge broker service module

- Add `src/harness/broker.rs`
  - capability index
  - reputation/load-aware candidate ranking
  - assignment reason logging

### A2A integration

- Update `src/bin/hsm_a2a_adapter.rs`
  - route discovery to `HarnessBroker`
  - structured assignment decisions

Deliverable: org-level delegation becomes deterministic and inspectable.

## Phase 6 — CLI and Operational Surface

### New harness control binary

- Add `src/bin/hsm_harness.rs`
  - commands:
    - `run-task`
    - `resume`
    - `inspect`
    - `replay`

### Documentation and runbooks

- Update `documentation/guides/A2A_MESSAGE_CONTRACTS.md`
- Update `documentation/README.md`
- Add `documentation/guides/HARNESS_V1_OPERATIONS.md`

Deliverable: operable runtime with clear commands.

## Minimum Tests Per Phase

- Add `tests/harness_runtime.rs` (state transitions + resume)
- Add `tests/harness_retry.rs` (retry budgets + backoff behavior)
- Add `tests/harness_context.rs` (tier budget determinism)
- Add `tests/harness_policy.rs` (allow/deny matrix)
- Add `tests/harness_broker.rs` (candidate ranking and tie-breaks)

## Suggested Shipping Order (2-week cadence)

1. Week 1: Phases 0-2
2. Week 2: Phases 3-4
3. Week 3: Phases 5-6

Each week ends with:

- `cargo build`
- targeted tests
- one `hsm-eval` run
- one `hsm_a2a_adapter` heartbeat/delegation smoke

## Immediate Next Commit Slice

Smallest next implementation that unlocks everything else:

1. add `src/harness/{mod.rs,types.rs,events.rs,runtime.rs,store.rs}`
2. export in `src/lib.rs`
3. wire `src/eval/runner.rs` to emit runtime transitions
4. add `tests/harness_runtime.rs`

This creates a real substrate for pause/resume, retries, policy, and broker layers to build on.

### Env vars (implemented)

- `HSM_HARNESS_LOG` — append-only JSONL path for transition events
- `HSM_HARNESS_TRACE_ID` — optional correlation id (default: random UUID)
- `HSM_HARNESS_AGENT_ID` — optional agent label (default: `eval`)
- `HSM_HARNESS_CHECKPOINT_DIR` — optional checkpoint directory for future resume blobs
