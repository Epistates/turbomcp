//! Request routing and handler dispatch system

use dashmap::DashMap;
use std::collections::HashMap;
use std::sync::Arc;
use turbomcp_core::RequestContext;
use turbomcp_protocol::{
    jsonrpc::{JsonRpcRequest, JsonRpcResponse},
    types::{
        CallToolRequest,
        CompleteRequestParams,
        CompletionResponse,
        CreateMessageRequest,
        // New MCP feature types
        ElicitRequest,
        ElicitResult,
        EmptyResult,
        GetPromptRequest,
        Implementation,
        InitializeRequest,
        InitializeResult,
        ListPromptsResult,
        ListResourceTemplatesRequest,
        ListResourceTemplatesResult,
        ListResourcesResult,
        ListRootsResult,
        ListToolsResult,
        LoggingCapabilities,
        PingRequest,
        PingResult,
        PromptsCapabilities,
        ReadResourceRequest,
        ResourcesCapabilities,
        Root,
        ServerCapabilities,
        SetLevelRequest,
        SubscribeRequest,
        ToolsCapabilities,
        UnsubscribeRequest,
    },
};

use crate::registry::HandlerRegistry;
use crate::{ServerError, ServerResult};
use futures::stream::{self, StreamExt};
use jsonschema::{Draft, JSONSchema};

/// Request router for dispatching MCP requests to appropriate handlers
pub struct RequestRouter {
    /// Handler registry
    registry: Arc<HandlerRegistry>,
    /// Route configuration
    config: RouterConfig,
    /// Custom route handlers
    custom_routes: HashMap<String, Arc<dyn RouteHandler>>,
    /// Resource subscription counters by URI
    resource_subscriptions: DashMap<String, usize>,
    /// Server-initiated request dispatcher (for bidirectional communication)
    server_request_dispatcher: Option<Arc<dyn ServerRequestDispatcher>>,
}

impl std::fmt::Debug for RequestRouter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RequestRouter")
            .field("config", &self.config)
            .field("custom_routes_count", &self.custom_routes.len())
            .finish()
    }
}

/// Router configuration
#[derive(Debug, Clone)]
pub struct RouterConfig {
    /// Enable request validation
    pub validate_requests: bool,
    /// Enable response validation
    pub validate_responses: bool,
    /// Default request timeout in milliseconds
    pub default_timeout_ms: u64,
    /// Enable request tracing
    pub enable_tracing: bool,
    /// Maximum concurrent requests
    pub max_concurrent_requests: usize,
    /// Enable bidirectional routing (server-initiated requests)
    pub enable_bidirectional: bool,
}

impl Default for RouterConfig {
    fn default() -> Self {
        Self {
            validate_requests: true,
            validate_responses: true,
            default_timeout_ms: 30_000,
            enable_tracing: true,
            max_concurrent_requests: 1000,
            enable_bidirectional: true,
        }
    }
}

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

impl RequestRouter {
    /// Create a new request router
    #[must_use]
    pub fn new(registry: Arc<HandlerRegistry>) -> Self {
        Self {
            registry,
            config: RouterConfig::default(),
            custom_routes: HashMap::new(),
            resource_subscriptions: DashMap::new(),
            server_request_dispatcher: None,
        }
    }

    /// Create a router with configuration
    #[must_use]
    pub fn with_config(registry: Arc<HandlerRegistry>, config: RouterConfig) -> Self {
        Self {
            registry,
            config,
            custom_routes: HashMap::new(),
            resource_subscriptions: DashMap::new(),
            server_request_dispatcher: None,
        }
    }

    /// Set the server request dispatcher for bidirectional communication
    pub fn set_server_request_dispatcher<D>(&mut self, dispatcher: D)
    where
        D: ServerRequestDispatcher + 'static,
    {
        self.server_request_dispatcher = Some(Arc::new(dispatcher));
    }

    /// Get the server request dispatcher  
    pub fn get_server_request_dispatcher(&self) -> Option<&Arc<dyn ServerRequestDispatcher>> {
        self.server_request_dispatcher.as_ref()
    }

    /// Check if bidirectional routing is enabled and supported
    pub fn supports_bidirectional(&self) -> bool {
        self.config.enable_bidirectional && self.server_request_dispatcher.is_some()
    }

