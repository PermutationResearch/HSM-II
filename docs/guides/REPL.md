# Headless REPL Usage

The `hyper-stigmergy` binary now ships a `--repl` flag that boots the live system without launching the TUI or web server. All stateful services (World, Council, SkillBank, RooDB, vault, chat) remain live, so the REPL commands interact with the same background event stream that the UI and HTTP endpoints use.

## Running

```sh
cargo run --bin hyper-stigmergy -- --repl
```

Once started you can type `help` to see the supported commands.

## Key Commands

| Command | Description |
| --- | --- |
| `plan` | Print a summary of the latest council plan steps. Use `plan <index>` to view a step's claim, plan text, linked skills, evidence messages, and QMD IDs. |
| `optimize <index>` | Trigger `PlanOptimize` on the matching plan step and capture the plan metadata + hashed trace in the answer dictionary. |
| `hire list` | Show the top few active hire trees generated during orchestration. |
| `hire complete <id> [status] [score]` | Mark a delegation hire as `completed`, `failed`, or `revoked` and emit a `HireComplete` event (score defaults to `0.75`). |
| `vault list` | Scan the configured vault directory (default `HSM_VAULT_DIR`) and report note counts/tags. |
| `vault search <query>` | Run semantic search over RooDB embeddings (uses Ollama for embeddings and `qmd` for hybrid hits). Supports wildcard (`vault search *`). |
| `vault index` | Re-embed the vault notes by calling the same `index_vault_embeddings` job the HTTP API exposes. |
| `status` | Show agent counts, plan steps, average JW, active hires, and RooDB status. |
| `council` | Run the council loop just like `/council` from the UI. |
| `inspect` / `clear` | Display / reset the last `AnswerDict`. |

## Answer Dictionary Contract

Every command populates an `AnswerDict` that the REPL prints and retains for `inspect`. The shape is:

```rust
struct AnswerDict {
    content: String,
    ready: bool,
    metadata: serde_json::Value,
    status: Option<String>,
    trace_hash: Option<String>,
}
```

- `content`: human-readable response (the plan summary, vault search lines, etc.).
- `ready`: `true` when the response is complete.
- `metadata`: structured data (step counts, skill IDs, message IDs, queries) that can feed downstream tooling.
- `status`: semantic label (`"plan"`, `"optimize"`, `"vault"`, etc.).
- `trace_hash`: low-cost hash of the dominant artifact (plan text, query, or hire ID) so workflows can reference the same evidence.

## Monitoring Hooks

- `status` now reports average JW across agents, the number of active hire trees, and whether RooDB is connected (via `roodb_url`).
- REPL commands reuse the existing `BgEvent` bus, so triggering `plan`, `optimize`, or `hire complete` still flows through the same persistence, JW, and skill-bank hooks as the UI.
