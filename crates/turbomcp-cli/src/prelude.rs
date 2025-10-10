//! Prelude module for convenient imports
//!
//! This module re-exports the most commonly used types for building
//! applications with the TurboMCP CLI library.
//!
//! # Example
//!
//! ```rust,no_run
//! use turbomcp_cli::prelude::*;
//!
//! #[tokio::main]
//! async fn main() -> CliResult<()> {
//!     let cli = Cli::parse();
//!     let executor = CommandExecutor::new(
//!         OutputFormat::Json,
//!         true,
//!         false
//!     );
//!     executor.execute(cli.command).await
//! }
//! ```

pub use crate::{
    // Core CLI types
    Cli,
    Commands,
    Connection,
    OutputFormat,
    TransportKind,

    // Subcommands
    ToolCommands,
    ResourceCommands,
    PromptCommands,
    CompletionCommands,
    ServerCommands,
    SamplingCommands,

    // Supporting types
    RefType,
    LogLevel,

    // Error handling
    CliError,
    CliResult,
    ErrorCategory,

    // Execution
    CommandExecutor,
    Formatter,

    // Entry point
    run,
};

// Re-export commonly used client types for convenience
pub use turbomcp_client::{Client, ClientBuilder};

// Re-export clap for custom CLI extensions
pub use clap::Parser;
