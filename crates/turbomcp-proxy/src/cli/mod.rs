//! CLI Interface for turbomcp-proxy
//!
//! This module provides a world-class command-line interface following
//! Rust ecosystem best practices:
//!
//! - Type-safe argument parsing with clap v4
//! - Multiple output formats (human, JSON, YAML)
//! - Colored output with TTY detection
//! - Proper exit codes
//! - User-friendly error messages
//! - Progress indicators for long operations
//!
//! ## Architecture
//!
//! ```text
//! cli/
//! ├── args.rs       # Shared argument types
//! ├── commands/     # Command implementations
//! ├── output/       # Output formatters
//! └── error.rs      # User-friendly error display
//! ```

// Declare modules (order matters for dependency resolution)
pub mod args;
pub mod commands;
pub mod error;
pub mod output;

use std::io::IsTerminal;

use clap::Parser;
use tracing::Level;

use crate::error::ProxyResult;

/// turbomcp-proxy - Universal MCP adapter and introspection tool
///
/// Inspect, proxy, and generate adapters for any MCP server.
#[derive(Parser, Debug)]
#[command(
    name = "turbomcp-proxy",
    version,
    about = "Universal MCP adapter - introspection, proxying, and code generation",
    long_about = "A world-class tool for working with Model Context Protocol (MCP) servers.\n\
                  Inspect capabilities, proxy connections, and generate typed adapters.",
    author
)]
pub struct Cli {
    /// Subcommand to execute
    #[command(subcommand)]
    pub command: commands::Command,

    /// Enable verbose logging (-v, -vv, -vvv for trace)
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    pub verbose: u8,

    /// Suppress all output except errors
    #[arg(short, long, global = true, conflicts_with = "verbose")]
    pub quiet: bool,

    /// Output format
    #[arg(short = 'f', long, value_enum, default_value = "human", global = true)]
    pub format: output::OutputFormat,

    /// Disable colored output
    #[arg(long, global = true)]
    pub no_color: bool,
}

impl Cli {
    /// Execute the CLI command
    ///
    /// # Errors
    ///
    /// Returns `ProxyError` if command execution fails.
    pub async fn execute(self) -> ProxyResult<()> {
        // Initialize tracing based on verbosity
        self.init_tracing();

        // Configure colored output
        if self.no_color || !std::io::stdout().is_terminal() {
            colored::control::set_override(false);
        }

        // Execute the command with the specified output format
        self.command.execute(self.format).await
    }

    /// Initialize tracing subscriber based on verbosity level
    fn init_tracing(&self) {
        let level = if self.quiet {
            Level::ERROR
        } else {
            match self.verbose {
                0 => Level::WARN,
                1 => Level::INFO,
                2 => Level::DEBUG,
                _ => Level::TRACE,
            }
        };

        tracing_subscriber::fmt()
            .with_max_level(level)
            .with_target(false)
            .with_thread_ids(false)
            .with_thread_names(false)
            .init();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_parsing() {
        // Should parse basic command
        let cli = Cli::try_parse_from([
            "turbomcp-proxy",
            "inspect",
            "--backend",
            "stdio",
            "--cmd",
            "python",
        ]);
        assert!(cli.is_ok());
    }

    #[test]
    fn test_verbosity_levels() {
        let cli = Cli::try_parse_from([
            "turbomcp-proxy",
            "-vvv",
            "inspect",
            "--backend",
            "stdio",
            "--cmd",
            "test",
        ])
        .unwrap();
        assert_eq!(cli.verbose, 3);
    }

    #[test]
    fn test_quiet_conflicts_with_verbose() {
        let cli = Cli::try_parse_from([
            "turbomcp-proxy",
            "-v",
            "--quiet",
            "inspect",
            "--backend",
            "stdio",
            "--cmd",
            "test",
        ]);
        assert!(cli.is_err());
    }
}
