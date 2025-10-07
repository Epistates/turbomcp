//! Server-to-client request adapter for bidirectional MCP communication.
//!
//! This module provides the adapter that bridges the `ServerToClientRequests` trait
//! (defined in turbomcp-core) to the concrete `BidirectionalRouter` implementation,
//! enabling tools to make server-initiated requests to clients.
//!
//! ## Architecture
//!
//! ```text
//! Tool Handler (ctx.create_message)
//!     ↓
//! ServerToClientRequests trait (turbomcp-core) ← TYPE-SAFE INTERFACE
//!     ↓
//! ServerToClientAdapter (this module) ← THE BRIDGE
//!     ↓
//! BidirectionalRouter (turbomcp-server)
//!     ↓
//! ServerRequestDispatcher (transport abstraction)
//!     ↓
//! Transport Layer (stdio, HTTP, WebSocket, etc.)
//! ```
//!
//! ## Design Improvements (v2.0.0)
//!
//! This adapter was redesigned to leverage the new type-safe trait:
//! - **Before**: Double serialization (typed → JSON → typed → JSON)
//! - **After**: Zero serialization (typed → typed)
//! - **Before**: Cannot propagate RequestContext
//! - **After**: Full context propagation for tracing and attribution
//! - **Before**: Generic `Box<dyn Error>` return type
//! - **After**: Structured `ServerError` with pattern matching
//!
//! ## Bug Fix History
//!
//! v1.x: Fixed critical bug where `RequestContext.server_capabilities` was never populated,
//! causing all sampling/elicitation requests to fail.
//!
//! v2.0: Improved trait design to eliminate double serialization and enable context propagation.

use futures::future::BoxFuture;
use turbomcp_core::context::capabilities::ServerToClientRequests;
use turbomcp_core::{Error as McpError, RequestContext};
use turbomcp_protocol::types::{
    CreateMessageRequest, CreateMessageResult, ElicitRequest, ElicitResult, ListRootsRequest,
    ListRootsResult,
};

use crate::routing::BidirectionalRouter;
use crate::ServerError;

/// Adapter that implements the `ServerToClientRequests` trait by delegating to `BidirectionalRouter`.
///
/// This adapter bridges the gap between the generic `ServerToClientRequests` trait (defined in
/// turbomcp-core) and the concrete `BidirectionalRouter` implementation (in turbomcp-server).
///
/// ## Thread Safety
///
/// This adapter is `Send + Sync` because:
/// - `BidirectionalRouter` implements `Clone` and contains only `Arc<dyn ServerRequestDispatcher>`
/// - All interior state is immutable after construction
///
/// ## Performance
///
/// **Zero-overhead abstraction**:
/// - No intermediate serialization (types flow directly through)
/// - One `Arc` clone when creating the adapter (shared state)
/// - Direct delegation to underlying router (no indirection)
/// - Context propagation with zero allocation
///
/// ## Design Improvement (v2.0.0)
///
/// Previously named `ServerCapabilitiesAdapter`, this was renamed to `ServerToClientAdapter`
/// for clarity and redesigned to eliminate double serialization.
#[derive(Debug, Clone)]
pub struct ServerToClientAdapter {
    /// The bidirectional router that handles the actual server-initiated requests
    bidirectional: BidirectionalRouter,
}

impl ServerToClientAdapter {
    /// Create a new adapter wrapping a bidirectional router.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use turbomcp_server::capabilities::ServerToClientAdapter;
    /// use turbomcp_server::routing::BidirectionalRouter;
    ///
    /// let router = BidirectionalRouter::new();
    /// let adapter = ServerToClientAdapter::new(router);
    /// ```
    pub fn new(bidirectional: BidirectionalRouter) -> Self {
        Self { bidirectional }
    }

    /// Check if bidirectional communication is supported.
    ///
    /// Returns `true` if a dispatcher has been configured, `false` otherwise.
    pub fn supports_bidirectional(&self) -> bool {
        self.bidirectional.supports_bidirectional()
    }
}

impl ServerToClientRequests for ServerToClientAdapter {
    fn create_message(
        &self,
        request: CreateMessageRequest,
        ctx: RequestContext,
    ) -> BoxFuture<'_, Result<CreateMessageResult, McpError>> {
        Box::pin(async move {
            // Delegate directly to the bidirectional router with full context propagation
            // No serialization needed - types flow through directly (zero-cost abstraction)
            self.bidirectional
                .send_create_message_to_client(request, ctx)
                .await
                .map_err(|e| McpError::Handler {
                    message: format!("Sampling request failed: {}", e),
                    context: Some("sampling".to_string()),
                })
        })
    }

    fn elicit(
        &self,
        request: ElicitRequest,
        ctx: RequestContext,
    ) -> BoxFuture<'_, Result<ElicitResult, McpError>> {
        Box::pin(async move {
            // Delegate directly to the bidirectional router with full context propagation
            // No serialization needed - types flow through directly (zero-cost abstraction)
            self.bidirectional
                .send_elicitation_to_client(request, ctx)
                .await
                .map_err(|e| McpError::Handler {
                    message: format!("Elicitation request failed: {}", e),
                    context: Some("elicitation".to_string()),
                })
        })
    }

    fn list_roots(
        &self,
        ctx: RequestContext,
    ) -> BoxFuture<'_, Result<ListRootsResult, McpError>> {
        Box::pin(async move {
            // Create the list roots request (only has optional _meta field)
            let list_roots_request = ListRootsRequest { _meta: None };

            // Delegate directly to the bidirectional router with full context propagation
            // No serialization needed - types flow through directly (zero-cost abstraction)
            self.bidirectional
                .send_list_roots_to_client(list_roots_request, ctx)
                .await
                .map_err(|e| McpError::Handler {
                    message: format!("Roots listing request failed: {}", e),
                    context: Some("roots".to_string()),
                })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_adapter_creation() {
        let router = BidirectionalRouter::new();
        let adapter = ServerToClientAdapter::new(router);

        // Should support bidirectional if dispatcher is configured
        // (not configured in this test, so should be false)
        assert!(!adapter.supports_bidirectional());
    }

    #[test]
    fn test_adapter_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<ServerToClientAdapter>();
    }

    #[test]
    fn test_adapter_implements_trait() {
        let router = BidirectionalRouter::new();
        let adapter = ServerToClientAdapter::new(router);

        // Verify that the adapter implements ServerToClientRequests
        let _: &dyn ServerToClientRequests = &adapter;
    }

    #[test]
    fn test_adapter_clone() {
        let router = BidirectionalRouter::new();
        let adapter1 = ServerToClientAdapter::new(router);
        let adapter2 = adapter1.clone();

        // Both should have the same bidirectional support status
        assert_eq!(
            adapter1.supports_bidirectional(),
            adapter2.supports_bidirectional()
        );
    }
}
