//! # 08: Elicitation Client - HTTP MCP Client
//!
//! **Learning Goals (5 minutes):**
//! - Connect to MCP server over HTTP
//! - Send real JSON-RPC requests
//! - Trigger elicitation demonstration
//!
//! **What this example demonstrates:**
//! - Real HTTP MCP client using reqwest
//! - JSON-RPC request/response handling
//! - MCP protocol methods (initialize, tools/list, tools/call)
//!
//! **Prerequisites:**
//! Start the elicitation server in another terminal:
//! ```bash
//! cargo run --example 08_elicitation_server
//! ```
//!
//! **Then run this client:**
//! ```bash
//! cargo run --example 08_elicitation_client
//! ```

use serde_json::{Value, json};

/// Simple HTTP MCP client
struct ElicitationClient {
    client: reqwest::Client,
    server_url: String,
    request_id: u32,
}

impl ElicitationClient {
    fn new(server_url: &str) -> Self {
        Self {
            client: reqwest::Client::new(),
            server_url: server_url.to_string(),
            request_id: 0,
        }
    }

    /// Send JSON-RPC request to MCP server
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

        println!("📤 Sending request: {}", method);
        println!("   {}", serde_json::to_string_pretty(&request)?);

        let response = self
            .client
            .post(format!("{}/mcp", self.server_url))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(format!("HTTP error: {}", response.status()).into());
        }

        let response_json: Value = response.json().await?;

        println!("📥 Response:");
        println!("   {}", serde_json::to_string_pretty(&response_json)?);
        println!();

        Ok(response_json)
    }

    /// Initialize MCP connection
    async fn initialize(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let params = json!({
            "protocolVersion": "2025-06-18",
            "capabilities": {
                "elicitation": {}
            },
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
                        tool["description"].as_str().unwrap_or("No description")
                    );
                }
            }
        }

        Ok(tools)
    }

    /// Call a tool
    async fn call_tool(
        &mut self,
        tool_name: &str,
        arguments: Value,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let params = json!({
            "name": tool_name,
            "arguments": arguments
        });

        let response = self.send_request("tools/call", Some(params)).await?;
        Ok(response)
    }

    /// Demonstrate elicitation workflow
    async fn demonstrate_elicitation(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        println!("🎯 Demonstrating MCP elicitation workflow...\n");

        // Call setup_user_profile to trigger elicitation demonstration
        let response = self.call_tool("setup_user_profile", json!({})).await?;

        if let Some(result) = response["result"].as_object()
            && let Some(content) = result["content"].as_array()
            && let Some(first_content) = content.first()
            && let Some(text) = first_content["text"].as_str()
        {
            println!("📋 Elicitation Result:");
            println!("{}", text);
        }

        Ok(())
    }

    /// Show current configuration
    async fn show_config(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        println!("📋 Checking current configuration...\n");

        let response = self.call_tool("show_config", json!({})).await?;

        if let Some(result) = response["result"].as_object()
            && let Some(content) = result["content"].as_array()
            && let Some(first_content) = content.first()
            && let Some(text) = first_content["text"].as_str()
        {
            println!("{}", text);
        }

        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 TurboMCP Elicitation Client");
    println!("==============================\n");

    let mut client = ElicitationClient::new("http://127.0.0.1:8080");

    // Initialize connection
    println!("🤝 Initializing MCP connection...");
    client.initialize().await?;
    println!();

    // List available tools
    println!("📋 Listing available tools...");
    let tools = client.list_tools().await?;
    println!();

    if tools.is_empty() {
        println!("❌ No tools available");
        return Ok(());
    }

    // Demonstrate elicitation
    client.demonstrate_elicitation().await?;
    println!();

    // Show final configuration
    client.show_config().await?;

    println!("\n✅ MCP elicitation demonstration complete!");
    println!("💡 This shows the elicitation schema that would be sent to MCP clients");
    println!("   In production, the client would present a form UI to collect user input");

    Ok(())
}
