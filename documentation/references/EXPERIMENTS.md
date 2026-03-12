# HSM-II Empirical Evaluation System

This document describes how to run the batch experiments and generate real data for the paper figures.

## Quick Start

### 1. Run Batch Experiments

```bash
# Run 20 trials of 1000 ticks each (simulated LLM)
cargo run --release --bin batch_experiment

# Or with custom parameters
cargo run --release --bin batch_experiment -- 20 1000 experiments
#                    num_runs  ticks_per_run  output_dir

# Use real Ollama LLM calls (slower but generates real LLM outputs)
cargo run --release --bin batch_experiment -- --real-llm 5 500 experiments_real

# View help
cargo run --release --bin batch_experiment -- --help
```

This will create:
```
experiments/
├── run_00/
│   ├── run_00_seed_XXX_snapshots.csv    # Tick-by-tick metrics
│   ├── run_00_seed_XXX_council.csv      # Council decisions
│   ├── run_00_seed_XXX_federation.csv   # Federation events
│   └── run_00_seed_XXX_summary.json     # Run summary
├── run_01/
│   └── ...
├── ...
├── run_19/
└── aggregate_summary.json               # Statistics across all runs
```

### 2. Generate Plots

```bash
# Install Python dependencies
pip install -r scripts/requirements.txt

# Generate all figures
python3 scripts/plot_results.py experiments
```

This creates:
```
experiments/figures/
├── fig_coherence.pdf       # Coherence growth over time
├── fig_skills.pdf          # Skill accumulation
├── fig_council.pdf         # Council mode effectiveness
├── fig_federation.pdf      # Trust dynamics
├── fig_dks.pdf             # DKS population dynamics
├── stats_summary.json      # Aggregate statistics
└── *.png                   # PNG versions for web
```

### 3. Copy Figures to Paper

```bash
cp experiments/figures/*.pdf paper/figures/
```

## Metrics Collected

The system collects the following real-time metrics every tick:

### Coherence Metrics
- `global_coherence`: Overall C(t) value
- `edge_density`: Hyperedge density
- `emergent_coverage`: Fraction of emergent edges
- `ontological_consistency`: Cross-tag consistency
- `belief_convergence`: Agent belief alignment

### Skill Metrics
- `skills_harvested`: Total harvested candidates
- `skills_promoted`: Skills passing jury (≥ level 2)
- `jury_pass_rate`: Validation success rate

### Council Metrics
- `council_proposals_total`: Total proposals
- `council_approved/rejected/deferred`: Outcome counts
- `council_mode_usage`: By-mode breakdown

### DKS Metrics
- `dks_population_size`: Entity count
- `dks_mean_stability`: Average Σ_e
- `dks_multifractal_width`: Compositionality
- `dks_stigmergic_edges`: Stigmergic deposits

### Federation Metrics
- `federation_trust_scores`: Per-peer trust τ_ij
- `knowledge_layer_counts`: By-layer distribution

## CSV Schema

### snapshots.csv
```
tick,timestamp,global_coherence,edge_density,emergent_coverage,
ontological_consistency,belief_convergence,skills_harvested,
skills_promoted,skills_level_2_plus,jury_pass_rate,
council_proposals_total,council_approved,council_rejected,council_deferred,
mean_agent_reward,grpo_entropy,dks_population_size,dks_mean_stability,
dks_multifractal_width,dks_stigmergic_edges
```

### council.csv
```
tick,mode,outcome,complexity,urgency
```

### federation.csv
```
tick,peer_id,trust_score,event_type
```

## Implementation Notes

### Simulation vs Real LLM
By default, batch experiments use **simulated** council decisions (fast, deterministic). To use actual Ollama LLM calls:

```bash
# Requires Ollama running with the model
cargo run --release --bin batch_experiment -- --real-llm 5 500 experiments_real
```

**Caveats with real LLM:**
- Much slower (seconds per council decision vs milliseconds)
- Requires Ollama server running at localhost:11434
- Model must be pulled: `ollama pull hf.co/tensorblock/DeepSeek-R1-Distill-Llama-8B-abliterated-GGUF:Q3_K_M`
- Arguments are cached to reduce redundant calls

### Implemented Features
| Feature | Status | Notes |
|---------|--------|-------|
| Argument Caching | ✅ | Reduces duplicate LLM calls |
| LLM Latency Budget | ⚠️ | Config exists (10s default) but not enforced |
| Request Batching | ⚠️ | Struct defined but not functional |
| Dynamic ηC | ❌ | Hardcoded to 0.1 |

See `IMPLEMENTATION_STATUS.md` for full details.

## Implementation Details

### Metrics Module (`src/metrics.rs`)
- `MetricsCollector`: Accumulates data during run
- `TickSnapshot`: Single-tick state capture
- `BatchAggregator`: Cross-run statistics

### Batch Runner (`src/batch_runner.rs`)
- Runs N independent trials with different seeds
- Simulates council decisions, skill distillation, federation updates
- Exports CSV + JSON for analysis

### Extension Traits
- `DKSMetrics`: Population/stability queries
- `TrustGraphMetrics`: Trust score access

## Reproducing Paper Results

To reproduce the exact figures from the paper:

```bash
# Run full batch (20 runs × 1000 ticks)
cargo run --release --bin batch_experiment -- 20 1000 experiments

# Generate all figures
python3 scripts/plot_results.py experiments

# Check aggregate statistics
cat experiments/aggregate_summary.json
```

The expected results (from 20 runs × 1000 ticks):
- Final coherence: ~440 (varies by seed)
- Coherence growth: ~+440 from initial
- Skills promoted: ~6 per run
- Jury pass rate: ~75%
- DKS stability: ~220
- Council proposals: 380 total (Simple: ~145, Orchestrate: ~87, LLM: ~148)
- Council outcomes: ~59% Approve, ~17% Reject, ~24% Defer
- Mode thresholds: Orchestrate ($u > 0.7$), LLM ($c > 0.6$), Simple (default)

## Customization

### Add New Metrics

1. Add field to `TickSnapshot` in `src/metrics.rs`
2. Update `collect_snapshot()` in `src/batch_runner.rs`
3. Update CSV export schema
4. Update Python plotting script

### Change Simulation Parameters

Edit `BatchConfig` in `src/bin/batch_experiment.rs`:
```rust
let config = BatchConfig {
    num_runs: 20,
    ticks_per_run: 1000,
    agent_count: 10,
    enable_dks: true,
    enable_federation: true,
    enable_llm_deliberation: true,
    enable_stigmergic_entities: true,
};
```

## Troubleshooting

### "No data found" error
- Check that experiments actually ran
- Verify CSV files exist in `experiments/run_*/`

### Plots look wrong
- Ensure matplotlib ≥ 3.7
- Check that all runs completed successfully

### Compilation errors
- Make sure `chrono` feature includes `"serde"`
- Verify `src/bin/batch_experiment.rs` exists