    /// Add a custom route handler
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
        // Validate request if enabled
        if self.config.validate_requests
            && let Err(e) = self.validate_request(&request)
        {
            return self.error_response(&request, e);
        }

        // Handle the request
        let result = match request.method.as_str() {
            // Core protocol methods
            "initialize" => self.handle_initialize(request, ctx).await,

            // Tool methods
            "tools/list" => self.handle_list_tools(request, ctx).await,
            "tools/call" => self.handle_call_tool(request, ctx).await,

            // Prompt methods
            "prompts/list" => self.handle_list_prompts(request, ctx).await,
            "prompts/get" => self.handle_get_prompt(request, ctx).await,

            // Resource methods
            "resources/list" => self.handle_list_resources(request, ctx).await,
            "resources/read" => self.handle_read_resource(request, ctx).await,
            "resources/subscribe" => self.handle_subscribe_resource(request, ctx).await,
            "resources/unsubscribe" => self.handle_unsubscribe_resource(request, ctx).await,

            // Logging methods
            "logging/setLevel" => self.handle_set_log_level(request, ctx).await,

            // Sampling methods
            "sampling/createMessage" => self.handle_create_message(request, ctx).await,

            // Roots methods
            "roots/list" => self.handle_list_roots(request, ctx).await,

            // Enhanced MCP features (new protocol methods)
            "elicit/request" => self.handle_elicitation(request, ctx).await,
            "complete/request" => self.handle_completion(request, ctx).await,
            "resources/templates/list" => self.handle_list_resource_templates(request, ctx).await,
            "ping/request" => self.handle_ping(request, ctx).await,

            // Custom routes
            method => {
                if let Some(handler) = self.custom_routes.get(method) {
                    let request_clone = request.clone();
                    handler
                        .handle(request, ctx)
                        .await
                        .unwrap_or_else(|e| self.error_response(&request_clone, e))
                } else {
                    self.method_not_found_response(&request)
                }
            }
        };

        // Validate response if enabled
        if self.config.validate_responses
            && let Err(e) = self.validate_response(&result)
        {
            tracing::warn!("Response validation failed: {}", e);
        }

