#!/bin/bash
# HSM-II Training Launcher
# Usage: ./start_training.sh [config_file] [scenario_name]

set -e

CONFIG_FILE=${1:-"training/config/training_config.json"}
SCENARIO=${2:-"all"}
DATE=$(date +%Y%m%d_%H%M%S)
RUN_ID="hsm2_${DATE}"

echo "╔═══════════════════════════════════════════════════════════════╗"
echo "║  HSM-II Training Launcher                                     ║"
echo "║  Run ID: $RUN_ID                                    ║"
echo "╚═══════════════════════════════════════════════════════════════╝"

# Check Ollama
if ! curl -s http://localhost:11434/api/tags > /dev/null; then
    echo "❌ Ollama not running. Starting..."
    ollama serve &
    sleep 5
fi

# Check GPU
if command -v nvidia-smi &> /dev/null; then
    echo "✅ GPU detected:"
    nvidia-smi --query-gpu=name,memory.total --format=csv,noheader
else
    echo "⚠️  No GPU detected - training will be slow"
fi

# Create run directory
RUN_DIR="training_data/${RUN_ID}"
mkdir -p "$RUN_DIR"/{checkpoints,logs,exports}

# Build if needed
if [[ ! -f "target/release/batch_experiment" ]] || [[ "src" -nt "target/release/batch_experiment" ]]; then
    echo "🔨 Building HSM-II..."
    cargo build --release --bin batch_experiment
fi

# Parse config for training parameters
if command -v jq &> /dev/null; then
    RUNS=$(jq -r '.training.runs_per_scenario // 50' "$CONFIG_FILE")
    TICKS=$(jq -r '.training.ticks_per_run // 5000' "$CONFIG_FILE")
else
    RUNS=50
    TICKS=5000
fi

# Start TensorBoard
if command -v tensorboard &> /dev/null; then
    echo "📊 Starting TensorBoard on port 6006..."
    tensorboard --logdir="$RUN_DIR/logs" --port=6006 &
    TB_PID=$!
fi

# Log system info
cat > "$RUN_DIR/system_info.json" << EOF
{
  "run_id": "$RUN_ID",
  "start_time": "$(date -Iseconds)",
  "config_file": "$CONFIG_FILE",
  "scenario": "$SCENARIO",
  "hostname": "$(hostname)",
  "gpu": $(nvidia-smi --query-gpu=name,memory.total,driver_version --format=json 2>/dev/null || echo '"N/A"'),
  "cpu_cores": $(nproc),
  "memory_gb": $(free -g | awk '/^Mem:/{print $2}')
}
EOF

echo ""
echo "🚀 Starting training..."
echo "   Runs: $RUNS"
echo "   Ticks per run: $TICKS"
echo "   Output: $RUN_DIR"
echo ""

# Run training based on scenario
if [[ "$SCENARIO" == "all" ]]; then
    # Run all scenarios
    cargo run --release --bin batch_experiment -- \
        --real-llm \
        --config "$CONFIG_FILE" \
        --output "$RUN_DIR" \
        2>&1 | tee "$RUN_DIR/logs/training.log"
else
    # Run specific scenario
    cargo run --release --bin batch_experiment -- \
        --real-llm \
        --scenario "$SCENARIO" \
        --config "$CONFIG_FILE" \
        --output "$RUN_DIR" \
        2>&1 | tee "$RUN_DIR/logs/training.log"
fi

# Generate plots
echo ""
echo "📈 Generating plots..."
if [[ -f "scripts/plot_results.py" ]]; then
    python3 scripts/plot_results.py "$RUN_DIR" --output "$RUN_DIR/figures"
fi

# Create summary
cat > "$RUN_DIR/TRAINING_COMPLETE.txt" << EOF
Training Complete
=================
Run ID: $RUN_ID
End Time: $(date -Iseconds)
Duration: $SECONDS seconds

Results: $RUN_DIR/exports/
Figures: $RUN_DIR/figures/
Logs: $RUN_DIR/logs/

To analyze:
  python3 training/scripts/analyze_results.py $RUN_DIR
EOF

echo ""
echo "✅ Training complete!"
echo "📁 Results: $RUN_DIR"
echo ""

# Kill TensorBoard
if [[ -n "$TB_PID" ]]; then
    kill $TB_PID 2>/dev/null || true
fi
