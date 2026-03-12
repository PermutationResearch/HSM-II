//! Example: Basic Hermes Bridge Usage
//!
//! This example shows how HSM-II can use the Hermes Bridge for tool execution.

use hermes_bridge::{
    ExecutionRequest, HermesClientBuilder,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    println!("=== HSM-II + Hermes Integration Demo ===\n");

    // Create client
    let client = HermesClientBuilder::new()
        .endpoint("http://localhost:8000")
        .timeout_secs(60)
        .default_toolsets(vec!["web".to_string(), "terminal".to_string()])
        .build()?;

    // Initialize (health check + fetch toolsets)
    client.initialize().await?;

    println!("✓ Connected to Hermes Agent\n");

    // Example 1: Simple execution
    println!("--- Example 1: Web Search ---");
    match client.web_search("latest AI agent frameworks 2025").await {
        Ok(result) => println!("Search result:\n{}\n", result),
        Err(e) => println!("Search failed: {}\n", e),
    }

    // Example 2: Terminal command
    println!("--- Example 2: Terminal Command ---");
    match client.terminal_command("ls -la", Some("/tmp")).await {
        Ok(result) => println!("Command output:\n{}\n", result),
        Err(e) => println!("Command failed: {}\n", e),
    }

    // Example 3: File operations
    println!("--- Example 3: File Operations ---");
    let test_content = "# Test File\n\nThis was created by HSM-II via Hermes.";
    
    if let Err(e) = client.write_file("/tmp/hsmii_test.md", test_content).await {
        println!("Write failed: {}\n", e);
    } else {
        println!("✓ File written successfully");
        
        match client.read_file("/tmp/hsmii_test.md").await {
            Ok(content) => println!("File content:\n{}\n", content),
            Err(e) => println!("Read failed: {}\n", e),
        }
    }

    // Example 4: Complex task with custom request
    println!("--- Example 4: Complex Task ---");
    let request = ExecutionRequest::builder(
        "Research the current state of multi-agent systems and summarize 3 key trends"
    )
    .toolsets(vec!["web".to_string(), "skills".to_string()])
    .max_turns(30)
    .build();

    match client.execute_full(request).await {
        Ok(response) => {
            println!("Task ID: {}", response.task_id);
            println!("Status: {:?}", response.status);
            println!("Result:\n{}\n", response.result);
            
            if !response.tool_calls.is_empty() {
                println!("Tool calls made:");
                for call in &response.tool_calls {
                    println!("  - {}: {:?}", call.name, call.arguments);
                }
            }
        }
        Err(e) => println!("Task failed: {}\n", e),
    }

    // Example 5: Health check
    println!("--- Example 5: Health Check ---");
    match client.refresh_health().await {
        Ok(health) => {
            println!("Status: {}", health.status);
            println!("Version: {}", health.version);
            println!("Uptime: {}s", health.uptime_seconds);
            println!("Available toolsets: {:?}\n", health.available_toolsets);
        }
        Err(e) => println!("Health check failed: {}\n", e),
    }

    println!("=== Demo Complete ===");
    Ok(())
}
