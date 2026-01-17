//! TCP transport implementation for v3.
//!
//! Provides line-based JSON-RPC over TCP sockets with connection limiting
//! and graceful shutdown support.

use std::sync::Arc;
use tokio::io::BufReader;
use tokio::net::TcpListener;
use tokio::sync::watch;
use turbomcp_core::error::{McpError, McpResult};
use turbomcp_core::handler::McpHandler;

use super::line::LineTransportRunner;
use crate::v3::config::{ConnectionCounter, ServerConfig};
use crate::v3::context::RequestContext;
use crate::v3::router;

/// Run a handler on TCP transport.
///
/// # Arguments
///
/// * `handler` - The MCP handler
/// * `addr` - Address to bind to (e.g., "0.0.0.0:9000")
///
/// # Example
///
/// ```rust,ignore
/// use turbomcp_server::v3::transport::tcp;
///
/// tcp::run(&handler, "0.0.0.0:9000").await?;
/// ```
pub async fn run<H: McpHandler>(handler: &H, addr: &str) -> McpResult<()> {
    run_with_config(handler, addr, &ServerConfig::default()).await
}

/// Run a handler on TCP transport with custom configuration.
///
/// # Arguments
///
/// * `handler` - The MCP handler
/// * `addr` - Address to bind to
/// * `config` - Server configuration (connection limits, etc.)
pub async fn run_with_config<H: McpHandler>(
    handler: &H,
    addr: &str,
    config: &ServerConfig,
) -> McpResult<()> {
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    // Set up signal handling for graceful shutdown
    let signal_task = tokio::spawn(async move {
        if let Ok(()) = tokio::signal::ctrl_c().await {
            tracing::info!("Received shutdown signal, stopping TCP server...");
            let _ = shutdown_tx.send(true);
        }
    });

    let result = run_with_shutdown(handler, addr, config, shutdown_rx).await;

    signal_task.abort();
    result
}

/// Run a handler on TCP transport with explicit shutdown signal.
///
/// # Arguments
///
/// * `handler` - The MCP handler
/// * `addr` - Address to bind to
/// * `config` - Server configuration (connection limits, etc.)
/// * `shutdown` - Watch receiver that triggers shutdown when `true` is received
///
/// # Example
///
/// ```rust,ignore
/// use tokio::sync::watch;
///
/// let (shutdown_tx, shutdown_rx) = watch::channel(false);
///
/// // Run server in background
/// let handle = tokio::spawn(async move {
///     tcp::run_with_shutdown(&handler, "0.0.0.0:9000", &config, shutdown_rx).await
/// });
///
/// // Later: trigger shutdown
/// shutdown_tx.send(true)?;
/// handle.await??;
/// ```
pub async fn run_with_shutdown<H: McpHandler>(
    handler: &H,
    addr: &str,
    config: &ServerConfig,
    mut shutdown: watch::Receiver<bool>,
) -> McpResult<()> {
    // Call lifecycle hooks
    handler.on_initialize().await?;

    let max_connections = config.connection_limits.max_tcp_connections;
    let connection_counter = Arc::new(ConnectionCounter::new(max_connections));

    // Create TCP listener
    let listener = TcpListener::bind(addr)
        .await
        .map_err(|e| McpError::internal(format!("Failed to bind to {}: {}", addr, e)))?;

    tracing::info!(
        "v3 MCP server listening on tcp://{} (max {} connections)",
        addr,
        max_connections
    );

    loop {
        tokio::select! {
            // Check for shutdown signal
            _ = shutdown.changed() => {
                if *shutdown.borrow() {
                    tracing::info!("TCP server shutting down...");
                    break;
                }
            }

            // Accept new connections
            accept_result = listener.accept() => {
                let (stream, peer_addr) = accept_result
                    .map_err(|e| McpError::internal(format!("Accept error: {}", e)))?;

                // Try to acquire a connection slot
                let guard = match connection_counter.try_acquire_arc() {
                    Some(guard) => guard,
                    None => {
                        tracing::warn!(
                            "Connection from {} rejected: at capacity ({}/{})",
                            peer_addr,
                            connection_counter.current(),
                            connection_counter.max()
                        );
                        // Send error and close
                        reject_connection(stream).await;
                        continue;
                    }
                };

                tracing::debug!(
                    "New TCP connection from {} ({}/{})",
                    peer_addr,
                    connection_counter.current(),
                    connection_counter.max()
                );

                // Spawn handler task
                let handler = handler.clone();
                tokio::spawn(async move {
                    // Guard dropped when task completes, releasing connection slot
                    let _guard = guard;

                    let (reader, writer) = stream.into_split();
                    let reader = BufReader::new(reader);

                    let runner = LineTransportRunner::new(handler);
                    if let Err(e) = runner.run(reader, writer, RequestContext::tcp).await {
                        tracing::error!("TCP connection error from {}: {}", peer_addr, e);
                    }

                    tracing::debug!("TCP connection from {} closed", peer_addr);
                });
            }
        }
    }

    // Call shutdown hook
    handler.on_shutdown().await?;
    Ok(())
}

/// Reject a connection with a capacity error.
async fn reject_connection(stream: tokio::net::TcpStream) {
    use tokio::io::AsyncWriteExt;

    let mut stream = stream;
    let error_response =
        router::JsonRpcOutgoing::error(None, McpError::internal("Server at maximum capacity"));

    if let Ok(response_str) = router::serialize_response(&error_response) {
        let _ = stream.write_all(response_str.as_bytes()).await;
        let _ = stream.write_all(b"\n").await;
        let _ = stream.flush().await;
    }
}

#[cfg(test)]
mod tests {
    // TCP tests are in /tests/ as they require network access
}
