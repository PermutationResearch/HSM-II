# Hermes Bridge for HSM-II

Bridge between Hyper-Stigmergic Morphogenesis II (Rust) and [Hermes Agent](https://github.com/NousResearch/hermes-agent) (Python) by [NousResearch](https://github.com/NousResearch).

## Quick Start

### 1. Start Hermes Extension Server

```bash
cd ../hermes-extension
pip install -r requirements.txt
python server.py
```

Server will start on `http://localhost:8000`.

### 2. Use in HSM-II

```rust
use hermes_bridge::{HermesClient, HermesClientBuilder};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Create client
    let client = HermesClientBuilder::new()
        .endpoint("http://localhost:8000")
        .build()?;

    // Initialize
    client.initialize().await?;

    // Execute tasks
    let result = client.web_search("AI agents").await?;
    println!("{}", result);

    Ok(())
}
```

## Architecture

```
┌──────────────┐      HTTP/JSON       ┌──────────────────┐
│   HSM-II     │  ═══════════════════►│  Hermes Extension │
│   (Rust)     │                      │  Server (Python)  │
│              │◄════════════════════│                  │
└──────────────┘                      └────────┬─────────┘
                                               │
                                               │ local
                                               ▼
                                      ┌──────────────────┐
                                      │  Hermes Agent    │
                                      │  Core (Python)   │
                                      └────────┬─────────┘
                                               │
                        ┌──────────────────────┼──────────────────────┐
                        ▼                      ▼                      ▼
                   ┌─────────┐          ┌──────────┐          ┌──────────┐
                   │   Web   │          │ Terminal │          │ Browser  │
                   │ Search  │          │ (Docker) │          │Automation│
                   └─────────┘          └──────────┘          └──────────┘
```

## Features

- **Tool Execution**: Web search, terminal commands, file operations
- **CASS Skill Sync**: Bidirectional skill exchange with Hermes
- **Subagent Delegation**: Spawn isolated Hermes workers
- **Health Monitoring**: Automatic health checks and caching
- **Retry Logic**: Built-in retry with exponential backoff

## Integration Modes

### 1. Hermes as CASS Skill Executor
```rust
// Execute CASS skill via Hermes
let skill_result = client
    .execute("Apply skill: Coherence Preservation")
    .await?;
```

### 2. Hermes as Federation Node
```rust
// Send stigmergic signal to Hermes gateway
federation.send_to_hermes(signal, "telegram_gateway").await?;
```

### 3. Skill Exchange
```rust
// Sync CASS skills with Hermes
let result = client.sync_skills(cass_skills).await?;
```

## Configuration

| Option | Default | Description |
|--------|---------|-------------|
| `endpoint` | `http://localhost:8000` | Hermes server URL |
| `timeout` | 60s | Request timeout |
| `max_turns` | 20 | Max tool calls per task |
| `max_retries` | 3 | Retry attempts |

## API Reference

### `HermesClient`

- `execute(prompt)` - Simple task execution
- `web_search(query)` - Web search
- `terminal_command(cmd, dir)` - Execute shell command
- `read_file(path)` / `write_file(path, content)` - File operations
- `spawn_subagent(task)` - Create subagent
- `schedule_job(schedule, task)` - Schedule cron job
- `sync_skills(skills)` - Bidirectional skill sync

## Examples

See `examples/` directory:
- `basic_usage.rs` - Simple execution patterns
- `cass_integration.rs` - CASS skill integration

## Testing

```bash
cargo test
```

For integration tests, ensure Hermes Extension server is running.
