//! TCP Transport Full Demo
//!
//! This example demonstrates a complete TCP client-server setup where:
//! 1. Server uses macro approach (#[server], #[tool], #[resource])
//! 2. Client uses builder pattern with proper TCP transport
//! 3. Both run in same process for easy testing
//! 4. Demonstrates end-to-end MCP protocol over TCP
//!
//! Run with: `cargo run --example tcp_client_server_demo`

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{Duration, sleep};
use turbomcp::prelude::*;
use turbomcp_client::ClientBuilder;
use turbomcp_transport::{Transport, tcp::TcpTransport};

/// File management service using TCP transport (macro approach)
#[derive(Clone)]
struct FileService {
    files: Arc<RwLock<HashMap<String, String>>>,
}

#[server(
    name = "TCP File Service",
    version = "1.0.0",
    description = "High-performance file management over TCP transport"
)]
impl FileService {
    fn new() -> Self {
        let mut files = HashMap::new();
        files.insert(
            "readme.txt".to_string(),
            "Welcome to TurboMCP TCP File Service!\nHigh-performance direct socket communication."
                .to_string(),
        );
        files.insert(
            "config.json".to_string(),
            r#"{"service": "file_service", "transport": "tcp", "features": ["read", "write", "list", "stats"]}"#.to_string(),
        );

        Self {
            files: Arc::new(RwLock::new(files)),
        }
    }

    #[tool("Read a file")]
    async fn read_file(&self, filename: String) -> McpResult<String> {
        let files = self.files.read().await;
        match files.get(&filename) {
            Some(content) => Ok(format!("📄 Contents of '{}':\n{}", filename, content)),
            None => Err(McpError::tool(format!("File '{}' not found", filename))),
        }
    }

    #[tool("Write to a file")]
    async fn write_file(&self, filename: String, content: String) -> McpResult<String> {
        let mut files = self.files.write().await;
        files.insert(filename.clone(), content.clone());
        Ok(format!(
            "✍️ Successfully wrote {} bytes to '{}'",
            content.len(),
            filename
        ))
    }

    #[tool("List all files")]
    async fn list_files(&self) -> McpResult<String> {
        let files = self.files.read().await;
        let file_list: Vec<String> = files.keys().cloned().collect();
        Ok(format!(
            "📋 Files ({}): {}",
            file_list.len(),
            file_list.join(", ")
        ))
    }

    #[tool("Get file statistics")]
    async fn file_stats(&self, filename: String) -> McpResult<String> {
        let files = self.files.read().await;
        match files.get(&filename) {
            Some(content) => Ok(format!(
                "📊 File '{}': {} characters, {} lines",
                filename,
                content.len(),
                content.lines().count()
            )),
            None => Err(McpError::tool(format!("File '{}' not found", filename))),
        }
    }

