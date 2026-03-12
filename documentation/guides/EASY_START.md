# Easy Start: Using HSM-II Personal Agent

## The Simple Answer

**Yes!** Using `open-hypergraphd.command` and the new personal agent together is now much easier.

## Three Ways to Use HSM-II

### Option 1: Just the Personal Agent (Easiest)
```bash
./tools/scripts/macos/run-personal-agent.command
```
That's it. No other setup needed.

### Option 2: Personal Agent + Visualization
```bash
# Terminal 1: Start your AI companion
./tools/scripts/macos/run-personal-agent.command

# Terminal 2: See what it's thinking
./tools/scripts/macos/open-hypergraphd.command
```

### Option 3: Full Stack (Research + Personal + Viz)
```bash
# Terminal 1: Start research backend
./tools/scripts/macos/run-hyper-stigmergy-II.command

# Terminal 2: Personal agent (connects to backend)
./tools/scripts/macos/run-personal-agent.command

# Browser: Visualization
./tools/scripts/macos/open-hypergraphd.command
```

---

## What Each Command Does

| Command | What It Starts | What You See |
|---------|----------------|--------------|
| `run-hyper-stigmergy-II.command` | Research backend + database | Terminal with metrics |
| `open-hypergraphd.command` | Web visualization | Browser with hypergraph |
| `run-personal-agent.command` | Your AI companion | Chat interface |

---

## Recommended: Personal Agent First

```bash
# 1. First time only - setup
./tools/scripts/macos/run-personal-agent.command
# → "What's your name?"
# → "What are your goals?"
# → "Choose personality..."

# 2. Daily use - just chat
./tools/scripts/macos/run-personal-agent.command
Ash> Help me research stigmergy

# 3. The agent does the work using HSM-II internally
[Spawns subagents → Coordinates → Returns results]

# 4. (Optional) Watch it work
./tools/scripts/macos/open-hypergraphd.command  # See the hypergraph in browser
```

---

## Example Session

```bash
$ ./tools/scripts/macos/run-personal-agent.command

🌱 Welcome to HSM-II Personal Agent
Let's set up your AI companion...

What's your name? Alice
What are your main goals? Research, coding, learning
Choose personality: 1 (Thoughtful Analyst)

✓ Setup complete!

🚀 Starting Ash...

Commands:
  Type your message to chat
  'exit' to quit
  'help' for more commands

─────────────────────────────────────────────────────────────────────────

Ash> Research stigmergic coordination for me
I'll coordinate multiple agents to research this comprehensively.
[Working...]

Here are the key findings on stigmergic coordination:
1. Indirect communication via environment
2. Emergent problem-solving
3. Self-organizing systems
4. [Details...]

I've saved this to your memory under "stigmergy_research_2025_02_25".

Ash> Remind me about this tomorrow
Scheduled for 9 AM tomorrow.

Ash> exit
Goodbye! Saving state...
```

---

## Integration with Existing Commands

The personal agent **can** connect to your existing HSM-II setup:

```bash
# If you already run this:
./tools/scripts/macos/run-hyper-stigmergy-II.command

# The personal agent can use it:
./tools/scripts/macos/run-personal-agent.command --connect-hypergraph

# Benefits:
# - Uses same database
# - Shares hypergraph state
# - Council decisions use full backend
# - DKS manages real agent pool
```

But it also works **standalone** if you just want a personal AI.

---

## Quick Reference

```bash
# Personal agent (NEW - easiest way to use HSM-II)
./tools/scripts/macos/run-personal-agent.command

# Traditional research mode (unchanged)
./tools/scripts/macos/run-hyper-stigmergy-II.command

# Visualization (unchanged)
./tools/scripts/macos/open-hypergraphd.command

# Manual CLI commands
cargo run --bin personal_agent -- start
cargo run --bin personal_agent -- bootstrap
cargo run --bin personal_agent -- status
```

---

## FAQ

**Q: Do I need to run `run-hyper-stigmergy-II.command` first?**  
A: No! `run-personal-agent.command` works standalone. The backend is optional.

**Q: Can I use the visualization with the personal agent?**  
A: Yes! Start both:
```bash
./tools/scripts/macos/run-personal-agent.command   # Terminal 1
./tools/scripts/macos/open-hypergraphd.command     # Terminal 2
```

**Q: What if I was using HSM-II for research?**  
A: Keep using `run-hyper-stigmergy-II.command`. The personal agent is a new option, not a replacement.

**Q: Is this like Hermes?**  
A: Yes! Inspired by [Hermes Agent](https://github.com/NousResearch/hermes-agent) but powered by HSM-II's advanced coordination.

---

## One-Liner Summary

> `./tools/scripts/macos/run-personal-agent.command` = Easiest way to use HSM-II as your AI companion

The research and visualization commands still work exactly as before. The personal agent is a **new, easier layer** on top.
