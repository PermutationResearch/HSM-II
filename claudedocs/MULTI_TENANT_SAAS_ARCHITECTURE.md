# HSM-II Multi-Tenant SaaS Architecture & Dream-Driven Routing

Complete reference for the multi-tenant autonomous business team layer introduced
in `feat: Multi-tenant SaaS layer with dream-driven task routing`.

---

## System Architecture Overview

```text
┌─────────────────────────────────────────────────────────────────────┐
│                          teamd (Axum Server)                        │
│                         port 8788 (default)                         │
├───────────────┬─────────────────────────────────────────────────────┤
│ Public Routes │ Protected Routes (require_tenant_auth middleware)    │
│               │                                                     │
│ POST register │ GET/PUT  brand      POST tasks     GET usage        │
│ POST token    │ GET/PUT  team/:role POST campaigns  GET campaigns   │
│ GET  health   │ POST tasks/:id/outcome  GET campaigns/:id/patterns  │
├───────────────┴─────────────────────────────────────────────────────┤
│                        TeamAppState                                 │
│  ┌──────────────┐  ┌──────────────────┐  ┌──────────────────────┐  │
│  │ TenantRegistry│  │PersistentAuthMgr │  │   UsageTracker       │  │
│  │ (LRU cache)   │  │ (Argon2 + JWT)   │  │ (per-tenant/day)     │  │
│  └──────┬───────┘  └────────┬─────────┘  └──────────┬───────────┘  │
│         │                   │                        │              │
│         ▼                   ▼                        ▼              │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │                    File-Based Persistence                    │   │
│  │  ~/.hsmii/tenants/{id}/    tenant state + orchestrator      │   │
│  │  ~/.hsmii/auth/            api_keys.json, tenants.json      │   │
│  │  ~/.hsmii/usage/           {tenant_id}.json per tenant      │   │
│  └─────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────┘
```

---

## Module Inventory

| Module | File | Lines | Purpose |
|--------|------|-------|---------|
| **Tenant** | `src/tenant.rs` | 598 | Tenant model, registry, LRU cache for orchestrators |
| **Team API** | `src/team_api.rs` | 923 | Axum handlers for all REST endpoints |
| **Dream Advisor** | `src/dream_advisor.rs` | 546 | Converts dream/campaign patterns into routing adjustments |
| **Usage Tracker** | `src/usage_tracker.rs` | 324 | Per-tenant daily counters (API calls, tokens, publishes) |
| **teamd binary** | `src/bin/teamd.rs` | 196 | Server entrypoint — wires everything together |
| **Auth extensions** | `src/auth.rs` (diff) | +290 | `PersistentAuthManager`, `TenantContext`, `require_tenant_auth` |
| **Routing extensions** | `src/autonomous_team.rs` (diff) | +200 | `RoleIntent`, `bid_with_context()`, dream advisor integration |

---

## How It Works: End-to-End Flow

### 1. Tenant Onboarding

```text
Client                         teamd                          Disk
  │                              │                              │
  │── POST /auth/register ──────>│                              │
  │   { name: "Acme Corp",      │── create_tenant() ──────────>│
  │     plan: "pro" }           │── create_tenant_key() ──────>│
  │<── { tenant_id, api_key } ──│<── persist (tenants.json) ───│
  │                              │                              │
  │── POST /auth/token ─────────>│                              │
  │   { api_key: "hsk_..." }    │── validate_key() (Argon2)    │
  │<── { token: "eyJ..." } ─────│── generate_jwt(tenant_id)    │
```

### 2. Task Routing with Dream Feedback

```text
Client                         teamd                     DreamAdvisor
  │                              │                              │
  │── POST /tasks ──────────────>│                              │
  │   { description: "write     │── get_orchestrator() ────────│
  │     a blog post" }          │── route_task(desc) ──────────│
  │                              │     for each member:         │
  │                              │       bid_with_context() ───>│
  │                              │       ← keyword + dream +    │
  │                              │         intent score         │
  │<── { assigned_to: "Writer", │                              │
  │     bid_score: 0.82,        │                              │
  │     system_prompt: "..." }  │                              │
  │                              │                              │
  │  ... execute task externally ...                            │
  │                              │                              │
  │── POST /tasks/:id/outcome ──>│                              │
  │   { success: true,          │── record_outcome() ──────────│
  │     quality: 0.9,           │── refresh_dream_advisor() ──>│
  │     role: "writer" }        │     campaign_patterns ──────>│
  │<── { dream_advisor_gen: 3 } │     └─> ingest + recompute   │
```

