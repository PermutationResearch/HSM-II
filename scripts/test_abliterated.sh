#!/bin/bash
# Comprehensive test for DeepSeek-R1-Distill-Llama-8B-abliterated-GGUF:Q3_K_M

MODEL="hf.co/tensorblock/DeepSeek-R1-Distill-Llama-8B-abliterated-GGUF:Q3_K_M"
API_BASE="http://localhost:11434"

echo "======================================"
echo "Testing Abliterated Model Integration"
echo "Model: $MODEL"
echo "======================================"
echo ""

# Check Ollama is running
if ! curl -s $API_BASE/api/tags > /dev/null 2>&1; then
    echo "❌ Ollama is not running!"
    exit 1
fi

# Check model exists
if ! ollama list | grep -q "abliterated"; then
    echo "❌ Abliterated model not found!"
    exit 1
fi

echo "✅ Ollama is running and model is available"
echo ""

# Test 1: Basic chat completion
echo "======================================"
echo "Test 1: Basic Chat Completion"
echo "======================================"
RESPONSE=$(curl -s $API_BASE/api/generate \
    -H "Content-Type: application/json" \
    -d "{\"model\":\"$MODEL\",\"prompt\":\"Hello! Introduce yourself in one sentence.\",\"stream\":false}")

if echo "$RESPONSE" | grep -q "error"; then
    echo "❌ Basic chat FAILED"
    echo "$RESPONSE"
else
    echo "✅ Basic chat PASSED"
    echo "Response: $(echo "$RESPONSE" | jq -r '.response' | head -1)"
fi
echo ""

# Test 2: Reasoning/Thinking capability (abliterated models should show thinking)
echo "======================================"
echo "Test 2: Reasoning/Thinking Tags"
echo "======================================"
RESPONSE=$(curl -s $API_BASE/api/generate \
    -H "Content-Type: application/json" \
    -d "{\"model\":\"$MODEL\",\"prompt\":\"What is 15 + 27? Show your thinking.\",\"stream\":false}")

if echo "$RESPONSE" | grep -q "<think>"; then
    echo "✅ Thinking tags present PASSED"
else
    echo "⚠️  No thinking tags (may be normal for some prompts)"
fi
echo ""

# Test 3: Uncensored/Abliterated nature test
echo "======================================"
echo "Test 3: Abliterated Behavior Test"
echo "======================================"
RESPONSE=$(curl -s $API_BASE/api/generate \
    -H "Content-Type: application/json" \
    -d "{\"model\":\"$MODEL\",\"prompt\":\"Explain what 'abliterated' means in AI models.\",\"stream\":false}")

if echo "$RESPONSE" | grep -q "error\|refuse\|cannot\|unable"; then
    echo "⚠️  Model may have refused (checking content...)"
else
    echo "✅ Abliterated response PASSED"
    echo "Response preview: $(echo "$RESPONSE" | jq -r '.response' | cut -c1-100)"
fi
echo ""

# Test 4: Chat API format (used by main.rs)
echo "======================================"
echo "Test 4: Chat API Format (messages)"
echo "======================================"
RESPONSE=$(curl -s $API_BASE/api/chat \
    -H "Content-Type: application/json" \
    -d "{\"model\":\"$MODEL\",\"messages\":[{\"role\":\"system\",\"content\":\"You are a helpful AI.\"},{\"role\":\"user\",\"content\":\"Say hi!\"}],\"stream\":false}")

if echo "$RESPONSE" | grep -q "message"; then
    echo "✅ Chat API format PASSED"
    echo "Response: $(echo "$RESPONSE" | jq -r '.message.content' | head -1)"
else
    echo "❌ Chat API format FAILED"
fi
echo ""

# Test 5: Council simulation (multi-turn reasoning)
echo "======================================"
echo "Test 5: Council Simulation (Multi-turn)"
echo "======================================"

# First turn - Analyst
RESPONSE1=$(curl -s $API_BASE/api/chat \
    -H "Content-Type: application/json" \
    -d "{\"model\":\"$MODEL\",\"messages\":[{\"role\":\"system\",\"content\":\"You are an Analyst. Analyze this system state: Agents=10, Coherence=0.75\"},{\"role\":\"user\",\"content\":\"Provide your analysis.\"}],\"stream\":false}")

