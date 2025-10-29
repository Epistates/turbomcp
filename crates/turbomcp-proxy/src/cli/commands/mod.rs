//! CLI command implementations
//!
//! Each command is implemented as a struct that can execute independently.

pub mod generate;
pub mod inspect;
pub mod serve;

use clap::Subcommand;

use crate::cli::output::OutputFormat;
use crate::error::ProxyResult;

/// All available CLI commands
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Inspect an MCP server to discover its capabilities
    #[command(visible_alias = "i")]
    Inspect(inspect::InspectCommand),

    /// Serve a proxy server to bridge MCP transports
    #[command(visible_alias = "s")]
    Serve(serve::ServeCommand),

    /// Generate optimized Rust proxy code
    #[command(visible_alias = "g")]
    Generate(generate::GenerateCommand),
    // Future commands (Phase 4+)
    // Schema(schema::SchemaCommand),
    // Adapter(adapter::AdapterCommand),
}

impl Command {
    /// Execute the command with the specified output format
    pub async fn execute(self, format: OutputFormat) -> ProxyResult<()> {
        match self {
            Command::Inspect(cmd) => cmd.execute(format).await,
            Command::Serve(cmd) => {
                // Serve command doesn't use output format
                cmd.execute().await
            }
            Command::Generate(cmd) => {
                // Generate command doesn't use output format
                cmd.execute().await
            }
        }
    }
}
