#!/bin/bash
# HSM-II Training Resume Script
# Resumes training from a checkpoint

set -e

CHECKPOINT_DIR=${1:-""}

if [[ -z "$CHECKPOINT_DIR" ]]; then
    echo "Usage: ./resume_training.sh <checkpoint_directory>"
    echo ""
    echo "Available checkpoints:"
    ls -lt training_data/*/checkpoints/ 2>/dev/null | head -20 || echo "  No checkpoints found"
    exit 1
fi

if [[ ! -d "$CHECKPOINT_DIR" ]]; then
    echo "❌ Checkpoint directory not found: $CHECKPOINT_DIR"
    exit 1
fi

echo "╔═══════════════════════════════════════════════════════════════╗"
echo "║  HSM-II Training Resume                                       ║"
echo "║  Checkpoint: $CHECKPOINT_DIR"
echo "╚═══════════════════════════════════════════════════════════════╝"

# Load checkpoint metadata
if [[ -f "$CHECKPOINT_DIR/checkpoint.json" ]]; then
    echo "📋 Checkpoint info:"
    cat "$CHECKPOINT_DIR/checkpoint.json"
    echo ""
fi

# Extract run directory
RUN_DIR=$(dirname "$CHECKPOINT_DIR")

# Resume training
echo "🚀 Resuming training..."
cargo run --release --bin batch_experiment -- \
    --real-llm \
    --resume "$CHECKPOINT_DIR" \
    --output "$RUN_DIR" \
    2>&1 | tee -a "$RUN_DIR/logs/training_resumed.log"

echo "✅ Training resumed and completed!"
