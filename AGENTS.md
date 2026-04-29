# Agent notes

## External skill libraries

1. **This repo — Remotion company ad template**  
   Skill: **`remotion-company-ad`** → `.claude/skills/remotion-company-ad/SKILL.md` (mirror: `.cursor/skills/remotion-company-ad/SKILL.md`).  
   Code: **`web/remotion-company-console-ad/`**.

2. **Nous Hermes — optional cross-domain skills**  
   When a task fits categories not covered here, consult **[hermes-agent/skills](https://github.com/NousResearch/hermes-agent/tree/main/skills)** (e.g. creative, media, software-development, data-science, devops). Read the relevant folder’s instructions from GitHub (or clone the repo) and adapt to this codebase.

Cursor loads the Hermes pointer from **`.cursor/rules/hermes-skills.mdc`**.

## On-disk skills and tool selection

HSM-II loads **Agent Skills** from `<HSMII_HOME>/skills` and **`HSM_SKILL_EXTERNAL_DIRS`** (see **`docs/AGENT_SKILLS.md`**). The catalog stays small in context; agents pull full playbooks with **`skill_md_read`** / `/skill <slug>`. Models pick tools and skills the same way they pick functions: **name, description, and JSON argument schemas** must make the match obvious—vague `SKILL.md` descriptions get ignored or misused. For a concise mental model (Claude-style `tool_use`, Hermes-style function calling, progressive disclosure, pitfalls), read skill **`llm-tool-skill-reasoning`** (`skills/llm-tool-skill-reasoning/SKILL.md`). Starting Company OS from the repo prepends **`./skills`** via `scripts/company-os-up.sh` when that path is not already listed in `HSM_SKILL_EXTERNAL_DIRS`.

## Company OS — intelligence layer vs ledger

When working on **`hsm_console`**, Postgres Company OS, or DRIs: canonical graph and integration patterns are in **`docs/company-os/world-model-and-intelligence.md`**. For DRI / “composer vs ledger vs edge” alignment (external intelligence services, governance, escalation), see **`docs/company-os/intelligence-layer-dri-alignment.md`**.

## Eval and meta-harness

Canonical guide: **`docs/EVAL_AND_META_HARNESS.md`** — `hsm-eval` vs `hsm_meta_harness` vs `hsm_outer_loop`, artifact paths, smoke recipes. Promoted **`HsmRunnerConfig`** / `best_config.json` applies to **eval binaries** until explicitly wired into the live agent runtime.

## Agent OS program (long-horizon operating discipline)

Portable **file pack** for the “principal architect” protocol: operating summary, implementation contract, capability matrix, milestones, momentum queues, verification checklists, smoke script.

- **Start here:** `docs/agent-os-program/OPERATING_SUMMARY.md`
- **M1 smoke:** `bash scripts/agent-os-milestone1-smoke.sh` (requires `hsm_console` or `HSM_CONSOLE_URL`)

## Execution discipline (team default)

Use these defaults for Company OS engineering work:

- Plan first for non-trivial tasks (3+ steps, architecture, migrations): track plan in `tasks/todo.md`.
- Re-plan immediately when runtime evidence contradicts the current plan.
- Use subagents for focused exploration/analysis where parallelism helps.
- Do not mark complete without verification evidence (tests, logs, and behavior diff when relevant).
- Never state that external actions happened unless they were executed and evidenced in-session (for example PR creation, deployment, assignment, or release).
- After planning, execute the first concrete steps immediately when permissions allow; if blocked, report the blocker and the next runnable command.
- Prefer elegant minimal changes; avoid patch stacking when a cleaner approach is obvious.
- Treat bug reports as autonomous fix tasks: reproduce, gather evidence, fix, verify.
- After user corrections, capture the pattern and new guardrail in `tasks/lessons.md`.
