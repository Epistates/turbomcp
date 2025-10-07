//! Request routing and handler dispatch system
//!
//! This module provides a comprehensive routing system for MCP protocol requests,
//! supporting all standard MCP methods with enterprise features like RBAC,
//! JSON Schema validation, timeout management, and bidirectional communication.

mod bidirectional;
mod config;
mod handlers;
mod traits;
mod utils;
mod validation;

// Re-export public types to maintain API compatibility
pub use bidirectional::BidirectionalRouter;
pub use config::RouterConfig;
pub use traits::{Route, RouteHandler, RouteMetadata, ServerRequestDispatcher};

use dashmap::DashMap;
use futures::stream::{self, StreamExt};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::warn;
use turbomcp_core::RequestContext;
use turbomcp_protocol::{
    jsonrpc::{JsonRpcRequest, JsonRpcResponse},
    types::{
        CreateMessageRequest, ElicitRequest, ElicitResult, ListRootsResult, PingRequest, PingResult,
    },
};

use crate::capabilities::ServerToClientAdapter;
use crate::metrics::ServerMetrics;
use crate::registry::HandlerRegistry;
use crate::{ServerError, ServerResult};

use handlers::{HandlerContext, ProtocolHandlers};
use turbomcp_core::context::capabilities::ServerToClientRequests;
use utils::{error_response, method_not_found_response};
use validation::{validate_request, validate_response};

/// Request router for dispatching MCP requests to appropriate handlers
pub struct RequestRouter {
    /// Handler registry
    registry: Arc<HandlerRegistry>,
    /// Route configuration
    config: RouterConfig,
    /// Custom route handlers
    custom_routes: HashMap<String, Arc<dyn RouteHandler>>,
    /// Resource subscription counters by URI (reserved for future functionality)
    #[allow(dead_code)]
    resource_subscriptions: DashMap<String, usize>,
    /// Bidirectional communication router
    bidirectional: BidirectionalRouter,
    /// Protocol handlers
    handlers: ProtocolHandlers,
    /// Server-to-client requests adapter for tool-initiated requests (sampling, elicitation, roots)
    /// This is injected into RequestContext so tools can make server-initiated requests
    server_to_client: Arc<dyn ServerToClientRequests>,
}

impl std::fmt::Debug for RequestRouter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RequestRouter")
            .field("config", &self.config)
            .field("custom_routes_count", &self.custom_routes.len())
            .finish()
    }
}

impl RequestRouter {
    /// Create a new request router
    #[must_use]
    pub fn new(registry: Arc<HandlerRegistry>, _metrics: Arc<ServerMetrics>) -> Self {
        // Timeout management is now handled by middleware
        let config = RouterConfig::default();

        let handler_context = HandlerContext::new(Arc::clone(&registry));

        let bidirectional = BidirectionalRouter::new();

        // Create the server-to-client adapter that bridges bidirectional router
        // to the ServerToClientRequests trait (type-safe, zero-cost abstraction)
        let server_to_client: Arc<dyn ServerToClientRequests> =
            Arc::new(ServerToClientAdapter::new(bidirectional.clone()));

        Self {
            registry,
            config,
            custom_routes: HashMap::new(),
            resource_subscriptions: DashMap::new(),
            bidirectional,
            handlers: ProtocolHandlers::new(handler_context),
            server_to_client,
        }
    }

    /// Create a router with configuration
    #[must_use]
    pub fn with_config(
        registry: Arc<HandlerRegistry>,
        config: RouterConfig,
        _metrics: Arc<ServerMetrics>,
    ) -> Self {
        // Timeout management is now handled by middleware

        let handler_context = HandlerContext::new(Arc::clone(&registry));

        let bidirectional = BidirectionalRouter::new();

        // Create the server-to-client adapter that bridges bidirectional router
        // to the ServerToClientRequests trait (type-safe, zero-cost abstraction)
        let server_to_client: Arc<dyn ServerToClientRequests> =
            Arc::new(ServerToClientAdapter::new(bidirectional.clone()));

        Self {
            registry,
            config,
            custom_routes: HashMap::new(),
            resource_subscriptions: DashMap::new(),
            bidirectional,
            handlers: ProtocolHandlers::new(handler_context),
            server_to_client,
        }
    }

    // Timeout configuration now handled by middleware - no longer needed

    /// Set the server request dispatcher for bidirectional communication
    pub fn set_server_request_dispatcher<D>(&mut self, dispatcher: D)
    where
        D: ServerRequestDispatcher + 'static,
    {
        self.bidirectional.set_dispatcher(dispatcher);
    }

    /// Get the server request dispatcher
    pub fn get_server_request_dispatcher(&self) -> Option<&Arc<dyn ServerRequestDispatcher>> {
        self.bidirectional.get_dispatcher()
    }

