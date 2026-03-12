# LARS — Hyper-Stigmergic Morphogenesis II

[LARS (larsql)](https://github.com/ryrobes/larsql) adds semantic SQL to the live world state,
letting you query agents, beliefs, edges, and improvements with natural-language operators
like `MEANS`, `SIMILAR_TO`, `TOPICS`, `ASK`, and `SUMMARIZE`.

---

## Architecture

```
RooDB (MySQL wire, :3307, TLS)
        │
        │  python3.12 lars/export-to-duckdb.py  (/exportdb in TUI)
        ▼
lars/hyper_stigmergy.duckdb   ← local DuckDB file (no TLS required)
        │
        │  ~/.lars/sql_connections/hyper_stigmergy.yaml
        ▼
LARS SQL server (:15432, PostgreSQL wire)
LARS Studio     (:5050, web UI)
```

**Why the bridge?**  DuckDB's MySQL scanner does not support TLS.
RooDB requires TLS. The Python bridge reads via `mysql-connector-python`
(TLS-capable) and writes a plain DuckDB file that LARS reads natively.

---

## Quick Start

### 1. Export the DB bridge

Run inside the TUI chat:
```
/exportdb
```

Or directly:
```bash
python3.12 lars/export-to-duckdb.py
```

### 2. Start LARS

```bash
./lars/start-lars.sh          # SQL server + Studio UI
./lars/start-lars.sh --sync   # Also watch for DB updates every 30s
./lars/start-lars.sh --no-studio  # SQL server only
```

### 3. Connect a SQL client

```
psql postgresql://admin:admin@localhost:15432/default
```

Or open [http://localhost:5050](http://localhost:5050) in your browser.

---

## Tables

All tables are under the `hyper_stigmergy` schema:

| Table | Description |
|-------|-------------|
| `agents` | Agent state (role, drives, description, learning_rate) |
| `hyper_edges` | Hyperedge connections (participants, weight, emergent flag) |
| `beliefs` | Agent belief content and strengths |
| `experiences` | Accumulated experience records |
| `improvement_events` | Self-improvement history (intent, mutation, coherence delta) |
| `ontology` | Ontological concept definitions |
| `system_snapshots` | Metadata for saved world snapshots |

---

## Semantic SQL Examples

### Standard SQL (always works)
```sql
SELECT agent_id, role, ROUND(curiosity, 3) AS curiosity
FROM hyper_stigmergy.agents
ORDER BY curiosity DESC;
```

### Semantic operators (require LARS server)
```sql
-- Find beliefs about system plateauing
SELECT * FROM hyper_stigmergy.beliefs
WHERE content MEANS 'system is plateauing or stagnating';

-- Find improvements similar to structural rebalancing
SELECT intent, mutation_type, coherence_after - coherence_before AS delta
FROM hyper_stigmergy.improvement_events
WHERE intent SIMILAR_TO 'structural rebalancing'
ORDER BY delta DESC;

-- Rate each agent's description stability
SELECT agent_id, role,
  ASK('Rate the stability of this description from 0 to 1. Return only the number.', description) AS stability
FROM hyper_stigmergy.agents;

-- Topic clusters in beliefs
SELECT TOPICS(content, 5) AS topic_cluster, COUNT(*) AS n
FROM hyper_stigmergy.beliefs
GROUP BY topic_cluster;

-- Summarize recent improvement intents
SELECT SUMMARIZE(intent) AS summary
FROM hyper_stigmergy.improvement_events
WHERE applied = 1
ORDER BY timestamp DESC
LIMIT 20;
```

---

## Cascades (YAML workflows)

Cascades chain SQL + LLM analysis steps. Run from the project root:

```bash
# Semantic belief analysis
lars cascade run beliefs_semantic --topic 'emergent coordination'

# Improvement pattern analysis
lars cascade run improvement_analysis --intent 'structural rebalancing'

# Agent health report
lars cascade run agent_health

# Belief topic clustering
lars cascade run belief_topics --n_topics 6

# Edge emergence analysis
lars cascade run edge_emergence
```

Cascade files live in `lars/cascades/queries/`.

---

## TUI Slash Commands

| Command | Description |
|---------|-------------|
| `/exportdb` | Export RooDB → DuckDB bridge (runs `export-to-duckdb.py`) |
| `/query <sql>` | Run raw SQL against live RooDB, show results in chat |
| `/lars` | Print LARS connection info and available cascades |

---

## Environment Variables

Override RooDB defaults with environment variables:

```bash
export ROODB_HOST=127.0.0.1
export ROODB_PORT=3307
export ROODB_USER=root
export ROODB_PASS=secret
export ROODB_DB=hyper_stigmergy
```

---

## Files

```
lars/
├── README.md                    this file
├── start-lars.sh                start LARS SQL + Studio
├── sync-db.sh                   manual DB sync (with --watch loop)
├── export-to-duckdb.py          RooDB → DuckDB Python bridge
├── hyper_stigmergy.duckdb       generated bridge file (gitignored)
└── cascades/
    └── queries/
        ├── beliefs_semantic.yaml
        ├── improvement_analysis.yaml
        ├── agent_health.yaml
        ├── belief_topics.yaml
        └── edge_emergence.yaml

~/.lars/
└── sql_connections/
    └── hyper_stigmergy.yaml     LARS connection pointing to DuckDB file
```

---

## Installation

```bash
python3.12 -m pip install larsql --break-system-packages
```

Requires Python ≥ 3.10.  The `export-to-duckdb.py` script auto-installs
`mysql-connector-python` and `duckdb` if missing.
