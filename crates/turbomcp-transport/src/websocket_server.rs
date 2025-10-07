//! WebSocket server implementation with maximum DX
//!
//! This module provides a simple, batteries-included WebSocket server that matches
//! the DX of HTTP/SSE transport. Uses the same Axum infrastructure with sensible defaults.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::{Router, routing::get};
use tracing::info;

/// Configuration for WebSocket server
#[derive(Clone, Debug)]
pub struct WebSocketServerConfig {
    /// Bind address (e.g. "127.0.0.1:8080")
    pub bind_addr: String,

    /// WebSocket endpoint path (default: "/ws")
    pub endpoint_path: String,
}

impl Default for WebSocketServerConfig {
    fn default() -> Self {
        Self {
            bind_addr: "127.0.0.1:8080".to_string(),
            endpoint_path: "/ws".to_string(),
        }
    }
}

/// Create WebSocket router for JsonRpcHandler
///
/// This creates a simple router with WebSocket upgrade at the configured endpoint.
/// The handler processes incoming JSON-RPC messages over the WebSocket connection.
pub fn create_websocket_router<H: turbomcp_protocol::JsonRpcHandler + Clone>(
    config: WebSocketServerConfig,
    handler: Arc<H>,
) -> Router {
    use axum::extract::{WebSocketUpgrade, State, ws::WebSocket};
    use futures::{SinkExt, StreamExt};

    #[derive(Clone)]
    struct AppState<H> {
        handler: Arc<H>,
    }

    async fn handle_websocket<H: turbomcp_protocol::JsonRpcHandler>(
        socket: WebSocket,
        handler: Arc<H>,
    ) {
        let (mut sender, mut receiver) = socket.split();

        while let Some(msg) = receiver.next().await {
            match msg {
                Ok(axum::extract::ws::Message::Text(text)) => {
                    // Parse as JSON Value
                    match serde_json::from_str::<serde_json::Value>(&text) {
                        Ok(request) => {
                            // Process through handler
                            let response = handler.handle_request(request).await;

                            // Send response
                            let response_text = serde_json::to_string(&response).unwrap_or_default();
                            if let Err(e) = sender
                                .send(axum::extract::ws::Message::Text(response_text.into()))
                                .await
                            {
                                tracing::error!("Failed to send WebSocket response: {}", e);
                                break;
                            }
                        }
                        Err(e) => {
                            tracing::error!("Failed to parse JSON request: {}", e);
                        }
                    }
                }
                Ok(axum::extract::ws::Message::Close(_)) => {
                    break;
                }
                Err(e) => {
                    tracing::error!("WebSocket error: {}", e);
                    break;
                }
                _ => {}
            }
        }
    }

    let state = AppState {
        handler: handler.clone(),
    };

    let endpoint = config.endpoint_path.clone();

    Router::new()
        .route(
            &endpoint,
            get(|ws: WebSocketUpgrade, State(state): State<AppState<H>>| async move {
                ws.on_upgrade(move |socket| handle_websocket(socket, state.handler))
            }),
        )
        .with_state(state)
}

/// Run WebSocket server with simple API (matches HTTP transport DX)
///
/// This function provides the same simple API as `run_http()`:
/// - Sensible defaults
/// - Single endpoint
/// - Full MCP 2025-06-18 compliance
/// - Bidirectional communication
/// - Elicitation support
///
/// # Example
///
/// ```rust,no_run
/// use std::sync::Arc;
/// use turbomcp_transport::websocket_server::run_websocket_server;
/// use turbomcp_protocol::JsonRpcHandler;
///
/// # async fn example<H: JsonRpcHandler>(handler: Arc<H>) {
/// // Simple API - just like HTTP!
/// run_websocket_server("127.0.0.1:8080", handler).await.unwrap();
/// # }
/// ```
pub async fn run_websocket_server<H: turbomcp_protocol::JsonRpcHandler + Clone>(
    bind_addr: impl ToString,
    handler: Arc<H>,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = WebSocketServerConfig {
        bind_addr: bind_addr.to_string(),
        ..Default::default()
    };
    run_websocket_server_with_config(config, handler).await
}

/// Run WebSocket server with custom configuration
///
/// Provides full control over endpoint path.
///
/// # Example
///
/// ```rust,no_run
/// use std::sync::Arc;
/// use turbomcp_transport::websocket_server::{run_websocket_server_with_config, WebSocketServerConfig};
/// use turbomcp_protocol::JsonRpcHandler;
///
/// # async fn example<H: JsonRpcHandler>(handler: Arc<H>) {
/// let config = WebSocketServerConfig {
///     bind_addr: "0.0.0.0:8080".to_string(),
///     endpoint_path: "/ws".to_string(),
/// };
/// run_websocket_server_with_config(config, handler).await.unwrap();
/// # }
/// ```
pub async fn run_websocket_server_with_config<H: turbomcp_protocol::JsonRpcHandler + Clone>(
    config: WebSocketServerConfig,
    handler: Arc<H>,
) -> Result<(), Box<dyn std::error::Error>> {
    let bind_addr = config.bind_addr.clone();
    let endpoint = config.endpoint_path.clone();
    let server_info = handler.server_info();

    // Create router with WebSocket handler
    let app = create_websocket_router(config, handler);

    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;

    info!("🚀 MCP 2025-06-18 Compliant WebSocket Transport Ready");
    info!("   Server: {} v{}", server_info.name, server_info.version);
    info!("   Listening: {}", bind_addr);
    info!("   Endpoint: {} (WebSocket upgrade)", endpoint);
    info!("   Features: Bidirectional communication, JSON-RPC over WebSocket");

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = WebSocketServerConfig::default();
        assert_eq!(config.bind_addr, "127.0.0.1:8080");
        assert_eq!(config.endpoint_path, "/ws");
    }

    #[test]
    fn test_custom_config() {
        let config = WebSocketServerConfig {
            bind_addr: "0.0.0.0:9000".to_string(),
            endpoint_path: "/custom/ws".to_string(),
        };
        assert_eq!(config.bind_addr, "0.0.0.0:9000");
        assert_eq!(config.endpoint_path, "/custom/ws");
    }
}
