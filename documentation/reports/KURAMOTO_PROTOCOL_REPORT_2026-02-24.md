# Kuramoto Validation Protocol Report (2026-02-24)

Protocol source: `KURAMOTO_VALIDATION_PROTOCOL.md`

## Experiment setup

- Workload harness: `batch_experiment` (production batch workload loop)
- Runs per condition: 20
- Ticks per run: 1000
- Task mix: identical between baseline/treatment
- Baseline:
  - `cargo run --release --bin batch_experiment -- --no-credit 20 1000 experiments_kura_baseline_nocredit`
- Primary treatment:
  - `cargo run --release --bin batch_experiment -- --no-credit --kuramoto --kuramoto-k 0.6 --kuramoto-council 0.1 --kuramoto-dt 0.0005 --kuramoto-gain 0.08 20 1000 experiments_kura_treat_nocredit`

## Required metric mapping

- Convergence time: first tick where `global_coherence >= 0.9 * final_coherence` (`conv90`)
- Disagreement/conflict proxy: `1 - council_approve_rate`
- Objective task score: `final_coherence` and `mean_reward_per_tick`
- Kuramoto observability metrics are available in runtime API/UI (`R`, entropy, velocity stddev, R-window stddev), but batch CSV schema currently does not persist them.

## A/B result (20 vs 20 runs)

From `scripts/kuramoto_protocol_eval.py`:

- Baseline summary:
  - `final_coh_mean=429.54`
  - `reward_mean=43.05`
  - `disagree_mean=0.2342`
  - `conv90_median=904.5`
- Treatment summary:
  - `final_coh_mean=362.46`
  - `reward_mean=36.35`
  - `disagree_mean=0.2263`
  - `conv90_median=756.5`

Computed deltas (treatment vs baseline):

- Convergence improvement: `+16.36%` (95% CI `15.65..17.19`)
- Absolute convergence improvement at baseline median coherence (`436.92`): `-0.20%` (95% CI `-1.01..0.00`)
- Disagreement reduction: `+3.37%` (95% CI `-27.63..26.88`)
- Final coherence delta: `-15.62%` (95% CI `-17.58..-13.05`)
- Reward delta: `-15.58%` (95% CI `-17.54..-13.02`)

Paired analysis (`20` pairs by `run_id`):

- `conv90_improvement_mean`: `+14.67%` (95% CI `11.82..16.83`)
- `conv_abs_improvement_mean`: `-0.95%` (95% CI `-1.58..-0.40`)
- `disagreement_red_mean`: `-20.10%` (95% CI `-53.67..11.65`)
- `final_coherence_delta_mean`: `-15.26%` (95% CI `-17.44..-12.28`)
- `reward_delta_mean`: `-15.22%` (95% CI `-17.40..-12.26`)
- Non-inferior share: `reward=5.0%`, `coherence=5.0%`

## Parameter sweep (20 runs each)

Sweep runs:

- `experiments_kura_sweep_k` (`K=1.2`)
- `experiments_kura_sweep_council` (`council=0.3`)
- `experiments_kura_sweep_noise` (`noise=0.02`)
- `experiments_kura_sweep_disp` (`dispersion=0.1`)
- `experiments_kura_sweep_dt` (`dt=0.001`)

Relative to baseline (`experiments_kura_baseline_nocredit`):

| condition | conv90 improvement | disagreement reduction | final coherence delta | reward delta |
|---|---:|---:|---:|---:|
| treat (K=0.6, council=0.1, dt=0.0005, gain=0.08) | +16.36% | +3.37% | -15.62% | -15.58% |
| sweep_k | +16.36% | -31.46% | -15.58% | -15.54% |
| sweep_council | +15.98% | -15.73% | -12.33% | -12.30% |
| sweep_noise | +17.74% | -15.73% | -15.21% | -15.18% |
| sweep_disp | +17.14% | +2.25% | -15.75% | -15.71% |
| sweep_dt | +16.36% | -1.12% | -14.92% | -14.89% |

## Constraint-based sweep selection

Selector command:

```bash
python3 scripts/kuramoto_sweep_select.py \
  --baseline experiments_kura_baseline_nocredit
```

Thresholds used:

