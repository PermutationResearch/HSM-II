# Hermes Integration Quickstart

This guide gets you running with Hermes Agent integration in HSM-II in 5 minutes.

## Prerequisites

- HSM-II built and running (`cargo build --release`)
- Python 3.9+ installed
- Hermes Agent repository cloned (optional, for full integration)

## Step 1: Start the Bridge Server

The Hermes Extension server provides the API that HSM-II communicates with:

```bash
cd hermes-extension
pip install -r requirements.txt
python server.py
```

You should see:
```
INFO:     Started server process [12345]
INFO:     Waiting for application startup.
INFO:     Application startup complete.
INFO:     Uvicorn running on http://0.0.0.0:8000
```

Test it's working:
```bash
curl http://localhost:8000/api/v1/health
```

## Step 2: Add Hermes Bridge to HSM-II

Add to `Cargo.toml`:

```toml
[dependencies]
hermes-bridge = { path = "hermes-bridge" }
```

## Step 3: Use in Your Code

```rust
use hermes_bridge::{HermesClient, HermesClientBuilder};

async fn research_task(client: &HermesClient, query: &str) -> anyhow::Result<String> {
    // Web search via Hermes
    let search_results = client.web_search(query).await?;
    
    // Save to file
    client.write_file("/tmp/research.md", &search_results).await?;
    
    Ok(search_results)
}

// In your main function:
let client = HermesClientBuilder::new()
    .endpoint("http://localhost:8000")
    .build()?;

client.initialize().await?;

let result = research_task(&client, "multi-agent coordination").await?;
```

## Step 4: Integrate with CASS

```rust
use hermes_bridge::{SkillConverter, CASSSkill, SkillLevel};

// Create a CASS skill
let skill = CASSSkill {
    id: "research_skill".to_string(),
    title: "Web Research".to_string(),
    principle: "Search web for current information".to_string(),
    level: SkillLevel::General,
    confidence: 0.9,
    embedding: None,
};

// Convert and sync
let converter = SkillConverter::new();
let hermes_skill = converter.cass_to_hermes(&skill);

// Sync with Hermes
client.sync_skills(vec![skill]).await?;
```

## Common Patterns

### Pattern 1: Council → Hermes Decision

```rust
// In council deliberation
if council_decision.requires_external_tool() {
    let hermes_result = client.execute(
        &council_decision.to_prompt()
    ).await?;
    
    council.update_with_result(hermes_result)?;
}
```

### Pattern 2: DKS → Hermes Subagent

```rust
// Spawn Hermes subagent for DKS entity
let subagent_result = client
    .spawn_subagent(
        "Analyze this data and report findings",
        Some(entity.context_json())
    )
    .await?;
```

### Pattern 3: Federation → Hermes Gateway

```rust
// Send stigmergic signal to human via Hermes
let message = FederationMessage {
    target_gateway: "discord".to_string(),
    signal: coherence_alert_signal(),
    ...
};

federation_client.send_to_hermes(message).await?;
```

## Troubleshooting

| Issue | Solution |
|-------|----------|
| Connection refused | Ensure server is running on port 8000 |
| Timeout | Increase timeout in BridgeConfig |
| Import errors | Install requirements.txt |
| Mock mode | Hermes modules not found; install Hermes to PYTHONPATH |

## Next Steps

1. Read full integration doc: `HERMES_INTEGRATION.md`
2. Explore examples: `hermes-bridge/examples/`
3. Add custom toolsets
4. Implement federation gateway
