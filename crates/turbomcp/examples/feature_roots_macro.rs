//! Roots Configuration via Macro - World-class DX
//!
//! This example demonstrates TurboMCP's superior developer experience with
//! clean, declarative roots configuration directly in the #[server] macro.
//!
//! Compare this to rust-sdk which:
//! - Has NO macro support for roots
//! - Requires manual builder configuration
//! - Provides no compile-time validation
//! - Lacks root-aware tool helpers
//!
//! Run with: `cargo run --example feature_roots_macro`
//! Test roots: echo '{"jsonrpc":"2.0","id":1,"method":"roots/list"}' | cargo run --example feature_roots_macro 2>/dev/null | jq

use std::path::PathBuf;
use tokio::fs;
use turbomcp::prelude::*;

/// File system operations server with macro-configured roots
#[derive(Clone)]
struct FileSystemServer {
    // Could store state if needed
}

#[server(
    name = "filesystem-server",
    version = "1.0.0",
    description = "World-class file system server with declarative roots",
    // Clean, simple root declarations - no complex array syntax!
    root = "file:///Users/nickpaterno/work/turbomcp:Project Root",
    root = "file:///Users/nickpaterno/Documents:Documents",
    root = "file:///Users/nickpaterno/Downloads:Downloads",
    root = "file:///tmp:Temporary Files"
)]
impl FileSystemServer {
    #[tool("List files in a directory relative to a root")]
    async fn list_files(
        &self,
        root_name: String,
        relative_path: Option<String>,
    ) -> McpResult<Vec<String>> {
        // In a production implementation, we would:
        // 1. Query the configured roots from the server
        // 2. Find the root matching root_name
        // 3. Construct the full path
        // 4. List files respecting root boundaries

        // For demonstration, we'll use a simple mapping
        let root_path = match root_name.as_str() {
            "Project Root" => "/Users/nickpaterno/work/turbomcp",
            "Documents" => "/Users/nickpaterno/Documents",
            "Downloads" => "/Users/nickpaterno/Downloads",
            "Temporary Files" => "/tmp",
            _ => return Err(mcp_error!("Unknown root: {}", root_name).into()),
        };

        let full_path = if let Some(rel) = relative_path {
            PathBuf::from(root_path).join(rel)
        } else {
            PathBuf::from(root_path)
        };

        // Ensure we don't escape the root
        let canonical = full_path
            .canonicalize()
            .map_err(|e| mcp_error!("Invalid path: {}", e))?;

        if !canonical.starts_with(root_path) {
            return Err(mcp_error!("Path escapes root boundary").into());
        }

        let mut entries = Vec::new();
        let mut dir = fs::read_dir(canonical)
            .await
            .map_err(|e| mcp_error!("Failed to read directory: {}", e))?;

        while let Some(entry) = dir
            .next_entry()
            .await
            .map_err(|e| mcp_error!("Failed to read entry: {}", e))?
        {
            if let Some(name) = entry.file_name().to_str() {
                entries.push(name.to_string());
            }
        }

        Ok(entries)
    }

    #[tool("Get file info respecting root boundaries")]
    async fn file_info(
        &self,
        root_name: String,
        file_path: String,
    ) -> McpResult<serde_json::Value> {
        let root_path = match root_name.as_str() {
            "Project Root" => "/Users/nickpaterno/work/turbomcp",
            "Documents" => "/Users/nickpaterno/Documents",
            "Downloads" => "/Users/nickpaterno/Downloads",
            "Temporary Files" => "/tmp",
            _ => return Err(mcp_error!("Unknown root: {}", root_name).into()),
        };

        let full_path = PathBuf::from(root_path).join(&file_path);
        let canonical = full_path
            .canonicalize()
            .map_err(|e| mcp_error!("Invalid path: {}", e))?;

        if !canonical.starts_with(root_path) {
            return Err(mcp_error!("Path escapes root boundary").into());
        }

        let metadata = fs::metadata(&canonical)
            .await
            .map_err(|e| mcp_error!("Failed to get file metadata: {}", e))?;

        Ok(serde_json::json!({
            "path": file_path,
            "root": root_name,
            "size": metadata.len(),
            "is_file": metadata.is_file(),
            "is_dir": metadata.is_dir(),
            "modified": metadata.modified()
                .map(|t| format!("{:?}", t))
                .unwrap_or_else(|_| "unknown".to_string())
        }))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt().with_env_filter("info").init();

    println!("🚀 FileSystem Server with Macro-Configured Roots");
    println!("======================================================");
    println!("This demonstrates TurboMCP's superior DX:");
    println!("- Roots configured directly in #[server] macro");
    println!("- Zero boilerplate for roots setup");
    println!("- Automatic registration with the server");
    println!();
    println!("The rust-sdk requires manual builder configuration,");
    println!("while TurboMCP provides declarative, compile-time roots!");
    println!();
    println!("Test the roots configuration:");
    println!(
        "  echo '{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"roots/list\"}}' | cargo run --example feature_roots_macro 2>/dev/null | jq"
    );
    println!();
    println!("Starting server on stdio...");

    let server = FileSystemServer {};
    server.run_stdio().await?;

    Ok(())
}
