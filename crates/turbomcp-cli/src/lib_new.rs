//! # `TurboMCP` CLI - Comprehensive Edition
//!
//! Complete MCP (Model Context Protocol) command-line interface with comprehensive features:
//!
//! ## Features
//!
//! - **Complete MCP Coverage**: All protocol operations (tools, resources, prompts, completions, sampling, etc.)
//! - **Multiple Transports**: STDIO, HTTP SSE, WebSocket, TCP, Unix sockets with auto-detection
//! - **Rich Output**: Human-readable, JSON, YAML, and table formats with colored output
//! - **Robust Error Handling**: Detailed errors with actionable suggestions
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
//! The CLI is built on three key components:
//!
//! 1. **Command Layer** (`cli_new`): Clap-based argument parsing
//! 2. **Execution Layer** (`executor`): Command execution using turbomcp-client
//! 3. **Transport Layer** (`transport`): Auto-detection and factory pattern
//! 4. **Output Layer** (`formatter`): Rich, multi-format output
//!
//! All MCP operations are delegated to `turbomcp-client` for reliability.

pub mod cli_new;
pub mod commands;
pub mod error;
pub mod executor;
pub mod formatter;
pub mod output;
pub mod transport;

// Legacy modules for backward compatibility
pub mod cli;
pub mod transports;

use clap::Parser;
use error::CliResult;
use executor::CommandExecutor;

/// Run the CLI application with new architecture
pub async fn run_cli_new() -> CliResult<()> {
    let cli = cli_new::Cli::parse();

    let executor = CommandExecutor::new(
        cli.format.clone(),
        !cli.no_color,
        cli.verbose,
    );

    if let Err(e) = executor.execute(cli.command).await {
        executor.display_error(&e);
        std::process::exit(1);
    }

    Ok(())
}

/// Legacy run_cli for backward compatibility
pub fn run_cli() {
    use tokio::runtime::Runtime;

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

// Re-export key types for library consumers
pub use cli_new::{Cli, Commands, Connection, OutputFormat, TransportKind};
pub use error::{CliError, CliResult, ErrorCategory};
pub use executor::CommandExecutor;
pub use formatter::Formatter;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_cli_parsing() {
        use clap::CommandFactory;
        cli_new::Cli::command().debug_assert();
    }
}
