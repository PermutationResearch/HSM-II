# HSM-II Usage Modes

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           HOW TO USE HSM-II                                  │
│                    (Choose your adventure)                                   │
└─────────────────────────────────────────────────────────────────────────────┘

                               YOU
                                │
                                ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                    "What do you want to do today?"                          │
└─────────────────────────────────────────────────────────────────────────────┘
         │                      │                      │
         ▼                      ▼                      ▼
   ┌──────────┐          ┌──────────┐          ┌──────────┐
   │ RESEARCH │          │   CHAT   │          │   BOTH   │
   └────┬─────┘          └────┬─────┘          └────┬─────┘
        │                     │                     │
        ▼                     ▼                     ▼
┌───────────────┐     ┌───────────────┐     ┌───────────────┐
│ I want to run │     │ I want an AI  │     │ I want to see │
│ experiments   │     │ assistant to  │     │ experiments   │
│ and collect   │     │ help me with  │     │ AND have a    │
│ data          │     │ daily tasks   │     │ personal AI   │
└───────┬───────┘     └───────┬───────┘     └───────┬───────┘
        │                     │                     │
        ▼                     ▼                     ▼
┌───────────────┐     ┌───────────────┐     ┌───────────────┐
│ Command:      │     │ Command:      │     │ Commands:     │
│               │     │               │     │               │
│ ./run-hyper-  │     │ ./run-        │     │ Terminal 1:   │
│ stigmergy-II  │     │ personal-     │     │ ./run-hyper-  │
│ .command      │     │ agent.command │     │ stigmergy-II  │
│               │     │               │     │ .command      │
│ Output:       │     │ Output:       │     │               │
│ Metrics, CSV  │     │ Chat, work    │     │ Terminal 2:   │
│ graphs        │     │ products      │     │ ./run-        │
│               │     │               │     │ personal-     │
│ Optional:     │     │ Optional:     │     │ agent.command │
│ ./open-       │     │ ./open-       │     │               │
│ hypergraphd   │     │ hypergraphd   │     │ Terminal 3:   │
│ .command      │     │ .command      │     │ ./open-       │
│ (to watch)    │     │ (to watch)    │     │ hypergraphd   │
│               │     │               │     │ .command      │
└───────────────┘     └───────────────┘     └───────────────┘
        │                     │                     │
        │                     │                     │
        ▼                     ▼                     ▼
┌───────────────┐     ┌───────────────┐     ┌───────────────┐
│ Best for:     │     │ Best for:     │     │ Best for:     │
│ • Papers      │     │ • Daily use   │     │ • Deep work   │
│ • Benchmarks  │     │ • Quick help  │     │ • Research +  │
│ • Algorithms  │     │ • Automation  │     │   application │
│ • Simulation  │     │ • Learning    │     │ • Full power  │
└───────────────┘     └─────────────┘     └───────────────┘


┌─────────────────────────────────────────────────────────────────────────────┐
│                         WHAT EACH STARTS                                     │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│  run-hyper-stigmergy-II.command    run-personal-agent.command               │
│  ─────────────────────────────     ──────────────────────────               │
│                                                                              │
│  ┌──────────────────────┐          ┌──────────────────────┐                 │
│  │ RooDB Database       │          │ Personal Agent       │                 │
│  │ ↓                    │          │ ├─ SOUL.md (persona) │                 │
│  │ Hypergraph Backend   │          │ ├─ MEMORY.md         │                 │
│  │ ├─ Stigmergy         │          │ ├─ USER.md           │                 │
│  │ ├─ DKS               │          │ ├─ HEARTBEAT.md      │                 │
│  │ ├─ CASS              │          │ └─ Chat Interface    │                 │
│  │ └─ Council           │          │                      │                 │
│  │ ↓                    │          │ Optional:            │                 │
│  │ API Server :9000     │◄─────────┤ Connects to backend  │                 │
│  └──────────────────────┘          └──────────────────────┘                 │
│           ▲                                                            │
│           └─────────────────────┐                                          │
│                                 │                                          │
│                    open-hypergraphd.command                               │
│                    ────────────────────────                               │
│                                                                              │
│                    ┌──────────────────────┐                                │
│                    │ Web Browser          │                                │
│                    │ ├─ Hypergraph viz    │                                │
│                    │ ├─ Coherence plots   │                                │
│                    │ └─ Agent positions   │                                │
│                    └──────────────────────┘                                │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘


┌─────────────────────────────────────────────────────────────────────────────┐
│                         SIMPLE DECISION TREE                                 │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│  Are you a researcher studying multi-agent systems?                         │
│  └──► run-hyper-stigmergy-II.command                                        │
│                                                                              │
│  Do you want an AI assistant for daily work?                                │
│  └──► run-personal-agent.command                                            │
│                                                                              │
│  Do you want BOTH?                                                          │
│  └──► Run both commands in separate terminals                               │
│                                                                              │
│  Do you just want to see pretty visualizations?                             │
│  └──► open-hypergraphd.command (needs backend running)                      │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘


┌─────────────────────────────────────────────────────────────────────────────┐
│                         EXAMPLE SESSIONS                                     │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│  RESEARCH MODE                    PERSONAL AGENT MODE                       │
│  ────────────                     ───────────────────                       │
│                                                                              │
│  $ ./run-hyper-stigmergy-II      $ ./run-personal-agent                     │
│  .command                        .command                                   │
│                                                                              │
│  ✓ build successful              🌱 Welcome! Setup...                       │
│  ✓ RooDB started                 ✓ Setup complete                           │
│  ✓ API: :9000                    🚀 Starting Ash...                         │
│                                                                              │
│  > Waiting for experiments...    Ash> Research stigmergy                   │
│                                  [Working...]                               │
│  (In another terminal)           Here's what I found:                       │
│  $ cargo run --bin batch_exp     1. Indirect communication                  │
│     -- 100 500                   2. Emergent behavior                       │
│                                  3. [Details]                               │
│  ✓ Experiment complete           Saved to memory.                          │
│  $ python plot.py                                                               │
│  (view graphs)                   Ash> exit                                 │
│                                  Goodbye!                                  │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘


┌─────────────────────────────────────────────────────────────────────────────┐
│                              KEY INSIGHT                                     │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│  The personal agent (run-personal-agent.command) uses the SAME advanced     │
│  coordination (stigmergy, DKS, CASS, Council) as the research mode,         │
│  but presents it as a simple chat interface.                                │
│                                                                              │
│  You get the power of HSM-II without needing to understand the internals.   │
│                                                                              │
│  Think of it as:                                                             │
│  • Research mode = "HSM-II engine exposed"                                  │
│  • Personal mode = "HSM-II made user-friendly"                              │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘
