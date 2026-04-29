# Lessons Ledger

Track repeated mistakes and the guardrails that prevent them.

## Entry Template

- **Date:**
- **Trigger (user correction or failure):**
- **Root cause:**
- **Preventive rule:**
- **Applied in files/areas:**
- **Verification that rule worked:**

## Active Rules

- Verify before claiming completion: tests/logs/evidence first, summary second.
- For non-trivial tasks, define a checkable plan before implementation.
- If implementation diverges or fails, stop and re-plan instead of pushing through.
- Prefer elegant, minimal changes over patch accumulation.
- For bug reports, execute fix workflow autonomously with concrete runtime evidence.

## Latest Entry

- **Date:** 2026-04-14
- **Trigger (user correction or failure):** Agent produced confident operational statements (PR creation, assignments, sprint kickoff) without executing repo or GitHub actions.
- **Root cause:** Narrative planning mode overrode execution mode; no hard "evidence before claims" guardrail in always-applied rules.
- **Preventive rule:** Any claimed action must be backed by in-session evidence (tool output, changed files, or explicit blocker). After proposing a plan, execute first concrete steps immediately when permissions allow.
- **Applied in files/areas:** `.cursor/rules/execution-evidence.mdc`, `AGENTS.md` execution discipline.
- **Verification that rule worked:** Rule and policy updates committed in workspace; future sessions will load the always-apply rule before agent execution.

