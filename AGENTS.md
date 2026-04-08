# Agent notes

## External skill libraries

1. **This repo — Remotion company ad template**  
   Skill: **`remotion-company-ad`** → `.claude/skills/remotion-company-ad/SKILL.md` (mirror: `.cursor/skills/remotion-company-ad/SKILL.md`).  
   Code: **`web/remotion-company-console-ad/`**.

2. **Nous Hermes — optional cross-domain skills**  
   When a task fits categories not covered here, consult **[hermes-agent/skills](https://github.com/NousResearch/hermes-agent/tree/main/skills)** (e.g. creative, media, software-development, data-science, devops). Read the relevant folder’s instructions from GitHub (or clone the repo) and adapt to this codebase.

Cursor loads the Hermes pointer from **`.cursor/rules/hermes-skills.mdc`**.

## Company OS — intelligence layer vs ledger

When working on **`hsm_console`**, Postgres Company OS, or DRIs: canonical graph and integration patterns are in **`docs/company-os/world-model-and-intelligence.md`**. For DRI / “composer vs ledger vs edge” alignment (external intelligence services, governance, escalation), see **`docs/company-os/intelligence-layer-dri-alignment.md`**.
