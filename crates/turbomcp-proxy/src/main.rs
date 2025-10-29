//! turbomcp-proxy CLI entry point
//!
//! A world-class command-line interface for MCP server introspection,
//! proxying, and code generation.

#![warn(clippy::all)]

#[cfg(not(feature = "cli"))]
fn main() {
    eprintln!("Error: CLI feature not enabled. Build with --features cli");
    std::process::exit(1);
}

#[cfg(feature = "cli")]
#[tokio::main]
async fn main() {
    use clap::Parser;

    // Parse command-line arguments
    let cli = turbomcp_proxy::cli::Cli::parse();

    // Execute the command and handle errors
    if let Err(e) = cli.execute().await {
        let exit_code = turbomcp_proxy::cli::error::display_error(&e);
        std::process::exit(exit_code);
    }
}
