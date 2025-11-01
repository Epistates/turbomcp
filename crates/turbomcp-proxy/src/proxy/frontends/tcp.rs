//! TCP frontend transport implementation
//!
//! Provides bidirectional TCP server for MCP protocol communication.
//! Ideal for high-performance network-based proxying.

use std::sync::Arc;
use std::time::Duration;

use serde_json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tracing::{debug, error, info};

use crate::error::{ProxyError, ProxyResult};
use crate::proxy::{BackendConnector, IdTranslator};

/// TCP frontend configuration
#[derive(Debug, Clone)]
pub struct TcpFrontendConfig {
    /// Bind address (e.g., "127.0.0.1:5000")
    pub bind: String,
    /// Request timeout
    pub timeout: Duration,
    /// Maximum request size
    pub max_request_size: usize,
}

impl TcpFrontendConfig {
    /// Create a new TCP frontend configuration
    pub fn new(bind: impl Into<String>, timeout: Duration, max_request_size: usize) -> Self {
        Self {
            bind: bind.into(),
            timeout,
            max_request_size,
        }
    }
}

/// TCP frontend for MCP protocol
pub struct TcpFrontend {
    config: TcpFrontendConfig,
    backend: BackendConnector,
    id_translator: Arc<IdTranslator>,
}

impl TcpFrontend {
    /// Create a new TCP frontend
    pub fn new(
        config: TcpFrontendConfig,
        backend: BackendConnector,
        id_translator: Arc<IdTranslator>,
    ) -> Self {
        Self {
            config,
            backend,
            id_translator,
        }
    }

    /// Run the TCP frontend server
    ///
    /// # Errors
    ///
    /// Returns error if binding fails or server encounters fatal error
    pub async fn run(&self) -> ProxyResult<()> {
        let listener = TcpListener::bind(&self.config.bind).await.map_err(|e| {
            ProxyError::backend_connection(format!(
                "Failed to bind TCP listener to {}: {}",
                self.config.bind, e
            ))
        })?;

        let addr = listener.local_addr().map_err(|e| {
            ProxyError::backend_connection(format!("Failed to get listener address: {}", e))
        })?;

        info!("TCP frontend listening on {}", addr);

        loop {
            let (socket, peer_addr) = listener
                .accept()
                .await
                .map_err(|e| ProxyError::backend_connection(format!("TCP accept error: {}", e)))?;

            debug!("Accepted TCP connection from {}", peer_addr);

            let backend = self.backend.clone();
            let id_translator = Arc::clone(&self.id_translator);
            let timeout = self.config.timeout;
            let max_request_size = self.config.max_request_size;

            tokio::spawn(async move {
                if let Err(e) =
                    handle_connection(socket, backend, id_translator, timeout, max_request_size)
                        .await
                {
                    error!("TCP connection error from {}: {}", peer_addr, e);
                }
            });
        }
    }
}

/// Handle individual TCP client connection
async fn handle_connection(
    mut socket: TcpStream,
    _backend: BackendConnector,
    _id_translator: Arc<IdTranslator>,
    timeout: Duration,
    max_request_size: usize,
) -> ProxyResult<()> {
    let mut buf = vec![0; max_request_size];
    let mut read_pos = 0;

    loop {
        // Read with timeout
        let _n = match tokio::time::timeout(timeout, socket.read(&mut buf[read_pos..])).await {
            Ok(Ok(0)) => {
                debug!("TCP client closed connection");
                break;
            }
            Ok(Ok(n)) => {
                read_pos += n;
                n
            }
            Ok(Err(e)) => {
                error!("TCP read error: {}", e);
                break;
            }
            Err(_) => {
                error!("TCP read timeout");
                break;
            }
        };

        // Try to parse JSON-RPC message (line-delimited)
        if let Some(line_end) = buf[..read_pos].windows(1).position(|w| w == b"\n") {
            let line = &buf[..line_end];

            // Attempt to parse as JSON-RPC
            match serde_json::from_slice::<serde_json::Value>(line) {
                Ok(message) => {
                    debug!("Received JSON-RPC message: {}", message);

                    // TODO: Route message through backend using id_translator
                    // This is a simplified implementation - full routing logic
                    // would handle request/response correlation like in HTTP/WebSocket

                    // For now, send back an error response
                    let response = serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": message.get("id"),
                        "error": {
                            "code": -32603,
                            "message": "TCP frontend not yet fully implemented"
                        }
                    });

                    let response_bytes = serde_json::to_vec(&response)?;

                    socket.write_all(&response_bytes).await.map_err(|e| {
                        ProxyError::backend_connection(format!("TCP write error: {}", e))
                    })?;
                    socket.write_all(b"\n").await.map_err(|e| {
                        ProxyError::backend_connection(format!("TCP write error: {}", e))
                    })?;

                    // Reset buffer
                    read_pos = 0;
                }
                Err(e) => {
                    debug!("Failed to parse JSON-RPC: {}", e);
                    read_pos = 0; // Reset on parse error
                }
            }
        } else if read_pos >= max_request_size {
            error!("TCP message exceeds maximum size");
            break;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tcp_frontend_config() {
        let config =
            TcpFrontendConfig::new("127.0.0.1:5000", Duration::from_secs(30), 10 * 1024 * 1024);
        assert_eq!(config.bind, "127.0.0.1:5000");
        assert_eq!(config.timeout, Duration::from_secs(30));
        assert_eq!(config.max_request_size, 10 * 1024 * 1024);
    }
}
