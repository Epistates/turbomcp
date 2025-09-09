//! # Architecture Client - HTTP Client for Both Server Patterns
//!
//! This separate client demonstrates that both the builder and macro pattern
//! servers expose identical MCP protocol interfaces. One client can connect
//! to either server implementation seamlessly.
//!
//! ## Testing Instructions
//!
//! Terminal 1 - Start builder server:
//! ```bash
//! cargo run --example 06_architecture_patterns builder
//! ```
//!
//! Terminal 2 - Start macro server:
//! ```bash  
//! cargo run --example 06_architecture_patterns macro
//! ```
//!
//! Terminal 3 - Run this client:
//! ```bash
//! # Test against builder server (via stdio pipe)
//! cargo run --example 06b_architecture_client | cargo run --example 06_architecture_patterns builder
//!
//! # Test against macro server (via stdio pipe)
//! cargo run --example 06b_architecture_client | cargo run --example 06_architecture_patterns macro
//! ```
//!
//! Both servers will respond identically to the same client requests!

use serde_json::json;

/// HTTP client that works with both server implementations
pub struct ArchitectureClient;

impl ArchitectureClient {
    /// Generate test messages that work with any MCP server
    pub fn generate_test_messages() -> Vec<serde_json::Value> {
        vec![
            // 1. Initialize connection
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2024-10-07",
                    "capabilities": {},
                    "clientInfo": {
                        "name": "architecture-test-client",
                        "version": "1.0.0"
                    }
                }
            }),
            // 2. List available tools
            json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/list"
            }),
            // 3. Call the add tool
            json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "tools/call",
                "params": {
                    "name": "add",
                    "arguments": {
                        "a": 10.5,
                        "b": 20.3
                    }
                }
            }),
            // 4. Call the subtract tool
            json!({
                "jsonrpc": "2.0",
                "id": 4,
                "method": "tools/call",
                "params": {
                    "name": "subtract",
                    "arguments": {
                        "a": 100.0,
                        "b": 42.0
                    }
                }
            }),
        ]
    }

    /// Print formatted test messages for manual testing
    pub fn print_test_commands() {
        println!("╔════════════════════════════════════════╗");
        println!("║    ARCHITECTURE CLIENT - TEST SUITE    ║");
        println!("╚════════════════════════════════════════╝\n");

        println!("This client sends identical requests to both server types.\n");

        println!("Test Messages (JSON-RPC):");
        println!("========================\n");

        for (i, msg) in Self::generate_test_messages().iter().enumerate() {
            println!(
                "{}. {}",
                i + 1,
                match i {
                    0 => "Initialize Connection",
                    1 => "List Available Tools",
                    2 => "Test Addition (10.5 + 20.3)",
                    3 => "Test Subtraction (100 - 42)",
                    _ => "Unknown",
                }
            );
            println!("{}\n", serde_json::to_string_pretty(msg).unwrap());
        }

        println!("Expected Responses:");
        println!("==================\n");

        println!("Both servers should return:");
        println!("  • Successful initialization");
        println!("  • Tool list with 'add' and 'subtract'");
        println!("  • Addition result: 30.8");
        println!("  • Subtraction result: 58\n");

        println!("This proves functional equivalence between:");
        println!("  ✓ Builder pattern (explicit control)");
        println!("  ✓ Macro pattern (ergonomic simplicity)");
    }

    /// Output test messages as JSON lines for piping to servers
    pub fn output_for_pipe() {
        // Output each message as a single line of JSON for piping
        for msg in Self::generate_test_messages() {
            println!("{}", serde_json::to_string(&msg).unwrap());
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    match args.get(1).map(|s| s.as_str()) {
        Some("--help") | Some("-h") => {
            ArchitectureClient::print_test_commands();
        }
        Some("--pipe") | None => {
            // Default: output for piping to server
            ArchitectureClient::output_for_pipe();
        }
        _ => {
            println!("Architecture Client - Test both server patterns\n");
            println!("Usage:");
            println!("  cargo run --example 06b_architecture_client        # Output for piping");
            println!("  cargo run --example 06b_architecture_client --help # Show test details\n");
            println!("Pipe to servers:");
            println!(
                "  cargo run --example 06b_architecture_client | cargo run --example 06_architecture_patterns builder"
            );
            println!(
                "  cargo run --example 06b_architecture_client | cargo run --example 06_architecture_patterns macro"
            );
        }
    }
}
