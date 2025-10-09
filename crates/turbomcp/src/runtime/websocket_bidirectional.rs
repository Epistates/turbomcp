//! WebSocket Runtime - Full MCP 2025-06-18 over WebSocket
//!
//! **Status**: Production implementation following MCP 2025-06-18 spec
//!
//! This module provides an adapter between the existing `WebSocketBidirectionalTransport`
//! and the `ServerRequestDispatcher` trait, enabling macro-generated servers to use
//! WebSocket for complete MCP protocol support (native full-duplex communication).
//!
//! ## Architecture
//!
//! ```text
//! ┌──────────────────────────────────────────┐
//! │ WebSocketBidirectionalTransport          │
//! │ (turbomcp-transport crate)              │
//! │ - Full-duplex WebSocket                  │
//! │ - Elicitation, ping, sampling, roots     │
//! │ - Connection management                  │
//! │ - Reconnection logic                     │
//! └──────────────────────────────────────────┘
//!              ▲
//!              │ Wraps
//!              │
//! ┌──────────────────────────────────────────┐
//! │ WebSocketDispatcher (this file)         │
//! │ - Implements ServerRequestDispatcher     │
//! │ - Delegates to transport                 │
//! │ - Error mapping                          │
//! └──────────────────────────────────────────┘
//!              ▲
//!              │ Uses
//!              │
//! ┌──────────────────────────────────────────┐
//! │ BidirectionalWrapper (generated)        │
//! │ - Injects ServerRequestDispatcher        │
//! │ - Threads RequestContext                 │
//! └──────────────────────────────────────────┘
//! ```
//!
//! ## Usage
//!
//! ```no_run
//! use std::sync::Arc;
//! use turbomcp::runtime::websocket_bidirectional::WebSocketDispatcher;
//! use turbomcp_transport::websocket_bidirectional::{
//!     WebSocketBidirectionalTransport, WebSocketBidirectionalConfig
//! };
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Create WebSocket transport
//! let config = WebSocketBidirectionalConfig::client("ws://localhost:8080".to_string());
//! let transport = WebSocketBidirectionalTransport::new(config).await?;
//! transport.connect().await?;
//!
//! // Create dispatcher adapter
//! let dispatcher = WebSocketDispatcher::new(Arc::new(transport));
//!
//! // Use with bidirectional wrapper
//! // let wrapper = MyServerBidirectional::with_dispatcher(server, dispatcher);
//! # Ok(())
//! # }
//! ```

use std::sync::Arc;

use turbomcp_protocol::types::{
    CreateMessageRequest, CreateMessageResult, ElicitRequest, ElicitResult, ListRootsRequest,
    ListRootsResult, PingRequest, PingResult,
};
use turbomcp_protocol::RequestContext;
use turbomcp_server::routing::ServerRequestDispatcher;
use turbomcp_transport::websocket_bidirectional::WebSocketBidirectionalTransport;

use crate::{ServerError, ServerResult};

/// WebSocket dispatcher adapter for server-initiated requests
///
/// This adapter wraps the existing `WebSocketBidirectionalTransport` and implements
/// the `ServerRequestDispatcher` trait for server→client requests, enabling seamless
/// integration with macro-generated servers.
///
/// ## Implementation Strategy
///
/// Rather than rewriting WebSocket protocol support, this adapter leverages
/// the battle-tested `WebSocketBidirectionalTransport` implementation (~1000 LOC)
/// which already handles:
/// - Full-duplex WebSocket communication
/// - Connection management and reconnection
/// - Compression and TLS support
/// - Request/response correlation
/// - Timeout handling
///
/// This adapter simply delegates to the transport and maps errors to `ServerError`.
///
/// ## Example
///
/// ```no_run
/// use std::sync::Arc;
/// use turbomcp::runtime::websocket_bidirectional::WebSocketDispatcher;
/// use turbomcp_transport::websocket_bidirectional::{
///     WebSocketBidirectionalTransport, WebSocketBidirectionalConfig
/// };
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let config = WebSocketBidirectionalConfig::client("ws://localhost:8080".to_string());
/// let transport = WebSocketBidirectionalTransport::new(config).await?;
/// let dispatcher = WebSocketDispatcher::new(Arc::new(transport));
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct WebSocketDispatcher {
    /// The underlying WebSocket transport
    transport: Arc<WebSocketBidirectionalTransport>,
}

impl WebSocketDispatcher {
    /// Create a new WebSocket dispatcher
    ///
    /// # Arguments
    ///
    /// * `transport` - The WebSocket transport to wrap (must be connected)
    ///
    /// # Example
    ///
    /// ```no_run
    /// use std::sync::Arc;
    /// use turbomcp::runtime::websocket_bidirectional::WebSocketDispatcher;
    /// use turbomcp_transport::websocket_bidirectional::{
    ///     WebSocketBidirectionalTransport, WebSocketBidirectionalConfig
    /// };
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let config = WebSocketBidirectionalConfig::client("ws://localhost:8080".to_string());
    /// let transport = WebSocketBidirectionalTransport::new(config).await?;
    /// transport.connect().await?;
    ///
    /// let dispatcher = WebSocketDispatcher::new(Arc::new(transport));
    /// # Ok(())
    /// # }
    /// ```
    pub fn new(transport: Arc<WebSocketBidirectionalTransport>) -> Self {
        Self { transport }
    }

