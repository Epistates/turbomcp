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

// New architecture modules
pub mod cli_new;
pub mod error;
pub mod executor;
pub mod formatter;
pub mod transport;

// Legacy modules (for backward compatibility)
pub mod cli;
pub mod commands;
pub mod output;
pub mod transports;

use clap::Parser;
use tokio::runtime::Runtime;

// Re-export new architecture types (primary API)
pub use cli_new::{
    Cli as CliNew, Commands as CommandsNew, Connection as ConnectionNew, OutputFormat,
    TransportKind as TransportKindNew,
};
pub use error::{CliError, CliResult, ErrorCategory};
pub use executor::CommandExecutor;
pub use formatter::Formatter;

// Re-export legacy types for backward compatibility
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

/// Run the CLI application with new architecture
///
/// This is the new, comprehensive implementation that leverages turbomcp-client
/// and provides complete MCP protocol coverage with rich output formatting.
///
/// # Example
///
/// ```rust,no_run
/// use turbomcp_cli::run_cli_new;
///
/// #[tokio::main]
/// async fn main() {
///     if let Err(e) = run_cli_new().await {
///         eprintln!("Error: {e}");
///         std::process::exit(1);
///     }
/// }
/// ```
pub async fn run_cli_new() -> CliResult<()> {
    let cli = cli_new::Cli::parse();

    let executor = CommandExecutor::new(cli.format.clone(), !cli.no_color, cli.verbose);

    if let Err(e) = executor.execute(cli.command).await {
        executor.display_error(&e);
        std::process::exit(1);
    }

    Ok(())
}

/// Run the new CLI in blocking mode (for use in non-async main)
///
/// This is a convenience wrapper around `run_cli_new` that creates
/// a tokio runtime and blocks on the async function.
pub fn run_cli_new_blocking() {
    let rt = match Runtime::new() {
        Ok(runtime) => runtime,
        Err(e) => {
            eprintln!("Failed to initialize async runtime: {e}");
            std::process::exit(1);
        }
    };

    rt.block_on(async move {
        if let Err(e) = run_cli_new().await {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    });
}
