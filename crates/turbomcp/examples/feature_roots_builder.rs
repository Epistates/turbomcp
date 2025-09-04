//! Roots Configuration Example using ServerBuilder
//!
//! This example shows how to configure filesystem roots using the
//! ServerBuilder API directly. Roots define the boundaries of where
//! servers can operate within the filesystem.
//!
//! Run with:
//! ```bash
//! cargo run --example feature_roots_builder
//! ```
//!
//! Test roots listing:
//! ```bash
//! echo '{"jsonrpc":"2.0","id":1,"method":"roots/list"}' | cargo run --example feature_roots_builder 2>/dev/null | jq
//! ```

use std::collections::HashMap;
use std::path::PathBuf;
use turbomcp_protocol::types::{
    CallToolRequest, CallToolResult, Content, Root, TextContent, Tool, ToolInputSchema,
};
use turbomcp_server::handlers::FunctionToolHandler;
use turbomcp_server::{ServerBuilder, ServerError};

/// List files in a directory (validates against configured roots)
async fn list_files(
    req: CallToolRequest,
    _ctx: turbomcp_core::RequestContext,
) -> Result<CallToolResult, ServerError> {
    let path = req
        .arguments
        .as_ref()
        .and_then(|args| args.get("path"))
        .and_then(|v| v.as_str())
        .unwrap_or(".");

    let path = PathBuf::from(path);

    let content = if !path.exists() {
        format!("Path does not exist: {}", path.display())
    } else if !path.is_dir() {
        format!("Path is not a directory: {}", path.display())
    } else {
        let mut files = Vec::new();
        match std::fs::read_dir(&path) {
            Ok(entries) => {
                for entry in entries.flatten() {
                    if let Some(name) = entry.file_name().to_str() {
                        let file_type = if entry.path().is_dir() {
                            "📁"
                        } else {
                            "📄"
                        };
                        files.push(format!("{} {}", file_type, name));
                    }
                }
                format!("Files in {}:\n{}", path.display(), files.join("\n"))
            }
            Err(e) => format!("Failed to read directory: {}", e),
        }
    };

    Ok(CallToolResult {
        content: vec![Content::Text(TextContent {
            text: content,
            annotations: None,
            meta: None,
        })],
        is_error: None,
    })
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing to stderr
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .init();

    eprintln!("🌳 Filesystem Server with Roots Configuration");
    eprintln!("============================================\n");

    // Get current directory and home directory for example roots
    let current_dir = std::env::current_dir()?.to_string_lossy().to_string();
    let home_dir = std::env::var("HOME").unwrap_or_else(|_| "/home/user".to_string());

    // Configure specific roots for the server
    let roots = vec![
        Root {
            uri: format!("file://{}", current_dir),
            name: Some("Project Root".to_string()),
        },
        Root {
            uri: format!("file://{}/Documents", home_dir),
            name: Some("Documents".to_string()),
        },
        Root {
            uri: format!("file://{}/Downloads", home_dir),
            name: Some("Downloads".to_string()),
        },
        Root {
            uri: "file:///tmp".to_string(),
            name: Some("Temporary Files".to_string()),
        },
    ];

    eprintln!("📁 Configured Roots:");
    for root in &roots {
        eprintln!(
            "  • {} - {}",
            root.name.as_deref().unwrap_or("Unnamed"),
            root.uri
        );
    }
    eprintln!();

    // Create list_files tool with schema
    let list_files_tool = Tool {
        name: "list_files".to_string(),
        title: Some("List Files".to_string()),
        description: Some("List files in a directory".to_string()),
        input_schema: ToolInputSchema {
            schema_type: "object".to_string(),
            properties: Some({
                let mut props = HashMap::new();
                props.insert(
                    "path".to_string(),
                    serde_json::json!({
                        "type": "string",
                        "description": "The directory path to list"
                    }),
                );
                props
            }),
            required: None,
            additional_properties: Some(false),
        },
        output_schema: None,
        annotations: None,
        meta: None,
    };

    // Create handler
    let list_files_handler = FunctionToolHandler::new(list_files_tool, list_files);

    // Build server with roots configuration
    let server = ServerBuilder::new()
        .name("filesystem-server")
        .version("1.0.0")
        .description("Server with configurable filesystem roots")
        .roots(roots) // Configure roots here
        .tool("list_files", list_files_handler)?
        .build();

    eprintln!("🔧 Available Tools:");
    eprintln!("  • list_files - List files in a directory");
    eprintln!();

    eprintln!("📋 Test Commands:");
    eprintln!("  List roots:");
    eprintln!("    echo '{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"roots/list\"}}' | \\");
    eprintln!("    cargo run --example feature_roots_builder 2>/dev/null | jq");
    eprintln!();
    eprintln!("  List files in current directory:");
    eprintln!(
        "    echo '{{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{{\"name\":\"list_files\",\"arguments\":{{\"path\":\".\"}}}}}}' | \\"
    );
    eprintln!("    cargo run --example feature_roots_builder 2>/dev/null | jq");
    eprintln!();

    eprintln!("✅ Server starting with stdio transport...\n");

    // Run server with stdio transport
    server.run_stdio().await?;

    Ok(())
}
