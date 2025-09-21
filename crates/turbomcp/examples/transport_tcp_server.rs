//! TCP Transport Server - High Performance Direct Socket
//!
//! This example demonstrates the TCP transport which provides
//! high-performance direct socket communication.
//!
//! Run with: `cargo run --example transport_tcp_server`

use std::sync::Arc;
use tokio::sync::RwLock;
use turbomcp::prelude::*;

/// File service using TCP transport (macro approach)
#[derive(Clone)]
struct FileService {
    files: Arc<RwLock<std::collections::HashMap<String, String>>>,
}

#[server(
    name = "File Service",
    version = "1.0.0",
    description = "TCP transport file management service"
)]
impl FileService {
    fn new() -> Self {
        let mut files = std::collections::HashMap::new();
        files.insert(
            "readme.txt".to_string(),
            "Welcome to TurboMCP File Service!\nThis service manages files over TCP transport."
                .to_string(),
        );
        files.insert("config.json".to_string(), r#"{"service": "file_service", "transport": "tcp", "features": ["read", "write", "list"]}"#.to_string());

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
            "✅ Successfully wrote {} characters to '{}'",
            content.len(),
            filename
        ))
    }

    #[tool("List all files")]
    async fn list_files(&self) -> McpResult<String> {
        let files = self.files.read().await;
        let file_list: Vec<String> = files.keys().cloned().collect();

        if file_list.is_empty() {
            Ok("📁 No files available".to_string())
        } else {
            Ok(format!(
                "📁 Available files ({}):\n{}",
                file_list.len(),
                file_list.join("\n")
            ))
        }
    }

    #[tool("Delete a file")]
    async fn delete_file(&self, filename: String) -> McpResult<String> {
        let mut files = self.files.write().await;

        match files.remove(&filename) {
            Some(_) => Ok(format!("🗑️  Successfully deleted '{}'", filename)),
            None => Err(McpError::tool(format!("File '{}' not found", filename))),
        }
    }

    #[tool("Get file statistics")]
    async fn get_stats(&self) -> McpResult<String> {
        let files = self.files.read().await;
        let total_files = files.len();
        let total_chars: usize = files.values().map(|content| content.len()).sum();

        Ok(format!(
            "📊 File Service Statistics:\n• Total files: {}\n• Total characters: {}\n• Transport: TCP (high performance)\n• Features: Read, Write, List, Delete",
            total_files, total_chars
        ))
    }

    #[resource("files://list")]
    async fn files_list_resource(&self) -> McpResult<String> {
        let files = self.files.read().await;
        let file_list: Vec<String> = files.keys().cloned().collect();
        Ok(format!("📁 File List:\n{}", file_list.join("\n• ")))
    }

    #[resource("files://stats")]
    async fn files_stats_resource(&self) -> McpResult<String> {
        let files = self.files.read().await;
        Ok(format!(
            "🚀 TCP File Service Status:\n• Transport: High-performance TCP\n• Files managed: {}\n• Status: Active",
            files.len()
        ))
    }

    #[prompt("Generate file management prompt")]
    async fn file_management_prompt(&self, operation: Option<String>) -> McpResult<String> {
        match operation.as_deref() {
            Some("backup") => Ok("Create a backup strategy for all files in the system. Include file verification and restore procedures.".to_string()),
            Some("organize") => Ok("Organize files into logical categories and suggest folder structures for better management.".to_string()),
            _ => Ok("Generate a comprehensive file management plan including organization, backup, and maintenance procedures.".to_string()),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // CRITICAL: For MCP STDIO protocol, logs MUST go to stderr, not stdout
    // stdout is reserved for pure JSON-RPC messages only
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_writer(std::io::stderr) // Fix: Send logs to stderr
        .init();

    tracing::info!("🚀 Starting TCP File Service");
    tracing::info!("🔗 This demonstrates TCP transport for high-performance communication");
    tracing::info!("📁 File management service with full CRUD operations");

    let service = FileService::new();

    tracing::info!("✅ TCP file service ready to start on 127.0.0.1:7071");
    tracing::info!("📡 Using REAL TCP transport for high-performance communication");

    // Run on TCP transport - REAL implementation, not STDIO!
    service.run_tcp("127.0.0.1:7071").await?;

    Ok(())
}
