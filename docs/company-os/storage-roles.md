# Storage roles and promotion contracts

This document defines **which store owns which facts** and **how artifacts move between stores** so the product story stays coherent: one operational graph, multiple specialized backends.

---

## 1. Storage topology

| Store | Engine | Owns | Does NOT own |
|-------|--------|------|-------------|
| **PostgreSQL** (Company OS) | Postgres via `sqlx`, migrations in `migrations/` | Companies, tasks, goals, memory, runs, governance, spend, DRIs, promotions — everything `company_id`-scoped that the console and APIs trust. | Raw vault search, local graph experiments, offline checkpoints. |
| **RooDB** | MySQL-protocol service (`--roodb`, default `127.0.0.1:3307`) | Semantic vault, skill bank, plan/audit traces, hire trees — `main.rs` runtime paths. | Company-scoped operational truth. Skills in RooDB are **staging**; they become authoritative when **promoted** to Postgres. |
| **Ladybug / `lbug`** | Embedded graph + optional native `lbug` (C++/CMake) | Local graph, Cypher, checkpoints, dev experiments. `GOLDEN_PATH.md` describes Ladybug-primary mode for single-node runs. | Multi-tenant company state. Beliefs are **local** until **imported** into Postgres. |
| **SQLite (`HsmSqliteStore`)** | Bundled SQLite via `DATABASE_URL` | Process-local subsystem tables (historically aliased `LadybugDb`). | Anything that needs fleet-wide consistency or console visibility. |

### One rule

> **No two stores may both be "source of truth" for the same predicate** without an automated reconciliation story.

Operational truth for companies → **Postgres only**.
Local beliefs / graph ticks → Ladybug/SQLite until **promoted**.
RooDB → **derived or experimental** until promoted.

---

## 2. Promotion pipelines

Both paths land artifacts in **Postgres** with clear provenance and audit trail in `store_promotions`.

### 2a. RooDB → Postgres

**API:** `POST /api/company/companies/{company_id}/promote/roodb-skills`

**Body:**
```json
{
  "skills": [
    { "skill_id": "...", "title": "...", "principle": "...", "confidence": 0.85 }
  ],
  "promoted_by": "operator-ui"
}
```

**What happens:**
1. For each skill, checks if already promoted (dedupe by `source_store + source_id`).
2. Inserts a `company_memory_entries` row with `source = "roodb_promotion"`, `kind = "skill"`, and `source_uri = "roodb://skills/{skill_id}"`.
3. Writes a `store_promotions` audit row with the source snapshot (JSON copy of the RooDB row at promotion time).

### 2b. Ladybug → Postgres

**API:** `POST /api/company/companies/{company_id}/promote/ladybug-bundle`

**Body:**
```json
{
  "beliefs": [
    { "content": "...", "title": "...", "confidence": 0.8, "tags": ["skill"] }
  ],
  "promoted_by": "operator-ui"
}
```

**What happens:**
1. For each belief, checks dedupe by `source_store + source_id` (uses `belief.id` or a generated index key).
2. Inserts a `company_memory_entries` row with `source = "ladybug_import"`, `kind` = `"skill"` if tagged, else `"belief"`, and `source_uri = "ladybug://beliefs/{source_id}"`.
3. Writes a `store_promotions` audit row.

### 2c. Rollback

**API:** `POST /api/company/companies/{company_id}/promote/rollback/{promotion_id}`

Deletes the target `company_memory_entries` row and marks the promotion as `rolled_back`.

### 2d. Audit

**API:** `GET /api/company/companies/{company_id}/promotions?source_store=roodb&status=promoted&limit=100`

Returns the promotion audit trail with source snapshots.

---

## 3. Console UI

The **Intelligence** page includes a **Store promotion pipeline** card with:
- **RooDB → Postgres** form: paste JSON array of skill rows, promote to company memory.
- **Ladybug → Postgres** form: paste JSON array of belief objects, import to company memory.
- **Audit log table**: shows all promotions with source, status, promoter, and rollback button.

---

## 4. Future directions

- **Automatic promotion job**: cron/heartbeat that reads new RooDB skills since last sync and promotes automatically (with governance event).
- **Capability ref binding**: after promotion, optionally PATCH `capability_refs` on relevant tasks so `llm-context` picks up the skill.
- **Consolidation**: as features stabilize, migrate RooDB-only paths into Postgres tables directly, reducing the number of active stores.

---

## See also

- [Company world model and intelligence](./world-model-and-intelligence.md) — canonical graph contract.
- [Intelligence layer and DRI alignment](./intelligence-layer-dri-alignment.md) — composer vs ledger vs edge.
- [`GOLDEN_PATH.md`](../../GOLDEN_PATH.md) — Ladybug-primary single-node setup.