    /// Check if bidirectional routing is enabled and supported
    pub fn supports_bidirectional(&self) -> bool {
        self.config.enable_bidirectional && self.bidirectional.supports_bidirectional()
    }

    /// Add a custom route handler
    ///
    /// # Errors
    ///
    /// Returns [`ServerError::Routing`] if a route for the same method already exists.
    pub fn add_route<H>(&mut self, handler: H) -> ServerResult<()>
    where
        H: RouteHandler + 'static,
    {
        let metadata = handler.metadata();
        let handler_arc: Arc<dyn RouteHandler> = Arc::new(handler);

        for method in &metadata.methods {
            if self.custom_routes.contains_key(method) {
                return Err(ServerError::routing_with_method(
                    format!("Route for method '{method}' already exists"),
                    method.clone(),
                ));
            }
            self.custom_routes
                .insert(method.clone(), Arc::clone(&handler_arc));
        }

        Ok(())
    }

    /// Route a JSON-RPC request to the appropriate handler
    pub async fn route(&self, request: JsonRpcRequest, ctx: RequestContext) -> JsonRpcResponse {
        // Inject server-to-client capabilities into context for tool-initiated requests
        // Enables type-safe sampling, elicitation, and roots listing with full context propagation
        let ctx = ctx.with_server_to_client(Arc::clone(&self.server_to_client));

        // Validate request if enabled
        if self.config.validate_requests
            && let Err(e) = validate_request(&request)
        {
            return error_response(&request, e);
        }

        // Handle the request
        let result = match request.method.as_str() {
            // Core protocol methods
            "initialize" => self.handlers.handle_initialize(request, ctx).await,

            // Tool methods
            "tools/list" => self.handlers.handle_list_tools(request, ctx).await,
            "tools/call" => self.handlers.handle_call_tool(request, ctx).await,

            // Prompt methods
            "prompts/list" => self.handlers.handle_list_prompts(request, ctx).await,
            "prompts/get" => self.handlers.handle_get_prompt(request, ctx).await,

            // Resource methods
            "resources/list" => self.handlers.handle_list_resources(request, ctx).await,
            "resources/read" => self.handlers.handle_read_resource(request, ctx).await,
            "resources/subscribe" => self.handlers.handle_subscribe_resource(request, ctx).await,
            "resources/unsubscribe" => {
                self.handlers
                    .handle_unsubscribe_resource(request, ctx)
                    .await
            }

            // Logging methods
            "logging/setLevel" => self.handlers.handle_set_log_level(request, ctx).await,

            // Sampling methods
            "sampling/createMessage" => self.handlers.handle_create_message(request, ctx).await,

            // Roots methods
            "roots/list" => self.handlers.handle_list_roots(request, ctx).await,

            // Enhanced MCP features (MCP 2025-06-18 protocol methods)
            "elicitation/create" => self.handlers.handle_elicitation(request, ctx).await,
            "completion/complete" => self.handlers.handle_completion(request, ctx).await,
            "resources/templates/list" => {
                self.handlers
                    .handle_list_resource_templates(request, ctx)
                    .await
            }
            "ping" => self.handlers.handle_ping(request, ctx).await,

            // Custom routes
            method => {
                if let Some(handler) = self.custom_routes.get(method) {
                    let request_clone = request.clone();
                    handler
                        .handle(request, ctx)
                        .await
                        .unwrap_or_else(|e| error_response(&request_clone, e))
                } else {
                    method_not_found_response(&request)
                }
            }
        };

        // Validate response if enabled
        if self.config.validate_responses
            && let Err(e) = validate_response(&result)
        {
            warn!("Response validation failed: {}", e);
        }

        result
    }

    /// Handle batch requests
    pub async fn route_batch(
        &self,
        requests: Vec<JsonRpcRequest>,
        ctx: RequestContext,
    ) -> Vec<JsonRpcResponse> {
        // Note: Server capabilities are injected in route() for each request
        let max_in_flight = self.config.max_concurrent_requests.max(1);
        stream::iter(requests.into_iter())
            .map(|req| {
                let ctx_cloned = ctx.clone();
                async move { self.route(req, ctx_cloned).await }
            })
            .buffer_unordered(max_in_flight)
            .collect()
            .await
    }

    /// Send an elicitation request to the client (server-initiated)
    ///
    /// # Errors
    ///
    /// Returns [`ServerError::Transport`] if:
    /// - The bidirectional dispatcher is not configured
    /// - The client request fails
    /// - The client does not respond
    pub async fn send_elicitation_to_client(
        &self,
        request: ElicitRequest,
        ctx: RequestContext,
    ) -> ServerResult<ElicitResult> {
        self.bidirectional
            .send_elicitation_to_client(request, ctx)
            .await
    }

