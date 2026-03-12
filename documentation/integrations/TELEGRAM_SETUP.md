# Telegram Bot Setup for HSM-II

This guide shows you how to set up and use the HSM-II Telegram bot to interact with your local AI agents.

## Quick Start

### 1. Create a Telegram Bot

1. Open Telegram and search for [@BotFather](https://t.me/botfather)
2. Start a chat and send `/newbot`
3. Follow the prompts to name your bot
4. **Save the API token** - you'll need it for HSM-II

### 2. Configure HSM-II

Set the environment variable:

```bash
export TELEGRAM_TOKEN="your-bot-token-from-botfather"
```

Or add it to your `.env` file:

```
TELEGRAM_TOKEN=your-bot-token-from-botfather
```

### 3. Start the Personal Agent with Telegram

```bash
# Start with Telegram gateway enabled
cargo run --bin personal_agent -- start --telegram

# Or with both Discord and Telegram
cargo run --bin personal_agent -- start --discord --telegram

# Daemon mode (runs in background)
cargo run --bin personal_agent -- start --telegram --daemon
```

### 4. Interact with Your Bot

1. Open Telegram and find your bot (by the username you created)
2. Start a conversation with `/start`
3. Send any message - HSM-II will respond using your configured local or remote LLM

## Configuration Options

### Allowed Chats (Security)

By default, your bot responds to anyone. To restrict to specific chats:

```rust
// In your configuration
let gateway_config = gateway::Config {
    telegram_token: std::env::var("TELEGRAM_TOKEN").ok(),
    telegram_allowed_chats: Some(vec![123456789, -1001234567890]), // Your chat IDs
    ..Default::default()
};
```

To find your chat ID:
1. Message [@userinfobot](https://t.me/userinfobot) on Telegram
2. It will reply with your ID

For group chats, add the bot to the group and check the logs - the chat ID will be printed when a message is received.

### Using with Local Models (Ollama)

The Telegram bot works seamlessly with local Ollama models:

```bash
# Start Ollama first
ollama run llama3.2

# In another terminal, start HSM-II with Telegram
export OLLAMA_URL=http://localhost:11434
export TELEGRAM_TOKEN=your-token
cargo run --bin personal_agent -- start --telegram
```

Your Telegram bot will now use your local Llama model - **fully private**, no data leaves your machine!

## Features

### Message Handling

- ✅ Text messages with full HSM-II tool access
- ✅ Reply threading (HSM-II sees what you're replying to)
- ✅ Long message splitting (automatically handles Telegram's 4096 char limit)
- ✅ Markdown formatting support

### Tool Access

Your Telegram bot can use all 57+ HSM-II tools:

- **File operations**: Read/write files on your machine
- **Shell commands**: Execute commands (be careful with permissions!)
- **Git operations**: Clone, commit, push repositories
- **Web browsing**: Search and scrape websites
- **Browser automation**: Full browser control via Browserbase

### Example Conversations

```
You: Check my disk space
Bot: Running command...
    Filesystem    Size   Used  Avail   Use%  Mounted on
    /dev/disk1    1.8T   890G   910G    50%   /

You: Search for "Rust async tutorial"
Bot: [Performs web search and summarizes results]

You: Clone https://github.com/example/repo and analyze it
Bot: [Clones repo, analyzes structure, provides summary]
```

## Architecture

```
┌─────────────┐      HTTP/WebSocket      ┌──────────────┐
│   Telegram  │◄────────────────────────►│  Telegram    │
│   Servers   │                          │  Bot (Rust)  │
└─────────────┘                          └──────┬───────┘
                                                │
                                                │ MessageHandler
                                                ▼
                                        ┌──────────────┐
                                        │   Personal   │
                                        │    Agent     │
                                        └──────┬───────┘
                                               │
                        ┌──────────────────────┼──────────────────────┐
                        │                      │                      │
                        ▼                      ▼                      ▼
                  ┌──────────┐          ┌──────────┐          ┌──────────┐
                  │  Ollama  │          │  OpenAI  │          │Anthropic │
                  │ (Local)  │          │ (Cloud)  │          │ (Cloud)  │
                  └──────────┘          └──────────┘          └──────────┘
```

## Troubleshooting

### Bot doesn't respond

1. Check the token is correct: `echo $TELEGRAM_TOKEN`
2. Check logs for connection errors
3. Ensure you've sent `/start` to the bot

### "Chat not in allowlist"

If you set `telegram_allowed_chats`, make sure your chat ID is in the list.

### Long messages cut off

The bot automatically splits messages > 4096 characters. If you're seeing issues, check the logs.

## Security Considerations

⚠️ **WARNING**: The Telegram bot has the same tool access as the CLI agent:

- It can execute shell commands
- It can read/write files
- It can access your git repositories

**Recommendations**:
1. Use `telegram_allowed_chats` to restrict access
2. Run in a container or VM for isolation
3. Monitor tool usage with logging
4. Don't share your bot token

## Advanced: Custom Message Handlers

You can implement custom logic for Telegram messages:

```rust
use hyper_stigmergy::personal::gateway::{MessageHandler, Message};

struct MyHandler;

#[async_trait]
impl MessageHandler for MyHandler {
    async fn handle(&self, msg: Message) -> anyhow::Result<String> {
        // Custom pre-processing
        if msg.content.starts_with("/admin") {
            // Admin-only commands
        }
        
        // Pass to default handler
        default_handle(msg).await
    }
}
```

## Next Steps

- Combine with [Discord](./HERMES_INTEGRATION.md) for multi-platform presence
- Set up [scheduled jobs](./SCHEDULER.md) for automated tasks
- Configure [federation](../architecture/FEDERATION.md) to connect multiple HSM-II instances
