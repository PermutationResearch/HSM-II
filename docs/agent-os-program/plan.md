# Plan — agent OS program (living)

## Objective

Close the **goal → task → execute → verify → memory → visibility → learn** loop on top of existing HSM-II Company OS, without a parallel shadow ledger.

## Current phase

**Bootstrap** — file pack, contract, capability matrix, smoke script, queues seeded.

## Near-term backlog

1. Run smoke; record metrics log row.
2. Dogfood M1 checklist on one internal company.
3. M2 metrics spine (automated append).
4. Document eval→live config gap as explicit ADR or task template.

## Risks

- Assuming `best_config.json` affects live agents (it does not) — see `EVAL_AND_META_HARNESS.md`.
