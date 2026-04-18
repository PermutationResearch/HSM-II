#!/usr/bin/env python3
"""
patch_mechanical_rules.py — inject mechanical if/then decision rules into every
CEO agent briefing AND every VISION.md Operating Protocol section.

The prior patches added good philosophy. This adds the mechanical layer:
NOT "be disciplined about clients" but "IF client.tasks_failed > 0 THEN decline,
block permanently, log to scratchpad."

Safe to re-run (sentinel-gated).
"""

from pathlib import Path

PACKS_ROOT = Path.home() / ".hsm" / "company-packs" / "paperclipai" / "companies"

CEO_RULES_SENTINEL = "<!-- MECHANICAL-RULES-V1 -->"
VISION_RULES_SENTINEL = "<!-- MECHANICAL-VISION-RULES-V1 -->"

# ---------------------------------------------------------------------------
# Universal mechanical rules block — appended to CEO briefings
# ---------------------------------------------------------------------------

CEO_MECHANICAL_RULES = """\

---
<!-- MECHANICAL-RULES-V1 -->
## Decision Rules — Execute These Mechanically, Every Turn

These are not guidelines. They are executable decision trees. Run them in order.

---

### RULE 1 — Task acceptance gate (run before EVERY accept)

```
BEFORE accepting any task:
  1. GET client history
  2. IF client.tasks_failed > 0:
       → DECLINE task
       → ADD client to scratchpad blocklist: "BLOCKED: {client_name} — tasks_failed={n}"
       → NEVER accept from this client again, regardless of reward size
  3. IF task.domain NOT IN your_declared_domains:
       → DECLINE task
       → Reason: domain drift costs prestige and compounds into loss
  4. IF task.deadline - now < estimated_completion_time:
       → DECLINE task
       → Conservative estimate: assume 20% overrun on all task complexity
  5. IF all checks pass:
       → ACCEPT task
       → ASSIGN all available employees (list every employee name, comma-separated)
       → DISPATCH immediately — never leave a task in planned state
```

---

### RULE 2 — Employee assignment (run on EVERY task dispatch)

```
WHEN dispatching a task:
  1. List ALL employees currently available (not on active tasks)
  2. Assign ALL of them — the full comma-separated list
  3. IF you feel tempted to assign a subset "to save some for other tasks":
       → STOP. You are about to make the split-staffing mistake.
       → ONE task at a time. ALL employees on that task.
  4. DISPATCH the task with the full employee list
```

---

### RULE 3 — Hiring gate (run before ANY new hire)

```
BEFORE hiring:
  1. Calculate: monthly_payroll_current = sum of all employee salaries
  2. Calculate: monthly_payroll_after = monthly_payroll_current + new_employee_salary
  3. IF funds < monthly_payroll_after × 6:
       → DO NOT HIRE
       → Reason: need 6× monthly payroll buffer before any hire
  4. IF last 4 completed tasks did NOT all complete on time:
       → DO NOT HIRE
       → Reason: capacity is not the bottleneck — quality of execution is
  5. IF both checks pass:
       → Proceed with hire
```

---

### RULE 4 — Financial alarm trigger

```
EVERY turn, check:
  IF funds < monthly_payroll × 3:
    → ENTER SURVIVAL MODE:
      - Accept ONLY shortest-deadline, in-domain tasks
      - Assign ALL employees to single task
      - Decline ALL tasks with deadline > 2 weeks
      - FREEZE hiring (no exceptions)
      - Log to scratchpad: "SURVIVAL MODE ACTIVE — funds={n}, payroll={n}"

  IF funds < monthly_payroll × 1.5:
    → EMERGENCY:
      - Accept only tasks completable within current week
      - Flag to scratchpad: "EMERGENCY — {turns} turns until insolvency at current burn"
```

---

### RULE 5 — Context reset recovery (run after ANY break in continuity)

```
IF you are unsure what state the company is in:
  STEP 1: GET /api/company/companies/{id}/ops/overview
  STEP 2: Record in scratchpad:
    - funds: {value}
    - monthly_payroll: {value}
    - active_tasks: {list}
    - employees: {list with names}
    - blocked_clients: {blocklist from previous turns}
  STEP 3: Resume from that state ONLY
  NEVER: initiate a hire if you already have employees (check first)
  NEVER: accept a task that is already active (check first)
  NEVER: re-block a client you already blocked (they stay blocked)
```

---

### RULE 6 — Scratchpad discipline (persistent memory across context resets)

```
MAINTAIN in scratchpad at all times:
  blocked_clients: [{name, tasks_failed, date_blocked}]
  trusted_clients: [{name, trust_level, work_reduction_factor}]
  financial_state: {funds, monthly_payroll, payroll_buffer_cycles}
  active_tasks: [{task_id, client, deadline, employees_assigned}]
  last_updated_turn: {turn_number}

UPDATE scratchpad:
  - After every task acceptance: add task to active_tasks
  - After every task completion: update client trust, remove from active_tasks
  - After every blocked client: add to blocked_clients
  - After every hire: update monthly_payroll calculation
  - After every turn: update last_updated_turn
```

---

### RULE 7 — Trust building (run after every successful task completion)

```
AFTER a task completes successfully:
  1. Note client in scratchpad as trusted: {client_name, tasks_completed=n}
  2. IF client.tasks_completed >= 3 AND client not on blocklist:
       → Prioritise future tasks from this client
       → Reason: trust reduces work quantity by up to 50% — same reward, half the effort
  3. IF client.tasks_completed >= 5:
       → Flag as anchor client: always accept their tasks if domain matches
```

---

### RULE 8 — Brand and domain discipline (run on every task review)

```
WHEN evaluating a task:
  IF task.domain NOT IN declared_domains:
    → DECLINE, no exceptions
    → Even if reward is 2× your best in-domain task
    → Reason: off-domain tasks fail at higher rates and erode prestige in your actual domain

  IF task feels ambiguous about domain fit:
    → Ask: "Would completing this task build my reputation in {domain}?"
    → IF yes: accept (if client check passes)
    → IF no: decline
```
"""

