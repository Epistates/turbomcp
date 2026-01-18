//! Unix domain socket frontend transport implementation
//!
//! Provides bidirectional Unix socket server for MCP protocol communication.
//! Ideal for same-host IPC (inter-process communication) with filesystem-based access control.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use serde_json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tracing::{debug, error, info};

use crate::error::{ProxyError, ProxyResult};
use crate::proxy::{BackendConnector, IdTranslator};

/// Unix socket frontend configuration
#[derive(Debug, Clone)]
pub struct UnixFrontendConfig {
    /// Socket file path
    pub path: String,
    /// Request timeout
    pub timeout: Duration,
    /// Maximum request size
    pub max_request_size: usize,
}

impl UnixFrontendConfig {
    /// Create a new Unix socket frontend configuration
    #[must_use]
    pub fn new(path: impl Into<String>, timeout: Duration, max_request_size: usize) -> Self {
        Self {
            path: path.into(),
            timeout,
            max_request_size,
        }
    }
}

/// Unix socket frontend for MCP protocol
pub struct UnixFrontend {
    config: UnixFrontendConfig,
    backend: BackendConnector,
    id_translator: Arc<IdTranslator>,
}

impl UnixFrontend {
    /// Create a new Unix socket frontend
    #[must_use]
    pub fn new(
        config: UnixFrontendConfig,
        backend: BackendConnector,
        id_translator: Arc<IdTranslator>,
    ) -> Self {
        Self {
            config,
            backend,
            id_translator,
        }
    }

    /// Run the Unix socket frontend server
    ///
    /// # Security Note
    ///
    /// Unix socket permissions are controlled by the filesystem.
    /// The socket file's permissions should be set to 0o600 or 0o660
    /// to restrict access to authorized users/groups.
    ///
    /// # Errors
    ///
    /// Returns error if binding fails or server encounters fatal error
    pub async fn run(&self) -> ProxyResult<()> {
        // Remove existing socket file if it exists
        if Path::new(&self.config.path).exists() {
            std::fs::remove_file(&self.config.path).map_err(|e| {
                ProxyError::backend_connection(format!(
                    "Failed to remove existing socket file {}: {}",
                    self.config.path, e
                ))
            })?;
        }

        let listener = UnixListener::bind(&self.config.path).map_err(|e| {
            ProxyError::backend_connection(format!(
                "Failed to bind Unix socket {}: {}",
                self.config.path, e
            ))
        })?;

        info!("Unix socket frontend listening on {}", self.config.path);

        loop {
            let (socket, _addr) = listener.accept().await.map_err(|e| {
                ProxyError::backend_connection(format!("Unix socket accept error: {e}"))
            })?;

            debug!("Accepted Unix socket connection");

            let backend = self.backend.clone();
            let id_translator = Arc::clone(&self.id_translator);
            let timeout = self.config.timeout;
            let max_request_size = self.config.max_request_size;

            tokio::spawn(async move {
                if let Err(e) =
                    handle_connection(socket, backend, id_translator, timeout, max_request_size)
                        .await
                {
                    error!("Unix socket connection error: {}", e);
                }
            });
        }
    }
}

/// Handle individual Unix socket client connection
async fn handle_connection(
    mut socket: UnixStream,
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
                debug!("Unix socket client closed connection");
                break;
            }
            Ok(Ok(n)) => {
                read_pos += n;
                n
            }
            Ok(Err(e)) => {
                error!("Unix socket read error: {}", e);
                break;
            }
            Err(_) => {
                error!("Unix socket read timeout");
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

                    // NOTE: Phase 2 - Route message through backend using id_translator
                    // Full routing logic would handle request/response correlation like in HTTP/WebSocket

                    // For now, send back an error response
                    let response = serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": message.get("id"),
                        "error": {
                            "code": -32603,
                            "message": "Unix socket frontend not yet fully implemented"
                        }
                    });

                    let response_bytes = serde_json::to_vec(&response)?;

                    socket.write_all(&response_bytes).await.map_err(|e| {
                        ProxyError::backend_connection(format!("Unix socket write error: {e}"))
                    })?;
                    socket.write_all(b"\n").await.map_err(|e| {
                        ProxyError::backend_connection(format!("Unix socket write error: {e}"))
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
            error!("Unix socket message exceeds maximum size");
            break;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unix_frontend_config() {
        let config =
            UnixFrontendConfig::new("/tmp/mcp.sock", Duration::from_secs(30), 10 * 1024 * 1024);
        assert_eq!(config.path, "/tmp/mcp.sock");
        assert_eq!(config.timeout, Duration::from_secs(30));
        assert_eq!(config.max_request_size, 10 * 1024 * 1024);
    }
}