### 3. Dream Consolidation Integration

```text
Dream Engine                  TeamOrchestrator            DreamAdvisor
  │                              │                              │
  │── CrystallizedPatterns ─────>│                              │
  │   with role_affinity map    │── ingest_dream_patterns() ──>│
  │                              │     for each pattern:        │
  │                              │       affinity × valence ×   │
  │                              │       confidence × persist   │
  │                              │       → EMA update           │
  │                              │     expand keywords ────────>│
  │                              │     recompute aggregates ───>│
  │                              │<── generation++ ─────────────│
```

---

## API Endpoints Reference

### Public (No Authentication)

| Method | Path | Request Body | Response | Purpose |
|--------|------|-------------|----------|---------|
| `POST` | `/api/v1/auth/register` | `{ name, plan? }` | `{ tenant_id, api_key, plan }` | Create tenant + admin key |
| `POST` | `/api/v1/auth/token` | `{ api_key }` | `{ token, expires_in: 86400 }` | Exchange API key for JWT |
| `GET` | `/health` | — | `{ status: "ok", version }` | Health check |

### Protected (Require `Bearer <JWT>` with `tenant_id`)

| Method | Path | Request Body | Response | Permission | Purpose |
|--------|------|-------------|----------|------------|---------|
| `GET` | `/api/v1/team` | — | `{ members[] }` | Read | List all 14 role agents |
| `GET` | `/api/v1/team/:role` | — | `{ role, persona, stats, prompt }` | Read | Inspect a specific agent |
| `PUT` | `/api/v1/team/:role/status` | `{ status }` | `{ message }` | Write/Admin | Enable/disable an agent |
| `GET` | `/api/v1/brand` | — | `{ name, positioning, voice, ... }` | Read | Get brand context |
| `PUT` | `/api/v1/brand` | `{ name?, positioning?, ... }` | `{ message }` | Write/Admin | Update brand (partial) |
| `POST` | `/api/v1/tasks` | `{ description, priority?, domain? }` | `{ task_id, assigned_to, bid_score, system_prompt }` | Write/Admin | Route task to best agent |
| `POST` | `/api/v1/tasks/:id/outcome` | `{ domain, success, quality, role }` | `{ status, dream_advisor_gen }` | Write/Admin | Record outcome, refresh dream |
| `POST` | `/api/v1/campaigns` | `{ name, goal, channels[] }` | `{ campaign_id, status }` | Write/Admin | Create marketing campaign |
| `GET` | `/api/v1/campaigns` | — | `{ campaigns[] }` | Read | List all campaigns |
| `GET` | `/api/v1/campaigns/:id` | — | `{ campaign, snapshot }` | Read | Campaign detail + metrics |
| `GET` | `/api/v1/campaigns/:id/patterns` | — | `{ patterns[] }` | Read | Extract dream patterns |
| `GET` | `/api/v1/usage` | — | `{ api_calls_today, llm_tokens, ... }` | Read | Usage & billing data |

---

## Tenant Plan Tiers

| Capability | Free | Starter | Pro | Enterprise |
|-----------|------|---------|-----|------------|
| **Team Members** | 5 | 10 | 14 (all) | 14 (all) |
| **Concurrent Campaigns** | 2 | 10 | 50 | 500 |
| **API Calls / Day** | 100 | 1,000 | 10,000 | 100,000 |
| **LLM Provider Override** | No | No | No | Yes |
| **Dream Advisor** | Yes | Yes | Yes | Yes |
| **Brand Context** | Yes | Yes | Yes | Yes |
| **Usage Analytics** | Yes | Yes | Yes | Yes |

---

## The 14 Business Role Agents

