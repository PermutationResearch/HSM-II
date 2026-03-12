#!/bin/bash
# Vast.ai Cloud Init Script
# Paste this into the "On-start Script" field when creating an instance

export DEBIAN_FRONTEND=noninteractive

# Update and install basics
apt-get update -qq
apt-get install -y -qq git curl build-essential python3 python3-pip htop nvtop jq zstd

# Clone repository (update with your repo)
cd /workspace
git clone https://github.com/yourusername/hyper-stigmergic-morphogenesisII.git 2>/dev/null || true
cd hyper-stigmergic-morphogenesisII

# Run setup
chmod +x training/scripts/*.sh
./training/scripts/setup_cloud_gpu.sh vastai

# Start training automatically (optional - comment out if you want manual control)
# ./training/scripts/start_training.sh

echo "Setup complete! Instance ready for training."
