# Rust-Native Tool System

HSM-II now has a complete Rust-native tool system that replaces the need for Hermes Python bridge in most cases.

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    PersonalAgent                            │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐     │
│  │   Persona    │  │   Memory     │  │   ToolRegistry│     │
│  │  (SOUL.md)   │  │ (MEMORY.md)  │  │              │     │
│  └──────────────┘  └──────────────┘  └──────┬───────┘     │
│                                              │              │
└──────────────────────────────────────────────┼──────────────┘
                                               │
                    ┌──────────────────────────┼──────────────┐
                    │                          │              │
              ┌─────▼─────┐  ┌──────────┐  ┌──▼──────┐  ┌────▼────┐
              │Web Search │  │File Tools│  │Shell    │  │  ...    │
              │           │  │          │  │ Tools   │  │         │
              └───────────┘  └──────────┘  └─────────┘  └─────────┘
```

## Available Tools

| Tool | Description | Parameters |
|------|-------------|------------|
| `web_search` | Search the web | `query`, `num_results` |
| `read` | Read file contents | `path`, `offset`, `limit` |
| `write` | Write to a file | `path`, `content` |
| `edit` | Edit file (find/replace) | `path`, `old_string`, `new_string` |
| `bash` | Execute bash commands | `command`, `working_dir` |
| `grep` | Search file contents | `pattern`, `path`, `include` |
| `find` | Find files | `path`, `name`, `type` |

## Usage

### Direct Tool Execution

```rust
use hyper_stigmergy::tools::{ToolRegistry, ToolCall};

let mut registry = ToolRegistry::with_default_tools();

// Execute a tool call
let call = ToolCall {
    name: "web_search".to_string(),
    parameters: serde_json::json!({
        "query": "Rust programming language",
        "num_results": 5
    }),
    call_id: "1".to_string(),
};

let result = registry.execute(call).await;
println!("{}", result.output.result);
```

### Via PersonalAgent

```rust
use hyper_stigmergy::personal::PersonalAgent;

let mut agent = PersonalAgent::initialize("~/.hsmii").await?;

// The agent will automatically use tools when needed
let result = agent.execute_task("Search for latest Rust news").await?;
println!("{}", result.output);
```

## Web Search Backends

The web search tool automatically selects the best available backend:

1. **DuckDuckGo** (default) - No API key required
2. **Brave Search** - Set `BRAVE_API_KEY` env var
3. **Serper (Google)** - Set `SERPER_API_KEY` env var
4. **Bing** - Set `BING_API_KEY` env var (not yet implemented)

### Setting API Keys

```bash
export BRAVE_API_KEY=your_key_here
export SERPER_API_KEY=your_key_here
```

## Creating Custom Tools

```rust
use hyper_stigmergy::tools::{Tool, ToolOutput, object_schema};
use serde_json::Value;

pub struct MyTool;

#[async_trait::async_trait]
impl Tool for MyTool {
    fn name(&self) -> &str {
        "my_tool"
    }
    
    fn description(&self) -> &str {
        "Description shown to LLM"
    }
    
    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            ("param1", "Description of param1", true),
            ("param2", "Description of param2", false),
        ])
    }
    
    async fn execute(&self, params: Value) -> ToolOutput {
        // Implementation
        ToolOutput::success("Done!")
    }
}

// Register it
registry.register(Arc::new(MyTool));
```

## Tool Execution Flow

1. User provides task to `PersonalAgent.execute_task()`
2. Agent builds system prompt with available tools
3. LLM generates response (potentially with tool call JSON)
4. Agent parses tool call from response
5. ToolRegistry executes the tool
6. Result is stored and returned to user
7. Tool call is recorded in memory for CASS skill learning

## Integration with CASS

Tool executions are recorded as experiences in the memory system. When similar tasks are executed repeatedly, CASS can:

1. Recognize the pattern
2. Distill it into a skill
3. Skip LLM call and execute the skill directly

## Comparison with Hermes

| Feature | Hermes (Python) | HSM-II Rust Tools |
|---------|-----------------|-------------------|
| Web Search | ✅ | ✅ Multiple backends |
| File Operations | ✅ | ✅ Read/Write/Edit |
| Terminal | ✅ | ✅ Bash with timeout |
| Code Search | ✅ | ✅ Grep/Find |
| Browser Automation | ✅ | ❌ Not yet |
| Multi-platform Gateway | ✅ | ✅ Via personal/gateway |
| Persistent Memory | ✅ MEMORY.md | ✅ MEMORY.md + CASS |
| Skill System | ✅ agentskills.io | ✅ CASS |
| Multi-Agent Coordination | ❌ | ✅ Hypergraph |
| Self-Replication | ❌ | ✅ DKS |

## Performance

- **Zero HTTP overhead** for local tools (file, bash, grep)
- **Native Rust speed** - no Python interpreter
- **Async throughout** - tools don't block the agent loop
- **Built-in timeouts** - 30s default for bash, 30s for web search

## Future Enhancements

- [ ] Browser automation tool (headless Chrome)
- [ ] Git operations tool
- [ ] Database query tool
- [ ] API client tool
- [ ] Image processing tool
- [ ] Document parsing tool (PDF, DOCX)

## Testing

```bash
# Run tool registry tests
cargo test --lib registry::tests

# Build and verify
cargo build --release
```
