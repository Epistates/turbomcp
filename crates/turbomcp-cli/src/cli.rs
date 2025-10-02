//! CLI argument parsing and configuration types

use clap::{Args, Parser, Subcommand, ValueEnum};

/// Main CLI application structure
#[derive(Parser, Debug)]
#[command(
    name = "turbomcp-cli",
    version,
    about = "Command-line interface for interacting with MCP servers - list tools, call tools, and export schemas."
)]
pub struct Cli {
    /// Subcommand to run
    #[command(subcommand)]
    pub command: Commands,
}

/// Available CLI subcommands
#[derive(Subcommand, Debug)]
pub enum Commands {
    /// List tools from a running server
    #[command(name = "tools-list")]
    ToolsList(Connection),
    /// Call a tool on a running server
    #[command(name = "tools-call")]
    ToolsCall {
        #[command(flatten)]
        conn: Connection,
        /// Tool name
        #[arg(long)]
        name: String,
        /// Arguments as JSON (object)
        #[arg(long, default_value = "{}")]
        arguments: String,
    },
    /// Export tool schemas from a running server
    #[command(name = "schema-export")]
    SchemaExport {
        #[command(flatten)]
        conn: Connection,
        /// Output file path (if not specified, outputs to stdout)
        #[arg(long)]
        output: Option<String>,
    },
}

/// Connection configuration for connecting to MCP servers
#[derive(Args, Debug, Clone)]
pub struct Connection {
    /// Transport protocol (stdio, http, ws) - auto-detected if not specified
    #[arg(long, value_enum)]
    pub transport: Option<TransportKind>,
    /// Server URL for http/ws or command for stdio
    #[arg(long, default_value = "http://localhost:8080/mcp")]
    pub url: String,
    /// Command to execute for stdio transport (overrides --url if provided)
    #[arg(long)]
    pub command: Option<String>,
    /// Bearer token or API key
    #[arg(long)]
    pub auth: Option<String>,
    /// Emit JSON output
    #[arg(long)]
    pub json: bool,
}

/// Available transport types for connecting to MCP servers
#[derive(Debug, Clone, ValueEnum, PartialEq)]
pub enum TransportKind {
    /// Standard input/output transport
    Stdio,
    /// HTTP transport with JSON-RPC
    Http,
    /// WebSocket transport
    Ws,
}

/// Determine transport based on explicit setting or auto-detection
///
/// # Auto-detection Rules
/// - Explicit `--transport` flag takes precedence
/// - `--command` option implies STDIO
/// - URLs starting with `ws://` or `wss://` imply WebSocket
/// - URLs starting with `http://` or `https://` imply HTTP
/// - Non-URL strings imply STDIO (command execution)
pub fn determine_transport(conn: &Connection) -> TransportKind {
    // Use explicit transport if provided
    if let Some(transport) = &conn.transport {
        return transport.clone();
    }

    // Auto-detect based on command/URL patterns
    if conn.command.is_some()
        || (!conn.url.starts_with("http://")
            && !conn.url.starts_with("https://")
            && !conn.url.starts_with("ws://")
            && !conn.url.starts_with("wss://"))
    {
        TransportKind::Stdio
    } else if conn.url.starts_with("ws://") || conn.url.starts_with("wss://") {
        TransportKind::Ws
    } else {
        TransportKind::Http
    }
}
