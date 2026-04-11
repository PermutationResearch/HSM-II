# Runtime capability matrix — HSM-II (as of bootstrap)

Legend: **Y** yes · **P** partial · **N** not in-tree / not default · **?** environment-dependent

| Capability | Status | Notes |
|------------|--------|--------|
| Repo read | **Y** | Full tree. |
| Repo write | **Y** | Agent/dev with filesystem access. |
| Shell | **Y** | Harness, tools, local dev. |
| Filesystem search | **Y** | ripgrep, tooling. |
| File edit | **Y** | Agents + IDE. |
| Git | **Y** | Tools, worktrees in harness docs. |
| Network | **P** | Allowed with policy; fetch tools gated. |
| Package install | **Y** | `cargo`, `npm` (developer machine). |
| Local database | **Y** | Postgres Company OS; SQLite in outer-loop tooling. |
| Browser control | **P** | Hermes bridge / tools — not universal product default. |
| Screenshot / vision | **P** | Tool-dependent. |
| Desktop input | **N**/ **P** | company-console-desktop; not general RPA. |
| Tool-calling | **Y** | Rich `src/tools/` registry. |
| Sub-agent / spawn | **P** | Task spawn, personas; bounded depth policy TBD in code. |
| Long-running background | **P** | Runs, polling; durable waits partially productized. |
| Cron / schedule | **P** | External + recurring concepts in docs; not fully unified. |
| Webhooks / events | **P** | Runtime events stream in console paths. |
| Persistent storage | **Y** | Postgres + `runs/` artifacts. |
| UI / dashboard | **Y** | company-console. |
| Secret management | **P** | Env, credentials panels; encrypt-at-rest policy product-dependent. |
| Approvals / interrupt | **P** | Approvals UI + run pause patterns. |
| Multi-machine | **P** | Architecture allows; not assumed in M1. |

**Gaps to close first (highest leverage):** explicit **verifier** step in product loop for generic tasks; **promoted eval config → live runtime** mapping (documented as risk in `EVAL_AND_META_HARNESS.md`); **momentum metrics** automation.
