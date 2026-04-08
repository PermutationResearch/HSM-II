# Seed Agent Skills–compatible folders

These layouts follow the open [Agent Skills](https://github.com/agentskills/agentskills) format ([specification](https://agentskills.io/specification)): a directory named like `plan/` with `SKILL.md` (YAML front matter + body), optional `scripts/`, `references/`, `assets/`.

Install under the agent home:

`$HSMII_HOME/skills/<skill-folder>/SKILL.md`

The front matter `name` must be **lowercase kebab-case** and **match the parent folder name** (e.g. folder `plan` → `name: plan`).

Example:

```bash
mkdir -p "$HSMII_HOME/skills"
cp -R templates/hsmii/skills/plan "$HSMII_HOME/skills/"
```

Optional extra roots (comma-separated): `HSM_SKILL_EXTERNAL_DIRS`. Validate with the upstream [skills-ref](https://github.com/agentskills/agentskills/tree/main/skills-ref) CLI when installed.

HSM-II CLI: `cargo run --bin hsm_skills -- list` with `HSMII_HOME` set. Set `HSM_AGENT_SKILLS_STRICT=1` to reject non-compliant skills at catalog load.
