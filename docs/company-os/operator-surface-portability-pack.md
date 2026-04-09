# Company OS Operator Surface and Portability Pack

This pack translates high-value Hermes patterns into Company OS implementation language and adoption guidance.

## 1) Open Standard Skill Portability (AgentSkills)

- Goal: skills can move between systems without lossy copy/paste or hidden metadata loss.
- API surface:
  - `GET /api/company/companies/{company_id}/skills/agentskills/export`
  - `POST /api/company/companies/{company_id}/skills/agentskills/import`
- Contract:
  - Export uses `standard=agentskills.io` and includes `provenance.source` and `provenance.pack`.
  - Import supports `dry_run` and `overwrite` controls.
  - Provenance is preserved in `company_skills.source` and can be packed as `source:pack`.

## 2) Migration Tooling Pattern (Dry-Run First)

- Goal: allow "bring your existing agent data" onboarding with predictable blast radius.
- API surface:
  - `POST /api/company/companies/{company_id}/migrations/legacy-agent-data`
- Expected payload lanes:
  - `skills[]` (portable records)
  - `memories[]` (title/body/tags/kind/scope)
  - `command_allowlist[]` (captured for audit and future policy mapping)
- Safety contract:
  - `dry_run=true` by default.
  - explicit source tagging in records (`legacy_migration:<source>`).
  - governance event written on apply.

## 3) Single Operator Surface Across Gateways

- Goal: one mental model for operator actions regardless of transport.
- Pattern:
  - keep shared slash semantics in gateway adapters.
  - route provider-specific behavior into adapter edges, not policy core.
  - keep approvals/tokens/audit shared centrally.
- Existing alignment:
  - runtime activity + completion events
  - signed action-token verification flows
  - tier-1 compatibility matrix endpoint for gateway capability status

## 4) Learning Loop Discoverability

- Goal: make self-improvement value visible without requiring operators to read backend docs.
- Console framing:
  - telemetry -> proposal -> replay -> apply -> rollback -> nudge
  - tie outcome metrics to operator trust (`first-pass success`, `repeat failures`, `rollback rate`)
  - expose cross-links to memory engine and promotion pipelines

## 5) Runtime Portability and Idle-Cost Story

- Goal: give one-person companies and lean teams deployment options by cost/risk profile.
- API surface:
  - `GET /api/company/runtime/portability-matrix`
- Contract:
  - include backend key, isolation mode, hibernation posture, and operator notes.
  - keep "integratable" rows explicit to avoid over-claiming capabilities.

## 6) Research Hooks for Self-Optimization

- Goal: connect runtime traces to eval and benchmark loops.
- Immediate hook points:
  - proposal replay outcomes
  - run telemetry snapshots
  - workflow feed failure buckets
- Suggested next increment:
  - trajectory export endpoint keyed by company + date range
  - benchmark job linkage to improvement run IDs

## 7) Security Documentation Structure (Trust Center Ready)

- Suggested trust-center sections:
  - approval model (tokenization, signatures, replay protection)
  - pairing/session model (shared sessions, participant controls)
  - runtime isolation (workspace boundaries, strict mode)
  - network/IO controls (SSRF, archive traversal, sanitized workdirs)
  - credential handling (encryption at rest, audit events)
  - tenant boundaries and retention

This structure maps directly to enterprise due-diligence checklists and shortens security review cycles.
