#!/usr/bin/env python3
"""
patch_company_packs.py — inject YC-bench survival heuristics into every company pack.

Adds two things to each company:
  1. VISION.md  — "Simulation Mechanics" + "Failure Catalogue" + "Decision Rules" block
                  inserted BEFORE the existing ⚠️ Operating Protocol section.
  2. agents/ceo/<file>.md — "Simulation Awareness" section appended to the CEO briefing.

Safe to re-run: uses sentinel comments to skip already-patched files.
"""

import os
import re
import sys
from pathlib import Path

PACKS_ROOT = Path.home() / ".hsm" / "company-packs" / "paperclipai" / "companies"

VISION_SENTINEL = "<!-- YC-BENCH-HEURISTICS-V1 -->"
CEO_SENTINEL    = "<!-- YC-BENCH-CEO-AWARENESS-V1 -->"

# ---------------------------------------------------------------------------
# Universal block inserted into VISION.md before the ⚠️ Operating Protocol
# ---------------------------------------------------------------------------
VISION_HEURISTICS = """\
<!-- YC-BENCH-HEURISTICS-V1 -->
## 🧠 Simulation Mechanics — Know These or You Will Fail

You are running a **1-year (52-week) autonomous business simulation**. These mechanics
are always active and are the proximate cause of every company failure observed in
benchmark runs. Read them once and treat them as hard constraints.

### The five laws

**LAW 1 — Payroll is the only clock that matters.**
Payroll fires every 2 weeks. If cash cannot cover it the company is terminated immediately.
Revenue is episodic; payroll is continuous. Never let headcount grow faster than revenue.

**LAW 2 — 35% of clients are RAT clients.**
RAT (Rogue/Adversarial/Toxic) clients are hidden and look identical to legitimate clients
until you check their history. Their tasks **always fail**, costing you trust, time, and
revenue. The only detection method: inspect `tasks_failed` in the client record BEFORE
every acceptance decision. `tasks_failed == 0` → safe. Any value > 0 → permanent block.

**LAW 3 — Trust is a multiplier, not a counter.**
Low trust reduces your team's effective work output by up to 50%. A single undetected RAT
client can cascade: one failure → trust drop → reduced capacity → missed deadlines →
more failures → death spiral. Protect trust aggressively. It is harder to rebuild than cash.

**LAW 4 — Context resets at turn 20.**
Your memory is wiped at turn 20. You will not know what decisions you made, what employees
you hired, or what tasks are running unless you explicitly re-read state. The mandatory
first action after ANY context gap or session restart: call the overview API to get funds,
active tasks, and employee list. Only then act.

**LAW 5 — Hiring is a permanent cost increase.**
Adding an employee raises the payroll floor immediately and persistently. Firing is slow and
costly. Never hire unless current revenue already covers the new payroll level for at least
6 consecutive payroll cycles at current task velocity.

---

### 🚨 Failure Mode Catalogue — How Companies Died in the Benchmark

**FAILURE: PAYROLL TRAP**
Pattern: hired aggressively early using all-8 employee assignment on large tasks → headcount
ballooned 2.7× before revenue caught up → bankrupt by week 6–8.
Rule: `funds > monthly_payroll × 6` BEFORE adding any employee. No exceptions.

**FAILURE: RAT SPIRAL**
Pattern: accepted tasks without checking client history → 3–5 consecutive task failures →
trust fell below 50% → effective team capacity halved → couldn't sustain payroll → dead.
Rule: check `tasks_failed` on EVERY task intake. This takes one API call. Skip it and die.

**FAILURE: CONTEXT AMNESIA**
Pattern: context reset at turn 20 → agent re-hired employees already on payroll (forgot) →
double payroll line → bankrupt next cycle.
Rule: overview API call is the MANDATORY first action after any break in continuity.
Never initiate hires, task acceptance, or employee assignment without first confirming
current state.

**FAILURE: SPLIT STAFFING DEATH**
Pattern: assigned 2–3 employees to each of 3 concurrent tasks → none had enough capacity
to complete on time → 3 simultaneous deadline failures → trust collapse.
Rule: ALL available employees on ONE task at a time. Breadth kills. Depth delivers.

**FAILURE: DOMAIN DRIFT**
Pattern: accepted high-payout tasks outside declared core domains → work quality degraded →
deadlines missed → prestige eroded → available client pool shrank → death spiral.
Rule: decline every task outside your declared domains regardless of payout. Domain
discipline is a revenue strategy, not a restriction.

**FAILURE: DEADLINE BLINDNESS**
Pattern: accepted a large task without verifying team could complete it before the deadline →
partial delivery → automatic failure → trust hit → cascaded.
Rule: before accepting, mentally calculate: (task_complexity / team_size) vs time_to_deadline.
If uncertain, choose a smaller task. Certainty of completion > size of payout.

---

### 💡 Hard Decision Rules — Apply These Mechanically

```
HIRE GATE:       funds > (monthly_payroll × 6)  AND  last 4 tasks all completed on time
TASK ACCEPT:     client.tasks_failed == 0
                 AND task.domain IN your_declared_domains
                 AND team_can_finish_before_deadline (estimate conservatively)
SURVIVAL MODE:   if funds < monthly_payroll × 3:
                   → stop all hiring
                   → accept only shortest-deadline in-domain tasks
                   → assign full team to single task
                   → no domain stretching
POST-RESET:      STEP 1: read VISION.md
                 STEP 2: GET overview → record funds, active_tasks, employee_list
                 STEP 3: resume exactly from that state
                 NEVER re-initiate something already running
STAFFING:        assign_employees = [ALL available employees]  ← always the full list
                 NEVER split across concurrent tasks
RAT BLOCK:       once client.tasks_failed > 0 → block permanently, no exceptions
```

---

"""