        result
    }

    /// Handle batch requests
    pub async fn route_batch(
        &self,
        requests: Vec<JsonRpcRequest>,
        ctx: RequestContext,
    ) -> Vec<JsonRpcResponse> {
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

    // Protocol method handlers

    async fn handle_initialize(
        &self,
        request: JsonRpcRequest,
        _ctx: RequestContext,
    ) -> JsonRpcResponse {
        match self.parse_params::<InitializeRequest>(&request) {
            Ok(_init_request) => {
                let result = InitializeResult {
                    protocol_version: turbomcp_protocol::PROTOCOL_VERSION.to_string(),
                    server_info: Implementation {
                        name: crate::SERVER_NAME.to_string(),
                        title: Some("TurboMCP Server".to_string()),
                        version: crate::SERVER_VERSION.to_string(),
                    },
                    capabilities: self.get_server_capabilities(),
                    instructions: None,
                };

                self.success_response(&request, result)
            }
            Err(e) => self.error_response(&request, e),
        }
    }

    async fn handle_list_tools(
        &self,
        request: JsonRpcRequest,
        _ctx: RequestContext,
    ) -> JsonRpcResponse {
        let tools = self.registry.get_tool_definitions();
        let result = ListToolsResult {
            tools,
            next_cursor: None,
        };
        self.success_response(&request, result)
    }

    async fn handle_call_tool(
        &self,
        request: JsonRpcRequest,
        ctx: RequestContext,
    ) -> JsonRpcResponse {
        // Inject the router as server capabilities for server-initiated requests
        let ctx = ctx.with_server_capabilities(
            Arc::new(self.clone()) as Arc<dyn turbomcp_core::ServerCapabilities>
        );

        match self.parse_params::<CallToolRequest>(&request) {
            Ok(call_request) => {
                let tool_name = &call_request.name;

                if let Some(handler) = self.registry.get_tool(tool_name) {
                    // RBAC: if handler metadata enforces allowed roles, check RequestContext
                    if self.config.validate_requests
                        && let Some(required_roles) = handler.allowed_roles()
                    {
                        let has_role = ctx
                            .metadata
                            .get("auth")
                            .and_then(|v| v.get("roles"))
                            .and_then(|v| v.as_array())
                            .is_some_and(|arr| {
                                let user_set: std::collections::HashSet<String> = arr
                                    .iter()
                                    .filter_map(|v| {
                                        v.as_str().map(std::string::ToString::to_string)
                                    })
                                    .collect();
                                required_roles.iter().any(|r| user_set.contains(r))
                            });
                        if !has_role {
                            return self.error_response(
                                &request,
                                ServerError::authentication(format!(
                                    "Access denied for tool '{tool_name}'"
                                )),
                            );
                        }
                    }

                    // Optional input validation using tool definition schema if present
                    if self.config.validate_requests
                        && let Some(arguments) = &call_request.arguments
                    {
                        // Best-effort shape check against ToolInput.properties/required
                        let tool_def = handler.tool_definition();
                        if let Some(props) = tool_def.input_schema.properties.as_ref() {
                            // Build a JSON Schema object dynamically from ToolInput
                            let mut schema = serde_json::json!({
                                "type": "object",
                                "properties": {},
                                "additionalProperties": tool_def.input_schema.additional_properties.unwrap_or(true)
                            });
                            if let Some(obj) =
                                schema.get_mut("properties").and_then(|v| v.as_object_mut())
                            {
                                for (k, v) in props {
                                    obj.insert(k.clone(), v.clone());
                                }
                            }
                            if let Some(required) = tool_def.input_schema.required.as_ref() {
                                schema.as_object_mut().unwrap().insert(
                                    "required".to_string(),
                                    serde_json::Value::Array(
                                        required
                                            .iter()
                                            .map(|s| serde_json::Value::String(s.clone()))
                                            .collect(),
                                    ),
                                );
                            }

                            // Compile and validate
                            if let Ok(compiled) = JSONSchema::options()
                                .with_draft(Draft::Draft7)
                                .compile(&schema)
                            {
                                let instance = serde_json::Value::Object(
                                    arguments.clone().into_iter().collect(),
                                );
                                let mut error_messages: Vec<String> = Vec::new();
                                if let Err(iter) = compiled.validate(&instance) {
                                    for e in iter {
                                        error_messages.push(format!("{}: {}", e.instance_path, e));
                                    }
                                }
                                if !error_messages.is_empty() {
                                    let joined = error_messages.join("; ");
                                    let err = ServerError::routing_with_method(
                                        format!("Argument validation failed: {joined}"),
                                        "tools/call".to_string(),
                                    );
                                    return self.error_response(&request, err);
                                }
                            }
                        }
                    }
                    match handler.handle(call_request, ctx).await {
                        Ok(result) => self.success_response(&request, result),
                        Err(e) => self.error_response(&request, e),
                    }
                } else {
                    let error = ServerError::not_found(format!("Tool '{tool_name}'"));
                    self.error_response(&request, error)
                }
            }
            Err(e) => self.error_response(&request, e),
        }
    }

    async fn handle_list_prompts(
        &self,
        request: JsonRpcRequest,
        _ctx: RequestContext,
    ) -> JsonRpcResponse {
        let prompts = self.registry.get_prompt_definitions();
        let result = ListPromptsResult {
            prompts,
            next_cursor: None,
        };
        self.success_response(&request, result)
    }

    async fn handle_get_prompt(
        &self,
        request: JsonRpcRequest,
        ctx: RequestContext,
    ) -> JsonRpcResponse {
        match self.parse_params::<GetPromptRequest>(&request) {
            Ok(prompt_request) => {
                let prompt_name = &prompt_request.name;

                if let Some(handler) = self.registry.get_prompt(prompt_name) {
                    match handler.handle(prompt_request, ctx).await {
                        Ok(result) => self.success_response(&request, result),
                        Err(e) => self.error_response(&request, e),
                    }
                } else {
                    let error = ServerError::not_found(format!("Prompt '{prompt_name}'"));
                    self.error_response(&request, error)
                }
            }
            Err(e) => self.error_response(&request, e),
        }
    }

    async fn handle_list_resources(
        &self,
        request: JsonRpcRequest,
        _ctx: RequestContext,
    ) -> JsonRpcResponse {
        let resources = self.registry.get_resource_definitions();
        let result = ListResourcesResult {
            resources,
            next_cursor: None,
        };
        self.success_response(&request, result)
    }

    async fn handle_read_resource(
        &self,
        request: JsonRpcRequest,
        ctx: RequestContext,
    ) -> JsonRpcResponse {
        match self.parse_params::<ReadResourceRequest>(&request) {
            Ok(resource_request) => {
                let resource_uri = &resource_request.uri;

                // Find handler by matching URI pattern
                for handler in &self.registry.resources {
                    let resource_def = handler.value().resource_definition();
                    if self.matches_uri_pattern(&resource_def.uri, resource_uri) {
                        match handler.value().handle(resource_request, ctx).await {
                            Ok(result) => return self.success_response(&request, result),
                            Err(e) => return self.error_response(&request, e),
                        }
                    }
                }

                let error = ServerError::not_found(format!("Resource '{resource_uri}'"));
                self.error_response(&request, error)
            }
            Err(e) => self.error_response(&request, e),
        }
    }

    async fn handle_subscribe_resource(
        &self,
        request: JsonRpcRequest,
        _ctx: RequestContext,
    ) -> JsonRpcResponse {
        match self.parse_params::<SubscribeRequest>(&request) {
            Ok(sub) => {
                let uri = sub.uri;
                let new_count_ref = self
                    .resource_subscriptions
                    .entry(uri.clone())
                    .and_modify(|c| *c += 1)
                    .or_insert(1usize);
                let new_count: usize = *new_count_ref;
                tracing::debug!(uri = %uri, count = new_count, "resource subscribed");
                self.success_response(&request, EmptyResult {})
            }
            Err(e) => self.error_response(&request, e),
        }
    }

    async fn handle_unsubscribe_resource(
        &self,
        request: JsonRpcRequest,
        _ctx: RequestContext,
    ) -> JsonRpcResponse {
        match self.parse_params::<UnsubscribeRequest>(&request) {
            Ok(unsub) => {
                let uri = unsub.uri;
                if let Some(mut entry) = self.resource_subscriptions.get_mut(&uri) {
                    let count = entry.value_mut();
                    if *count > 0 {
                        *count -= 1;
                    }
                    if *count == 0 {
                        drop(entry);
                        self.resource_subscriptions.remove(&uri);
                    }
                    tracing::debug!(uri = %uri, "resource unsubscribed");
                }
                self.success_response(&request, EmptyResult {})
            }
            Err(e) => self.error_response(&request, e),
        }
    }

    async fn handle_set_log_level(
        &self,
        request: JsonRpcRequest,
        ctx: RequestContext,
    ) -> JsonRpcResponse {
        match self.parse_params::<SetLevelRequest>(&request) {
            Ok(level_request) => {
                // Use first available logging handler
                if let Some(handler_entry) = self.registry.logging.iter().next() {
                    match handler_entry.value().handle(level_request, ctx).await {
                        Ok(result) => self.success_response(&request, result),
                        Err(e) => self.error_response(&request, e),
                    }
                } else {
                    let error = ServerError::not_found("No logging handler available");
                    self.error_response(&request, error)
                }
            }
            Err(e) => self.error_response(&request, e),
        }
    }

    async fn handle_create_message(
        &self,
        request: JsonRpcRequest,
        ctx: RequestContext,
    ) -> JsonRpcResponse {
        match self.parse_params::<CreateMessageRequest>(&request) {
            Ok(message_request) => {
                // Use first available sampling handler
                if let Some(handler_entry) = self.registry.sampling.iter().next() {
                    match handler_entry.value().handle(message_request, ctx).await {
                        Ok(result) => self.success_response(&request, result),
                        Err(e) => self.error_response(&request, e),
                    }
                } else {
                    let error = ServerError::not_found("No sampling handler available");
                    self.error_response(&request, error)
                }
            }
            Err(e) => self.error_response(&request, e),
        }
    }

    async fn handle_list_roots(
        &self,
        request: JsonRpcRequest,
        _ctx: RequestContext,
    ) -> JsonRpcResponse {
        // Get configured roots from registry
        let roots = self.registry.get_roots();

        // If no roots configured, provide OS-specific defaults
        let roots = if roots.is_empty() {
            let mut default_roots: Vec<Root> = Vec::new();
            #[cfg(target_os = "linux")]
            {
                default_roots.push(Root {
                    uri: "file:///".to_string(),
                    name: Some("root".to_string()),
                });
            }
            #[cfg(target_os = "macos")]
            {
                default_roots.push(Root {
                    uri: "file:///".to_string(),
                    name: Some("root".to_string()),
                });
                default_roots.push(Root {
                    uri: "file:///Volumes".to_string(),
                    name: Some("Volumes".to_string()),
                });
            }
            #[cfg(target_os = "windows")]
            {
                // Common drive letters; clients can probe for availability
                for drive in ['C', 'D', 'E', 'F', 'G', 'H'] {
                    default_roots.push(Root {
                        uri: format!("file:///{}:/", drive),
                        name: Some(format!("{}:", drive)),
                    });
                }
            }
            default_roots
        } else {
            roots
        };

        let result = ListRootsResult { roots };
        self.success_response(&request, result)
    }

    // ========================================================================
    // Enhanced MCP Feature Handlers
    // ========================================================================

    async fn handle_elicitation(
        &self,
        request: JsonRpcRequest,
        _ctx: RequestContext,
    ) -> JsonRpcResponse {
        match self.parse_params::<ElicitRequest>(&request) {
            Ok(_elicit_request) => {
                // Default elicitation handler - returns decline action
                // This should be overridden by applications with proper elicitation handlers
                let result = ElicitResult {
                    action: turbomcp_protocol::types::ElicitationAction::Decline,
                    content: None,
                    _meta: Some(serde_json::json!({
                        "message": "Elicitation not supported - no handler registered"
                    })),
                };
                self.success_response(&request, result)
            }
            Err(e) => self.error_response(&request, e),
        }
    }

    async fn handle_completion(
        &self,
        request: JsonRpcRequest,
        _ctx: RequestContext,
    ) -> JsonRpcResponse {
        match self.parse_params::<CompleteRequestParams>(&request) {
            Ok(_complete_request) => {
                // Default completion handler - returns empty completions
                // This should be overridden by applications with proper completion handlers
                let result = CompletionResponse {
                    values: Vec::new(),
                    has_more: Some(false),
                    total: Some(0),
                };
                self.success_response(&request, result)
            }
            Err(e) => self.error_response(&request, e),
        }
    }

    async fn handle_list_resource_templates(
        &self,
        request: JsonRpcRequest,
        _ctx: RequestContext,
    ) -> JsonRpcResponse {
        match self.parse_params::<ListResourceTemplatesRequest>(&request) {
            Ok(_template_request) => {
                // Default resource template handler - returns empty templates
                // This should be overridden by applications with proper resource template handlers
                let result = ListResourceTemplatesResult {
                    resource_templates: Vec::new(),
                    next_cursor: None,
                    _meta: Some(serde_json::json!({
                        "message": "Resource templates not supported - no handler registered"
                    })),
                };
                self.success_response(&request, result)
            }
            Err(e) => self.error_response(&request, e),
        }
    }

    async fn handle_ping(&self, request: JsonRpcRequest, _ctx: RequestContext) -> JsonRpcResponse {
        match self.parse_params::<PingRequest>(&request) {
            Ok(_ping_request) => {
                // Default ping handler - basic health check response
                let result = PingResult {
                    _meta: Some(serde_json::json!({
                        "status": "healthy",
                        "timestamp": chrono::Utc::now().to_rfc3339(),
                        "server": "turbomcp-server",
                    })),
                };
                self.success_response(&request, result)
            }
            Err(e) => self.error_response(&request, e),
        }
    }

    // ========================================================================
    // Server-Initiated Request Methods (Bidirectional Communication)
    // ========================================================================

    /// Send an elicitation request to the client (server-initiated)
    pub async fn send_elicitation_to_client(
        &self,
        request: ElicitRequest,
        ctx: RequestContext,
    ) -> ServerResult<ElicitResult> {
        if let Some(dispatcher) = &self.server_request_dispatcher {
            dispatcher.send_elicitation(request, ctx).await
        } else {
            Err(ServerError::Handler {
                message: "Server request dispatcher not configured for bidirectional communication"
                    .to_string(),
                context: Some("elicitation".to_string()),
            })
        }
    }

    /// Send a ping request to the client (server-initiated)
    pub async fn send_ping_to_client(
        &self,
        request: PingRequest,
        ctx: RequestContext,
    ) -> ServerResult<PingResult> {
        if let Some(dispatcher) = &self.server_request_dispatcher {
            dispatcher.send_ping(request, ctx).await
        } else {
            Err(ServerError::Handler {
                message: "Server request dispatcher not configured for bidirectional communication"
                    .to_string(),
                context: Some("ping".to_string()),
            })
        }
    }

    /// Send a create message request to the client (server-initiated)
    pub async fn send_create_message_to_client(
        &self,
        request: CreateMessageRequest,
        ctx: RequestContext,
    ) -> ServerResult<turbomcp_protocol::types::CreateMessageResult> {
        if let Some(dispatcher) = &self.server_request_dispatcher {
            dispatcher.send_create_message(request, ctx).await
        } else {
            Err(ServerError::Handler {
                message: "Server request dispatcher not configured for bidirectional communication"
                    .to_string(),
                context: Some("create_message".to_string()),
            })
        }
    }

    /// Send a list roots request to the client (server-initiated)
    pub async fn send_list_roots_to_client(
        &self,
        request: turbomcp_protocol::types::ListRootsRequest,
        ctx: RequestContext,
    ) -> ServerResult<ListRootsResult> {
        if let Some(dispatcher) = &self.server_request_dispatcher {
            dispatcher.send_list_roots(request, ctx).await
        } else {
            Err(ServerError::Handler {
                message: "Server request dispatcher not configured for bidirectional communication"
                    .to_string(),
                context: Some("list_roots".to_string()),
            })
        }
    }

    // Helper methods

    fn get_server_capabilities(&self) -> ServerCapabilities {
        ServerCapabilities {
            tools: if self.registry.tools.is_empty() {
                None
            } else {
                Some(ToolsCapabilities::default())
            },
            prompts: if self.registry.prompts.is_empty() {
                None
            } else {
                Some(PromptsCapabilities::default())
            },
            resources: if self.registry.resources.is_empty() {
                None
            } else {
                Some(ResourcesCapabilities::default())
            },
            logging: if self.registry.logging.is_empty() {
                None
            } else {
                Some(LoggingCapabilities)
            },
            completions: None, // Completion capabilities not enabled by default
            experimental: None,
        }
    }

    fn parse_params<T>(&self, request: &JsonRpcRequest) -> ServerResult<T>
    where
        T: serde::de::DeserializeOwned,
    {
        match &request.params {
            Some(params) => serde_json::from_value(params.clone()).map_err(|e| {
                ServerError::routing_with_method(
                    format!("Invalid parameters: {e}"),
                    request.method.clone(),
                )
            }),
            None => Err(ServerError::routing_with_method(
                "Missing required parameters".to_string(),
                request.method.clone(),
            )),
        }
    }

    fn success_response<T>(&self, request: &JsonRpcRequest, result: T) -> JsonRpcResponse
    where
        T: serde::Serialize,
    {
        JsonRpcResponse::success(serde_json::to_value(result).unwrap(), request.id.clone())
    }

    fn error_response(&self, request: &JsonRpcRequest, error: ServerError) -> JsonRpcResponse {
        JsonRpcResponse::error_response(
            turbomcp_protocol::jsonrpc::JsonRpcError {
                code: error.error_code(),
                message: error.to_string(),
                data: None,
            },
            request.id.clone(),
        )
    }

    fn method_not_found_response(&self, request: &JsonRpcRequest) -> JsonRpcResponse {
        JsonRpcResponse::error_response(
            turbomcp_protocol::jsonrpc::JsonRpcError {
                code: -32601,
                message: format!("Method '{}' not found", request.method),
                data: None,
            },
            request.id.clone(),
        )
    }

    fn validate_request(&self, _request: &JsonRpcRequest) -> ServerResult<()> {
        // Lightweight structural validation using protocol validator
        let validator = turbomcp_protocol::validation::ProtocolValidator::new();
        match validator.validate_request(_request) {
            turbomcp_protocol::validation::ValidationResult::Invalid(errors) => {
                let msg = errors
                    .into_iter()
                    .map(|e| {
                        format!(
                            "{}: {}{}",
                            e.code,
                            e.message,
                            e.field_path
                                .map(|p| format!(" (@ {p})"))
                                .unwrap_or_default()
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("; ");
                Err(ServerError::routing_with_method(
                    format!("Request validation failed: {msg}"),
                    _request.method.clone(),
                ))
            }
            _ => Ok(()),
        }
    }

    fn validate_response(&self, _response: &JsonRpcResponse) -> ServerResult<()> {
        let validator = turbomcp_protocol::validation::ProtocolValidator::new();
        match validator.validate_response(_response) {
            turbomcp_protocol::validation::ValidationResult::Invalid(errors) => {
                let msg = errors
                    .into_iter()
                    .map(|e| {
                        format!(
                            "{}: {}{}",
                            e.code,
                            e.message,
                            e.field_path
                                .map(|p| format!(" (@ {p})"))
                                .unwrap_or_default()
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("; ");
                Err(ServerError::routing(format!(
                    "Response validation failed: {msg}"
                )))
            }
            _ => Ok(()),
        }
    }

    fn matches_uri_pattern(&self, pattern: &str, uri: &str) -> bool {
        // Convert simple templates to regex (very basic):
        // - '*' => '.*'
        // - '{param}' => '[^/]+'
        let mut regex_str = String::from("^");
        let mut chars = pattern.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '*' => regex_str.push_str(".*"),
                '{' => {
                    // consume until '}'
                    for nc in chars.by_ref() {
                        if nc == '}' {
                            break;
                        }
                    }
                    regex_str.push_str("[^/]+");
                }
                '.' | '+' | '?' | '(' | ')' | '|' | '^' | '$' | '[' | ']' | '\\' => {
                    regex_str.push('\\');
                    regex_str.push(c);
                }
                other => regex_str.push(other),
            }
        }
        regex_str.push('$');
        let re = regex::Regex::new(&regex_str).unwrap_or_else(|_| regex::Regex::new("^$").unwrap());
        re.is_match(uri)
    }
}

impl Clone for RequestRouter {
    fn clone(&self) -> Self {
        Self {
            registry: Arc::clone(&self.registry),
            config: self.config.clone(),
            custom_routes: self.custom_routes.clone(),
            resource_subscriptions: DashMap::new(),
            server_request_dispatcher: self.server_request_dispatcher.clone(),
        }
    }
}

// Implementation of ServerCapabilities for RequestRouter
// This enables tools to make server-initiated requests through the context
impl turbomcp_core::ServerCapabilities for RequestRouter {
    fn create_message(
        &self,
        request: serde_json::Value,
    ) -> futures::future::BoxFuture<
        '_,
        Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>>,
    > {
        Box::pin(async move {
            let request: CreateMessageRequest = serde_json::from_value(request)
                .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
            let ctx = turbomcp_core::RequestContext::new();
            let result = self
                .send_create_message_to_client(request, ctx)
                .await
                .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
            serde_json::to_value(result)
                .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
        })
    }

    fn elicit(
        &self,
        request: serde_json::Value,
    ) -> futures::future::BoxFuture<
        '_,
        Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>>,
    > {
        Box::pin(async move {
            let request: ElicitRequest = serde_json::from_value(request)
                .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
            let ctx = turbomcp_core::RequestContext::new();
            let result = self
                .send_elicitation_to_client(request, ctx)
                .await
                .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
            serde_json::to_value(result)
                .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
        })
    }

    fn list_roots(
        &self,
    ) -> futures::future::BoxFuture<
        '_,
        Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>>,
    > {
        Box::pin(async move {
            let ctx = turbomcp_core::RequestContext::new();
            let result = self
                .send_list_roots_to_client(turbomcp_protocol::types::ListRootsRequest {}, ctx)
                .await
                .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
            serde_json::to_value(result)
                .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
        })
    }
}

/// Route definition for custom routing
#[derive(Clone)]
pub struct Route {
    /// Route method pattern
    pub method: String,
    /// Route handler
    pub handler: Arc<dyn RouteHandler>,
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

/// Router alias for convenience
pub type Router = RequestRouter;
