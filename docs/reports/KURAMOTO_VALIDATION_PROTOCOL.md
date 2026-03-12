# Kuramoto Validation Protocol (HSM-II)

This protocol prevents over-claiming outcomes (for example, "reduced disagreement") without evidence.

## Scope

- Model under test: graph Kuramoto-like synchronizer in `src/kuramoto.rs`
- Not in scope: full generalized Kuramoto-Sivashinsky PDE claims

## Experimental Design

1. Baseline: run system with Kuramoto disabled (or `coupling_strength=0`, `council_influence=0`).
2. Treatment: Kuramoto enabled.
3. Use at least 20 random seeds per condition.
4. Sweep key parameters:
   - `coupling_strength`
   - `council_influence`
   - `noise_amplitude`
   - `dispersion`
   - `dt`
5. Keep task mix and workload identical across baseline/treatment.

## Required Metrics

- `R` (order parameter): coherence only, not chaos proof.
- `phase_entropy`: normalized phase spread.
- `velocity_stddev`: instability proxy in oscillator velocities.
- `r_window_stddev`: coherence volatility proxy.
- Task-level outcomes:
  - convergence time
  - disagreement/conflict rate
  - objective task score (quality/success)

## Acceptance Criteria (example)

- Median convergence time improves by >=20%.
- Disagreement/conflict rate drops by >=15%.
- Task score is non-inferior (no statistically meaningful regression).
- Results hold across >=70% of tested seeds.

## Reporting Rules

- Do not claim "chaos reduction" from `R` alone.
- If prerequisites warnings fire (poor connectivity, large `dt`, antipodal council lock), report them.
- Publish parameter ranges, seeds, and summary statistics with confidence intervals.