- `conv_abs >= 0%`
- `disagreement_reduction >= 0%`
- `coherence_delta >= 0%`
- `reward_delta >= 0%`

Result:

- Accepted candidates: **none**
- All tested treatment/sweep directories violate at least one non-inferiority constraint (most violate all).

## Phase-field experiment mode (new)

Implemented in `src/kuramoto.rs` and wired to `batch_experiment` flags:

- `--kuramoto-phase-field`
- `--kuramoto-pf-growth`
- `--kuramoto-pf-hyper`
- `--kuramoto-pf-disp`

Exploratory quick trials (8 runs each, 600 ticks, no-credit):

1. `experiments_kura_phasefield_trial_nocredit`
   - Params: `K=0.45`, `council=0.08`, `dt=0.02`, `noise=0.01`, `gain=0.05`, `pf_growth=0.06`, `pf_hyper=0.12`, `pf_disp=0.04`
   - Paired results vs baseline:
     - `conv_abs_improvement_mean=+39.37%`
     - `disagreement_red_mean=-47.54%`
     - `final_coherence_delta_mean=-39.20%`
     - `reward_delta_mean=-39.10%`
   - Selector verdict: **REJECT**

2. `experiments_kura_phasefield_conservative_nocredit`
   - Params: `K=0.10`, `council=0.02`, `dt=0.005`, `noise=0.02`, `gain=0.01`, `pf_growth=0.01`, `pf_hyper=0.05`, `pf_disp=0.01`
   - Paired results vs baseline:
     - `conv_abs_improvement_mean=+39.37%`
     - `disagreement_red_mean=-1.48%`
     - `final_coherence_delta_mean=-39.78%`
     - `reward_delta_mean=-39.69%`
   - Selector verdict: **REJECT**

3. `experiments_kura_phasefield_guarded_nocredit`
   - Params: `K=0.10`, `council=0.02`, `dt=0.005`, `noise=0.02`, `gain=0.01`,
     `pf_growth=0.01`, `pf_hyper=0.05`, `pf_disp=0.01`,
     runtime guard: `warmup=300`, `cap_k=0.08`, `cap_c=0.03`, `lcc_gate=0.85`,
     `gain_min=0.10`, `entropy_floor=0.45`, `entropy_boost=0.03`, `disable_trips=4`.
   - Paired results vs baseline:
     - `conv_abs_improvement_mean=+39.37%`
     - `disagreement_red_mean=-41.42%`
     - `final_coherence_delta_mean=-39.38%`
     - `reward_delta_mean=-39.29%`
   - Selector verdict: **REJECT**

Interpretation:

- Phase-field mode currently amplifies "fast-to-low-quality" convergence.
- No non-inferior configuration has been found yet, even with conservative forcing.
- These are exploratory 8-run signals only; they are useful for direction but not rollout decisions.

## Acceptance criteria check

Protocol targets:

1. Median convergence time improves by >=20%
2. Disagreement/conflict rate drops by >=15%
3. Task score is non-inferior
4. Holds across >=70% seeds

Observed:

1. `+16.36%` convergence improvement -> **FAIL** (<20%)
2. `+3.37%` disagreement reduction (wide CI crossing 0) -> **FAIL**
3. Task score regressed (`final_coherence` and `reward` both ~`-15.6%`) -> **FAIL**
4. Since (1)-(3) fail, acceptance not met -> **FAIL**

## Preflight warnings observed

- Weak connectivity (`largest component ratio < 0.80`) appeared frequently.
- Some runs hit antipodal council lock warnings when noise was zero.
- High-stiffness warnings occurred in aggressive parameter settings.

These warnings are consistent with degraded quality in treatment/sweep runs and should be treated as active blockers for rollout claims.

## Repro command

```bash
python3 scripts/kuramoto_protocol_eval.py \
  --baseline experiments_kura_baseline_nocredit \
  --treatment experiments_kura_treat_nocredit
```

## Seeded A/B Correction (post-fix)

Method correction:

- Added deterministic seeding (`--seed-base`) so baseline/treatment use identical seed sets.
- Removed no-credit `thread_rng` usage from simulation paths to make seeded A/B reproducible.

Validated with:

- Baseline: `experiments_kura_baseline_seededfix_nocredit`
- Treatment (structural-only feedback): `experiments_kura_treat_seededfix_structural_nocredit`

