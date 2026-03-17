# HSM-II: Complete System Reference

**Hyper-Stigmergic Morphogenesis II** — A federated multi-agent hypergraph system
with emergent collective intelligence. Agents are LLM-powered, trails are
hypergraph edges, decisions emerge from stigmergic consensus and dream consolidation.

> *"Ants solving problems through pheromone trails"* — except the ants are AI
> agents, the pheromones are typed hyperedges, and the colony learns in its sleep.

---

## Table of Contents

1. [System Architecture](#1-system-architecture)
2. [Core Engine: Hypergraph + Stigmergy](#2-core-engine-hypergraph--stigmergy)
3. [Agent System](#3-agent-system)
4. [Council Decision System](#4-council-decision-system)
5. [Consensus & Emergent Association](#5-consensus--emergent-association)
6. [Dream Consolidation Engine](#6-dream-consolidation-engine)
7. [Dynamic Kinetic Stability (DKS)](#7-dynamic-kinetic-stability-dks)
8. [MiroFish Business Decision Engine](#8-mirofish-business-decision-engine)
9. [Autonomous Business Team](#9-autonomous-business-team)
10. [Multi-Tenant SaaS Layer](#10-multi-tenant-saas-layer)
11. [Knowledge & Skill Systems](#11-knowledge--skill-systems)
12. [Communication & Coordination](#12-communication--coordination)
13. [LLM Integration](#13-llm-integration)
14. [Tool System](#14-tool-system)
15. [Personal Agent](#15-personal-agent)
16. [External Gateways](#16-external-gateways)
17. [Federation](#17-federation)
18. [Governance & Security](#18-governance--security)
19. [Observability & Metrics](#19-observability--metrics)
20. [GPU Acceleration](#20-gpu-acceleration)
21. [Server Binaries](#21-server-binaries)
22. [Cross-System Leverage Map](#22-cross-system-leverage-map)
23. [Feedback Loops](#23-feedback-loops)
24. [What Users Can Do](#24-what-users-can-do)

---

## 1. System Architecture

### High-Level Topology

```text
┌──────────────────────────────────────────────────────────────────────────────┐
│                          USER-FACING LAYER                                   │
│                                                                              │
│  personal_agent ──── tui_codex_demo ──── teamd (REST API) ──── gateways     │
│  (TUI assistant)     (Codex-style UI)    (Multi-tenant SaaS)  (Discord/TG)  │
└────────────┬──────────────┬────────────────────┬────────────────┬────────────┘
             │              │                    │                │
┌────────────▼──────────────▼────────────────────▼────────────────▼────────────┐
│                         ORCHESTRATION LAYER                                  │
│                                                                              │
│  conductord ──── council (debate/orchestrate/simple/ralph) ──── scheduler    │
│  (Optimization)  (Multi-mode decisions)                        (Cron jobs)   │
└────────────┬──────────────┬────────────────────┬────────────────┬────────────┘
             │              │                    │                │
┌────────────▼──────────────▼────────────────────▼────────────────▼────────────┐
│                         INTELLIGENCE LAYER                                   │
│                                                                              │
│  dream engine ── DKS ── CASS ── autocontext ── mirofish ── optimize_anything│
│  (Pattern learn)  (Evolution) (Skills) (Closed-loop)  (Decisions)  (GEPA)   │
└────────────┬──────────────┬────────────────────┬────────────────┬────────────┘
             │              │                    │                │
┌────────────▼──────────────▼────────────────────▼────────────────▼────────────┐
│                         FOUNDATION LAYER                                     │
│                                                                              │
│  hypergraphd ──── stigmergic_policy ──── social_memory ──── federation      │
│  (Knowledge graph)  (Trace policy)        (Reputation)       (Cross-system)  │
│                                                                              │
│  llm (multi-provider) ── tools (60+) ── auth ── vault ── reasoning_braid   │
└──────────────────────────────────────────────────────────────────────────────┘
```

### Module Count by Domain

| Domain | Modules | Key Files |
|--------|---------|-----------|
| Core Graph & Storage | 12 | hypergraph, database, embedded_graph_store, property_graph, meta_graph, query_engine, columnar_engine, disk_backed_vector_index, hnsw_index, embedding_index, transaction_layer, vault |
| Agent Architecture | 8 | agent, agent_core/, personal/, coder_assistant/, action, skill, reward, world_controller |
| Decision & Council | 6 | council/ (8 sub), consensus, ouroboros_compat/ (5 phases), optimize_anything/ (3 sub) |
| Learning & Dreams | 7 | dream/ (7 sub), autocontext/ (5 sub), dks/ (7 sub), cass/ (3 sub), dspy, dspy_session, rlm_v2/ |
| Communication | 4 | communication/ (4 sub), gateways/ (2 sub), email/ (4 sub), external_connectors |
| Business Operations | 7 | autonomous_team, mirofish, scenario_simulator, onboard, tenant, team_api, usage_tracker |
| Infrastructure | 10 | llm/ (5 sub), tools/ (14 sub), auth, scheduler/ (1 sub), observability, metrics, flags/, pi_tools, pi_ai_compat/ |
| Specialized Engines | 7 | kuramoto, reasoning_braid, prolog_engine, prolog_embedding_bridge, investigation_engine, investigation_tools, navigation/ |
| Runtime | 5 | loop_main, conductor, graph_runtime, batch_runner, workflow |
| **Total** | **~82 modules** | **200+ source files** |

---

## 2. Core Engine: Hypergraph + Stigmergy

The foundation is an **embedding-augmented hypergraph** where vertices and edges
carry semantic embeddings, enabling similarity search over the knowledge structure.

### Vertex Types

| VertexKind | What It Represents | Created By |
|------------|-------------------|------------|
| `Agent` | An AI agent in the system | Agent registration |
| `Tool` | An available capability | Tool registry |
| `Memory` | A stored fact or observation | Memory system |
| `Task` | A unit of work | Task routing |
| `Property` | A data attribute | Graph operations |
| `Ontology` | A category/concept | Knowledge engineering |
| `Belief` | A held belief (with confidence) | Onboarding, inference |
| `Experience` | A recorded experience | Dream consolidation |

### HyperEdge Properties

| Property | Type | Purpose |
|----------|------|---------|
| `embedding` | `Vec<f64>` | Semantic vector for similarity search |
| `scope` | `EdgeScope` | Local, Federated, or Global |
| `provenance` | `String` | Which system created it |
| `trust_tags` | `Vec<String>` | Federation trust markers |
| `origin_system` | `String` | Source system for federation |

### What This Enables

| Capability | How | User Benefit |
|-----------|-----|-------------|
| **Semantic search** | Embedding cosine similarity over vertices/edges | Find related concepts without exact keyword match |
| **Cross-domain transfer** | Hyperedges connect vertices across domains | Insights from marketing applied to product decisions |
| **Federated knowledge** | Scope + provenance + trust tags | Multiple HSM-II instances share knowledge safely |
| **Stigmergic trails** | Edges accumulate "pheromone" from agent activity | Popular/successful paths strengthen over time |
| **Temporal patterns** | Dream engine reads edge creation sequences | System learns what action sequences work |

---

## 3. Agent System

### The 6 Council Roles

| Role | Archetype | Drive Profile | Specialization |
|------|-----------|---------------|----------------|
| **Architect** | System designer | High harmony | Integration, structure, system design |
| **Catalyst** | Change agent | High growth | Innovation, bridge-building, transformation |
| **Chronicler** | Historian | High transcendence | Pattern recognition, historical context |
| **Critic** | Skeptic | High curiosity | Evidence rigor, failure mode analysis |
| **Explorer** | Scout | High curiosity | Novelty seeking, boundary probing |
| **Coder** | Builder | High growth | Code implementation, tool execution |

### Agent Properties

| Property | Type | Purpose |
|----------|------|---------|
| `id` | `AgentId (u64)` | Unique identifier |
| `drives` | `Drives` | 4 motivational dimensions (curiosity, harmony, growth, transcendence) |
| `learning_rate` | `f64` | Adaptation speed |
| `role` | `Role` | Council role assignment |
| `bid_bias` | `f64` | Consensus voting weight |
| `jw` | `f64` | Thermodynamic Wage (JW = E × η × W) |

### Thermodynamic Wage (JW) Metric

The JW metric is inspired by thermodynamic work — agents are rewarded based on:

| Component | Symbol | Meaning |
|-----------|--------|---------|
| Energy | E | Agent's coherence contribution |
| Efficiency | η | Network degree utilization |
| Work | W | Actual output quality |

**Formula**: `JW = coherence_contribution × network_efficiency × output_quality`

**Used for**: Agent compensation, reputation weighting, bid priority

---

## 4. Council Decision System

The council provides **three coordination modes** selected automatically based on
decision complexity, urgency, and agent availability.

### Council Modes

| Mode | When Used | Process | Duration |
|------|-----------|---------|----------|
| **Simple** | Routine decisions, low complexity | Direct voting, majority wins | Fast (1 round) |
| **Debate** | Complex decisions, multiple trade-offs | Structured pros/cons, rebuttals, synthesis | Medium (3-5 rounds) |
| **Orchestrate** | Urgent + complex, hierarchical tasks | Commander delegates sub-tasks, coordinates | Varies |
| **RALPH** | Research-backed decisions | Research → Analyze → Leverage → Plan → Harvest | Deep (5 phases) |
| **LLM Deliberation** | Nuanced judgment needed | LLM-powered multi-stance debate with phases | Medium-deep |

### Mode Selection Algorithm

| Factor | Weight | Triggers |
|--------|--------|----------|
| `complexity` | 0.4 | > 0.7 → Debate or Orchestrate |
| `urgency` | 0.3 | > 0.8 → Orchestrate (speed) |
| `agent_count` | 0.2 | < 3 → Simple; > 5 → Debate |
| `required_roles` | 0.1 | Specialized roles → match mode to roles |

### Council Evidence Types

| Evidence Kind | Source | Used In |
|---------------|--------|---------|
| `StigmergicTrace` | Policy execution history | All modes |
| `AgentBid` | Role-weighted votes | Simple, Debate |
| `DebateArgument` | Structured pros/cons | Debate |
| `SubTaskResult` | Delegated work output | Orchestrate |
| `RalphFinding` | Research-backed insights | RALPH |

### What Users Get From Councils

| Use Case | Council Mode | Output |
|----------|-------------|--------|
| "Should we launch this feature?" | Debate | Structured pros/cons from all roles, synthesis |
| "Execute this 5-step plan" | Orchestrate | Delegated sub-tasks with progress tracking |
| "Is this API design good?" | Simple | Quick vote with rationale from each role |
| "Deep-dive: market entry strategy" | RALPH | Research report with evidence chains |

---

## 5. Consensus & Emergent Association

### How Consensus Works

```text
Agent bids → Anti-Majority Correlation Penalty (ACPO) → Bayesian Utility Score → Verdict
```

| Stage | Mechanism | Purpose |
|-------|-----------|---------|
| **1. Agent Bidding** | Each agent scores the proposal using role-weighted bid | Diverse perspectives |
| **2. ACPO Penalty** | Pairwise cosine correlation detects groupthink | Prevents echo chambers |
| **3. Utility Scoring** | `(AssociationCount × CoherenceDelta) / AgentDiversity` | Measures real value |
| **4. Verdict** | > 0.7 → Promote; 0.3-0.7 → Maintain; < 0.3 → Suspend | Lifecycle management |

### 9 Types of Emergent Associations

| Association Type | What It Detects | Example |
|-----------------|----------------|---------|
| `BridgeFormation` | New connections between clusters | Marketing insights reaching engineering |
| `BeliefResolution` | Conflicting beliefs resolved | "We should/shouldn't use microservices" → resolved |
| `RiskMitigation` | Identified and mitigated risks | Security vulnerability found and patched |
| `ClusterEmergence` | New conceptual clusters forming | New product category emerging from data |
| `CrossDomainTransfer` | Skills moving between domains | Customer support patterns improving sales |
| `IdentityBridge` | Self-referencing cycle broken | Circular dependency resolved |
| `CrossSystemConsensus` | Multiple federated systems agree | Cross-org alignment on API standards |
| `CrossSystemSynthesis` | Multi-system knowledge fusion | Combined insights from 3 org instances |
| `FederatedCluster` | Cross-system pattern emerges | Industry-wide trend detected |

### Skill Lifecycle (4-State)

```text
Active ──(utility > 0.7)──→ Advanced
  │                            │
  │←──(revival attempt)──── Suspended ←──(utility < 0.3)──│
  │                                                        │
  └──(long-term unused)──→ Deprecated                      │
                              └────────────────────────────┘
```

---

## 6. Dream Consolidation Engine

The dream engine runs **offline experience replay** — like mammalian sleep
consolidation but for an AI agent system.

### Dream Pipeline (6 Stages)

| Stage | Process | Input → Output |
|-------|---------|----------------|
| **1. Trajectory Assembly** | Sequence traces chronologically with outcome tags | Raw traces → Ordered sequences |
| **2. Temporal Motif Detection** | Sliding window + feature-space clustering | Sequences → Recurring patterns |
| **3. Crystallization** | Compress motif clusters into transferable patterns | Clusters → CrystallizedPatterns |
| **4. Stigmergic Deposition** | Write DreamTrail hyperedges, boost/decay traces | Patterns → Hypergraph updates |
| **5. DKS Survival Pressure** | Only persistent patterns survive | Weak patterns → Pruned |
| **6. Proto-Skill Generation** | Promote high-confidence patterns to CASS | Mature patterns → Usable skills |

### CrystallizedPattern Properties

| Property | Type | Purpose |
|----------|------|---------|
| `narrative` | String | Human-readable description of the pattern |
| `embedding` | Vec<f64> | Semantic vector for similarity matching |
| `motif` | TemporalMotif | Trace sequence template with transition weights |
| `valence` | f64 | Positive/negative outcome association |
| `confidence` | f64 | Observation-based reliability |
| `observation_count` | u64 | How many times seen |
| `role_affinity` | HashMap<Role, f64> | Which roles are most associated |
| `persistence_score` | f64 | DKS survival metric |
| `temporal_reach` | u64 | How far back the pattern's eligibility trace extends |

### Dream Configuration Defaults

| Parameter | Default | Controls |
|-----------|---------|----------|
| `dream_interval` | 50 ticks | How often dreams run |
| `replay_horizon` | 200 ticks | How far back to look |
| `motif_window_size` | 8 | Sliding window size |
| `cluster_threshold` | 0.7 | Similarity for grouping |
| `min_observations` | 3 | Before crystallizing |
| `eligibility_lambda` | 0.9 | Trace decay rate |
| `max_patterns` | 256 | Hard pattern limit |
| `pattern_decay_rate` | 0.02 | DKS pressure |
| `positive_trace_boost` | 0.15 | Reinforcement strength |
| `negative_trace_decay` | 0.10 | Penalty strength |
| `proto_skill_confidence_threshold` | 0.7 | Skill promotion gate |
| `proto_skill_min_observations` | 5 | Skill maturation minimum |

### What Dreams Accomplish

| Outcome | Mechanism | User Benefit |
|---------|-----------|-------------|
| **Routing improvement** | DreamAdvisor adjusts task routing weights | Better agent-task matching over time |
| **Skill discovery** | Proto-skills promoted to CASS | New capabilities emerge from experience |
| **Failure avoidance** | Negative valence patterns penalize bad paths | System avoids repeating mistakes |
| **Knowledge compression** | 200 ticks of traces → handful of patterns | Efficient long-term memory |
| **Cross-role learning** | Role affinity map transfers insights | Explorer's discovery helps Architect's design |

---

## 7. Dynamic Kinetic Stability (DKS)

Inspired by origin-of-life chemistry: entities that persist do so not because
they're "fit" but because they're **kinetically stable** — hard to destroy.

### DKS Entity Properties

| Property | Type | Purpose |
|----------|------|---------|
| Self-replication | Replicator with Metabolism | Entities reproduce using energy/resources |
| Far-from-equilibrium | Flux-driven energy flow | System maintains ordered state |
| Persistence selection | Not fitness — stability | Stable patterns survive, fragile ones die |
| Complexity generation | MultifractalSpectrum | Non-equilibrium dynamics create new structures |

### How DKS Applies to HSM-II

| DKS Concept | HSM-II Implementation | Effect |
|-------------|----------------------|--------|
| **Replicator** | Successful patterns in dream engine | Good strategies reproduce |
| **Decay** | `pattern_decay_rate: 0.02` | Unused patterns fade |
| **Selection pressure** | `persistence_score` threshold | Only robust patterns survive |
| **Energy flow** | Agent JW metrics + task outcomes | Active contribution = energy |
| **Stigmergic entity** | Patterns + reputation + skills | Compound entities evolve |

### DKS Submodules

| Submodule | Purpose |
|-----------|---------|
| `flux` | Energy and resource flow between entities |
| `multifractal` | Complexity spectrum analysis |
| `population` | Population dynamics and statistics |
| `replicator` | Self-replication mechanics |
| `selection` | Persistence-based selection pressure |
| `stigmergic_entity` | Integration with stigmergic fields |

---

## 8. MiroFish Business Decision Engine

A trajectory-based prediction engine for business decision support.

### Core Concepts

| Concept | Implementation | Purpose |
|---------|---------------|---------|
| **Trajectory** | `Vec<TrajectoryStep>` | Step-by-step action plan from current state to goal |
| **Probability Flow Network** | `FlowState` + `FlowTransition` | Bayesian state machine for outcome modeling |
| **Projection Curve** | `Vec<ProjectionPoint>` | Time-series visualization of probability evolution |
| **Domain Templates** | Pre-built scenario structures | Quick-start for common business decisions |
| **Confidence Recalibration** | Back-testing against outcomes | Self-correcting predictions |

### TrajectoryStep Properties

| Property | Type | Purpose |
|----------|------|---------|
| `action` | String | What to do |
| `expected_outcome` | String | What should happen |
| `time_horizon` | String | When (e.g., "Q2 2026") |
| `success_probability` | f64 | Likelihood of success |
| `resources` | Vec<String> | What's needed |
| `depends_on` | Vec<usize> | Prerequisites (step indices) |
| `risks` | Vec<String> | What could go wrong |

### Probability Flow Network

```text
State A (p=0.6) ──[trigger: "campaign launch", p=0.7]──→ State B (p=0.42)
       │                                                       │
       └──[trigger: "competitor response", p=0.3]──→ State C  (p=0.18)
                                                       │
                                                  [terminal, impact: -5]
```

| Operation | Complexity | Output |
|-----------|-----------|--------|
| `step()` | O(transitions) | Next state probabilities via Bayesian update |
| `simulate(n)` | O(n × transitions) | Full probability evolution over n steps |
| `most_likely_outcome()` | O(states) | Terminal state with highest probability |
| `expected_impact()` | O(states) | Weighted average impact score |
| `projection_curve()` | O(history) | Time-series for visualization |

### What Users Can Do With MiroFish

| Use Case | Input | Output |
|----------|-------|--------|
| "Should we enter market X?" | Scenario description | Trajectory with step-by-step probability |
| "What if competitor does Y?" | Branching condition | Probability flow across outcomes |
| "When will we break even?" | Financial parameters | Projection curve with confidence bands |
| "Compare strategy A vs B" | Two scenarios | Side-by-side trajectories with cumulative probability |
| "What's our critical path?" | Full trajectory | Non-parallelizable bottleneck steps |

---

## 9. Autonomous Business Team

A 14-role AI team that operates as a virtual organization with shared brand
context, campaign management, and learning-from-outcomes.

### The 14 Business Roles

| # | Role | Label | Intent | Proactivity | Sample Keywords |
|---|------|-------|--------|-------------|----------------|
| 1 | CEO | Chief Executive Officer | Strategy | 0.9 | vision, strategy, leadership, direction |
| 2 | CTO | Chief Technology Officer | Strategy | 0.8 | tech, architecture, infrastructure, scale |
| 3 | CFO | Chief Financial Officer | Strategy | 0.7 | budget, revenue, cost, financial, ROI |
| 4 | CMO | Chief Marketing Officer | Strategy | 0.8 | brand, marketing, campaign, growth |
| 5 | COO | Chief Operations Officer | Strategy | 0.7 | operations, process, efficiency, workflow |
| 6 | Developer | Developer | Execution | 0.6 | code, implement, build, debug, API |
| 7 | Designer | Designer | Execution | 0.6 | design, UI, UX, visual, wireframe |
| 8 | Writer | Content Writer | Execution | 0.5 | write, blog, copy, content, article |
| 9 | Marketer | Marketer | Execution | 0.6 | social, campaign, ads, SEO, growth |
| 10 | Analyst | Business Analyst | Support | 0.5 | data, analysis, metrics, report |
| 11 | Support | Customer Support | Support | 0.4 | support, help, ticket, customer |
| 12 | HR | Human Resources | Support | 0.3 | hiring, culture, team, onboarding |
| 13 | Sales | Sales | Support | 0.5 | sales, deal, pipeline, prospect |
| 14 | Legal | Legal Counsel | Support | 0.3 | legal, compliance, contract, terms |

### RoleIntent: Strategy vs Execution Routing

| Intent | Task Fit: Execution | Task Fit: Strategy | Task Fit: Neutral |
|--------|--------------------|--------------------|-------------------|
| **Strategy** (CEO, CMO, CFO, COO, CTO) | 0.2 (penalized) | 0.9 (boosted) | 0.5 |
| **Execution** (Dev, Designer, Writer, Marketer) | 0.9 (boosted) | 0.2 (penalized) | 0.5 |
| **Support** (Analyst, Support, HR, Sales, Legal) | 0.5 (always neutral) | 0.5 | 0.5 |

### Bid Formula (Enhanced with DreamAdvisor)

| Signal | Weight | Source |
|--------|--------|--------|
| `keyword_score` | 0.35 | Static + dream-expanded keywords |
| `proactivity` | 0.15 | Role's default proactivity |
| `domain_bonus` | 0.15 | Historical performance in domain |
| `dream_signal` | 0.15 | DreamAdvisor lookup |
| `intent_modifier` | 0.10 | RoleIntent.task_fit() |
| `noise` | 0.10 | Random tiebreaker |

### DreamAdvisor Feedback Loop

```text
Task outcome reported → member.record_outcome()
         ↓
CampaignStore.extract_dream_patterns() → (domain, narrative, valence)
         ↓
DreamAdvisor.ingest_campaign_patterns() → EMA update per (role, domain)
         ↓
Next route_task() → bid_with_context() uses updated advisor
         ↓
Better agent-task matching → better outcomes → loop continues
```

### Channel Connectors

| Channel | Type | Status |
|---------|------|--------|
| Blog | `ChannelType::Blog` | Available |
| Twitter/X | `ChannelType::Twitter` | Available |
| Reddit | `ChannelType::Reddit` | Available |
| Hacker News | `ChannelType::HackerNews` | Available |
| Email | `ChannelType::Email` | Available |
| LinkedIn | `ChannelType::LinkedIn` | Available |
| Product Hunt | `ChannelType::ProductHunt` | Available |

---

## 10. Multi-Tenant SaaS Layer

Full tenant isolation for serving multiple organizations from a single deployment.

### Plan Tiers

| Capability | Free | Starter | Pro | Enterprise |
|-----------|------|---------|-----|------------|
| Team Members | 5 | 10 | 14 (all) | 14 (all) |
| Concurrent Campaigns | 2 | 10 | 50 | 500 |
| API Calls / Day | 100 | 1,000 | 10,000 | 100,000 |
| LLM Provider Override | No | No | No | Yes |

### API Endpoints

| Method | Path | Auth | Purpose |
|--------|------|------|---------|
| `POST` | `/api/v1/auth/register` | None | Create tenant + admin key |
| `POST` | `/api/v1/auth/token` | None | Exchange API key for JWT |
| `GET` | `/api/v1/team` | JWT | List all agents |
| `GET` | `/api/v1/team/:role` | JWT | Inspect specific agent |
| `PUT` | `/api/v1/team/:role/status` | JWT+Write | Enable/disable agent |
| `GET/PUT` | `/api/v1/brand` | JWT | Read/update brand context |
| `POST` | `/api/v1/tasks` | JWT+Write | Route task to best agent |
| `POST` | `/api/v1/tasks/:id/outcome` | JWT+Write | Record outcome → dream loop |
| `POST` | `/api/v1/campaigns` | JWT+Write | Create campaign |
| `GET` | `/api/v1/campaigns` | JWT | List campaigns |
| `GET` | `/api/v1/campaigns/:id` | JWT | Campaign detail + metrics |
| `GET` | `/api/v1/campaigns/:id/patterns` | JWT | Extract dream patterns |
| `GET` | `/api/v1/usage` | JWT | Billing and usage data |
| `GET` | `/health` | None | Health check |

### Tenant Isolation

| Resource | Isolation | Storage |
|----------|----------|---------|
| TeamOrchestrator | Full per tenant | `~/.hsmii/tenants/{id}/` |
| Brand Context | Full | `brand.json` per tenant |
| Campaigns | Full | `campaigns.json` per tenant |
| DreamAdvisor | Full | `dream_advisor.json` per tenant |
| API Keys | Scoped via `tenant_id` in JWT | `api_keys.json` shared |
| Usage Counters | Full per tenant | `~/.hsmii/usage/{id}.json` |

---

## 11. Knowledge & Skill Systems

### CASS (Context-Aware Semantic Skills)

| Component | Purpose |
|-----------|---------|
| `CASS` | Skill registry with embedding-based retrieval |
| `ContextManager` | Snapshot current context for skill relevance scoring |
| `EmbeddingEngine` | Vector embeddings for semantic similarity |
| `SemanticGraph` | Graph of skill relationships and compositions |
| `SkillNode` | Individual skill with embedding, metadata, usage stats |

### AutoContext (Closed-Loop Learning)

A unified learning system following a sports team metaphor:

| Role | Function | Maps To |
|------|----------|---------|
| **Competitor** | Executes tasks, generates results | Agent execution |
| **Analyst** | Evaluates performance, finds gaps | Quality assessment |
| **Coach** | Generates playbooks and strategies | Optimization |
| **Curator** | Validates and persists successful playbooks | Knowledge management |

| Component | Purpose |
|-----------|---------|
| `AutoContextLoop` | Main loop: compete → analyze → coach → curate |
| `PlaybookHarness` | Test playbooks against real scenarios |
| `DistillationRouter` | Route training data to appropriate model tiers |
| `ValidationPipeline` | Multi-stage validation before accepting playbooks |

### Optimize Anything (GEPA-Inspired)

| Component | Purpose |
|-----------|---------|
| `OptimizationSession` | Manages evolutionary optimization of text artifacts |
| `Evaluator` | Scores candidates (keyword-based or LLM-judge) |
| `Artifact` | Text being optimized (code, prompts, configs) |
| `Candidate` | A variant being tested |
| `OptimizationMode` | Evolutionary strategy selection |

### RLM-V2 (Recursive Language Model)

Instead of dumping context into one LLM call:

| Stage | What Happens |
|-------|-------------|
| 1. Generate "code" | Model emits tool calls to process context |
| 2. Chunk context | Split input into manageable slices |
| 3. Parallel sub-queries | `llm_query()` dispatched concurrently |
| 4. Build answer | Aggregate results across iterations |
| 5. Terminate | `FINAL(answer)` when confident |

| Limit | Default |
|-------|---------|
| Max iterations | 20 |
| Max depth | 3 |
| Max sub-queries | 50 |

### DSPy Integration

| Component | Purpose |
|-----------|---------|
| `DspySignature` | Compile-time template for structured LLM outputs |
| `DspyContext` | Question + grounding + agent context + priors |
| `Demonstration` | Few-shot examples for in-context learning |

### Reasoning Braid

| Feature | Implementation |
|---------|---------------|
| Parallel Prolog threads | Multiple inference paths run concurrently |
| Neural-symbolic fusion | Prolog results woven into LLM synthesis |
| Dead-end pruning | Failed branches cut early |
| Depth/timeout limits | Configurable resource bounds |

---

## 12. Communication & Coordination

### Protocols Available

| Protocol | Mechanism | Best For |
|----------|-----------|----------|
| **Gossip** | Epidemic information spread | Eventually-consistent updates |
| **Swarm** | Collective behavior (bee waggle-dance inspired) | Coordination without central control |
| **Stigmergic Field** | Indirect coordination via shared environment | Emergent patterns from local actions |

### Kuramoto Synchronization

Maps coupled-oscillator physics onto the agent system:

| Concept | Implementation | Purpose |
|---------|---------------|---------|
| Phase oscillator | Each agent has a phase angle | Agent state representation |
| Natural frequency | From agent energy score | Intrinsic work rate |
| Coupling | Via hyperedge connections | Mutual influence |
| Order parameter R | Global phase coherence | System health metric |

**When R → 1**: Agents are synchronized — consensus is easy, coordination is tight.
**When R → 0**: Agents are desynchronized — diverse exploration, high creativity.

---

## 13. LLM Integration

### Multi-Provider Architecture

| Provider | Use Case | Failover |
|----------|----------|---------|
| **Ollama** | Local inference, privacy, low-latency | Primary for personal agent |
| **OpenAI** | High-capability tasks, GPT-4 | Fallback for complex tasks |
| **Anthropic** | Claude models, nuanced reasoning | Secondary fallback |

### LLM Client Features

| Feature | Implementation |
|---------|---------------|
| Retry with backoff | `RetryConfig` with configurable attempts |
| KV cache | `CacheManager` for prompt caching |
| Token tracking | `Usage` struct with in/out counts |
| Metrics | `MetricsSnapshot` for observability |
| FrankenTorch | Hybrid Candle + PyTorch inference engine |
| Model quantization | `Quantization` enum for resource optimization |

---

## 14. Tool System

60+ production-ready tools organized by domain:

| Domain | Tools | Purpose |
|--------|-------|---------|
| **Web/Browser** | `web_search`, `browser_tools` | Internet access and scraping |
| **File Operations** | `file_tools` (read, write, edit, glob) | Filesystem interaction |
| **Shell** | `shell_tools` | Command execution |
| **Git** | `git_tools` | Version control operations |
| **API/Data** | `api_tools` | External API calls |
| **Calculations** | `calculation_tools` | Math and computation |
| **Text Processing** | `text_tools` | Parsing, formatting, extraction |
| **System** | `system_tools` | OS-level operations |
| **Predictions** | `prediction_tool` | MiroFish integration |
| **RLM** | `rlm_tool` | Recursive language model execution |

### Tool Integration Points

| Integrates With | How |
|----------------|-----|
| CASS | Tools registered as skills with semantic embeddings |
| Memory | Tool results stored in persistent memory |
| Council | Tool selection informed by council decisions |
| Stigmergic traces | Every tool call leaves a trace |

---

## 15. Personal Agent

Transforms HSM-II from research system into practical personal AI assistant,
inspired by Hermes Agent's `MEMORY.md + USER.md + SOUL.md` architecture.

### Personal Agent Architecture

| Component | Hermes Equivalent | Purpose |
|-----------|-------------------|---------|
| `Persona` | SOUL.md | Agent personality, voice, capabilities |
| `PersonalMemory` | MEMORY.md + USER.md | Persistent beliefs, projects, facts |
| `Heartbeat` | Cron system | Scheduled tasks, routines |
| `Gateway` | Communication layer | Email, Slack, Discord connections |
| `OllamaClient` | LLM backend | Local inference engine |
| `ToolRegistry` | Tool system | Available capabilities |

### Personal Agent Features

| Feature | What It Does | User Benefit |
|---------|-------------|-------------|
| **Onboarding** | 8-question guided setup | Instant personalization |
| **Belief extraction** | Learns from conversations | Gets smarter over time |
| **Document ingestion** | Bulk knowledge import | Rapid knowledge transfer |
| **Heartbeat routines** | Cron-like scheduled actions | Automated daily tasks |
| **Multi-gateway** | Email, Discord, Telegram | Reach user wherever they are |
| **Memory persistence** | Facts survive across sessions | Consistent long-term memory |
| **Enhanced metrics** | JW scores, contribution tracking | Transparent performance |

---

## 16. External Gateways

| Gateway | Library | Features |
|---------|---------|----------|
| **Discord** | serenity | Full bot with command handling |
| **Telegram** | teloxide | Bot API with message routing |
| **Email** | IMAP/SMTP | AI classification, smart responses, thread memory |

### Email Agent Capabilities

| Capability | Implementation |
|-----------|---------------|
| Auto-categorization | `EmailClassifier` with `Priority` and `Category` enums |
| Smart responses | `ResponseGenerator` with `Tone` selection |
| Thread tracking | `ConversationThread` in `EmailMemory` |
| Template system | `ResponseTemplate` for common patterns |

---

## 17. Federation

Cross-system knowledge sharing between multiple HSM-II instances.

| Component | Purpose |
|-----------|---------|
| `FederationClient` | Connect to remote HSM-II instances |
| `FederationServer` | Serve local knowledge to remote instances |
| `TrustGraph` | Weighted trust relationships between systems |
| `TrustPolicy` | Rules for accepting/rejecting remote knowledge |
| `ConflictMediator` | Resolve contradictions between systems |
| `ConflictRecord` | Audit trail of resolved conflicts |

### Federation Scope Levels

| Scope | Meaning | Visible To |
|-------|---------|-----------|
| `Local` | This instance only | Current system |
| `Federated` | Shared with trusted peers | Trust-linked systems |
| `Global` | Available to all peers | Entire federation |

---

## 18. Governance & Security

### Ouroboros 5-Phase Gate

Inspired by Cardano's Ouroboros protocol, every significant action passes through:

| Phase | Module | Purpose |
|-------|--------|---------|
| **1. Policy** | `phase1_policy` | Constitutional rules enforcement |
| **2. Risk Gate** | `phase2_risk_gate` | Risk assessment and threshold checking |
| **3. Council Bridge** | `phase3_council_bridge` | Council decision integration |
| **4. Evidence Contract** | `phase4_evidence_contract` | Evidence requirements verification |
| **5. Ops Memory** | `phase5_ops_memory` | Runtime SLO verification, operational memory |

### Auth System

| Feature | Implementation |
|---------|---------------|
| API key hashing | Argon2 (password-grade) |
| JWT tokens | 24-hour expiry with role-based claims |
| Rate limiting | Per-minute, per-hour, per-day configurable |
| Permissions | Read, Write, Admin, Chat, ToolUse, WorkflowManage |
| Multi-tenant | `tenant_id` in JWT claims |
| Persistent storage | File-based with write-through |

### Vault

| Feature | Purpose |
|---------|---------|
| `VaultNote` | Encrypted knowledge storage with tags, links, wikilinks |
| `VaultMergeStats` | Merge conflict tracking for concurrent edits |

### Social Memory & Reputation

| Metric | Tracks |
|--------|--------|
| `successful_deliveries` / `failed_deliveries` | Task completion rate |
| `on_time_deliveries` / `missed_deadlines` | Punctuality |
| `promises_kept` / `promises_broken` | Reliability |
| `safe_shares` / `unsafe_shares` | Data handling trustworthiness |
| `capability_profiles` | Per-domain skill evidence |

**Data Sensitivity Levels**: Public → Internal → Confidential → Secret

---

## 19. Observability & Metrics

| Component | Purpose |
|-----------|---------|
| `observability.rs` | Tracing and Prometheus metrics integration |
| `metrics.rs` | Aggregated statistics and experiment tracking |
| `metrics_dks_ext.rs` | DKS-specific metrics extensions |
| `batch_runner.rs` | Batch experiment execution with RooDB persistence |

### Tracked Metrics

| Metric Category | Examples |
|----------------|---------|
| Agent performance | JW scores, task completion, reliability |
| System health | Kuramoto order parameter R, coherence delta |
| Dream quality | Patterns crystallized, survival rate, proto-skills promoted |
| API usage | Calls per tenant per day, LLM tokens consumed |
| Consensus quality | Bid correlation (groupthink), diversity factor |

---

## 20. GPU Acceleration

Feature-gated (`gpu` feature, enabled by default):

| Component | Purpose |
|-----------|---------|
| `gpu::buffer` | GPU memory management |
| `gpu::compute` | WGPU compute shaders |
| `gpu::graph` | GPU-accelerated graph operations |

**Used for**: Embedding similarity calculations, graph traversals, matrix operations
on large hypergraphs.

---

## 21. Server Binaries

| Binary | Port | Purpose | When To Use |
|--------|------|---------|-------------|
| **`teamd`** | 8788 | Multi-tenant business team API | SaaS deployment |
| **`personal_agent`** | — | TUI personal assistant | Individual use |
| **`hypergraphd`** | WS | Hypergraph database server | Multi-agent deployment |
| **`conductord`** | — | Decision optimization coordinator | Multi-agent deployment |
| **`agentd`** | — | Individual agent daemon (×N) | Multi-agent deployment |
| **`ouroboros_gate`** | — | 5-phase governance gate | Policy enforcement |
| **`investigate`** | — | Recursive investigation CLI | Data analysis |
| **`batch_experiment`** | — | Empirical evaluation runner | Research/testing |
| **`tui_codex_demo`** | — | Codex-style TUI demo | UI development |

### Distributed Deployment

```text
┌─────────────────────┐     ┌────────────────────┐
│    personal_agent    │     │       teamd         │
│  (User interaction)  │     │  (SaaS REST API)    │
└──────────┬──────────┘     └─────────┬──────────┘
           │                          │
           ▼                          ▼
┌──────────────────────────────────────────────────┐
│                  conductord                       │
│  (Optimization + Council coordination)            │
└──────────────────────┬───────────────────────────┘
                       │
           ┌───────────┼───────────┐
           ▼           ▼           ▼
    ┌──────────┐ ┌──────────┐ ┌──────────┐
    │  agentd  │ │  agentd  │ │  agentd  │
    │(Architect)│ │(Catalyst)│ │ (Coder)  │
    └────┬─────┘ └────┬─────┘ └────┬─────┘
         │            │            │
         ▼            ▼            ▼
    ┌──────────────────────────────────────┐
    │           hypergraphd                │
    │  (Shared knowledge + WebSocket)      │
    └──────────────────────────────────────┘
```

---

## 22. Cross-System Leverage Map

How every major subsystem uses other subsystems:

| System | Uses | How It Leverages |
|--------|------|-----------------|
| **Dream Engine** | Stigmergic traces, DKS, CASS, Hypergraph | Reads traces → detects motifs → crystallizes → deposits → generates skills |
| **DreamAdvisor** | Dream Engine, CampaignStore, BusinessRole | Translates crystallized patterns + campaign feedback into routing adjustments |
| **Council** | Agents, Stigmergic traces, Consensus | Multi-role deliberation informed by execution history |
| **Consensus** | Agents, ACPO, Kuramoto | Role-weighted bids with anti-groupthink penalty |
| **Autonomous Team** | Council roles → Business roles, DreamAdvisor, Brand, Channels | Maps 6 council roles to 14 business roles with dream-enhanced routing |
| **MiroFish** | Scenario Simulator, Ollama, Trajectories | LLM-driven trajectory planning with Bayesian state transitions |
| **AutoContext** | CASS, optimize_anything, persistence | Closed-loop: execute → evaluate → optimize → validate → persist |
| **CASS** | Embedding Engine, Semantic Graph, Dream Proto-Skills | Skills with semantic embeddings, composed and ranked by context |
| **Personal Agent** | Memory, Heartbeat, Gateway, Tools, LLM | User-facing interface combining all subsystems |
| **Multi-Tenant** | TeamOrchestrator, Auth, UsageTracker | Isolates full team per customer with persistent state |
| **Federation** | Trust Graph, Conflict Mediator, Hypergraph | Cross-instance knowledge sharing with trust and conflict resolution |
| **Social Memory** | Promise tracking, Capability evidence, Reputation | Tracks agent reliability → informs consensus bids and routing |
| **Ouroboros Gate** | Policy, Risk, Council, Evidence, Ops Memory | 5-phase governance for high-stakes actions |
| **Investigation Engine** | Tools, LLM, Evidence chains | Recursive sub-agent delegation for dataset analysis |
| **Reasoning Braid** | Prolog Engine, LLM | Parallel symbolic inference woven into neural synthesis |
| **Kuramoto** | Agent energy, Hyperedge coupling | Phase synchronization → global coordination health metric |
| **GPU** | Hypergraph, Embeddings | Hardware acceleration for graph ops and similarity |

---

## 23. Feedback Loops

HSM-II has **5 interconnected feedback loops** that make the system self-improving:

### Loop 1: Task Execution → Social Reputation

```text
Agent executes task → outcome recorded → social_memory updated
→ reputation scores change → consensus bids re-weighted → better routing
```

### Loop 2: Campaign Outcomes → Dream Routing

```text
Campaign metrics collected → extract_dream_patterns()
→ DreamAdvisor.ingest() → routing adjustments updated
→ next task routed differently → better outcomes
```

### Loop 3: Dream Consolidation → Skill Generation

```text
Traces accumulate (200 ticks) → dream engine runs → motifs detected
→ patterns crystallized → DKS survival filter → proto-skills promoted
→ CASS gains new skills → agents have new capabilities
```

### Loop 4: AutoContext → Playbook Evolution

```text
Agent competes (executes task) → analyst evaluates quality
→ coach generates playbook improvements → curator validates
→ validated playbooks persisted → next execution uses better playbooks
```

### Loop 5: Consensus → Skill Lifecycle

```text
Skill applied → emergent associations detected → consensus evaluates
→ utility score computed → Promote / Maintain / Suspend verdict
→ skill lifecycle updated → affects future skill availability
```

### How Loops Compound

```text
Loop 1 (reputation) feeds → Loop 2 (routing accuracy)
Loop 2 (routing)    feeds → Loop 3 (dream quality — better traces)
Loop 3 (skills)     feeds → Loop 4 (autocontext has better tools)
Loop 4 (playbooks)  feeds → Loop 5 (higher skill quality → more promotions)
Loop 5 (skills)     feeds → Loop 1 (agents with better skills → better reputation)
```

---

## 24. What Users Can Do

### For Individual Users (Personal Agent)

| What You Can Do | How | System Used |
|----------------|-----|-------------|
| Run a personal AI assistant | `cargo run --bin personal_agent` | Personal, LLM, Tools, Memory |
| Teach it about your business | 8-question onboarding flow | Onboard, Hypergraph |
| Import documents for knowledge | Document ingestion pipeline | Onboard, CASS, Embeddings |
| Set up automated routines | Heartbeat with cron schedules | Scheduler, Heartbeat |
| Connect email/Discord/Telegram | Gateway configuration | Gateways, Email |
| Ask questions with reasoning | Multi-step investigation | Investigation Engine, RLM-V2 |
| Get business decision support | Scenario simulation | MiroFish, Scenario Simulator |
| See your agent's performance | JW metrics, contribution stats | Metrics, Observability |

### For Teams (Autonomous Business Team)

| What You Can Do | How | System Used |
|----------------|-----|-------------|
| Get a 14-role AI team instantly | Register tenant via API | Tenant, Team API, Auth |
| Route tasks to the best agent | `POST /tasks` with description | Autonomous Team, DreamAdvisor |
| Get ready-to-use LLM prompts | Prompt returned in task response | Persona, Brand Context |
| Run marketing campaigns | Create campaigns with channels | CampaignStore, Channels |
| Track campaign performance | Snapshots with CTR, CAC, sentiment | CampaignStore, Metrics |
| Set brand voice/constraints | Update brand context via API | BrandContext |
| Enable/disable roles as needed | Toggle agent status | Team API |
| Let routing improve over time | Report task outcomes | DreamAdvisor feedback loop |
| View what the system learned | Extract dream patterns | DreamAdvisor, Dream Engine |

### For SaaS Operators

| What You Can Do | How | System Used |
|----------------|-----|-------------|
| Serve multiple organizations | Multi-tenant with plan tiers | Tenant, Auth, UsageTracker |
| Enforce usage limits | Plan-based daily API limits | UsageTracker, TenantSettings |
| Monitor billing metrics | Per-tenant usage counters | UsageTracker |
| Scale with LRU caching | 100-tenant in-memory cache | TenantRegistry |
| Upgrade/downgrade customers | `update_plan()` | TenantRegistry |

### For Researchers

| What You Can Do | How | System Used |
|----------------|-----|-------------|
| Run controlled experiments | `cargo run --bin batch_experiment` | Batch Runner, Metrics |
| Test agent coordination | Configurable agent parameters | Agent, Council, Consensus |
| Study emergent behavior | 9 types of emergent associations | Consensus, Hypergraph |
| Analyze synchronization | Kuramoto order parameter R | Kuramoto |
| Benchmark DKS evolution | Population dynamics with stigmergy | DKS, Metrics |
| Test federation protocols | Multi-instance with trust graphs | Federation |
| Investigate datasets | Recursive investigation agent | Investigation Engine |

### For Developers Building On HSM-II

| What You Can Do | How | System Used |
|----------------|-----|-------------|
| Add custom tools | Implement tool trait, register in `tools/` | Tool System, CASS |
| Create new agent roles | Extend `BusinessRole` or `Role` | Agent, Autonomous Team |
| Build custom channels | Implement `ChannelConnector` trait | Autonomous Team |
| Add LLM providers | Extend `LlmProvider` enum | LLM Client |
| Create governance rules | Add Ouroboros phase handlers | Ouroboros Compat |
| Build federation bridges | Use `FederationClient/Server` | Federation |
| Add new evaluation modes | Extend `Evaluator` trait | Optimize Anything |
| Write custom dream processors | Extend dream submodules | Dream Engine |
| GPU-accelerate operations | Use `gpu::compute` | GPU Module |
| Create new council modes | Implement council trait | Council |

---

## Appendix: Complete File Persistence Layout

```text
~/.hsmii/
├── auth/
│   ├── api_keys.json               # Hashed API keys (all tenants)
│   └── tenants.json                # Tenant registry
├── tenants/
│   └── {tenant-uuid}/
│       ├── brand.json              # Brand context
│       ├── campaigns.json          # Campaign store
│       ├── team_members.json       # Agent state and history
│       └── dream_advisor.json      # Dream routing adjustments
├── usage/
│   └── {tenant-uuid}.json          # Daily usage counters
├── memory/
│   ├── MEMORY.md                   # Personal agent memory
│   ├── USER.md                     # User profile
│   └── facts/                      # Belief storage
├── persona/
│   └── persona.json                # Agent personality definition
├── heartbeat/
│   └── routines.json               # Scheduled tasks
├── vault/
│   └── notes/                      # Encrypted knowledge notes
├── cass/
│   └── skills/                     # Semantic skill embeddings
├── dream/
│   └── patterns/                   # Crystallized dream patterns
└── logs/
    └── *.log                       # Application logs
```

---

## Appendix: Dependency Stack

| Category | Libraries |
|----------|-----------|
| **Async Runtime** | tokio, tokio-stream, futures |
| **HTTP** | axum, tower, tower-http, reqwest |
| **Database** | mysql_async (RooDB) |
| **Auth** | argon2, jsonwebtoken |
| **Serialization** | serde, serde_json, bincode |
| **Math/ML** | nalgebra, ndarray, rand, rand_distr |
| **Graph** | petgraph |
| **AI/LLM** | ollama-rs |
| **TUI** | ratatui, crossterm |
| **Bots** | serenity (Discord), teloxide (Telegram) |
| **GPU** | wgpu, bytemuck, pollster |
| **Scheduling** | cron |
| **WASM** | wasmtime |
| **Code Tools** | walkdir, glob |
| **Observability** | prometheus, tracing-subscriber |
| **WebSocket** | tokio-tungstenite |

---

*This document covers ~82 modules, 9 binaries, 200+ source files, and 60+ tools
that comprise HSM-II. Generated from codebase analysis of the `claude/exciting-heisenberg` branch.*
