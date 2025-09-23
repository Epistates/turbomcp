//! # 08: Elicitation Client - STDIO MCP Client
//!
//! **Learning Goals (5 minutes):**
//! - Connect to MCP server via subprocess and STDIO
//! - Send real JSON-RPC requests over stdin/stdout
//! - Trigger elicitation demonstration
//!
//! **What this example demonstrates:**
//! - Real STDIO MCP client spawning server subprocess
//! - JSON-RPC request/response handling over STDIO
//! - MCP protocol methods (initialize, tools/list, tools/call)
//!
//! **Usage:**
//! ```bash
//! cargo run --example 08_elicitation_client
//! ```
//!
//! This will automatically start the server as a subprocess and demonstrate
//! the complete elicitation workflow.

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};

/// STDIO MCP client that spawns and communicates with server subprocess
struct ElicitationClient {
    process: Child,
    reader: Option<BufReader<tokio::process::ChildStdout>>,
    request_id: u32,
}

impl ElicitationClient {
    /// Spawn the elicitation server as a subprocess
    async fn new() -> Result<Self, Box<dyn std::error::Error>> {
        println!("🚀 Starting elicitation server subprocess...");

        let mut process = Command::new("cargo")
            .args(["run", "--example", "08_elicitation_server"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()?;

        println!("✅ Server started, initializing MCP connection...");

        let reader = process.stdout.take().map(BufReader::new);

        Ok(Self {
            process,
            reader,
            request_id: 0,
        })
    }

    /// Send JSON-RPC request to server via stdin and read response from stdout
    async fn send_request(
        &mut self,
        method: &str,
        params: Option<Value>,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        self.request_id += 1;

        let request = json!({
            "jsonrpc": "2.0",
            "id": self.request_id,
            "method": method,
            "params": params
        });

        println!("📤 Sending: {}", method);

        // Send request to server stdin
        if let Some(stdin) = &mut self.process.stdin {
            let request_str = serde_json::to_string(&request)?;
            stdin.write_all(request_str.as_bytes()).await?;
            stdin.write_all(b"\n").await?;
            stdin.flush().await?;
        } else {
            return Err("Server stdin not available".into());
        }

        // Read response from server stdout
        if let Some(reader) = &mut self.reader {
            let mut line = String::new();
            reader.read_line(&mut line).await?;

            if line.trim().is_empty() {
                return Err("Server returned empty response".into());
            }

            let response: Value = serde_json::from_str(line.trim())?;
            println!("📥 Response: {}", serde_json::to_string_pretty(&response)?);
            println!();

            Ok(response)
        } else {
            Err("Server stdout not available".into())
        }
    }

    /// Initialize MCP connection
    async fn initialize(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let params = json!({
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": {
                "name": "elicitation-client",
                "version": "1.0.0"
            }
        });

        let response = self.send_request("initialize", Some(params)).await?;

        if let Some(result) = response["result"].as_object()
            && let Some(server_info) = result["serverInfo"].as_object()
        {
            println!(
                "✅ Connected to server: {} v{}",
                server_info["name"].as_str().unwrap_or("unknown"),
                server_info["version"].as_str().unwrap_or("unknown")
            );
        }

        Ok(())
    }

    /// List available tools
    async fn list_tools(&mut self) -> Result<Vec<String>, Box<dyn std::error::Error>> {
        let response = self.send_request("tools/list", None).await?;

        let mut tools = Vec::new();
        if let Some(result) = response["result"].as_object()
            && let Some(tools_array) = result["tools"].as_array()
        {
            for tool in tools_array {
                if let Some(name) = tool["name"].as_str() {
                    tools.push(name.to_string());
                    println!(
                        "🔧 Tool: {} - {}",
                        name,
                        tool["description"].as_str().unwrap_or("no description")
                    );
                }
            }
        }

        Ok(tools)
    }

    /// Call a tool with arguments
    async fn call_tool(
        &mut self,
        name: &str,
        arguments: Value,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let params = json!({
            "name": name,
            "arguments": arguments
        });

        let response = self.send_request("tools/call", Some(params)).await?;
        Ok(response)
    }

    /// Demonstrate the complete elicitation workflow
    async fn run_demo(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        println!("🎯 TurboMCP Elicitation Demo");
        println!("============================\n");

        // Initialize connection
        self.initialize().await?;
        println!();

        // List tools
        println!("📋 Available tools:");
        let tools = self.list_tools().await?;
        println!();

        // Demonstrate tool calls
        if tools.contains(&"show_config".to_string()) {
            println!("🔍 Testing show_config tool:");
            let result = self.call_tool("show_config", json!({})).await?;
            println!("Result: {}", serde_json::to_string_pretty(&result)?);
            println!();
        }

        if tools.contains(&"setup_user_profile".to_string()) {
            println!("👤 Testing setup_user_profile tool (demonstrates elicitation):");
            let result = self.call_tool("setup_user_profile", json!({})).await?;
            println!("Result: {}", serde_json::to_string_pretty(&result)?);
            println!();
        }

        if tools.contains(&"explain_elicitation".to_string()) {
            println!("📚 Testing explain_elicitation tool:");
            let result = self.call_tool("explain_elicitation", json!({})).await?;
            println!("Result: {}", serde_json::to_string_pretty(&result)?);
            println!();
        }

        println!("✅ Elicitation demo completed successfully!");

        Ok(())
    }

    /// Clean shutdown
    async fn shutdown(mut self) -> Result<(), Box<dyn std::error::Error>> {
        println!("🛑 Shutting down server...");

        // Close stdin to signal server to exit
        drop(self.process.stdin.take());

        // Wait for process to exit
        let _ = self.process.wait().await;

        println!("✅ Shutdown complete");
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create client and run demo
    let mut client = ElicitationClient::new().await?;

    // Run the complete demonstration
    if let Err(e) = client.run_demo().await {
        eprintln!("❌ Demo failed: {}", e);
        let _ = client.shutdown().await;
        return Err(e);
    }

    // Clean shutdown
    client.shutdown().await?;

    Ok(())
}