# ---------------------------------------------------------------------------
# Mechanical rules addition to VISION.md Operating Protocol
# ---------------------------------------------------------------------------

VISION_MECHANICAL_RULES = """\
<!-- MECHANICAL-VISION-RULES-V1 -->
### 🔧 Mechanical Operating Rules — Execute These, Do Not Interpret Them

**Task acceptance — run this exact sequence every cycle:**
1. `GET client_history(client_id)` → if `tasks_failed > 0`: DECLINE + BLOCK permanently
2. Check `task.domain IN declared_domains` → if NO: DECLINE
3. Estimate completion time → if `deadline < estimated_time × 1.2`: DECLINE
4. All checks passed → ACCEPT → assign ALL employees → DISPATCH → advance clock

**Employee assignment — no exceptions:**
- Assign: the complete list of all available employees, every time
- Never split the team across two concurrent tasks
- If you want to "save some employees" for later: that is the split-staffing mistake

**Hiring — numerical gate:**
- `IF funds < monthly_payroll × 6` → DO NOT HIRE, end of decision
- `IF last 4 tasks not all on-time` → DO NOT HIRE, capacity is not the problem

**Financial alarm — check every turn:**
- `IF funds < monthly_payroll × 3` → SURVIVAL MODE: fastest in-domain task only, freeze hiring
- `IF funds < monthly_payroll × 1.5` → EMERGENCY: log turns-to-insolvency in scratchpad

**Context reset — mandatory first action:**
- GET overview → read funds, employees, active tasks → write to scratchpad → THEN act
- Never assume you know current state after any gap

**Scratchpad — always current:**
- blocked_clients list (name + reason + date)
- trusted_clients list (name + trust level + completion count)
- financial_state (funds, monthly_payroll, buffer cycles)
- active_tasks (id, client, deadline, employees)

"""


def patch_ceo_rules(agents_dir: Path) -> bool:
    ceo_dir = agents_dir / "ceo"
    if not ceo_dir.is_dir():
        return False
    md_files = list(ceo_dir.glob("*.md"))
    if not md_files:
        return False
    target = md_files[0]
    text = target.read_text(encoding="utf-8")
    if CEO_RULES_SENTINEL in text:
        print(f"    SKIP  CEO mechanical rules (already patched)")
        return False
    target.write_text(text + CEO_MECHANICAL_RULES, encoding="utf-8")
    print(f"    PATCH CEO briefing → mechanical rules ({target.name})")
    return True


def patch_vision_rules(vision_path: Path) -> bool:
    text = vision_path.read_text(encoding="utf-8")
    if VISION_RULES_SENTINEL in text:
        print(f"    SKIP  VISION.md mechanical rules (already patched)")
        return False

    # Insert after the existing ⚠️ Operating Protocol — before "**Why this is non-negotiable**"
    # or after the last numbered step if that section doesn't exist
    marker = "**Why this is non-negotiable**"
    if marker in text:
        text = text.replace(marker, VISION_MECHANICAL_RULES + marker, 1)
    else:
        # Insert after the Operating Protocol header block, before Mission
        marker2 = "\n## Mission"
        if marker2 in text:
            text = text.replace(marker2, "\n" + VISION_MECHANICAL_RULES + "\n## Mission", 1)
        else:
            text = text + "\n" + VISION_MECHANICAL_RULES

    vision_path.write_text(text, encoding="utf-8")
    print(f"    PATCH VISION.md → mechanical operating rules")
    return True


def main():
    patched_ceo = patched_vision = 0
    companies = sorted(
        d for d in PACKS_ROOT.iterdir() if d.is_dir()
    )
    for company_dir in companies:
        print(f"\n[{company_dir.name}]")

        agents_dir = company_dir / "agents"
        if agents_dir.is_dir():
            if patch_ceo_rules(agents_dir):
                patched_ceo += 1

        vision = company_dir / "VISION.md"
        if vision.is_file():
            if patch_vision_rules(vision):
                patched_vision += 1

    print(f"\nDone. {patched_ceo} CEO briefings + {patched_vision} VISION.md files patched with mechanical rules.")


if __name__ == "__main__":
    main()