    /// Get a reference to the underlying transport
    ///
    /// This allows access to transport-specific methods like connection management.
    pub fn transport(&self) -> &Arc<WebSocketBidirectionalTransport> {
        &self.transport
    }
}

#[async_trait::async_trait]
impl ServerRequestDispatcher for WebSocketDispatcher {
    /// Send an elicitation request to the client
    ///
    /// ## MCP 2025-06-18 Compliance
    ///
    /// - Method: `elicitation/create`
    /// - Format: JSON-RPC 2.0
    /// - Timeout: 60 seconds (configurable)
    /// - Correlation: UUID request ID
    ///
    /// Delegates to `WebSocketBidirectionalTransport::send_elicitation`.
    async fn send_elicitation(
        &self,
        request: ElicitRequest,
        _ctx: RequestContext,
    ) -> ServerResult<ElicitResult> {
        self.transport
            .send_elicitation(request, None)
            .await
            .map_err(|e| ServerError::Handler {
                message: format!("WebSocket elicitation failed: {}", e),
                context: Some("WebSocket transport".to_string()),
            })
    }

    /// Send a ping request to the client
    ///
    /// ## MCP 2025-06-18 Compliance
    ///
    /// - Method: `ping`
    /// - Format: JSON-RPC 2.0
    /// - Timeout: 60 seconds (configurable)
    /// - Response: Empty object `{}`
    ///
    /// Delegates to `WebSocketBidirectionalTransport::send_ping`.
    async fn send_ping(
        &self,
        request: PingRequest,
        _ctx: RequestContext,
    ) -> ServerResult<PingResult> {
        self.transport
            .send_ping(request, None)
            .await
            .map_err(|e| ServerError::Handler {
                message: format!("WebSocket ping failed: {}", e),
                context: Some("WebSocket transport".to_string()),
            })
    }

    /// Send a sampling/createMessage request to the client
    ///
    /// ## MCP 2025-06-18 Compliance
    ///
    /// - Method: `sampling/createMessage`
    /// - Format: JSON-RPC 2.0
    /// - Timeout: 60 seconds (configurable)
    /// - Parameters: messages, modelPreferences, systemPrompt, maxTokens
    ///
    /// Delegates to `WebSocketBidirectionalTransport::send_sampling`.
    async fn send_create_message(
        &self,
        request: CreateMessageRequest,
        _ctx: RequestContext,
    ) -> ServerResult<CreateMessageResult> {
        self.transport
            .send_sampling(request, None)
            .await
            .map_err(|e| ServerError::Handler {
                message: format!("WebSocket sampling failed: {}", e),
                context: Some("WebSocket transport".to_string()),
            })
    }

    /// Send a roots/list request to the client
    ///
    /// ## MCP 2025-06-18 Compliance
    ///
    /// - Method: `roots/list`
    /// - Format: JSON-RPC 2.0
    /// - Timeout: 60 seconds (configurable)
    /// - Response: List of Root objects
    ///
    /// Delegates to `WebSocketBidirectionalTransport::send_list_roots`.
    async fn send_list_roots(
        &self,
        request: ListRootsRequest,
        _ctx: RequestContext,
    ) -> ServerResult<ListRootsResult> {
        self.transport
            .send_list_roots(request, None)
            .await
            .map_err(|e| ServerError::Handler {
                message: format!("WebSocket roots/list failed: {}", e),
                context: Some("WebSocket transport".to_string()),
            })
    }

    /// Check if server→client requests are supported
    ///
    /// Always returns `true` for WebSocket since full-duplex is native.
    fn supports_bidirectional(&self) -> bool {
        true
    }

    /// Get client capabilities
    ///
    /// Returns `None` for WebSocket as capabilities are exchanged during handshake.
    async fn get_client_capabilities(&self) -> ServerResult<Option<serde_json::Value>> {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use turbomcp_transport::websocket_bidirectional::WebSocketBidirectionalConfig;

    #[tokio::test]
    async fn test_websocket_dispatcher_creation() {
        let config = WebSocketBidirectionalConfig::default();
        let transport = WebSocketBidirectionalTransport::new(config).await.unwrap();
        let dispatcher = WebSocketDispatcher::new(Arc::new(transport));

        assert!(dispatcher.supports_bidirectional());
    }

    #[tokio::test]
    async fn test_websocket_dispatcher_capabilities() {
        let config = WebSocketBidirectionalConfig::default();
        let transport = WebSocketBidirectionalTransport::new(config).await.unwrap();
        let dispatcher = WebSocketDispatcher::new(Arc::new(transport));

        let capabilities = dispatcher.get_client_capabilities().await.unwrap();
        assert!(capabilities.is_none());
    }

    #[tokio::test]
    async fn test_websocket_dispatcher_send_ping_not_connected() {
        let config = WebSocketBidirectionalConfig::default();
        let transport = WebSocketBidirectionalTransport::new(config).await.unwrap();
        let dispatcher = WebSocketDispatcher::new(Arc::new(transport));

        let request = PingRequest { _meta: None };
        let result = dispatcher
            .send_ping(request, RequestContext::new())
            .await;

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("WebSocket not connected"));
    }
}
