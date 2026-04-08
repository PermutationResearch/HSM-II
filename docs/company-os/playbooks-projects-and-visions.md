# Playbooks by project, with `visions.md` as north star

This note aligns **how procedures are organized** in Company OS with **where the company is going**.

## Projects first

In HSM Company OS, a **project** is a Paperclip-style **container for tasks** (`GET/POST /api/company/companies/{id}/projects`; tasks may set `project_id` when created). Treat playbooks that *ship work* as:

- **Scoped by project** — issues, implementation tasks, and “how we run this product area” live under that project’s task graph and attachments.
- **Shared across projects only when intentional** — use **`companies.context_markdown`**, **`company_memory_entries`** with `scope: shared` (and `kind: broadcast` when you mean “everyone should see this”), or a small number of **company-wide** SOPs—not a single giant playbook that ignores boundaries.

So: **one playbook narrative per project** (or per stream inside a project), not one undifferentiated blob for the whole company.

## `visions.md` — direction, not procedure

Keep a **`visions.md`** at the **pack / company workspace root** (alongside or above per-agent files). It should capture:

- **Why** the company (or this workspace) exists — outcomes, principles, non-goals.
- **What “good” looks like** for the next horizon (quarter, year), without prescribing every step.
- **Constraints** that trump local optimization (brand, risk, compliance, architecture).

**Playbooks and SOPs** answer *how*; **`visions.md`** answers *whether we should* and *what to optimize for*. When a playbook conflicts with the vision, **update the playbook** or **explicitly amend the vision**—do not let drift stay implicit.

## How this composes with `AGENTS.md` and memory

| Artifact | Role |
|----------|------|
| **`visions.md`** | North star; reviewers and agents check new playbooks/tasks against it. |
| **`AGENTS.md`** | Operational index: roles, tools (`company_memory_*`, stigmergic notes, `llm-context`), where feedback goes (issues, shared memory), pointers to `visions.md` and key project folders. |
| **Per-project playbooks / SOPs** | Executable procedure + task templates under that project. |
| **Shared / broadcast memory** | Cross-cutting “everyone must know” facts; should still be **consistent** with `visions.md`. |

Agent-scoped memory remains **private to that workforce row** in `llm-context`; **shared** pool + **company context** are how you avoid N× duplicated “vision fragments” in private notes.

## Practical habit

1. When adding or changing a playbook, ask: **which project** owns it, and **one line** in the doc that ties to **`visions.md`** (“Supports vision §…”).  
2. For company-wide announcements, prefer **`scope: shared`** memory or a **`broadcast`** entry plus a **single task** or **retro issue** that links the run or decision—still tagged to a project when the work is owned there.

See also:

- [`docs/company-os/intelligence-layer-dri-alignment.md`](./intelligence-layer-dri-alignment.md) — intelligence vs Company OS vs DRIs; integration checklist.  
- [`docs/issues/memory-workspace-shared-context.md`](../issues/memory-workspace-shared-context.md) — shared vs agent memory in **`GET …/tasks/{id}/llm-context`**.  
- [`docs/company-os/world-model-and-intelligence.md`](./world-model-and-intelligence.md) — canonical Postgres graph vs optional Paperclip layer; console/agents/issues as one model.