| Role | Label | Tag | Intent | Proactivity | Activation Keywords (sample) |
|------|-------|-----|--------|-------------|------------------------------|
| CEO | Chief Executive Officer | `[CEO]` | Strategy | 0.9 | vision, strategy, leadership, direction |
| CTO | Chief Technology Officer | `[CTO]` | Strategy | 0.8 | tech, architecture, infrastructure, scale |
| CFO | Chief Financial Officer | `[CFO]` | Strategy | 0.7 | budget, revenue, cost, financial, ROI |
| CMO | Chief Marketing Officer | `[CMO]` | Strategy | 0.8 | brand, marketing, campaign, growth, audience |
| COO | Chief Operations Officer | `[COO]` | Strategy | 0.7 | operations, process, efficiency, workflow |
| Developer | Developer | `[DEV]` | Execution | 0.6 | code, implement, build, debug, API, feature |
| Designer | Designer | `[DSG]` | Execution | 0.6 | design, UI, UX, visual, mockup, wireframe |
| Writer | Content Writer | `[WRT]` | Execution | 0.5 | write, blog, copy, content, docs, article |
| Marketer | Marketer | `[MKT]` | Execution | 0.6 | social, campaign, ads, SEO, growth, content |
| Analyst | Business Analyst | `[ANL]` | Support | 0.5 | data, analysis, metrics, report, insight |
| Support | Customer Support | `[SUP]` | Support | 0.4 | support, help, ticket, customer, issue |
| HR | Human Resources | `[HR]` | Support | 0.3 | hiring, culture, team, onboarding, policy |
| Sales | Sales | `[SAL]` | Support | 0.5 | sales, deal, pipeline, prospect, close |
| Legal | Legal Counsel | `[LGL]` | Support | 0.3 | legal, compliance, contract, terms, risk |

---

## RoleIntent System: How Strategy vs Execution Routing Works

The `RoleIntent` classification prevents misrouted tasks — a CEO should not win
a bid for "write a blog post" even if marketing keywords match.

### Intent Classification

| Intent | Roles | Task Fit: Execution | Task Fit: Strategy | Task Fit: Neutral |
|--------|-------|--------------------|--------------------|-------------------|
| **Strategy** | CEO, CMO, CFO, COO, CTO | 0.2 (penalized) | 0.9 (boosted) | 0.5 |
| **Execution** | Developer, Designer, Writer, Marketer | 0.9 (boosted) | 0.2 (penalized) | 0.5 |
| **Support** | Analyst, Support, HR, Sales, Legal | 0.5 (neutral) | 0.5 (neutral) | 0.5 |

### Execution Task Detectors

Keywords: `write`, `build`, `create`, `implement`, `code`, `design`, `draft`, `publish`, `ship`, `fix`, `deploy`, `develop`, `produce`, `make`, `configure`, `test`, `debug`

### Strategy Task Detectors

Keywords: `strategy`, `plan`, `decide`, `evaluate`, `assess`, `review`, `prioritize`, `vision`, `roadmap`, `direction`, `allocate`, `approve`, `budget`

---

## Bid Formula: How Task Routing Decides

### Enhanced Formula (with DreamAdvisor active)

| Signal | Weight | Source | Range |
|--------|--------|--------|-------|
| `keyword_score` | 0.35 | Static activation keywords + dream-expanded keywords | [0, 1] |
| `proactivity` | 0.15 | Role's default proactivity level | [0, 1] |
| `domain_bonus` | 0.15 | Historical performance in this domain | [0, 1] |
| `dream_signal` | 0.15 | DreamAdvisor lookup: (role, domain) → adjustment | [0, 1] |
| `intent_modifier` | 0.10 | RoleIntent.task_fit(is_execution, is_strategy) | [0, 1] |
| `noise` | 0.10 | Random tiebreaker | [0, 0.1] |

**Total**: `(kw×0.35 + pro×0.15 + dom×0.15 + dream×0.15 + intent×0.10 + noise×0.10).clamp(0, 1)`

### Original Formula (no DreamAdvisor — backward compatible)

| Signal | Weight | Source | Range |
|--------|--------|--------|-------|
| `keyword_score` | 0.50 | Static activation keywords only | [0, 1] |
| `proactivity` | 0.20 | Role's default proactivity level | [0, 1] |
| `domain_bonus` | 0.20 | Historical performance | [0, 1] |
| `noise` | 0.10 | Random tiebreaker | [0, 0.1] |

