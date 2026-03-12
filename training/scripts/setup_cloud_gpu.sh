#!/bin/bash
# HSM-II Cloud GPU Setup Script
# Usage: ./setup_cloud_gpu.sh [vastai|runpod|lambda]

set -e

PROVIDER=${1:-"vastai"}
echo "╔═══════════════════════════════════════════════════════════════╗"
echo "║  HSM-II Cloud GPU Setup                                       ║"
echo "║  Provider: $PROVIDER                                          ║"
echo "╚═══════════════════════════════════════════════════════════════╝"

# Detect OS
if [[ "$OSTYPE" == "linux-gnu"* ]]; then
    OS="linux"
elif [[ "$OSTYPE" == "darwin"* ]]; then
    OS="mac"
else
    echo "Unknown OS: $OSTYPE"
    exit 1
fi

echo "[1/8] Updating system packages..."
if command -v apt-get &> /dev/null; then
    sudo apt-get update -qq
    sudo apt-get install -y -qq build-essential curl git python3 python3-pip \
        htop nvtop jq zstd unzip
elif command -v yum &> /dev/null; then
    sudo yum update -y -q
    sudo yum install -y -q gcc curl git python3 python3-pip htop jq zstd unzip
fi

echo "[2/8] Installing Rust..."
if ! command -v rustc &> /dev/null; then
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source "$HOME/.cargo/env"
else
    rustup update
fi
rustc --version

echo "[3/8] Installing CUDA (if not present)..."
if ! command -v nvidia-smi &> /dev/null; then
    echo "WARNING: CUDA not detected. Installing..."
    # Ubuntu/Debian CUDA install
    if command -v apt-get &> /dev/null; then
        wget https://developer.download.nvidia.com/compute/cuda/repos/ubuntu2204/x86_64/cuda-keyring_1.0-1_all.deb
        sudo dpkg -i cuda-keyring_1.0-1_all.deb
        sudo apt-get update
        sudo apt-get install -y cuda-toolkit-12-1
        echo 'export PATH=/usr/local/cuda/bin:$PATH' >> ~/.bashrc
        rm cuda-keyring_1.0-1_all.deb
    fi
fi
nvidia-smi

echo "[4/8] Installing Ollama..."
if ! command -v ollama &> /dev/null; then
    curl -fsSL https://ollama.com/install.sh | sh
fi

# Configure Ollama for remote access (if needed)
if [[ "$PROVIDER" != "local" ]]; then
    sudo systemctl stop ollama 2>/dev/null || true
    sudo systemctl disable ollama 2>/dev/null || true
fi

echo "[5/8] Pulling required models..."
ollama pull hf.co/yourGGUF/heretic_HelpingAI-3B-coder_GGUF:BF16 2>/dev/null || true
ollama pull hf.co/tensorblock/DeepSeek-R1-Distill-Llama-8B-abliterated-GGUF:Q3_K_M 2>/dev/null || true

echo "[6/8] Installing Python dependencies..."
pip3 install -q torch torchvision torchaudio --index-url https://download.pytorch.org/whl/cu121 2>/dev/null || true
pip3 install -q numpy pandas matplotlib seaborn plotly wandb tensorboard 2>/dev/null || true

echo "[7/8] Building HSM-II..."
cd /workspace/hyper-stigmergic-morphogenesisII 2>/dev/null || cd ~/hyper-stigmergic-morphogenesisII
cargo build --release 2>&1 | tail -5

echo "[8/8] Setting up monitoring..."
mkdir -p training_data checkpoints logs

# Create systemd service for Ollama (background)
cat > /tmp/ollama.service << 'EOF'
[Unit]
Description=Ollama Service
After=network.target

[Service]
Type=simple
User=root
ExecStart=/usr/local/bin/ollama serve
Restart=always
RestartSec=10
Environment="OLLAMA_HOST=0.0.0.0:11434"
Environment="OLLAMA_NUM_PARALLEL=4"
Environment="OLLAMA_MAX_LOADED_MODELS=2"

[Install]
WantedBy=multi-user.target
EOF

sudo mv /tmp/ollama.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable ollama
sudo systemctl start ollama

echo ""
echo "✅ Setup complete!"
echo ""
echo "Next steps:"
echo "  1. Verify Ollama: curl http://localhost:11434/api/tags"
echo "  2. Start training: ./training/scripts/start_training.sh"
echo "  3. Monitor: tail -f logs/training.log"
echo ""
