//! Example 15: Elicitation Comparison - Client
//!
//! Simple client that works with the elicitation comparison server.
//! This demonstrates how clients interact with elicitation handlers.
//!
//! First, run the server in one terminal:
//! ```bash
//! cargo run --example 15_elicitation_comparison_server
//! ```
//!
//! Then run this client in another terminal:
//! ```bash
//! cargo run --example 15_elicitation_comparison_client
//! ```

use std::io::{self, Write};
use turbomcp_client::Client;
use turbomcp_transport::stdio::StdioTransport;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 Elicitation Comparison Client");
    println!("=================================");
    println!("This client demonstrates elicitation interaction.");
    println!();

    // Create transport and client
    let transport = StdioTransport::new();
    let mut client = Client::new(transport);

    // Initialize connection
    let init_result = client.initialize().await?;
    println!(
        "✅ Connected to server: {} v{}",
        init_result.server_info.name, init_result.server_info.version
    );

    // List available tools
    let tools = client.list_tools().await?;
    println!("\n📦 Available tools:");
    for tool in &tools {
        println!("  - {}", tool);
    }

    loop {
        println!("\n🎯 Choose a deployment method:");
        println!("1. Deploy with macro approach");
        println!("2. Deploy with builder approach");
        println!("3. List deployment methods");
        println!("4. Exit");
        print!("\nChoice (1-4): ");
        io::stdout().flush()?;

        let mut choice = String::new();
        io::stdin().read_line(&mut choice)?;

        match choice.trim() {
            "1" => {
                print!("Enter project name: ");
                io::stdout().flush()?;
                let mut project = String::new();
                io::stdin().read_line(&mut project)?;

                // Call the macro-based deployment tool
                let mut args = std::collections::HashMap::new();
                args.insert("project".to_string(), serde_json::json!(project.trim()));

                println!("\n📝 Note: In a real MCP client, this would trigger UI for elicitation.");
                println!("For this demo, the server will use default values.\n");

                match client.call_tool("deploy_macro", Some(args)).await {
                    Ok(result) => println!("Result: {}", result),
                    Err(e) => println!("Error: {}", e),
                }
            }
            "2" => {
                print!("Enter project name: ");
                io::stdout().flush()?;
                let mut project = String::new();
                io::stdin().read_line(&mut project)?;

                // Call the builder-based deployment tool
                let mut args = std::collections::HashMap::new();
                args.insert("project".to_string(), serde_json::json!(project.trim()));

                println!("\n📝 Note: In a real MCP client, this would trigger UI for elicitation.");
                println!("For this demo, the server will use default values.\n");

                match client.call_tool("deploy_builder", Some(args)).await {
                    Ok(result) => println!("Result: {}", result),
                    Err(e) => println!("Error: {}", e),
                }
            }
            "3" => match client.call_tool("list_methods", None).await {
                Ok(result) => println!("{}", result),
                Err(e) => println!("Error: {}", e),
            },
            "4" | "exit" | "quit" => {
                println!("👋 Goodbye!");
                break;
            }
            _ => {
                println!("❌ Invalid choice. Please try again.");
            }
        }
    }

    Ok(())
}
