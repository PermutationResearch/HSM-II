## Operator playbook (paste into pack `AGENTS.md` or equivalent)

- **Vision:** Read **`visions.md`** at this pack root first. Playbooks are **by project**; they implement direction from the vision—if something conflicts, fix the playbook or update the vision explicitly.
- **Feedback:** Route actionable work to **tasks/issues** with `owner_persona` set to the right agent (e.g. engineering). Link **run ids** or **workspace paths** in the spec. Use **stigmergic notes** on the task for handoffs. For “everyone must know,” use **`scope: shared`** / **`broadcast`** company memory or **`context_markdown`**, still consistent with **`visions.md`**.
- **Memory:** **`llm-context`** includes **shared** pool + **this agent’s** scoped entries—not other agents’ private scoped rows; coordinate via **shared** entries, tasks, and company context.
- **Tools:** `company_memory_search` / `company_memory_append`; `GET …/memory/export.md` for a git-friendly index; task **`workspace_attachment_paths`** for stable file pointers under `hsmii_home`.
- **`company_memory_append` (APR):** The API requires an explicit `scope` each call (`shared` or `agent`)—never omit it. **Default bias:** use **`shared`** for anything another teammate would need (policies, canonical env URLs, decisions, incident facts). Reserve **`agent`** for private working style, scratch, or data that must not enter the company pool. High-signal “everyone read this” → **`shared`** + **`kind`: broadcast** (see tool schema).
- **`company_memory_search`:** Omit `mode` or use **`shared`** for the company pool; **`mine`** for this agent’s rows only; **`both`** when the task mixes company truth with private notes.

See **`docs/company-os/playbooks-projects-and-visions.md`** for the full model.
