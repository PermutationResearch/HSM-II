#!/bin/bash
# Import Unsloth Qwen3.5-35B-A3B Q3_K_M (~15GB) into Ollama for macOS.
# Run after freeing ~16GB disk space. Needs: hf, huggingface-cli, or curl.
#
# Usage: OLLAMA_IMPORT_DIR=~/path bash scripts/import_qwen_q3km.sh
set -e

GGUF="Qwen3.5-35B-A3B-Q3_K_M.gguf"
MODEL_NAME="qwen3.5-35b-a3b-q3km"
# Download to ~/Downloads (change if you need another location)
DIR="${OLLAMA_IMPORT_DIR:-$HOME/Downloads}"
mkdir -p "$DIR"
cd "$DIR"

echo "Downloading $GGUF (~15GB) to $DIR ..."
if command -v hf &>/dev/null; then
  hf download unsloth/Qwen3.5-35B-A3B-GGUF "$GGUF" --local-dir .
elif command -v huggingface-cli &>/dev/null; then
  huggingface-cli download unsloth/Qwen3.5-35B-A3B-GGUF "$GGUF" --local-dir .
else
  curl -L -o "$GGUF" "https://huggingface.co/unsloth/Qwen3.5-35B-A3B-GGUF/resolve/main/$GGUF"
fi

echo "Creating Modelfile..."
echo "FROM $(pwd)/$GGUF
PARAMETER temperature 0.7" > Modelfile

echo "Importing into Ollama as $MODEL_NAME..."
ollama create "$MODEL_NAME" -f Modelfile

echo "Done. Use: OLLAMA_MODEL=$MODEL_NAME or set in your app config."
