//! # `TurboMCP` CLI - Comprehensive Edition
//!
//! Complete MCP (Model Context Protocol) command-line interface with comprehensive features.
//!
//! ## Features
//!
//! - **Complete MCP Coverage**: All protocol operations (tools, resources, prompts, completions, sampling, etc.)
//! - **Multiple Transports**: STDIO, HTTP SSE, WebSocket, TCP, Unix sockets with auto-detection
//! - **Rich Output**: Human-readable, JSON, YAML, and table formats with colored output
//! - **Robust Error Handling**: Detailed errors with actionable suggestions

#![allow(clippy::result_large_err)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::option_as_ref_deref)]
#![allow(clippy::needless_return)]
//! - **Production Ready**: Built on turbomcp-client and turbomcp-transport
//! - **Enterprise Features**: Connection presets, configuration files, verbose logging
//!
//! ## Quick Start
//!
//! ```bash
//! # List tools from a server
//! turbomcp-cli tools list --url http://localhost:8080/mcp
//!
//! # Call a tool with arguments
//! turbomcp-cli tools call calculate --arguments '{"a": 5, "b": 3}'
//!
//! # Get server info in table format
//! turbomcp-cli server info --format table
//!
//! # List resources from STDIO server
//! turbomcp-cli resources list --command "./my-server"
//! ```
//!
//! ## Architecture
//!
//! The CLI uses a layered architecture:
//! - **Command Layer** (`cli_new`): Clap-based argument parsing
//! - **Execution Layer** (`executor`): Command execution using turbomcp-client
//! - **Transport Layer** (`transport`): Auto-detection and factory pattern
//! - **Output Layer** (`formatter`): Rich, multi-format output
//!
//! All MCP operations are delegated to `turbomcp-client` for reliability.

// Core modules
pub mod cli;
pub mod error;
pub mod executor;
pub mod formatter;
pub mod prelude;
pub mod transport;

// Legacy modules removed in 2.0 - all functionality is now in the new architecture
// If you were using the legacy API (tools-list, tools-call, schema-export):
// - Use the new hierarchical commands: tools list, tools call, tools schema
// - Use `#[tokio::main]` instead of `run_cli()`
// - Import from `cli` module instead of `cli_new`

use clap::Parser;

// Clean re-exports (no more "New" suffixes!)
pub use cli::{
    Cli, Commands, CompletionCommands, Connection, LogLevel, OutputFormat, PromptCommands, RefType,
    ResourceCommands, SamplingCommands, ServerCommands, ToolCommands, TransportKind,
};
pub use error::{CliError, CliResult, ErrorCategory};
pub use executor::CommandExecutor;
pub use formatter::Formatter;

/// Run the CLI application
///
/// This is the main entry point for the TurboMCP CLI library. It provides complete
/// MCP protocol coverage with rich output formatting and comprehensive error handling.
///
/// Returns a `CliResult` that the caller can handle appropriately. This allows
/// the caller to control error formatting, exit codes, and runtime configuration.
///
/// # Example
///
/// ```rust,no_run
/// use turbomcp_cli::prelude::*;
///
/// #[tokio::main]
/// async fn main() {
///     if let Err(e) = turbomcp_cli::run().await {
///         eprintln!("Error: {}", e);
///         std::process::exit(1);
///     }
/// }
/// ```
pub async fn run() -> CliResult<()> {
    let cli = Cli::parse();
    let executor = CommandExecutor::new(cli.format.clone(), !cli.no_color, cli.verbose);
    executor.execute(cli.command).await
}