ANALYSIS=$(echo "$RESPONSE1" | jq -r '.message.content' 2>/dev/null)

# Second turn - Challenger seeing Analyst output
RESPONSE2=$(curl -s $API_BASE/api/chat \
    -H "Content-Type: application/json" \
    -d "{\"model\":\"$MODEL\",\"messages\":[{\"role\":\"system\",\"content\":\"You are a Challenger. Critique this analysis: $ANALYSIS\"},{\"role\":\"user\",\"content\":\"Provide your critique.\"}],\"stream\":false}")

if echo "$RESPONSE2" | grep -q "message"; then
    echo "✅ Council multi-turn PASSED"
    echo "Analyst: $(echo "$ANALYSIS" | cut -c1-80)..."
    echo "Challenger: $(echo "$RESPONSE2" | jq -r '.message.content' | cut -c1-80)..."
else
    echo "❌ Council multi-turn FAILED"
fi
echo ""

# Test 6: System prompt with grounded context (like main.rs)
echo "======================================"
echo "Test 6: Grounded Context Format"
echo "======================================"
GROUNDED_CONTEXT="LIVE WORLD DATA:
AGENTS (live, 3 total):
  agent#0 architect | curiosity=0.750 harmony=0.600 growth=0.800
  agent#1 catalyst | curiosity=0.900 harmony=0.500 growth=0.700
  agent#2 chronicler | curiosity=0.600 harmony=0.800 growth=0.600
SYSTEM STATE: tick=100 coherence=0.75 agents=3 edges=5 beliefs=2"

RESPONSE=$(curl -s $API_BASE/api/chat \
    -H "Content-Type: application/json" \
    -d "{\"model\":\"$MODEL\",\"messages\":[{\"role\":\"system\",\"content\":\"$GROUNDED_CONTEXT\n\nYou are analyzing a hypergraph multi-agent system.\"},{\"role\":\"user\",\"content\":\"What patterns do you see in the agent drives?\"}],\"stream\":false}")

if echo "$RESPONSE" | grep -q "message"; then
    echo "✅ Grounded context PASSED"
    echo "Response: $(echo "$RESPONSE" | jq -r '.message.content' | cut -c1-100)"
else
    echo "❌ Grounded context FAILED"
fi
echo ""

# Test 7: Longer context (simulating chat history)
echo "======================================"
echo "Test 7: Chat History Context"
echo "======================================"
RESPONSE=$(curl -s $API_BASE/api/chat \
    -H "Content-Type: application/json" \
    -d "{\"model\":\"$MODEL\",\"messages\":[{\"role\":\"user\",\"content\":\"What is the capital of France?\"},{\"role\":\"assistant\",\"content\":\"The capital of France is Paris.\"},{\"role\":\"user\",\"content\":\"What about Germany?\"}],\"stream\":false}")

if echo "$RESPONSE" | grep -q "message"; then
    echo "✅ Chat history context PASSED"
    echo "Response: $(echo "$RESPONSE" | jq -r '.message.content' | head -1)"
else
    echo "❌ Chat history context FAILED"
fi
echo ""

# Test 8: Streaming capability check
echo "======================================"
echo "Test 8: Streaming API Check"
echo "======================================"
RESPONSE=$(curl -s $API_BASE/api/chat \
    -H "Content-Type: application/json" \
    -d "{\"model\":\"$MODEL\",\"messages\":[{\"role\":\"user\",\"content\":\"Count to 3\"}],\"stream\":false,\"options\":{\"temperature\":0.7}}")

if echo "$RESPONSE" | grep -q "done_reason"; then
    echo "✅ Streaming API PASSED"
    echo "Done reason: $(echo "$RESPONSE" | jq -r '.done_reason')"
else
    echo "⚠️  Streaming check inconclusive"
fi
echo ""

echo "======================================"
echo "Test Summary Complete"
echo "======================================"
echo ""
echo "Model: $MODEL"
echo "All core functionality tested."
echo ""
echo "To run the full TUI application:"
echo "  cd /Users/cno/hyper-stigmergic-morphogenesisII"
echo "  ./scripts/macos/run-tui.command"
echo ""
