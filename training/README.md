# HSM-II Training System

Complete training pipeline for Hyper-Stigmergic Morphogenesis II on cloud GPU infrastructure.

## Quick Start

### 1. Rent GPU (Vast.ai Example)

```bash
# Find cheapest A100
vastai search offers 'gpu_name == "A100"'

# Create instance with auto-setup
vastai create instance <offer-id> \
  --image pytorch/pytorch:2.1.0-cuda12.1-cudnn8-runtime \
  --disk 100 \
  --onstart-cmd "$(cat training/scripts/cloud_init_vastai.sh)"
```

### 2. Manual Setup (Any Provider)

```bash
# SSH into instance
ssh root@<instance-ip>

# Run setup
cd hyper-stigmergic-morphogenesisII
./training/scripts/setup_cloud_gpu.sh

# Start training
./training/scripts/start_training.sh
```

### 3. Monitor Training

```bash
# View logs
tail -f training_data/hsm2_*/logs/training.log

# TensorBoard
tensorboard --logdir=training_data --port=6006

# In another terminal, forward port
ssh -L 6006:localhost:6006 root@<instance-ip>
# Then open http://localhost:6006
```

## Training Scenarios

| Scenario | Agents | Focus | Duration |
|----------|--------|-------|----------|
| `coding_collaboration` | 15 | Skill distillation, code review | ~4h |
| `conflict_resolution` | 20 | Belief convergence | ~6h |
| `emergent_topology` | 50 | Hypergraph structure | ~8h |
| `federated_learning` | 10 | Trust dynamics | ~3h |
| `skill_evolution` | 12 | Long-term skill evolution | ~10h |

Run specific scenario:
```bash
./training/scripts/start_training.sh training/config/training_config.json coding_collaboration
```

## Configuration

Edit `training/config/training_config.json`:

```json
{
  "training": {
    "runs_per_scenario": 50,    // Number of runs
    "ticks_per_run": 5000,      // Simulation length
    "parallel_runs": 4          // Parallelism
  },
  "llm": {
    "latency_budget_ms": 15000,  // LLM timeout
    "models": {
      "council": "hf.co/yourGGUF/heretic_HelpingAI-3B-coder_GGUF:BF16"
    }
  }
}
```

## Cost Estimates (A100 80GB)

| Scenario | Hours | Cost (@$1.50/hr) |
|----------|-------|------------------|
| Single scenario (50 runs) | 8h | $12 |
| All scenarios (250 runs) | 40h | $60 |
| Extended (500 runs) | 80h | $120 |

## Checkpointing

Checkpoints are saved every 30 minutes to `training_data/<run_id>/checkpoints/`.

Resume from checkpoint:
```bash
./training/scripts/resume_training.sh training_data/hsm2_20240221_120000/checkpoints/ckpt_500
```

## Analysis

After training completes:

```bash
# Generate analysis report
python3 training/scripts/analyze_results.py training_data/hsm2_20240221_120000

# This creates:
# - analysis/analysis_report.json
# - analysis/coherence_trajectories.png
# - analysis/mode_distribution.png
```

## Data Export

Training data is exported in multiple formats:
- `snapshots.csv` - Tick-by-tick metrics
- `council.csv` - Council decisions
- `federation.csv` - Trust dynamics
- `summary.json` - Run summary

## Contrastive Pretraining (Datasets)

The contrastive training script is at `training/train_script.py`.

**Manifest format** (`training/data/manifest.jsonl`): one JSON per line.
```json
{"text_a": "...", "text_b": "...", "label": 1}
```
See `training/data/manifest.sample.jsonl` for a minimal example.

**Run (default settings)**
```bash
python3 training/train_script.py
```

**Reward report output**
- The script writes `training/reward_reports.jsonl` for integration with HSM-II reward evaluators.

### Build a Sharded Manifest

Use `training/scripts/build_manifest.py` to build sharded manifests for large corpora:
```bash
python3 training/scripts/build_manifest.py \\
  --config training/config/dataset_manifest_config.json \\
  --output training/data/shards \\
  --shard-size 5000000
```

Update `HSM_MANIFEST` to point to a shard or merged manifest.
- `checkpoint.bincode` - Full state for resumption

## Troubleshooting

### Ollama Connection Failed
```bash
# Check Ollama is running
curl http://localhost:11434/api/tags

# Restart Ollama
sudo systemctl restart ollama
```

### Out of Memory
```bash
# Reduce batch size in config
# Or reduce parallel_runs
```

### Slow Training
- Check GPU: `nvidia-smi`
- Check Ollama is using GPU: `ollama ps`
- Consider using smaller model for council decisions

## Cloud Provider Specifics

### Vast.ai
- Cheapest option
- Setup script: `training/scripts/cloud_init_vastai.sh`
- Paste into "On-start Script" field

### RunPod
- More reliable
- Setup script: `training/scripts/cloud_init_runpod.sh`
- Use as "Container Start Command"

### Lambda Labs
- Best for long runs
- Persistent storage available
- Manual setup recommended

## Results

Training produces:
1. **Metrics CSVs** - Raw data for analysis
2. **Checkpoints** - Resume training anytime
3. **Visualizations** - Plots of coherence, skills, council decisions
4. **Trained Skill Bank** - Evolved skills in `skill_bank.bincode`
