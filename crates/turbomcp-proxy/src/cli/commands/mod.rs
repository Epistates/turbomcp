//! CLI command implementations
//!
//! Each command is implemented as a struct that can execute independently.

pub mod adapter;
pub mod generate;
pub mod inspect;
pub mod schema;
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

    /// Export server capabilities as schemas (`OpenAPI`, GraphQL, Protobuf)
    #[command(visible_alias = "sch")]
    Schema(schema::SchemaCommand),

    /// Run protocol adapters (REST API, GraphQL)
    #[command(visible_alias = "adp")]
    Adapter(adapter::AdapterCommand),
}

impl Command {
    /// Execute the command with the specified output format
    ///
    /// # Errors
    ///
    /// Returns `ProxyError` if the command execution fails.
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
            Command::Schema(cmd) => {
                // Schema command uses output format for format selection
                cmd.execute(format).await
            }
            Command::Adapter(cmd) => {
                // Adapter command doesn't use output format
                cmd.execute(format).await
            }
        }
    }
}
