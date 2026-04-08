# HSM-II roadmap — context, observability, quality

This file tracks **phased** work aligned with the “interesting to have” list. Milestone 1 is partially implemented in-tree.

## Milestone 1 (done / in progress)

| Item | Status | Notes |
|------|--------|--------|
| Policy-as-data (YAML) | **Done** | `HSM_POLICY_FILE` → `policy_config::LoadedPolicy`. `tools.deny` enforced in `HarnessPolicyGate`. |
| Context manifest | **Done** | Personal agent: `assemble_prompt_sections_with_manifest`. Company OS: `GET .../llm-context` returns `context_manifest` + same `hsm.context_manifest` tracing + telemetry `company.task.llm_context` when enabled. Full JSON: `HSM_LOG_CONTEXT_MANIFEST=1`. |
| Context tiers (metadata) | **Done** | Hot/warm/cold per section in policy + defaults; used in manifest (not yet hard token budgets). |
| Diagnostic bundle | **Done** | `cargo run --bin hsm_diagnostic` → zip with version, redacted env, rustc. |
| Telemetry (opt-in) | **Done** | `HSM_TELEMETRY_*` + `src/telemetry.rs`. |

## Milestone 2 (next)

| Item | Goal |
|------|------|
| In-app privacy / telemetry UI | Surface consent tiers in company console or settings JSON; mirror env flags. |
| OpenTelemetry (OTLP) | Export traces/metrics when `OTEL_*` set; complement Prometheus. |
| Token budgets per tier | Enforce caps beyond byte-truncation (model-aware estimates). |
| Resolver API | Single “resolve context for agent/task” over skills FS + Postgres memory. |

## Milestone 3 (later)

| Item | Goal |
|------|------|
| Replay / scenario export | Serializable harness or tick traces for CI + demos. |
| Lab mode | Fixture worlds; no production Postgres. |
| External context store | Optional HTTP backend (e.g. OpenViking-style) behind trait. |
| `hsm eval` as product command | Document + wrap existing eval binaries as one UX. |

## References

- Example policy: `templates/hsmii/policy.example.yaml`
- Telemetry env: `.env.example` (`HSM_TELEMETRY_*`)
- Manifest logs: `RUST_LOG=hsm.context_manifest=info` (and `debug` for full JSON when `HSM_LOG_CONTEXT_MANIFEST=1`)