---

## DreamAdvisor: How Dream Patterns Improve Routing

### Data Flow

| Stage | Input | Transform | Output |
|-------|-------|-----------|--------|
| **Campaign Feedback** | `(domain, narrative, valence)` from CampaignStore | Keyword relevance × valence × 0.5 dampening | Per-(role, domain) adjustment via EMA |
| **Crystallized Dreams** | `CrystallizedPattern` with `role_affinity` | affinity × valence × confidence × persistence | Per-(role, domain) adjustment via EMA |
| **Keyword Expansion** | `associated_task_keys` from dream motifs | Append to role's keyword vocabulary | More keyword hits in bid calculation |
| **Aggregation** | All per-(role, domain) entries | Average per role | Fallback when domain key doesn't match |

### Exponential Moving Average (EMA)

All adjustments use `entry = old × 0.7 + new × 0.3` to avoid volatile swings
from single data points while still adapting over time.

### Quality Gates

| Filter | Threshold | Effect |
|--------|-----------|--------|
| Pattern confidence | < 0.3 | Rejected entirely |
| Persistence score | < 0.1 | Rejected entirely |
| Affinity magnitude | < 0.01 | Skipped for that role |
| Adjustment range | [-1.0, 1.0] | Clamped after EMA |

### Query Performance

| Operation | Complexity | Benchmark |
|-----------|-----------|-----------|
| `advise(role, task_keys)` | O(task_keys.len()) HashMap lookups | 14 calls < 5ms (100 patterns loaded) |
| `expanded_keyword_hits()` | O(expanded_keywords.len()) | Negligible |
| `ingest_campaign_patterns()` | O(patterns × 14 roles × keywords) | Background, not in hot path |

---

## Multi-Tenant Isolation

### What's Isolated Per Tenant

| Resource | Isolation Level | Storage |
|----------|----------------|---------|
| **TeamOrchestrator** | Full — separate instance per tenant | `~/.hsmii/tenants/{id}/` |
| **14 Role Agents** | Full — separate state, personas, history | In orchestrator |
| **Brand Context** | Full — name, voice, positioning, values | `brand.json` per tenant |
| **Campaign Store** | Full — campaigns, metrics, snapshots | `campaigns.json` per tenant |
| **DreamAdvisor** | Full — routing adjustments, keywords | `dream_advisor.json` per tenant |
| **Social Memory** | Full — in-memory per orchestrator | Per tenant lifecycle |
| **API Keys** | Scoped — JWT contains `tenant_id` | `api_keys.json` (shared store) |
| **Usage Counters** | Full — per-tenant daily counters | `~/.hsmii/usage/{tenant_id}.json` |

### LRU Cache Strategy

| Parameter | Value | Purpose |
|-----------|-------|---------|
| Default capacity | 100 orchestrators | Memory-bound tenant count |
| Eviction | LRU (least recently used) | Hot tenants stay in memory |
| Miss penalty | ~5ms (disk load) | Transparent reload |
| Write-through | Every mutation persisted to disk | Safe eviction |

---

## Auth System Architecture

### Key Lifecycle

| Step | Component | Detail |
|------|-----------|--------|
| 1. Create | `PersistentAuthManager::create_tenant_key()` | Generates `hsk_<uuid>`, hashes with Argon2 |
| 2. Exchange | `validate_key()` | Verifies Argon2 hash, returns JWT with `tenant_id` |
| 3. Authorize | `require_tenant_auth` middleware | Validates JWT, extracts `TenantContext`, checks rate limit |
| 4. Revoke | `revoke_key()` | Marks inactive, persists to disk |

### JWT Claims

| Field | Type | Purpose |
|-------|------|---------|
| `sub` | String | Key ID |
| `key_id` | String | Key ID (redundant for compat) |
| `permissions` | Vec<Permission> | Read, Write, Admin |
| `tenant_id` | Option<String> | Tenant scope (None for legacy tokens) |
| `iat` | i64 | Issued at (Unix timestamp) |
| `exp` | i64 | Expires at (24h from issue) |