# ---------------------------------------------------------------------------
# CEO briefing addition — appended to the CEO agent file
# ---------------------------------------------------------------------------
CEO_AWARENESS = """\

---
<!-- YC-BENCH-CEO-AWARENESS-V1 -->
## Simulation Awareness — Your Operating Reality

You are the CEO of a company running inside a deterministic 1-year business simulation.
The following context is always true. It overrides any instruction that conflicts with it.

### What the simulation is

- **Duration**: 52 weeks. You operate turn-by-turn. Each turn you receive state and make decisions.
- **Win condition**: survive all 52 weeks with positive funds. Secondary: maximise total revenue.
- **Loss condition**: funds drop below payroll at any payroll cycle → immediate termination.

### Your primary job in this simulation

1. **Protect cash.** Every decision is evaluated against: "does this help me make payroll
   for the next 6 cycles?"
2. **Eliminate RAT clients.** 35% of available clients are adversarial. Their tasks always
   fail. Check `tasks_failed` before accepting anything. Block anyone with > 0 failures.
3. **Full-team single-task execution.** One task at a time. All employees assigned.
   No concurrent tasks. No split staffing.
4. **Context continuity.** At turn 20 your memory resets. After ANY gap: call overview API
   first. Confirm state before acting. Never assume you know current funds or headcount.

### Decisions you make every cycle (in order)

1. Check financial state: can I cover the next 3 payrolls at current velocity?
2. Scan available tasks: any from RAT-flagged clients? Filter them out immediately.
3. Pick the single best in-domain task your team can reliably complete before deadline.
4. Assign ALL employees to that one task.
5. Dispatch. Advance clock.

### What you must never do

- Accept a task without checking client `tasks_failed` first.
- Assign employees to more than one concurrent task.
- Hire when `funds < monthly_payroll × 6`.
- Accept tasks outside your declared core domains.
- Assume you know your current state after a context gap — always verify via overview API.
- Re-initiate something already running after a context reset.

### When to override normal strategy

If `funds < monthly_payroll × 3`:
- Stop accepting large tasks regardless of payout.
- Assign full team to the fastest-completing in-domain task available.
- Freeze all hiring.
- Do not accept speculative work.
- Survive first; grow second.
"""


def patch_vision(vision_path: Path, company_name: str) -> bool:
    text = vision_path.read_text(encoding="utf-8")
    if VISION_SENTINEL in text:
        print(f"  SKIP  VISION.md (already patched)")
        return False

    # Insert before the ⚠️ Operating Protocol section (or before Mission if no protocol)
    marker = "## ⚠️ Operating Protocol"
    if marker not in text:
        marker = "## Mission"
    if marker not in text:
        # Append after the header block (before first ## section)
        idx = text.find("\n## ")
        if idx == -1:
            text = text + "\n" + VISION_HEURISTICS
        else:
            text = text[:idx] + "\n" + VISION_HEURISTICS + text[idx:]
    else:
        text = text.replace(marker, VISION_HEURISTICS + marker, 1)

    vision_path.write_text(text, encoding="utf-8")
    print(f"  PATCH VISION.md")
    return True


def patch_ceo(agents_dir: Path, company_name: str) -> bool:
    ceo_dir = agents_dir / "ceo"
    if not ceo_dir.is_dir():
        print(f"  SKIP  CEO briefing (no agents/ceo/ dir)")
        return False

    md_files = list(ceo_dir.glob("*.md"))
    if not md_files:
        print(f"  SKIP  CEO briefing (no .md files in agents/ceo/)")
        return False

    # Patch the first (usually only) .md file
    target = md_files[0]
    text = target.read_text(encoding="utf-8")
    if CEO_SENTINEL in text:
        print(f"  SKIP  CEO briefing (already patched)")
        return False

    text = text + CEO_AWARENESS
    target.write_text(text, encoding="utf-8")
    print(f"  PATCH CEO briefing ({target.name})")
    return True


def main():
    companies = sorted(PACKS_ROOT.iterdir())
    patched_vision = patched_ceo = 0

    for company_dir in companies:
        if not company_dir.is_dir():
            continue
        name = company_dir.name
        print(f"\n[{name}]")

        vision = company_dir / "VISION.md"
        if vision.is_file():
            if patch_vision(vision, name):
                patched_vision += 1
        else:
            print(f"  SKIP  VISION.md (not found)")

        agents_dir = company_dir / "agents"
        if agents_dir.is_dir():
            if patch_ceo(agents_dir, name):
                patched_ceo += 1
        else:
            print(f"  SKIP  CEO briefing (no agents/ dir)")

    print(f"\nDone. Patched {patched_vision} VISION.md files, {patched_ceo} CEO briefings.")


if __name__ == "__main__":
    main()
