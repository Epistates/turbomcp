//! # `TurboMCP` CLI
//!
//! Command-line interface for interacting with MCP servers, providing tools for
//! testing, debugging, and managing MCP server instances.
//!
//! ## Features
//!
//! - Connect to MCP servers via multiple transports (HTTP, WebSocket, STDIO)
//! - List available tools and their schemas
//! - Call tools with JSON arguments
//! - Export tool schemas for documentation
//! - Support for authentication via bearer tokens
//! - JSON and human-readable output formats
//!
//! ## Usage
//!
//! ```bash
//! # List tools from HTTP server
//! turbomcp-cli tools-list --transport http --url http://localhost:8080/mcp
//!
//! # Call a tool with arguments
//! turbomcp-cli tools-call --transport http --url http://localhost:8080/mcp \
//!   add --arguments '{"a": 5, "b": 3}'
//!
//! # Export tool schemas
//! turbomcp-cli schema-export --transport http --url http://localhost:8080/mcp --json
//! ```

pub mod cli;
pub mod commands;
pub mod output;
pub mod transports;

use clap::Parser;
use tokio::runtime::Runtime;

// Re-export for backward compatibility with existing tests and external consumers
pub use cli::{Cli, Commands, Connection, TransportKind};
pub use commands::{
    schema_export as cmd_schema_export, tools_call as cmd_tools_call, tools_list as cmd_tools_list,
};
pub use output::display as output;

/// Run the CLI application
pub fn run_cli() {
    let cli = cli::Cli::parse();
    let rt = match Runtime::new() {
        Ok(runtime) => runtime,
        Err(e) => {
            eprintln!("Failed to initialize async runtime: {e}");
            std::process::exit(1);
        }
    };
    rt.block_on(async move {
        match cli.command {
            cli::Commands::ToolsList(conn) => {
                if let Err(e) = commands::tools_list(conn).await {
                    eprintln!("error: {e}");
                    std::process::exit(1);
                }
            }
            cli::Commands::ToolsCall {
                conn,
                name,
                arguments,
            } => {
                if let Err(e) = commands::tools_call(conn, name, arguments).await {
                    eprintln!("error: {e}");
                    std::process::exit(1);
                }
            }
            cli::Commands::SchemaExport { conn, output } => {
                if let Err(e) = commands::schema_export(conn, output).await {
                    eprintln!("error: {e}");
                    std::process::exit(1);
                }
            }
        }
    });
}
