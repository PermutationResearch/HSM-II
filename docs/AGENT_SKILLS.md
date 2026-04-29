# Agent Skills in HSM-II

HSM-II’s personal agent treats on-disk skills as **[Agent Skills](https://github.com/agentskills/agentskills)**-compatible where possible. The normative format is defined in the **[Agent Skills specification](https://agentskills.io/specification)** (directory layout, required YAML front matter, optional `scripts/`, `references/`, `assets/`, progressive disclosure).

## What HSM-II does

- Scans `<HSMII_HOME>/skills` and `HSM_SKILL_EXTERNAL_DIRS` for `**/SKILL.md`.
- **Catalog** stores metadata for progressive disclosure; full `SKILL.md` body is loaded via `skill_md_read` or `/skill <slug>`.
- **Extra files** under the skill folder are loaded with `skill_resource_read` (relative paths only; no `..`).
- **`skills_list`** returns `skill_id` (YAML `name`), `skill_dir`, and optional `license`, `compatibility`, `metadata`, `allowed_tools` when present.
- **Validation**: mismatched `name` vs folder name, invalid `name` syntax, or missing `description` when using Agent Skills front matter → **warning** by default; set **`HSM_AGENT_SKILLS_STRICT=1`** to **exclude** those entries from the catalog.

## Upstream tooling

To validate packs against the reference rules, use the **`skills-ref`** CLI from the [agentskills/skills-ref](https://github.com/agentskills/agentskills/tree/main/skills-ref) tree when you have it installed (`skills-ref validate ./my-skill`).

## Seeds in this repo

See `templates/hsmii/skills/` for a compliant example (`name` matches the folder name, lowercase kebab-case).

## Why descriptions matter (tool + skill selection)

Models map user intent to **tool names/descriptions/schemas** and to **skill catalog blurbs** the same way they would pick a function: unclear text leads to skipped skills, wrong tools, or overuse of generic primitives. For a concise, model-centric explanation (Claude-style `tool_use`, Hermes-style function calling, progressive disclosure, pitfalls), load the on-disk skill **`llm-tool-skill-reasoning`** via `skill_md_read` or open `skills/llm-tool-skill-reasoning/SKILL.md`.
