//! STDIO transport implementation.
//!
//! Provides line-based JSON-RPC over stdin/stdout.

use tokio::io::BufReader;
use turbomcp_core::error::McpResult;
use turbomcp_core::handler::McpHandler;

use super::line::LineTransportRunner;
use crate::config::ServerConfig;
use crate::context::RequestContext;

/// Run a handler on STDIO transport.
///
/// This is the default transport for MCP servers, reading JSON-RPC
/// requests from stdin and writing responses to stdout.
///
/// Uses default protocol configuration (strict latest-version-only).
/// For multi-version support, use [`run_with_config`].
///
/// # Example
///
/// ```rust,ignore
/// use turbomcp_server::transport::stdio;
///
/// stdio::run(&handler).await?;
/// ```
pub async fn run<H: McpHandler>(handler: &H) -> McpResult<()> {
    // Call lifecycle hooks
    handler.on_initialize().await?;

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let reader = BufReader::new(stdin);

    let runner = LineTransportRunner::new(handler.clone());
    let result = runner.run(reader, stdout, RequestContext::stdio).await;

    // Call shutdown hook regardless of result
    handler.on_shutdown().await?;

    result
}

/// Run a handler on STDIO transport with custom server configuration.
///
/// Use this to enable multi-version protocol support, allowing older
/// MCP clients (e.g. 2025-06-18) to connect alongside the latest spec.
///
/// # Example
///
/// ```rust,ignore
/// use turbomcp_server::transport::stdio;
/// use turbomcp_server::{ServerConfig, ProtocolConfig};
///
/// let config = ServerConfig::builder()
///     .protocol(ProtocolConfig::multi_version())
///     .build();
///
/// stdio::run_with_config(&handler, &config).await?;
/// ```
pub async fn run_with_config<H: McpHandler>(handler: &H, config: &ServerConfig) -> McpResult<()> {
    // Call lifecycle hooks
    handler.on_initialize().await?;

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let reader = BufReader::new(stdin);

    let runner = LineTransportRunner::with_config(handler.clone(), config.clone());
    let result = runner.run(reader, stdout, RequestContext::stdio).await;

    // Call shutdown hook regardless of result
    handler.on_shutdown().await?;

    result
}

#[cfg(test)]
mod tests {
    // STDIO tests require actual stdin/stdout, so they're integration tests
    // See /tests/integration_test.rs for STDIO tests
}
