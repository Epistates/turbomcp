//! Server-initiated request handling (bidirectional communication)

use turbomcp_protocol::RequestContext;
use turbomcp_protocol::types::{
    CreateMessageRequest, ElicitRequest, ElicitResult, ListRootsResult, PingRequest, PingResult,
};

use crate::{McpError, ServerResult};

use super::traits::ServerRequestDispatcher;

/// Bidirectional communication methods for server-initiated requests
pub struct BidirectionalRouter {
    dispatcher: Option<std::sync::Arc<dyn ServerRequestDispatcher>>,
}

impl std::fmt::Debug for BidirectionalRouter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BidirectionalRouter")
            .field("has_dispatcher", &self.dispatcher.is_some())
            .finish()
    }
}

impl BidirectionalRouter {
    /// Create a new bidirectional router
    pub fn new() -> Self {
        Self { dispatcher: None }
    }

    /// Set the server request dispatcher
    pub fn set_dispatcher<D>(&mut self, dispatcher: D)
    where
        D: ServerRequestDispatcher + 'static,
    {
        self.dispatcher = Some(std::sync::Arc::new(dispatcher));
    }

    /// Get the server request dispatcher
    pub fn get_dispatcher(&self) -> Option<&std::sync::Arc<dyn ServerRequestDispatcher>> {
        self.dispatcher.as_ref()
    }

    /// Check if bidirectional communication is supported
    pub fn supports_bidirectional(&self) -> bool {
        self.dispatcher.is_some()
    }

    /// Send an elicitation request to the client (server-initiated)
    pub async fn send_elicitation_to_client(
        &self,
        request: ElicitRequest,
        ctx: RequestContext,
    ) -> ServerResult<ElicitResult> {
        if let Some(dispatcher) = &self.dispatcher {
            dispatcher.send_elicitation(request, ctx).await
        } else {
            Err(McpError::internal(
                "Server request dispatcher not configured for bidirectional communication",
            )
            .with_operation("elicitation"))
        }
    }

    /// Send a ping request to the client (server-initiated)
    pub async fn send_ping_to_client(
        &self,
        request: PingRequest,
        ctx: RequestContext,
    ) -> ServerResult<PingResult> {
        if let Some(dispatcher) = &self.dispatcher {
            dispatcher.send_ping(request, ctx).await
        } else {
            Err(McpError::internal(
                "Server request dispatcher not configured for bidirectional communication",
            )
            .with_operation("ping"))
        }
    }

    /// Send a create message request to the client (server-initiated)
    pub async fn send_create_message_to_client(
        &self,
        request: CreateMessageRequest,
        ctx: RequestContext,
    ) -> ServerResult<turbomcp_protocol::types::CreateMessageResult> {
        if let Some(dispatcher) = &self.dispatcher {
            dispatcher.send_create_message(request, ctx).await
        } else {
            Err(McpError::internal(
                "Server request dispatcher not configured for bidirectional communication",
            )
            .with_operation("create_message"))
        }
    }

    /// Send a list roots request to the client (server-initiated)
    pub async fn send_list_roots_to_client(
        &self,
        request: turbomcp_protocol::types::ListRootsRequest,
        ctx: RequestContext,
    ) -> ServerResult<ListRootsResult> {
        if let Some(dispatcher) = &self.dispatcher {
            dispatcher.send_list_roots(request, ctx).await
        } else {
            Err(McpError::internal(
                "Server request dispatcher not configured for bidirectional communication",
            )
            .with_operation("list_roots"))
        }
    }
}

impl Default for BidirectionalRouter {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for BidirectionalRouter {
    fn clone(&self) -> Self {
        Self {
            dispatcher: self.dispatcher.clone(),
        }
    }
}
