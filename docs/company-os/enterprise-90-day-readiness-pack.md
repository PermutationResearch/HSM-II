# Enterprise 90-Day Readiness Pack

This document is the implementation and evidence tracker for:

- **P0 (days 1-30):** route auth enforcement, secret encryption, high-risk audit logging
- **P1 (days 31-60):** SSO/RBAC, tenancy DB hardening, retention policy engine
- **P2 (days 61-90):** docs pack, SLA/support artifacts, incident response drills + evidence

## P0 Delivered

- **Route auth enforcement**
  - `HSM_COMPANY_API_BEARER_TOKEN` gate on Company OS routes.
  - Health endpoint remains open for liveness checks.
- **Secret encryption**
  - Credentials encrypted at write-time when `HSM_COMPANY_CREDENTIALS_KEY` is set.
  - Optional strict mode: `HSM_COMPANY_REQUIRE_CREDENTIAL_ENCRYPTION=1`.
- **High-risk audit logs**
  - High-severity governance events for:
    - credential upsert
    - credential delete
    - verified handoff review decisions
    - promotion rollbacks

## P1 In Progress (Schema Foundation Added)

Migration: `20260413170000__enterprise_p1_sso_rbac_retention.sql`

- `company_sso_providers`
- `company_roles`
- `company_role_bindings`
- `company_retention_policies`

### P1 Remaining Runtime Work

- OIDC login + callback + JWK rotation validation path
- RBAC middleware wired to company endpoints (role -> permission checks)
- Tenant hardening:
  - explicit tenant claims propagation
  - row-level guards on critical writes
  - anti-cross-tenant query lint checks
- Retention engine worker:
  - dry-run mode with report
  - legal hold override
  - purge/archival execution + audit

## P2 Artifacts Checklist

### Documentation Pack

- Architecture and threat model
- Data handling and retention policy manual
- Access control and key management SOP
- Change management and release policy
- Customer security responsibilities matrix

### SLA / Support Artifacts

- Support tier definitions (response + resolution targets)
- Incident severity matrix
- Escalation tree and duty rotation
- Planned maintenance and change notification policy
- Monthly service review template

### Incident Response Drills + Evidence

- Tabletop calendar (quarterly)
- Drill scenario library (credential leak, tenant breakout attempt, data corruption, provider outage)
- Evidence template:
  - timeline
  - detection method
  - blast radius
  - containment steps
  - customer comms
  - corrective actions

## Environment Variables

- `HSM_COMPANY_API_BEARER_TOKEN`
- `HSM_COMPANY_CREDENTIALS_KEY`
- `HSM_COMPANY_REQUIRE_CREDENTIAL_ENCRYPTION`

## Evidence Log Template

Use this block per milestone:

```text
Date:
Milestone:
Owner:
Change:
Validation:
Artifacts:
Follow-ups:
```
