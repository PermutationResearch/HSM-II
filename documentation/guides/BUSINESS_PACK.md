# Business pack authoring (one page)

Drop **`business/pack.yaml`** under your agent home (same level as `MEMORY.md` / `config.json`), or use **`business_pack.yaml`** at the home root. Optional: run **`hsm-business-pack validate`** before starting the agent.

## Folder layout

```text
<agent_home>/
  config.json
  MEMORY.md
  business/
    pack.yaml              # required in this layout
    policies/              # short, stable rules
    knowledge/             # FAQs, exports from email/CRM, pricing tables
    personas/              # voice samples per role (optional)
```

**Anti-pattern:** pasting the same policy paragraph in YAML `shared_policies`, a policy file, *and* every persona’s instructions. Pick **one source of truth** (usually a file) and reference it from YAML with a single line if needed.

## Persona selection (precedence)

1. **`HSM_BUSINESS_PERSONA`** environment variable (highest).  
2. **`business_persona`** in `config.json` (see `EnhancedAgentConfig`).  
3. If neither is set: **shared** company + policies + knowledge are injected; persona-specific blocks are skipped (you’ll see a hint in the prompt).

## Injection merge order (system prompt)

For `EnhancedPersonalAgent` chat (non-council tools path), the rough order is:

1. RLM living prompt seed / rendered living prompt  
2. **Business pack block** (company, `last_reviewed`, shared policies, loaded policy file excerpts, knowledge excerpts, then active persona)  
3. Belief snippets, CASS skill match, AutoContext hints  
4. Tool list and tool-call instructions  

Council synthesis path also prepends the business block after the living prompt.

## Size limits (hard caps in code)

| Asset | Cap |
|--------|-----|
| Each `policy_files` entry | 48 KiB read (truncated with warning) |
| Each `knowledge_files` / persona `extra_files` entry | 64 KiB (truncated with warning) |
| Total knowledge excerpt section in the injected prompt | ~120 KiB (further chunks dropped) |

These are **bytes**, not LLM tokens. If you need more context, split into multiple files and prioritize what you list first in `knowledge_files`.

## Schema version

Only **`schema_version: 1`** is accepted today. Bumping the format requires a code migration path.

## Ops logging (no secrets)

With `RUST_LOG` including `hsm_business_pack` (or default tracing for your binary):

- **Startup:** company name, industry, `schema_version`, `last_reviewed`, persona **keys** (not contents).  
- **Each message:** `active_persona` and **relative paths** in `bound_file_paths` (policy + knowledge + that persona’s `extra_files`).

Content of emails or policies is **not** logged here.

## Starters (copy-paste businesses)

From repo root:

```bash
cargo run -p hyper-stigmergy --bin hsm-business-pack -- list-starters
cargo run -p hyper-stigmergy --bin hsm-business-pack -- init generic_smb --to ~/.hsmii/business
```

Available starters: **`generic_smb`**, **`property_management`**, **`gestion_velora`**, **`velora_enticy`** (Syndicat ENTICY + marketing personas), **`online_commerce_squad`** (e-commerce / growth multi-persona + DSPy/GEPA bridge doc), **`construction_trades`**, **`online_services`**, **`marketing_solo`**.

## CLI validate

```bash
cargo run -p hyper-stigmergy --bin hsm-business-pack -- validate --home ~/.hsmii
cargo run -p hyper-stigmergy --bin hsm-business-pack -- validate --pack ./business/pack.yaml
```

Errors (bad schema, empty company name, unsafe paths) exit **non-zero** and **block** agent load. Warnings (missing files, empty persona instructions) still allow load but are printed at startup.
