//! Interactive Elicitation Client
//!
//! This client connects to the elicitation_stdio_server and demonstrates
//! interactive task management through the MCP protocol.
//!
//! Run the server first in one terminal:
//! ```bash
//! cargo run --example elicitation_stdio_server 2>/dev/null
//! ```
//!
//! Then pipe them together:
//! ```bash
//! cargo run --example elicitation_stdio_server 2>/dev/null | \
//!   cargo run --package turbomcp-client --example elicitation_interactive_client
//! ```

use serde_json::json;
use std::collections::HashMap;
use std::error::Error;
use std::io::{self, Write};
use turbomcp_client::Client;
use turbomcp_transport::stdio::StdioTransport;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Set up logging to stderr so it doesn't interfere with stdio protocol
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter("warn")
        .init();

    eprintln!("🚀 Interactive Task Manager Client");
    eprintln!("===================================");
    eprintln!();

    // Create stdio transport for communication
    let transport = StdioTransport::new();

    // Create the client
    let client = Client::new(transport);

    // Initialize the connection
    eprintln!("[Client] Connecting to server...");
    let init_result = client.initialize().await?;
    eprintln!(
        "[Client] Connected to: {} v{}",
        init_result.server_info.name, init_result.server_info.version
    );

    // List available tools
    let tools = client.list_tools().await?;
    eprintln!();
    eprintln!("Available tools:");
    for tool in &tools {
        eprintln!(
            "  • {} - {}",
            tool.name,
            tool.description.as_deref().unwrap_or("No description")
        );
    }

    // Interactive loop
    loop {
        eprintln!();
        eprintln!("What would you like to do?");
        eprintln!("1. Create a new task");
        eprintln!("2. List all tasks");
        eprintln!("3. Update task priority");
        eprintln!("4. Exit");
        eprint!("Choice (1-4): ");
        io::stderr().flush()?;

        let mut choice = String::new();
        io::stdin().read_line(&mut choice)?;

        match choice.trim() {
            "1" => {
                // Create a new task
                eprintln!();
                eprintln!("Creating a new task...");
                eprintln!("The server will prompt you for details.");

                // Since we don't have ElicitationHandler in the client yet,
                // we'll simulate the interaction by providing the data directly
                eprintln!();
                eprint!("Task title: ");
                io::stderr().flush()?;
                let mut title = String::new();
                io::stdin().read_line(&mut title)?;

                eprint!("Task description (optional): ");
                io::stderr().flush()?;
                let mut description = String::new();
                io::stdin().read_line(&mut description)?;

                eprintln!("Priority levels: low, medium, high, critical");
                eprint!("Task priority: ");
                io::stderr().flush()?;
                let mut priority = String::new();
                io::stdin().read_line(&mut priority)?;

                eprint!("Assign to someone? (y/n): ");
                io::stderr().flush()?;
                let mut assign = String::new();
                io::stdin().read_line(&mut assign)?;

                let mut args = HashMap::new();
                // For now, we pass the data as tool arguments
                // In a full implementation, this would be handled through elicitation
                args.insert("title".to_string(), json!(title.trim()));
                args.insert("description".to_string(), json!(description.trim()));
                args.insert("priority".to_string(), json!(priority.trim()));

                if assign.trim().to_lowercase() == "y" {
                    eprint!("Assignee name: ");
                    io::stderr().flush()?;
                    let mut assignee = String::new();
                    io::stdin().read_line(&mut assignee)?;
                    args.insert("assigned_to".to_string(), json!(assignee.trim()));
                }

                // Note: The server expects elicitation, but since our client doesn't
                // support it yet, we'll get an error. This demonstrates the need for
                // ElicitationHandler implementation.
                match client.call_tool("create_task", Some(args)).await {
                    Ok(result) => {
                        if let Some(content_array) = result.as_array() {
                            for content_val in content_array {
                                if let Some(text) = content_val.get("text").and_then(|t| t.as_str())
                                {
                                    eprintln!();
                                    eprintln!("{}", text);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!();
                        eprintln!("Note: Task creation requires elicitation support.");
                        eprintln!("Error: {}", e);
                        eprintln!();
                        eprintln!(
                            "To fully test elicitation, the client needs ElicitationHandler implementation."
                        );
                    }
                }
            }
            "2" => {
                // List all tasks
                eprintln!();
                eprintln!("Fetching task list...");

                let result = client.call_tool("list_tasks", None).await?;
                if let Some(content_array) = result.as_array() {
                    for content_val in content_array {
                        if let Some(text) = content_val.get("text").and_then(|t| t.as_str()) {
                            eprintln!();
                            eprintln!("{}", text);
                        }
                    }
                }
            }
            "3" => {
                // Update task priority
                eprintln!();
                eprint!("Enter task ID to update: ");
                io::stderr().flush()?;
                let mut task_id = String::new();
                io::stdin().read_line(&mut task_id)?;

                let mut args = HashMap::new();
                args.insert("task_id".to_string(), json!(task_id.trim()));

                match client.call_tool("update_priority", Some(args)).await {
                    Ok(result) => {
                        if let Some(content_array) = result.as_array() {
                            for content_val in content_array {
                                if let Some(text) = content_val.get("text").and_then(|t| t.as_str())
                                {
                                    eprintln!();
                                    eprintln!("{}", text);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!();
                        eprintln!("Error: {}", e);
                    }
                }
            }
            "4" => {
                eprintln!("Goodbye!");
                break;
            }
            _ => {
                eprintln!("Invalid choice. Please try again.");
            }
        }
    }

    Ok(())
}
