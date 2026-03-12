#!/bin/bash
# Test script for DeepSeek-R1-Distill-Llama-8B-abliterated-GGUF:Q6_K

MODEL="hf.co/tensorblock/DeepSeek-R1-Distill-Llama-8B-abliterated-GGUF:Q6_K"

echo "======================================"
echo "Testing DeepSeek-R1-Distill-Llama-8B-abliterated-GGUF:Q6_K"
echo "======================================"
echo ""

# Check if Ollama is running
if ! curl -s http://localhost:11434/api/tags > /dev/null 2>&1; then
    echo "❌ Ollama is not running. Starting Ollama..."
    ollama serve &
    sleep 3
fi

# Check if model exists
echo "Checking if model exists..."
if ollama list | grep -q "DeepSeek-R1-Distill-Llama-8B-abliterated"; then
    echo "✅ Model already pulled"
else
    echo "⬇️  Model not found. Pulling... (this may take several minutes)"
    echo "Model: $MODEL"
    ollama pull "$MODEL"
    if [ $? -ne 0 ]; then
        echo "❌ Failed to pull model"
        exit 1
    fi
fi

echo ""
echo "======================================"
echo "Testing model inference..."
echo "======================================"

# Test the model with a simple prompt
RESPONSE=$(curl -s http://localhost:11434/api/generate \
    -H "Content-Type: application/json" \
    -d "{\"model\":\"$MODEL\",\"prompt\":\"Say 'Hello from DeepSeek Abliterated' and nothing else.\",\"stream\":false}" \
    2>&1)

if echo "$RESPONSE" | grep -q "error"; then
    echo "❌ Model test failed:"
    echo "$RESPONSE" | head -20
    exit 1
fi

if echo "$RESPONSE" | grep -q "response"; then
    echo "✅ Model is working!"
    echo ""
    echo "Response preview:"
    echo "$RESPONSE" | grep -o '"response":"[^"]*"' | head -1 | cut -d'"' -f4
else
    echo "⚠️  Unexpected response format:"
    echo "$RESPONSE" | head -20
fi

echo ""
echo "======================================"
echo "Model test complete"
echo "======================================"