    /// Send a ping request to the client (server-initiated)
    ///
    /// # Errors
    ///
    /// Returns [`ServerError::Transport`] if:
    /// - The bidirectional dispatcher is not configured
    /// - The client request fails
    /// - The client does not respond
    pub async fn send_ping_to_client(
        &self,
        request: PingRequest,
        ctx: RequestContext,
    ) -> ServerResult<PingResult> {
        self.bidirectional.send_ping_to_client(request, ctx).await
    }

    /// Send a create message request to the client (server-initiated)
    ///
    /// # Errors
    ///
    /// Returns [`ServerError::Transport`] if:
    /// - The bidirectional dispatcher is not configured
    /// - The client request fails
    /// - The client does not support sampling
    pub async fn send_create_message_to_client(
        &self,
        request: CreateMessageRequest,
        ctx: RequestContext,
    ) -> ServerResult<turbomcp_protocol::types::CreateMessageResult> {
        self.bidirectional
            .send_create_message_to_client(request, ctx)
            .await
    }

    /// Send a list roots request to the client (server-initiated)
    ///
    /// # Errors
    ///
    /// Returns [`ServerError::Transport`] if:
    /// - The bidirectional dispatcher is not configured
    /// - The client request fails
    /// - The client does not support roots
    pub async fn send_list_roots_to_client(
        &self,
        request: turbomcp_protocol::types::ListRootsRequest,
        ctx: RequestContext,
    ) -> ServerResult<ListRootsResult> {
        self.bidirectional
            .send_list_roots_to_client(request, ctx)
            .await
    }
}

impl Clone for RequestRouter {
    fn clone(&self) -> Self {
        Self {
            registry: Arc::clone(&self.registry),
            config: self.config.clone(),
            custom_routes: self.custom_routes.clone(),
            resource_subscriptions: DashMap::new(),
            bidirectional: self.bidirectional.clone(),
            handlers: ProtocolHandlers::new(HandlerContext::new(Arc::clone(&self.registry))),
            server_to_client: Arc::clone(&self.server_to_client),
        }
    }
}

// Design Note: ServerCapabilities trait implementation
//
// RequestRouter currently uses BidirectionalRouter for server-initiated requests
// (sampling, elicitation, roots) instead of directly implementing the ServerCapabilities
// trait from turbomcp_core::context::capabilities.
//
// Current Pattern:
// - RequestRouter contains BidirectionalRouter which handles server-to-client requests
// - BidirectionalRouter uses ServerRequestDispatcher trait for transport-agnostic dispatch
// - This pattern provides better separation of concerns and testability
//
// Alternative (not implemented):
// - RequestRouter could implement ServerCapabilities trait directly
// - This would allow passing router as &dyn ServerCapabilities to tools
// - Current pattern is preferred as it keeps routing and bidirectional concerns separate
//
// See: crates/turbomcp-server/src/routing/bidirectional.rs for current implementation

/// Router alias for convenience
pub type Router = RequestRouter;

// ===================================================================
// JsonRpcHandler Implementation - For HTTP Transport Integration
// ===================================================================

#[async_trait::async_trait]
impl turbomcp_core::JsonRpcHandler for RequestRouter {
    /// Handle a JSON-RPC request via the HTTP transport
    ///
    /// This implementation enables `RequestRouter` to be used directly with
    /// the HTTP transport layer (`run_server`), supporting the builder pattern
    /// for programmatic server construction.
    ///
    /// # Architecture
    ///
    /// - Parses raw JSON into `JsonRpcRequest`
    /// - Creates default `RequestContext` (no auth/session for HTTP)
    /// - Routes through the existing `route()` method
    /// - Serializes `JsonRpcResponse` back to JSON
    ///
    /// This provides the same request handling as the macro pattern but
    /// allows runtime handler registration via `ServerBuilder`.
    async fn handle_request(&self, req_value: serde_json::Value) -> serde_json::Value {
        // Parse the request
        let req: JsonRpcRequest = match serde_json::from_value(req_value) {
            Ok(r) => r,
            Err(e) => {
                return serde_json::json!({
                    "jsonrpc": "2.0",
                    "error": {
                        "code": -32700,
                        "message": format!("Parse error: {}", e)
                    },
                    "id": null
                });
            }
        };

        // Create default context for HTTP requests
        // Note: For authenticated HTTP requests, middleware should inject auth info
        let ctx = RequestContext::default();

        // Route the request through the standard routing system
        let response = self.route(req, ctx).await;

        // Serialize response
        match serde_json::to_value(&response) {
            Ok(v) => v,
            Err(e) => {
                serde_json::json!({
                    "jsonrpc": "2.0",
                    "error": {
                        "code": -32603,
                        "message": format!("Internal error: failed to serialize response: {}", e)
                    },
                    "id": response.id
                })
            }
        }
    }
}
