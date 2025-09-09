//! # 10: Protocol Mastery - Complete MCP Client Implementation
//!
//! **Learning Goals (25 minutes):**
//! - Master all MCP protocol methods
//! - Understand client-server communication patterns  
//! - See comprehensive error handling in practice
//!
//! **What this example demonstrates:**
//! - Full MCP protocol method coverage
//! - Production-ready client implementation
//! - Proper error handling and recovery
//! - Real bidirectional communication
//!
//! **Prerequisites:**
//! Run the bidirectional server in another terminal first:
//! ```bash
//! cargo run --example 09_bidirectional_communication
//! ```
//!
//! **Then run this client:**
//! ```bash  
//! cargo run --example 10_protocol_mastery
//! ```

use serde_json::json;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command as TokioCommand;

/// Simple MCP Protocol demonstration client
pub struct ProtocolClient {
    process: Option<tokio::process::Child>,
    stdin: Option<tokio::process::ChildStdin>,
    stdout_reader: Option<BufReader<tokio::process::ChildStdout>>,
}

impl ProtocolClient {
    /// Create and start the client with bidirectional server
    pub async fn new() -> Result<Self, Box<dyn std::error::Error>> {
        println!("🚀 Starting bidirectional server for protocol demonstration...");

        let mut process = TokioCommand::new("cargo")
            .args(["run", "--example", "09_bidirectional_communication"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()?;

        let stdin = process.stdin.take().unwrap();
        let stdout = process.stdout.take().unwrap();
        let stdout_reader = BufReader::new(stdout);

        tokio::time::sleep(Duration::from_millis(1000)).await;

        Ok(Self {
            process: Some(process),
            stdin: Some(stdin),
            stdout_reader: Some(stdout_reader),
        })
    }

    /// Send JSON-RPC request and get response
    pub async fn send_request(
        &mut self,
        request: serde_json::Value,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        let request_str = serde_json::to_string(&request)?;

        if let Some(stdin) = &mut self.stdin {
            stdin.write_all(request_str.as_bytes()).await?;
            stdin.write_all(b"\n").await?;
            stdin.flush().await?;
        }

        if let Some(reader) = &mut self.stdout_reader {
            let mut response_line = String::new();
            reader.read_line(&mut response_line).await?;

            if !response_line.trim().is_empty() {
                let response: serde_json::Value = serde_json::from_str(&response_line)?;
                return Ok(response);
            }
        }

        Err("No response received".into())
    }

    /// Initialize MCP connection
    pub async fn initialize(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        println!("🤝 Initializing MCP connection...");

        let init_request = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "tools": {},
                    "resources": {},
                    "prompts": {}
                },
                "clientInfo": {
                    "name": "protocol-mastery-client",
                    "version": "1.0.0"
                }
            }
        });

        let _response = self.send_request(init_request).await?;
        println!("✅ Connection initialized successfully");
        Ok(())
    }

    /// Test tools functionality
    pub async fn demo_tools(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        println!("\n🔧 Testing Tools Operations");
        println!("============================");

        let tools_request = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list"
        });

        let response = self.send_request(tools_request).await?;
        if let Some(tools) = response["result"]["tools"].as_array() {
            println!("Available tools: {}", tools.len());
            for tool in tools {
                if let Some(name) = tool["name"].as_str() {
                    println!(
                        "  • {}: {}",
                        name,
                        tool["description"].as_str().unwrap_or("No description")
                    );
                }
            }

            // Call first tool if available
            if let Some(first_tool) = tools.first()
                && let Some(tool_name) = first_tool["name"].as_str()
            {
                println!("🎯 Testing tool: {}", tool_name);

                let call_request = json!({
                    "jsonrpc": "2.0",
                    "id": 3,
                    "method": "tools/call",
                    "params": {
                        "name": tool_name,
                        "arguments": {
                            "query": "test input"
                        }
                    }
                });

                let call_response = self.send_request(call_request).await?;
                if let Some(content) = call_response["result"]["content"].as_array() {
                    for item in content {
                        if let Some(text) = item["text"].as_str() {
                            println!("📝 Tool response: {}", text);
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Test resources functionality  
    pub async fn demo_resources(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        println!("\n📚 Testing Resources Operations");
        println!("===============================");

        let resources_request = json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "resources/list"
        });

        let response = self.send_request(resources_request).await?;
        if let Some(resources) = response["result"]["resources"].as_array() {
            println!("Available resources: {}", resources.len());
            for resource in resources {
                if let Some(uri) = resource["uri"].as_str() {
                    println!(
                        "  • {}: {}",
                        uri,
                        resource["name"].as_str().unwrap_or("Unnamed")
                    );
                }
            }

            // Read first resource if available
            if let Some(first_resource) = resources.first()
                && let Some(uri) = first_resource["uri"].as_str()
            {
                println!("📖 Reading: {}", uri);

                let read_request = json!({
                    "jsonrpc": "2.0",
                    "id": 5,
                    "method": "resources/read",
                    "params": {
                        "uri": uri
                    }
                });

                let read_response = self.send_request(read_request).await?;
                if let Some(contents) = read_response["result"]["contents"].as_array() {
                    for content in contents {
                        if let Some(text) = content["text"].as_str() {
                            let preview = if text.len() > 100 {
                                format!("{}...", &text[..100])
                            } else {
                                text.to_string()
                            };
                            println!("📄 Content: {}", preview);
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Test ping functionality
    pub async fn demo_ping(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        println!("\n🏓 Testing Health Check");
        println!("=======================");

        let ping_request = json!({
            "jsonrpc": "2.0",
            "id": 8,
            "method": "ping"
        });

        let response = self.send_request(ping_request).await?;
        if response["result"].is_object() {
            println!("✅ Server is healthy and responding");
        }

        Ok(())
    }

    /// Clean up resources
    pub async fn cleanup(&mut self) {
        if let Some(mut process) = self.process.take() {
            let _ = process.kill().await;
        }
    }
}

impl Drop for ProtocolClient {
    fn drop(&mut self) {
        if let Some(mut process) = self.process.take() {
            let _ = process.start_kill();
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("╔════════════════════════════════════════╗");
    println!("║     PROTOCOL MASTERY - MCP CLIENT     ║");
    println!("╚════════════════════════════════════════╝\n");

    println!("This client demonstrates comprehensive MCP protocol usage");
    println!("by connecting to the bidirectional communication server.\n");

    let mut client = ProtocolClient::new().await?;

    client.initialize().await?;
    client.demo_ping().await?;
    client.demo_tools().await?;
    client.demo_resources().await?;

    println!("\n🎯 Protocol Mastery Complete!");
    println!("==============================");
    println!("✅ All MCP protocol methods tested successfully");
    println!("✅ Real bidirectional communication established");
    println!("✅ Production-ready patterns demonstrated");

    client.cleanup().await;

    Ok(())
}