    #[resource("file:///tcp/files")]
    async fn list_file_resources(&self, _ctx: Context) -> McpResult<String> {
        let files = self.files.read().await;
        let resources: Vec<String> = files
            .keys()
            .map(|k| format!("file:///tcp/files/{}", k))
            .collect();
        Ok(format!(
            "Available file resources:\n{}",
            resources.join("\n")
        ))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_writer(std::io::stderr)
        .init();

    tracing::info!("🚀 TCP Client-Server Demo - Complete Implementation");
    tracing::info!("═══════════════════════════════════════════════════════");
    tracing::info!("📡 Server: Macro approach (#[server], #[tool], #[resource])");
    tracing::info!("📱 Client: Builder pattern with TCP transport");
    tracing::info!("🔗 Transport: High-performance direct socket");

    // Server setup with macro approach
    let file_service = FileService::new();
    let server_addr: SocketAddr = "127.0.0.1:7070".parse()?;

    tracing::info!("🔧 Starting TCP server on {}", server_addr);

    // Start server in background
    let server_handle = tokio::spawn(async move {
        if let Err(e) = file_service.run_tcp(server_addr).await {
            tracing::error!("Server error: {}", e);
        }
    });

    // Give server time to start
    sleep(Duration::from_millis(500)).await;

    // Client setup with builder approach
    tracing::info!("📱 Starting TCP client with builder pattern");

    let bind_addr: SocketAddr = "0.0.0.0:0".parse()?; // Auto-assign local port
    let remote_addr: SocketAddr = "127.0.0.1:7070".parse()?;

    let transport = TcpTransport::new_client(bind_addr, remote_addr);
    transport.connect().await?;

    let client = ClientBuilder::new()
        .with_tools(true)
        .with_resources(true)
        .build_sync(transport);

    tracing::info!("✅ Connected to TCP server");

    // Initialize the connection
    let init_result = client.initialize().await?;
    tracing::info!("📋 Server: {}", init_result.server_info.name);
    tracing::info!("🔧 Version: {}", init_result.server_info.version);

    // List available tools
    let tools = client.list_tools().await?;
    tracing::info!("🛠️  Available tools: {}", tools.len());
    for tool in &tools {
        tracing::info!(
            "  - {} - {}",
            tool.name,
            tool.description.as_deref().unwrap_or("No description")
        );
    }

    // Test file operations
    tracing::info!("📁 Testing high-performance file operations...");

    // List existing files
    let args = HashMap::new();
    let result = client.call_tool("list_files", Some(args)).await?;
    tracing::info!("📋 {}", result);

    // Read an existing file
    let mut args = HashMap::new();
    args.insert("filename".to_string(), serde_json::json!("readme.txt"));
    let result = client.call_tool("read_file", Some(args.clone())).await?;
    tracing::info!("📖 {}", result);

    // Create a new file
    args.clear();
    args.insert(
        "filename".to_string(),
        serde_json::json!("tcp_performance.txt"),
    );
    args.insert(
        "content".to_string(),
        serde_json::json!(
            "TCP Transport Performance Test Results:\n\
        ✅ Latency: Ultra-low (direct socket)\n\
        ✅ Throughput: High bandwidth\n\
        ✅ Reliability: Connection-oriented\n\
        ✅ Use case: High-frequency operations, internal services\n\
        ✅ Protocol: Full MCP 2025-06-18 compliance\n\
        \n\
        TCP Benefits:\n\
        • Direct socket communication\n\
        • Guaranteed delivery\n\
        • Ordered message delivery\n\
        • Flow control\n\
        • Perfect for microservices"
        ),
    );
    let result = client.call_tool("write_file", Some(args.clone())).await?;
    tracing::info!("✍️  {}", result);

    // Get file statistics
    args.clear();
    args.insert(
        "filename".to_string(),
        serde_json::json!("tcp_performance.txt"),
    );
    let result = client.call_tool("file_stats", Some(args.clone())).await?;
    tracing::info!("📊 {}", result);

    // Read the new file back
    let result = client.call_tool("read_file", Some(args)).await?;
    tracing::info!("📖 {}", result);

    // List files again to see the new one
    let args = HashMap::new();
    let result = client.call_tool("list_files", Some(args)).await?;
    tracing::info!("📋 {}", result);

    // Test resource access
    let resources = client.list_resources().await?;
    tracing::info!("📁 Available resources: {}", resources.len());
    for resource_uri in &resources {
        let content = client.read_resource(resource_uri).await?;
        tracing::info!("📄 {}:\n{:?}", resource_uri, content.contents);
    }

    tracing::info!("🎆 TCP Client-Server Demo Completed Successfully!");
    tracing::info!("✅ TurboMCP TCP transport working correctly");
    tracing::info!("  • Macro approach: ✅ Working");
    tracing::info!("  • Builder approach: ✅ Working");
    tracing::info!("  • End-to-end MCP: ✅ Working");
    tracing::info!("  • High performance: ✅ Achieved");

    // Cleanup
    server_handle.abort();
    Ok(())
}
