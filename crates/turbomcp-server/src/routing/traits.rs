//! Router traits and type definitions

use turbomcp_protocol::RequestContext;
use turbomcp_protocol::{
    jsonrpc::{JsonRpcRequest, JsonRpcResponse},
    types::{
        CreateMessageRequest, ElicitRequest, ElicitResult, ListRootsResult, PingRequest, PingResult,
    },
};

use crate::ServerResult;

/// Server request dispatcher trait for server-initiated requests
#[async_trait::async_trait]
pub trait ServerRequestDispatcher: Send + Sync {
    /// Send an elicitation request to the client
    async fn send_elicitation(
        &self,
        request: ElicitRequest,
        ctx: RequestContext,
    ) -> ServerResult<ElicitResult>;

    /// Send a ping request to the client
    async fn send_ping(
        &self,
        request: PingRequest,
        ctx: RequestContext,
    ) -> ServerResult<PingResult>;

    /// Send a sampling create message request to the client
    async fn send_create_message(
        &self,
        request: CreateMessageRequest,
        ctx: RequestContext,
    ) -> ServerResult<turbomcp_protocol::types::CreateMessageResult>;

    /// Send a roots list request to the client
    async fn send_list_roots(
        &self,
        request: turbomcp_protocol::types::ListRootsRequest,
        ctx: RequestContext,
    ) -> ServerResult<ListRootsResult>;

    /// Check if client supports bidirectional communication
    fn supports_bidirectional(&self) -> bool;

    /// Get client capabilities
    async fn get_client_capabilities(&self) -> ServerResult<Option<serde_json::Value>>;
}

/// Route handler trait for custom routes
#[async_trait::async_trait]
pub trait RouteHandler: Send + Sync {
    /// Handle the request
    async fn handle(
        &self,
        request: JsonRpcRequest,
        ctx: RequestContext,
    ) -> ServerResult<JsonRpcResponse>;

    /// Check if this handler can handle the request
    fn can_handle(&self, method: &str) -> bool;

    /// Get handler metadata
    fn metadata(&self) -> RouteMetadata {
        RouteMetadata::default()
    }
}

/// Route metadata
#[derive(Debug, Clone)]
pub struct RouteMetadata {
    /// Route name
    pub name: String,
    /// Route description
    pub description: Option<String>,
    /// Route version
    pub version: String,
    /// Supported methods
    pub methods: Vec<String>,
    /// Route tags
    pub tags: Vec<String>,
}

impl Default for RouteMetadata {
    fn default() -> Self {
        Self {
            name: "unknown".to_string(),
            description: None,
            version: "1.0.0".to_string(),
            methods: Vec::new(),
            tags: Vec::new(),
        }
    }
}

/// Route definition for custom routing
#[derive(Clone)]
pub struct Route {
    /// Route method pattern
    pub method: String,
    /// Route handler
    pub handler: std::sync::Arc<dyn RouteHandler>,
    /// Route metadata
    pub metadata: RouteMetadata,
}

impl std::fmt::Debug for Route {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Route")
            .field("method", &self.method)
            .field("metadata", &self.metadata)
            .finish()
    }
}
