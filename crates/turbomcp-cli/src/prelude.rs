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
    // Error handling
    CliError,
    CliResult,
    // Execution
    CommandExecutor,
    Commands,
    CompletionCommands,
    Connection,
    ErrorCategory,

    Formatter,

    LogLevel,

    OutputFormat,
    PromptCommands,
    // Supporting types
    RefType,
    ResourceCommands,
    SamplingCommands,

    ServerCommands,
    // Subcommands
    ToolCommands,
    TransportKind,

    // Entry point
    run,
};

// Re-export commonly used client types for convenience
pub use turbomcp_client::{Client, ClientBuilder};

// Re-export clap for custom CLI extensions
pub use clap::Parser;
