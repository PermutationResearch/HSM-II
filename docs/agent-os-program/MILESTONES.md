# Milestones — agent OS program on HSM-II

## M1 — Closed loop smoke (current)

**Outcome:** One scripted or documented path proves: task/run exists → execution signal → evidence → human-visible → checklist signed → one improve-queue item.

**Exit criteria:** All items in `verification/MILESTONE_1_CHECKLIST.md` checked or waived with owner + date.

## M2 — Observable metrics spine

**Outcome:** Append-only log (start with `momentum/METRICS_LOG.md` or JSONL under `runs/agent-os-metrics/`) fed by at least one automated source (e.g. smoke script or `hsm-eval` manifest parse).

**Exit criteria:** Dashboard or doc section lists how to read the log; 7-day retention policy documented.

## M3 — Eval ↔ task bridge (pilot)

**Outcome:** One **task template** or `capability_refs` pattern links a benchmark artifact to a “replay / verify” procedure (even if manual first run).

**Exit criteria:** `knowledge.md` section + one example task JSON in docs.

## M4 — Proactive queue (pilot)

**Outcome:** Scheduled or manual “stale task / blocked run” scan produces entries in `momentum/QUEUES.md` **recurring** or **improve**.

**Exit criteria:** Script or SQL recipe checked in under `scripts/`.