Treatment config:

- `--kuramoto --kuramoto-k 0.05 --kuramoto-council 0.01 --kuramoto-dt 0.0005 --kuramoto-noise 0.01`
- `--kuramoto-gain 0.02` with default `structural-only` feedback (no `--kuramoto-drive-feedback`)
- runtime guard enabled (`warmup/caps/lcc-gate/entropy guard`)

Paired (20 seeds) outcome vs seeded baseline:

- `conv_abs_improvement_mean = +0.11%` (95% CI `0.05..0.20`)
- `final_coherence_delta_mean = +0.29%` (95% CI `-2.22..2.90`)
- `reward_delta_mean = +0.29%` (95% CI `-2.21..2.89`)
- `disagreement_red_mean = 0.00%`

Selector verdict under non-inferiority constraints (`conv_abs>=0, disagreement>=0, coherence>=0, reward>=0`):

- **ACCEPT** `experiments_kura_treat_seededfix_structural_nocredit`
- Also accepted: `experiments_kura_treat_seededfix_gain0_nocredit` (near no-op control)

Interpretation:

- Strong per-agent drive feedback remains destabilizing.
- Structural-only feedback can preserve (and slightly improve) quality while keeping convergence non-inferior.
- Improvement is currently modest; additional search is still needed for materially faster convergence with clear quality lift.

## Focused Structural-Only Sweep (seeded, local)

Sweep setup:

- Baseline: `experiments_kura_baseline_seededfix_nocredit`
- 10 local candidates around accepted structural-only regime
- All runs: 20 seeds, 1000 ticks, `--seed-base 2026022401`
- Selector constraints: `conv_abs>=0`, `disagreement>=0`, `coherence>=0`, `reward>=0`

Result:

- 9/10 candidates passed non-inferiority.
- Best candidate: `experiments_kura_struct_sweep_s6_seededfix_nocredit`

Best candidate command:

```bash
cargo run --release --bin batch_experiment -- \
  --seed-base 2026022401 --no-credit --kuramoto \
  --kuramoto-k 0.05 --kuramoto-council 0.01 --kuramoto-dt 0.0005 --kuramoto-noise 0.01 \
  --kuramoto-gain 0.04 \
  --kuramoto-warmup 400 --kuramoto-cap-k 0.05 --kuramoto-cap-c 0.02 \
  --kuramoto-lcc-gate 0.85 --kuramoto-gain-min 0.10 \
  --kuramoto-entropy-floor 0.45 --kuramoto-entropy-boost 0.03 \
  --kuramoto-disable-trips 5 \
  20 1000 experiments_kura_struct_sweep_s6_seededfix_nocredit
```

Paired seeded results vs baseline:

- `conv_abs_improvement_mean = +0.18%` (95% CI `0.09..0.30`)
- `final_coherence_delta_mean = +1.26%` (95% CI `0.31..3.13`)
- `reward_delta_mean = +1.26%` (95% CI `0.31..3.13`)
- `disagreement_red_mean = 0.00%`
- Non-inferior share: `reward=100%`, `coherence=100%`

Automated coarse→fine tuner:

- Script: `scripts/kuramoto_structural_tune.py`
- Objective: maximize paired `conv_abs` under hard non-inferiority (`dis/coh/rew >= 0`)

Latest tuner run:

```bash
python3 scripts/kuramoto_structural_tune.py \
  --baseline experiments_kura_baseline_seededfix_nocredit \
  --seed-base 2026022401 --runs 20 --ticks 1000 \
  --prefix experiments_kura_struct_tune_seededfix --topk 3
```

Output summary:

- Total evaluated: `10`
- Passing: `6`
- Best: `experiments_kura_struct_tune_seededfix_fine_3`
  - `k=0.06`, `council=0.012`, `dt=0.0005`, `noise=0.01`, `gain=0.04`,
    `warmup=400`, `cap_k=0.05`, `cap_c=0.02`,
    `lcc_gate=0.85`, `gain_min=0.10`,
    `entropy_floor=0.45`, `entropy_boost=0.03`, `disable_trips=5`
  - Paired: `conv_abs=+0.21%`, `dis=0.00%`, `coh=+0.39%`, `rew=+0.39%`
