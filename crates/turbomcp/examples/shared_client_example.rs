use turbomcp_client::{Client, SharedClient};
use turbomcp_core::Result;
use turbomcp_transport::stdio::StdioTransport;

/// Example demonstrating SharedClient usage for concurrent access
///
/// This example shows how to use SharedClient to share an MCP client
/// across multiple async tasks without exposing Arc/Mutex complexity.
#[tokio::main]
async fn main() -> Result<()> {
    // Create a regular client
    let transport = StdioTransport::new();
    let client = Client::new(transport);

    // Wrap it in SharedClient for concurrent access
    let shared = SharedClient::new(client);

    println!("SharedClient Example");
    println!("===================");

    // Initialize the client once
    println!("Initializing connection...");
    match shared.initialize().await {
        Ok(result) => {
            println!("✓ Connected to: {}", result.server_info.name);
            println!("  Version: {}", result.server_info.version);
        }
        Err(e) => {
            println!("✗ Failed to initialize: {}", e);
            println!("  This is expected when no MCP server is available");
            return Ok(());
        }
    }

    // Clone the shared client for concurrent usage
    let shared1 = shared.clone();
    let shared2 = shared.clone();
    let shared3 = shared.clone();

    println!("\nSpawning concurrent tasks...");

    // Spawn multiple concurrent tasks that use the same client
    let task1 = tokio::spawn(async move {
        println!("Task 1: Listing tools...");
        match shared1.list_tools().await {
            Ok(tools) => println!("Task 1: Found {} tools", tools.len()),
            Err(e) => println!("Task 1: Error listing tools: {}", e),
        }
    });

    let task2 = tokio::spawn(async move {
        println!("Task 2: Listing prompts...");
        match shared2.list_prompts().await {
            Ok(prompts) => println!("Task 2: Found {} prompts", prompts.len()),
            Err(e) => println!("Task 2: Error listing prompts: {}", e),
        }
    });

    let task3 = tokio::spawn(async move {
        println!("Task 3: Listing resources...");
        match shared3.list_resources().await {
            Ok(resources) => println!("Task 3: Found {} resources", resources.len()),
            Err(e) => println!("Task 3: Error listing resources: {}", e),
        }
    });

    // Wait for all tasks to complete
    match tokio::try_join!(task1, task2, task3) {
        Ok(_) => {}
        Err(e) => println!("Task join error: {}", e),
    }

    println!("\n✓ All tasks completed successfully!");
    println!("\nKey Benefits of SharedClient:");
    println!("• Thread-safe concurrent access to MCP client");
    println!("• Clean API without exposed Arc/Mutex types");
    println!("• Clone-able for easy sharing across tasks");
    println!("• Maintains strict MCP protocol compliance");

    Ok(())
}
