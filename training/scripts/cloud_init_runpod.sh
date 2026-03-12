#!/bin/bash
# RunPod Container Start Script
# Set as "Container Start Command" or run manually

apt-get update -qq
apt-get install -y -qq git curl build-essential python3 python3-pip jq zstd

# Install Rust
if ! command -v cargo &> /dev/null; then
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source $HOME/.cargo/env
fi

# Install Ollama
curl -fsSL https://ollama.com/install.sh | sh

# Setup Ollama environment
export OLLAMA_HOST=0.0.0.0:11434
export OLLAMA_NUM_PARALLEL=4
export OLLAMA_MAX_LOADED_MODELS=2

# Start Ollama in background
ollama serve &
sleep 10

# Pull models
ollama pull hf.co/yourGGUF/heretic_HelpingAI-3B-coder_GGUF:BF16
ollama pull hf.co/tensorblock/DeepSeek-R1-Distill-Llama-8B-abliterated-GGUF:Q3_K_M

# Clone/build project
cd /workspace
git clone https://github.com/yourusername/hyper-stigmergic-morphogenesisII.git 2>/dev/null || true
cd hyper-stigmergic-morphogenesisII
cargo build --release --bin batch_experiment

echo "Ready! Run: ./training/scripts/start_training.sh"
