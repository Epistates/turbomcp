//! Adapter for transport layer's WebSocketDispatcher to implement ServerRequestDispatcher
//!
//! This adapter bridges the transport layer's WebSocketDispatcher (which handles
//! WebSocket-specific mechanics) with the server layer's ServerRequestDispatcher trait
//! (which defines the server-initiated request interface).

use turbomcp_protocol::RequestContext;
use turbomcp_protocol::types::{
    CreateMessageRequest, CreateMessageResult, ElicitRequest, ElicitResult, ListRootsRequest,
    ListRootsResult, PingRequest, PingResult,
};
use turbomcp_transport::axum::WebSocketDispatcher;

use super::traits::ServerRequestDispatcher;
use crate::{ServerError, ServerResult};

/// Adapter that wraps transport layer's WebSocketDispatcher and implements ServerRequestDispatcher
///
/// This adapter enables ServerBuilder to use the transport layer's WebSocket infrastructure
/// while maintaining the server layer's ServerRequestDispatcher interface.
#[derive(Clone, Debug)]
pub struct WebSocketDispatcherAdapter {
    /// Transport layer's WebSocket dispatcher
    dispatcher: WebSocketDispatcher,
}

impl WebSocketDispatcherAdapter {
    /// Create a new adapter wrapping the transport dispatcher
    pub fn new(dispatcher: WebSocketDispatcher) -> Self {
        Self { dispatcher }
    }
}

#[async_trait::async_trait]
impl ServerRequestDispatcher for WebSocketDispatcherAdapter {
    async fn send_elicitation(
        &self,
        request: ElicitRequest,
        _ctx: RequestContext,
    ) -> ServerResult<ElicitResult> {
        self.dispatcher
            .send_elicitation_request(request)
            .await
            .map_err(|e| ServerError::Handler {
                message: e,
                context: Some("WebSocket elicitation request".to_string()),
            })
    }

    async fn send_ping(
        &self,
        request: PingRequest,
        _ctx: RequestContext,
    ) -> ServerResult<PingResult> {
        self.dispatcher
            .send_ping_request(request)
            .await
            .map_err(|e| ServerError::Handler {
                message: e,
                context: Some("WebSocket ping request".to_string()),
            })
    }

    async fn send_create_message(
        &self,
        request: CreateMessageRequest,
        _ctx: RequestContext,
    ) -> ServerResult<CreateMessageResult> {
        self.dispatcher
            .send_create_message_request(request)
            .await
            .map_err(|e| ServerError::Handler {
                message: e,
                context: Some("WebSocket sampling request".to_string()),
            })
    }

    async fn send_list_roots(
        &self,
        request: ListRootsRequest,
        _ctx: RequestContext,
    ) -> ServerResult<ListRootsResult> {
        self.dispatcher
            .send_list_roots_request(request)
            .await
            .map_err(|e| ServerError::Handler {
                message: e,
                context: Some("WebSocket roots list request".to_string()),
            })
    }

    fn supports_bidirectional(&self) -> bool {
        self.dispatcher.supports_bidirectional()
    }

    async fn get_client_capabilities(&self) -> ServerResult<Option<serde_json::Value>> {
        // WebSocket connections don't have a separate capability negotiation
        // Capabilities are exchanged during the MCP initialize handshake
        Ok(None)
    }
}
