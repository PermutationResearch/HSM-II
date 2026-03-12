# Telegram + Ollama: Fully Private AI Assistant

This example shows how to run HSM-II with a Telegram bot using a local Ollama model - **your data never leaves your machine**.

## Prerequisites

1. [Ollama](https://ollama.ai) installed
2. Telegram bot token from [@BotFather](https://t.me/botfather)
3. Rust toolchain

## Setup

### 1. Pull a Model

```bash
# Pull Llama 3.2 (small, fast, good for most tasks)
ollama pull llama3.2

# Or for coding tasks
ollama pull codellama

# Or for larger context
ollama pull mistral
```

### 2. Start Ollama

```bash
# Run in background or separate terminal
ollama serve
```

Verify it's working:
```bash
curl http://localhost:11434/api/tags
```

### 3. Configure Environment

```bash
export TELEGRAM_TOKEN="your-bot-token-here"
export OLLAMA_URL="http://localhost:11434"

# Optional: restrict to your chat only
# Get your chat ID from @userinfobot
export TELEGRAM_ALLOWED_CHATS="123456789"
```

Or create a `.env` file:
```
TELEGRAM_TOKEN=your-bot-token-here
OLLAMA_URL=http://localhost:11434
```

### 4. Run HSM-II

```bash
# Clone the repo if you haven't
git clone https://github.com/PermutationResearch/HSM-II.git
cd HSM-II

# Bootstrap the agent (first time only)
cargo run --bin personal_agent -- bootstrap

# Start with Telegram
cargo run --bin personal_agent -- start --telegram
```

You should see:
```
🚀 Starting HSM-II Personal Agent...
Telegram gateway started
Agent is running. Press Ctrl+C to stop.
```

### 5. Chat on Telegram

1. Find your bot on Telegram (by the username you created)
2. Send `/start`
3. Send any message!

## Example Conversation

```
You: Hello! What's your name?
Bot: Hi! I'm HSM-II, your personal AI assistant. I'm running locally 
     on your machine using the Llama 3.2 model. How can I help you today?

You: What's 2+2?
Bot: 2+2 equals 4.

You: Search for "latest Rust features"
Bot: [Uses web_search tool and summarizes results]

You: Create a file called test.txt with "Hello World"
Bot: [Creates the file on your local machine]
     ✓ File created successfully at /home/user/test.txt
```

## Architecture

```
┌─────────────┐         ┌─────────────┐         ┌─────────────┐
│   Telegram  │◄───────►│   HSM-II    │◄───────►│   Ollama    │
│   (Cloud)   │   HTTPS │  (Your PC)  │   HTTP  │  (Local)    │
└─────────────┘         └──────┬──────┘         └─────────────┘
                               │
                    ┌──────────┼──────────┐
                    │          │          │
                    ▼          ▼          ▼
              ┌────────┐ ┌────────┐ ┌────────┐
              │  File  │ │  Shell │ │  Git   │  ... 57+ tools
              │ System │ │Commands│ │  Ops   │
              └────────┘ └────────┘ └────────┘
```

**Your messages**:
- ✅ Encrypted to Telegram servers
- ✅ Never leave your machine for AI processing
- ✅ Tool execution happens locally
- ✅ No API keys needed for Ollama

## Advanced: Multiple Models

Switch between models mid-conversation:

```
You: /model llama3.2
Bot: Switched to Llama 3.2 (local)

You: /model claude-3.5-sonnet
Bot: Switched to Claude 3.5 Sonnet (Anthropic API)
```

## Troubleshooting

### "Connection refused" to Ollama

Ollama might not be running or is bound to localhost only:

```bash
# Check if Ollama is running
ps aux | grep ollama

# Bind to all interfaces (if needed)
OLLAMA_HOST=0.0.0.0 ollama serve
```

### Telegram bot not responding

1. Check token: `echo $TELEGRAM_TOKEN`
2. Verify bot isn't blocked on Telegram
3. Check logs for errors

### Slow responses

Llama 3.2 on CPU can be slow. Options:
- Use a smaller model: `ollama pull phi3`
- Enable GPU: `ollama serve` automatically uses GPU if available
- Use quantized models (ollama defaults to 4-bit)

### Out of memory

Large models need RAM. Monitor with:
```bash
# On macOS
vm_stat 1

# On Linux
free -h

# On Windows (PowerShell)
Get-Process ollama
```

## Security

Since this runs tools on your machine:

```bash
# Recommended: Run in container
docker run -it \
  -e TELEGRAM_TOKEN=$TELEGRAM_TOKEN \
  -e OLLAMA_URL=http://host.docker.internal:11434 \
  hsm-ii:latest

# Recommended: Restrict chat access
export TELEGRAM_ALLOWED_CHATS="your-chat-id"
```

## Next Steps

- Add [scheduled jobs](./SCHEDULER.md) for automated reports
- Connect to [Discord](./HERMES_INTEGRATION.md) for multi-platform
- Set up [federation](../architecture/FEDERATION.md) with other HSM-II instances

## Privacy Guarantee

With this setup:
- ✅ No data sent to OpenAI/Anthropic
- ✅ Telegram only sees encrypted messages
- ✅ AI processing happens locally
- ✅ You control all data

**Note**: Telegram servers still process your messages (as with any Telegram chat). For complete privacy, use the CLI or TUI mode instead.
