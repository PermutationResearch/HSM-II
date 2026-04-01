# DSPy, GEPA, and meta-harness — how HSM-II fits “The Arc”

This repo supports **agentic loops** where sub-agents fetch context and tools, and **optimization loops** that improve prompts/signatures over measured runs—not a single monolithic prompt for everything.

## 1. Concepts

- **DSPy-style optimization** — `hyper_stigmergy::dspy`: demonstrations, traces, `optimize_signature` / `optimize_all_signatures` backed by the project database.  
- **GEPA** — `src/bin/hsm_gepa.rs`: collect failure traces, cluster, drive mutation order for optimization bundles.  
- **Meta-harness** — `src/bin/hsm_meta_harness.rs`: search over harness / eval configurations; produces artifacts (e.g. `turns_hsm.jsonl`) for analysis.  
- **optimize_anything** — `src/optimize_anything/`: broader optimization entry points used by eval and tooling.  
- **Trace → skill** — `hsm_trace2skill` / `trace2skill`: turn eval traces into trajectory records for replay and skill promotion.

## 2. Practical use per commerce persona

Treat each **persona** (supplier_sourcing, product_creative, merchandising_copy, social_media_manager, logistics_fulfillment) as a **separable skill surface**:

1. Capture **gold demonstrations** (good supplier emails, PDPs, posts, policy copy).  
2. Run **evals** or scripted scenarios (`hsm-eval`, `hsm_meta_harness`) with clear metrics (rubric scores, tool success, human labels).  
3. Run **GEPA / DSPy** optimization on the signature or template tied to that role.  
4. Promote improved prompts into **business pack knowledge** or **AGENTS.md** snippets for that deployment.

## 3. Cost and architecture note

Moving from “one giant prompt” to **specialized sub-agents + smaller models** for routine steps, reserving large models for hard cases, is an **operational** choice—mirror it in `operations.yaml` **budgets** and in which persona runs which tool set (`HSM_TOOL_ALLOW` / block prefixes).

## 4. Commands (illustrative)

Exact flags evolve; prefer `--help` on each binary:

- `cargo run -p hyper-stigmergy --bin hsm_gepa -- --help`  
- `cargo run -p hyper-stigmergy --bin hsm_meta_harness -- --help`  
- `cargo run -p hyper-stigmergy --bin hsm-eval -- --help`  

This file is **guidance**, not a substitute for your runbooks and legal review.
