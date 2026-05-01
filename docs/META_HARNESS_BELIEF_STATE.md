# Meta-harness belief state (harness log)

This describes the optional **`belief_state`** object emitted by **`scripts/meta-harness/evaluate_turn.py`** next to the existing composite **`score`**. It is a **deliberate, low-dimensional logging model** inspired by Bayesian control ideas in Papamarkou et al., *Position: Agentic AI systems should be making Bayes-consistent decisions* (SSRN 6143772; stable copy: [HAL hal-05480691](https://hal.science/hal-05480691)).

## What it is not

- **Not** production routing for `hsm_console`, agent-chat, or promoted **`HsmRunnerConfig`** (see **`docs/EVAL_AND_META_HARNESS.md`**).
- **Not** a calibrated value-of-information (VoI) calculation for real tool costs.
- **Not** a claim that the LLM is Bayesian; only that the **harness** summarizes turn evidence in a conjugate-friendly scalar for **offline analysis and smoke regressions**.

## Model (`harness_beta_task_success_v1`)

Single **Beta** pseudo-posterior over a latent “task succeeded well enough for this harness” quantity:

- **Online-ish updates:** each stream **`error`** line adds failure mass; **`tool_start` / `tool_end`** (and **`sub_agent_spawned`**) increments tool count and, past soft thresholds, add small failure mass (heavy tool use).
- **Terminal update:** **`finalize_response` / `final_answer` / `done`** outcomes, answer length, “no tools” prior, and lack of finalize add success or failure mass.

Fields in JSON:

| Field | Meaning |
|--------|--------|
| `task_success.alpha`, `task_success.beta` | Beta pseudo-counts after the turn. |
| `task_success.posterior_mean` | `α / (α + β)`. |
| `task_success.entropy_nats` | Bernoulli entropy of the posterior mean (diagnostic). |
| `voi_proxy.extra_tool_nats` | **Proxy only:** entropy × headroom under a soft tool budget (see `belief_state.py`). |
| `voi_proxy.information_saturated` | Heuristic “little left to learn” / extreme-mean flag for quick filtering. |

## Code

- **`scripts/meta-harness/belief_state.py`** — `HarnessBeliefV1`, `BetaBelief`, VoI proxy helper.
- **`scripts/meta-harness/evaluate_turn.py`** — integrates belief updates in the same pass as **`score_turn`**.

## Future work (product-side)

A real **Bayes-consistent control layer** would live next to orchestration (tool choice, budgets, stopping), use **explicit utilities and costs**, and be **validated** against telemetry — with the Python harness used only to **measure** behavior, not to substitute for that layer.
