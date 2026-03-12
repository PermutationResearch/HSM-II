# Ouroboros -> HSM-II Migration Phases (Implemented Foundation)

This document tracks the concrete foundation implemented in `src/ouroboros_compat`.

## Phase 1 - Constitution and Identity Policy

- Module: `src/ouroboros_compat/phase1_policy.rs`
- Enforces:
  - Identity-core path protection (`Deny`)
  - Non-creator self-modification (`ReviewRequired`)
  - Release invariants check (`version == tag == readme`) before self-mod

## Phase 2 - Risk Gate

- Module: `src/ouroboros_compat/phase2_risk_gate.rs`
- Classifies actions into `Low` or `High` risk.
- High-risk kinds:
  - `SelfModification`
  - `ExternalWrite`
  - `FederationSync`
- Produces a structured `RiskAssessment` used for council gating.

## Phase 3 - Council Bridge

- Module: `src/ouroboros_compat/phase3_council_bridge.rs`
- Maps high-risk actions to HSM-II council proposals and mode selection.
- Uses HSM-II `ModeSwitcher` with default thresholds.
- Approval defaults:
  - `min_confidence_for_approval = 0.65`
  - `min_evidence_coverage_for_approval = 1.0`

## Phase 4 - Evidence Contract

- Module: `src/ouroboros_compat/phase4_evidence_contract.rs`
- Requires investigation-backed evidence bundle for high-risk actions:
  - Investigation session id
  - Tool-call audit trail
  - Evidence chain count
  - Claim/evidence counts
  - Coverage floor

## Phase 5 - Ops, Federation, and Memory

- Module: `src/ouroboros_compat/phase5_ops_memory.rs`
- Runtime SLO checks:
  - coherence >= 0.70
  - stability >= 0.28
  - mean trust >= 0.65
  - council confidence >= 0.65
  - evidence coverage >= 1.0
- Full-mesh federation health check against trust graph edges.
- Export scheduler:
  - per-high-risk action
  - hourly
  - daily
- Event-sourced memory core with mutable cache projection:
  - belief events
  - experience events
  - skill promotion events
  - action audit events

## Current Status

The compatibility foundation is implemented and exported from `src/lib.rs`.
Phase integration now includes:
- High-risk gate checks on:
  - coder assistant `write`/`edit`/`bash`
  - `/federation sync`
  - `/save` (and keyboard `s`/`S`)
- Persisted compatibility records in RooDB:
  - `ouroboros_gate_audits`
  - `ouroboros_memory_events`
- Web API read endpoints:
  - `GET /api/ouroboros/gate-audits?limit=200`
  - `GET /api/ouroboros/memory-events?limit=200`
