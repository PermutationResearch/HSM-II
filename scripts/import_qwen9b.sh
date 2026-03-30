#!/bin/bash
# Import Unsloth Qwen3.5-9B into Ollama for macOS.
# UD-Q8_K_XL (~13GB) - high quality but UD format may not work with Ollama.
# Q4_K_M (~5.7GB) - standard quant, guaranteed to work.
#
# Usage: QUANT=Q4_K_M bash scripts/import_qwen9b.sh   # default, works
#        QUANT=UD-Q8_K_XL bash scripts/import_qwen9b.sh  # try UD (may fail)
set -e

QUANT="${QUANT:-Q4_K_M}"
case "$QUANT" in
  Q4_K_M)   GGUF="Qwen3.5-9B-Q4_K_M.gguf";   MODEL_NAME="qwen3.5-9b-q4km";   SIZE="~5.7GB" ;;
  UD-Q8_K_XL) GGUF="Qwen3.5-9B-UD-Q8_K_XL.gguf"; MODEL_NAME="qwen3.5-9b-ud-q8xl"; SIZE="~13GB" ;;
  Q5_K_M)   GGUF="Qwen3.5-9B-Q5_K_M.gguf";   MODEL_NAME="qwen3.5-9b-q5km";   SIZE="~6.5GB" ;;
  Q3_K_M)   GGUF="Qwen3.5-9B-Q3_K_M.gguf";   MODEL_NAME="qwen3.5-9b-q3km";   SIZE="~4.5GB" ;;
  *) echo "Unknown QUANT. Use: Q4_K_M, UD-Q8_K_XL, Q5_K_M, Q3_K_M"; exit 1 ;;
esac

DIR="${OLLAMA_IMPORT_DIR:-$HOME/Downloads}"
mkdir -p "$DIR"
cd "$DIR"

echo "Downloading $GGUF ($SIZE) to $DIR ..."
if command -v hf &>/dev/null; then
  hf download unsloth/Qwen3.5-9B-GGUF "$GGUF" --local-dir .
elif command -v huggingface-cli &>/dev/null; then
  huggingface-cli download unsloth/Qwen3.5-9B-GGUF "$GGUF" --local-dir .
else
  curl -L -o "$GGUF" "https://huggingface.co/unsloth/Qwen3.5-9B-GGUF/resolve/main/$GGUF"
fi

echo "Creating Modelfile..."
echo "FROM $(pwd)/$GGUF
PARAMETER temperature 0.7" > Modelfile

echo "Importing into Ollama as $MODEL_NAME..."
ollama create "$MODEL_NAME" -f Modelfile

echo "Done. Use: OLLAMA_MODEL=$MODEL_NAME"
