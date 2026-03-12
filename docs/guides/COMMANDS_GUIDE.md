# HSM-II Commands Guide: Old vs New

## Quick Answer

| What you want | Old Command | New Command |
|---------------|-------------|-------------|
| **Research/Simulation** | `run-hyper-stigmergy-II.command` | Same (unchanged) |
| **Visualize hypergraph** | `open-hypergraphd.command` | Same (unchanged) |
| **Personal AI assistant** | ❌ Didn't exist | `run-personal-agent.command` ⭐ |
| **Chat with your agent** | ❌ Didn't exist | `cargo run --bin personal_agent -- start` |

---

## Three Modes of Operation

HSM-II now operates in **three distinct modes**:

```
┌─────────────────────────────────────────────────────────────────────┐
│                     HSM-II OPERATING MODES                           │
├─────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  Mode 1: RESEARCH          Mode 2: VISUALIZATION    Mode 3: PERSONAL │
│  ─────────────────         ───────────────────      ───────────────  │
│                                                                      │
│  run-hyper-stigmergy-      open-hypergraphd         run-personal-    │
│  II.command                .command                 agent.command    │
│                                                                      │
│  • Experiments             • Web UI               • Chat interface   │
│  • Metrics                 • Real-time viz        • Discord/Telegram │
│  • Batch runs              • Graph exploration    • Persistent memory│
│  • Paper data              • Coherence plots      • Does your work   │
│                                                                      │
│  Use case:                 Use case:              Use case:          │
│  "Study multi-agent        "See what's          "Help me research    │
│   coordination"            happening"             this topic"        │
│                                                                      │
└─────────────────────────────────────────────────────────────────────┘
```

---

## Detailed Comparison

### Mode 1: Research (`run-hyper-stigmergy-II.command`)

**What it does:**
- Builds and runs the HSM-II simulation backend
- Starts RooDB database
- Runs experiments, collects metrics
- Outputs data for analysis

**Use when:**
- Running batch experiments
- Collecting research data
- Testing DKS/CASS/Council implementations
- Generating paper figures

```bash
# Start research backend
./scripts/macos/run-hyper-stigmergy-II.command

# Then in another terminal, run experiments
cargo run --release --bin batch_experiment -- 100 1000
```

**Output:**
```
✓ build successful — starting monolith backend
✓ API target: http://127.0.0.1:9000
```

---

### Mode 2: Visualization (`open-hypergraphd.command`)

**What it does:**
- Opens the web-based visualization UI
- Shows real-time hypergraph state
- Displays coherence metrics
- Shows agent positions and edges

**Use when:**
- You want to *see* the hypergraph
- Debugging stigmergic patterns
- Monitoring experiments
- Exploring the world model

```bash
# Open visualization (requires backend running)
./scripts/macos/open-hypergraphd.command

# Or manually open
open viz/index.html
```

**Shows:**
- Hypergraph nodes (agents)
- Stigmergic edges
- Coherence over time
- DKS population metrics

---

### Mode 3: Personal Agent (`run-personal-agent.command`) ⭐ NEW

**What it does:**
- Your personal AI companion
- Natural language interface
- Persistent memory (SOUL.md, MEMORY.md, USER.md)
- Discord/Telegram integration
- Actually helps you with tasks

**Use when:**
- You want an AI assistant
- Research help
- Task automation
- Daily work companion

```bash
# Start personal agent (interactive)
./scripts/macos/run-personal-agent.command

# Or with specific options
cargo run --release --bin personal_agent -- start --discord
```

**Output:**
```
🚀 Starting Ash...

Commands:
  Type your message to chat
  'exit' to quit
  'help' for more commands

─────────────────────────────────────────────────────────────────────────

Ash> 
```

---

## How They Relate

```
Research Mode                    Personal Agent Mode
─────────────────                ───────────────────
                                 
Batch experiments  ───────────→  Skills learned from
↓                                   your usage
Metrics saved      ───────────→  MEMORY.md updated
↓                                   with facts
DKS simulation     ───────────→  DKS manages agent
↓                                   pool size
CASS semantic      ───────────→  CASS retrieves
   search                         relevant skills
↓                                   
Council decides    ───────────→  Council decides
   modes                            when to spawn
                                    subagents

         ↓
    Shared Core (hypergraph, stigmergy, etc.)
         ↓
    
Visualization Mode
    (Web UI shows both)
```

---

## Typical Workflows

### Workflow A: Pure Research (Original HSM-II)

```bash
# 1. Start backend
./scripts/macos/run-hyper-stigmergy-II.command

# 2. In another terminal, run experiments
cargo run --release --bin batch_experiment -- 50 500

# 3. Visualize
./scripts/macos/open-hypergraphd.command

# 4. Analyze results
python scripts/plot_results.py
```

### Workflow B: Personal Assistant (New)

```bash
# 1. First-time setup
./scripts/macos/run-personal-agent.command
# → "What's your name?"
# → "Choose personality..."

# 2. Daily use
./scripts/macos/run-personal-agent.command
Ash> Research quantum computing advances

# 3. Daemon mode (24/7)
./scripts/macos/run-personal-agent.command --daemon --discord
```

### Workflow C: Combined (Research + Assistant)

```bash
# Terminal 1: Start backend for both
./scripts/macos/run-hyper-stigmergy-II.command

# Terminal 2: Use personal agent (connects to same backend)
cargo run --release --bin personal_agent -- start

# Browser: Visualize what's happening
./scripts/macos/open-hypergraphd.command
```

---

## Migration: Using Existing Setup

If you've been using `run-hyper-stigmergy-II.command`:

### Your existing workflow still works:
```bash
./scripts/macos/run-hyper-stigmergy-II.command  # Unchanged
./scripts/macos/open-hypergraphd.command         # Unchanged
```

### New option available:
```bash
./scripts/macos/run-personal-agent.command       # NEW - Try it!
```

### They can run together:
```bash
# Tab 1: Research backend
./scripts/macos/run-hyper-stigmergy-II.command

# Tab 2: Personal agent (uses same backend)
cargo run --release --bin personal_agent -- start

# Tab 3: Visualization
./scripts/macos/open-hypergraphd.command
```

---

## Easy Start (Recommended)

### For New Users

```bash
# Just want an AI assistant?
./scripts/macos/run-personal-agent.command

# That's it. You'll be guided through setup.
```

### For Existing HSM-II Users

```bash
# Keep using what you know
./scripts/macos/run-hyper-stigmergy-II.command

# But also try the new personal agent
./scripts/macos/run-personal-agent.command
```

---

## Command Reference

| Command | Purpose | When to Use |
|---------|---------|-------------|
| `run-hyper-stigmergy-II.command` | Research backend | Experiments, metrics, simulation |
| `open-hypergraphd.command` | Web visualization | See hypergraph, monitor state |
| `run-personal-agent.command` | Personal AI | Daily use, chat, tasks |
| `cargo run --bin personal_agent -- bootstrap` | First setup | Initialize your agent |
| `cargo run --bin personal_agent -- status` | Check health | See agent state |
| `cargo run --bin batch_experiment` | Run experiments | Research data collection |

---

## Summary

**`open-hypergraphd.command`** = Just opens the visualization UI (unchanged)

**`run-hyper-stigmergy-II.command`** = Research backend (unchanged)

**`run-personal-agent.command`** = ⭐ NEW - Your AI companion (Hermes-like grounded interface)

All three can work together, or you can use them independently based on your needs.