### Backward Compatibility

| Scenario | Behavior |
|----------|----------|
| Legacy JWT without `tenant_id` | `#[serde(default)]` → deserializes as `None` |
| Legacy `ApiKey` without `tenant_id` | `#[serde(default)]` → `None` |
| `require_auth` middleware | Works unchanged — `tenant_id` is optional |
| `require_tenant_auth` middleware | **Rejects** tokens without `tenant_id` (403 Forbidden) |

---

## Usage Tracking

### Tracked Metrics

| Metric | Granularity | Aggregation Available |
|--------|-------------|----------------------|
| **API Calls** | Per day (`YYYY-MM-DD`) | Today, This Month |
| **LLM Tokens** | Per day | This Month |
| **Channel Publishes** | Per day | This Month |

### Lifecycle

| Operation | Trigger | Detail |
|-----------|---------|--------|
| **Record** | Every API handler call | `record_api_call()` increments in-memory counter |
| **Flush** | Background loop (default 300s) | Writes all counters to `~/.hsmii/usage/` |
| **Load** | Server startup | Reads existing JSON files |
| **Prune** | Manual call | Removes entries older than N days |
| **Check Limit** | `check_daily_limit(tenant_id, max)` | Returns bool — within quota |

---

## What Users Can Do With This

### For SaaS Operators

| Capability | How | API |
|-----------|-----|-----|
| **Onboard new customers** | Register tenant, distribute API key | `POST /auth/register` |
| **Tier customers by plan** | Free/Starter/Pro/Enterprise limits | `plan` field at registration |
| **Upgrade/downgrade plans** | `TenantRegistry::update_plan()` | Programmatic (no API yet) |
| **Monitor usage** | Daily/monthly counters per tenant | `GET /usage` |
| **Enforce rate limits** | Automatic via `check_daily_limit()` | Built into middleware |
| **Delete tenants** | Full cleanup: cache + disk + registry | `TenantRegistry::delete_tenant()` |

### For End Users (Tenant API Consumers)

| Capability | How | API |
|-----------|-----|-----|
| **Get an AI team instantly** | Register → get 14 role agents ready | `POST /auth/register` |
| **Route tasks to the best agent** | Describe task, get optimal agent + system prompt | `POST /tasks` |
| **Configure brand identity** | Set name, voice, positioning, forbidden words | `PUT /brand` |
| **Run marketing campaigns** | Create campaign, set channels, track metrics | `POST /campaigns` |
| **Enable/disable agents** | Toggle roles on/off as needed | `PUT /team/:role/status` |
| **Inspect agent capabilities** | See keywords, persona, reliability, prompt | `GET /team/:role` |
| **Report task outcomes** | Close the feedback loop for smarter routing | `POST /tasks/:id/outcome` |
| **View campaign analytics** | Snapshot with CTR, CAC, sentiment | `GET /campaigns/:id` |
| **Extract dream patterns** | See what the dream engine learned from campaigns | `GET /campaigns/:id/patterns` |
| **Check billing/usage** | API calls, tokens, publishes for the month | `GET /usage` |

### For Developers Building On Top

| Capability | How | Leverage Point |
|-----------|-----|----------------|
| **LLM-agnostic task execution** | Use `system_prompt` from routing response as LLM input | `TaskResponse.system_prompt` |
| **Custom routing** | Feed `bid_with_context()` a custom DreamAdvisor | `TeamMember::bid_with_context()` |
| **Dream engine integration** | Pipe `CrystallizedPattern`s to improve routing | `ingest_dream_patterns()` |
| **Custom channels** | Implement `ChannelConnector` trait | `ChannelConnector` in autonomous_team |
| **Programmatic tenant mgmt** | Use `TenantRegistry` directly in Rust | Full API surface |
| **Webhook on task outcome** | Extend `record_task_outcome` handler | Add webhook dispatch |
| **Custom evaluators** | Add Axum middleware after routing | Standard Axum patterns |

---

## How This Leverages Existing HSM-II Systems

| HSM-II Component | What This Layer Uses | How It's Leveraged |
|-----------------|---------------------|-------------------|
| **TeamOrchestrator** | Full orchestrator per tenant | Each tenant gets isolated 14-agent team with brand, campaigns, social memory |
| **BusinessRole (14 roles)** | Role personas, keywords, proactivity | Drives task routing via keyword matching and bid calculation |
| **BrandContext** | Per-tenant brand identity | System prompts incorporate brand voice, positioning, values, forbidden words |
| **CampaignStore** | Per-tenant campaign lifecycle | Campaigns track metrics → feed `extract_dream_patterns()` → dream loop |
| **SocialMemory** | Per-tenant community signal tracking | Isolated social memory per orchestrator instance |
| **Dream Engine (CrystallizedPattern)** | Offline dream consolidation | Patterns with `role_affinity` directly feed DreamAdvisor routing table |
| **Auth System (Argon2 + JWT)** | Extended with `tenant_id` in claims | Multi-tenant tokens, persistent key storage, rate limiting |
| **ChannelConnector trait** | Per-tenant publish channels | Blog, Twitter, Reddit, HackerNews, Email, LinkedIn, ProductHunt |
| **Persona system** | Per-role persona configuration | Each agent has name, capabilities, voice → system prompt generation |
| **system_prompt_for()** | Per-role + brand context prompt | API returns ready-to-use LLM system prompt for the winning agent |

---

## File Persistence Layout

```
~/.hsmii/
├── auth/
│   ├── api_keys.json          # All API keys (hashed) across tenants
│   └── tenants.json           # Tenant registry (id, name, plan, settings)
├── tenants/
│   ├── {tenant-uuid-1}/
│   │   ├── brand.json         # Brand context
│   │   ├── campaigns.json     # Campaign store
│   │   ├── team_members.json  # Agent state, history, reliability
│   │   └── dream_advisor.json # Dream routing adjustments
│   └── {tenant-uuid-2}/
│       └── ...
└── usage/
    ├── {tenant-uuid-1}.json   # Daily API/token/publish counters
    └── {tenant-uuid-2}.json
```

---

## Quick Start Example

```bash
# 1. Start the server
cargo run --bin teamd

# 2. Register a tenant
curl -X POST http://localhost:8788/api/v1/auth/register \
  -H 'Content-Type: application/json' \
  -d '{"name": "Acme Corp", "plan": "pro"}'
# → { "tenant_id": "abc-123", "api_key": "hsk_...", ... }

# 3. Get a JWT
curl -X POST http://localhost:8788/api/v1/auth/token \
  -H 'Content-Type: application/json' \
  -d '{"api_key": "hsk_..."}'
# → { "token": "eyJ...", "expires_in": 86400 }

# 4. Route a task
curl -X POST http://localhost:8788/api/v1/tasks \
  -H 'Authorization: Bearer eyJ...' \
  -H 'Content-Type: application/json' \
  -d '{"description": "Write a blog post about our new AI feature"}'
# → { "assigned_to": "Content Writer", "bid_score": 0.82, "system_prompt": "..." }

# 5. Report outcome (feeds dream loop)
curl -X POST http://localhost:8788/api/v1/tasks/task-id/outcome \
  -H 'Authorization: Bearer eyJ...' \
  -H 'Content-Type: application/json' \
  -d '{"domain": "blog", "success": true, "quality": 0.9, "role": "writer"}'
# → { "status": "recorded", "dream_advisor_generation": 1 }
```

---

## Test Coverage

| Module | Tests | Coverage Areas |
|--------|-------|---------------|
| `tenant.rs` | 8 tests | Plan defaults, LRU cache, CRUD, persistence roundtrip, plan updates |
| `team_api.rs` | 5 tests | Role parsing, status parsing, channel parsing, task routing, brand roundtrip |
| `dream_advisor.rs` | 8 tests | Empty advisor, positive/negative campaigns, crystallized patterns, quality gates, aggregation fallback, performance (< 5ms), serde roundtrip, disk persistence |
| `usage_tracker.rs` | 7 tests | API calls, LLM tokens, publishes, daily limits, flush/reload, unknown tenant, pruning |
| `auth.rs` (new) | 3 tests | Tenant key creation/validation, tenant key listing, backward-compatible claims deserialization |

**Total: 31 new tests across the 5 modules.**
