//! Streamable HTTP transport for WASM MCP servers.
//!
//! This module implements the MCP 2025-11-25 Streamable HTTP transport specification
//! for Cloudflare Workers and other edge runtimes.
//!
//! ## Features
//!
//! - **GET**: Establish SSE stream for server-initiated messages
//! - **POST**: Send JSON-RPC request, receive JSON or SSE response
//! - **DELETE**: Terminate session
//! - **Session Management**: `Mcp-Session-Id` header tracking
//! - **Message Replay**: `Last-Event-ID` support for resumability
//! - **Origin Validation**: DNS rebinding protection
//!
//! ## Security
//!
//! ### CORS Handling
//!
//! This handler implements secure CORS handling by echoing the request `Origin`
//! header instead of using a wildcard (`*`):
//!
//! - Echoes the request origin for browser clients
//! - Falls back to `*` only for non-browser clients (no Origin header)
//! - Adds `Vary: Origin` header when origin is specified (required for caching)
//!
//! ### Origin Validation
//!
//! Configurable origin validation protects against DNS rebinding attacks.
//! Use `StreamableConfig` to specify allowed origins for production deployments.
//!
//! ## Example
//!
//! ```ignore
//! use turbomcp_wasm::wasm_server::*;
//! use turbomcp_wasm::wasm_server::streamable::*;
//!
//! let server = McpServer::builder("my-server", "1.0.0")
//!     .tool("hello", "Say hello", hello_handler)
//!     .build();
//!
//! let streamable = StreamableHandler::new(server)
//!     .with_session_store(MemorySessionStore::new())
//!     .with_config(StreamableConfig::production());
//!
//! // In your Worker fetch handler:
//! streamable.handle(req).await
//! ```

#[cfg(target_arch = "wasm32")]
use std::cell::RefCell;
use std::collections::HashMap;
#[cfg(target_arch = "wasm32")]
use std::rc::Rc;
use std::sync::Arc;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Mutex;

use serde_json::Value;
use turbomcp_transport_streamable::{
    HttpMethod, OriginValidation, Session, SessionId, SessionStore, SseEncoder, SseEvent,
    StoredEvent, StreamableConfig, StreamableError, StreamableRequest, StreamableResponse, headers,
};
use worker::{Headers, Request, Response};

use super::context::RequestContext;
use super::server::{McpServer, PromptHandlerKind, ResourceHandlerKind, ToolHandlerKind};
use super::types::{JsonRpcRequest, JsonRpcResponse, error_codes};

/// In-memory session store for WASM environments (single-threaded).
///
/// This store is suitable for development and testing in WASM Workers,
/// but sessions will not persist across Worker restarts or be shared
/// between Worker instances.
///
/// For production use with session persistence, use `KvSessionStore`
/// or `DurableObjectSessionStore`.
#[cfg(target_arch = "wasm32")]
#[derive(Clone)]
pub struct MemorySessionStore {
    sessions: Rc<RefCell<HashMap<String, Session>>>,
    events: Rc<RefCell<HashMap<String, Vec<StoredEvent>>>>,
}

#[cfg(target_arch = "wasm32")]
impl MemorySessionStore {
    /// Create a new in-memory session store.
    pub fn new() -> Self {
        Self {
            sessions: Rc::new(RefCell::new(HashMap::new())),
            events: Rc::new(RefCell::new(HashMap::new())),
        }
    }
}

#[cfg(target_arch = "wasm32")]
impl Default for MemorySessionStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_arch = "wasm32")]
impl SessionStore for MemorySessionStore {
    type Error = std::convert::Infallible;

    async fn create(&self) -> Result<SessionId, Self::Error> {
        let id = SessionId::new();
        let session = Session::new(id.clone());
        self.sessions
            .borrow_mut()
            .insert(id.as_str().to_string(), session);
        self.events
            .borrow_mut()
            .insert(id.as_str().to_string(), Vec::new());
        Ok(id)
    }

    async fn get(&self, id: &SessionId) -> Result<Option<Session>, Self::Error> {
        Ok(self.sessions.borrow().get(id.as_str()).cloned())
    }

    async fn update(&self, session: &Session) -> Result<(), Self::Error> {
        self.sessions
            .borrow_mut()
            .insert(session.id.as_str().to_string(), session.clone());
        Ok(())
    }

    async fn store_event(&self, id: &SessionId, event: StoredEvent) -> Result<(), Self::Error> {
        if let Some(events) = self.events.borrow_mut().get_mut(id.as_str()) {
            events.push(event);
        }
        Ok(())
    }

    async fn replay_from(
        &self,
        id: &SessionId,
        last_event_id: &str,
    ) -> Result<Vec<StoredEvent>, Self::Error> {
        let events = self.events.borrow();
        let session_events = match events.get(id.as_str()) {
            Some(e) => e,
            None => return Ok(Vec::new()),
        };

        let start_index = session_events
            .iter()
            .position(|e| e.id == last_event_id)
            .map(|i| i + 1)
            .unwrap_or(0);

        Ok(session_events[start_index..].to_vec())
    }

    async fn destroy(&self, id: &SessionId) -> Result<(), Self::Error> {
        self.sessions.borrow_mut().remove(id.as_str());
        self.events.borrow_mut().remove(id.as_str());
        Ok(())
    }
}

/// In-memory session store for native environments (thread-safe).
///
/// This version uses `Arc<Mutex>` for thread-safety on native targets.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone)]
pub struct MemorySessionStore {
    sessions: Arc<Mutex<HashMap<String, Session>>>,
    events: Arc<Mutex<HashMap<String, Vec<StoredEvent>>>>,
}

#[cfg(not(target_arch = "wasm32"))]
impl MemorySessionStore {
    /// Create a new in-memory session store.
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            events: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Default for MemorySessionStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl SessionStore for MemorySessionStore {
    type Error = std::convert::Infallible;

    async fn create(&self) -> Result<SessionId, Self::Error> {
        let id = SessionId::new();
        let session = Session::new(id.clone());
        self.sessions
            .lock()
            .unwrap()
            .insert(id.as_str().to_string(), session);
        self.events
            .lock()
            .unwrap()
            .insert(id.as_str().to_string(), Vec::new());
        Ok(id)
    }

    async fn get(&self, id: &SessionId) -> Result<Option<Session>, Self::Error> {
        Ok(self.sessions.lock().unwrap().get(id.as_str()).cloned())
    }

    async fn update(&self, session: &Session) -> Result<(), Self::Error> {
        self.sessions
            .lock()
            .unwrap()
            .insert(session.id.as_str().to_string(), session.clone());
        Ok(())
    }

    async fn store_event(&self, id: &SessionId, event: StoredEvent) -> Result<(), Self::Error> {
        if let Some(events) = self.events.lock().unwrap().get_mut(id.as_str()) {
            events.push(event);
        }
        Ok(())
    }

    async fn replay_from(
        &self,
        id: &SessionId,
        last_event_id: &str,
    ) -> Result<Vec<StoredEvent>, Self::Error> {
        let events = self.events.lock().unwrap();
        let session_events = match events.get(id.as_str()) {
            Some(e) => e,
            None => return Ok(Vec::new()),
        };

        let start_index = session_events
            .iter()
            .position(|e| e.id == last_event_id)
            .map(|i| i + 1)
            .unwrap_or(0);

        Ok(session_events[start_index..].to_vec())
    }

    async fn destroy(&self, id: &SessionId) -> Result<(), Self::Error> {
        self.sessions.lock().unwrap().remove(id.as_str());
        self.events.lock().unwrap().remove(id.as_str());
        Ok(())
    }
}

/// Streamable HTTP handler for MCP servers.
///
/// Wraps an `McpServer` and provides full Streamable HTTP transport support
/// including GET/POST/DELETE methods, SSE streaming, session management,
/// and message replay.
pub struct StreamableHandler<S: SessionStore = MemorySessionStore> {
    server: McpServer,
    session_store: S,
    config: StreamableConfig,
    #[cfg(target_arch = "wasm32")]
    event_sequence: RefCell<u64>,
    #[cfg(not(target_arch = "wasm32"))]
    event_sequence: std::sync::atomic::AtomicU64,
}

impl StreamableHandler<MemorySessionStore> {
    /// Create a new streamable handler with in-memory session storage.
    pub fn new(server: McpServer) -> Self {
        Self {
            server,
            session_store: MemorySessionStore::new(),
            config: StreamableConfig::default(),
            #[cfg(target_arch = "wasm32")]
            event_sequence: RefCell::new(0),
            #[cfg(not(target_arch = "wasm32"))]
            event_sequence: std::sync::atomic::AtomicU64::new(0),
        }
    }
}

impl<S: SessionStore> StreamableHandler<S> {
    /// Create a new streamable handler with a custom session store.
    pub fn with_session_store<NewS: SessionStore>(
        self,
        session_store: NewS,
    ) -> StreamableHandler<NewS> {
        StreamableHandler {
            server: self.server,
            session_store,
            config: self.config,
            #[cfg(target_arch = "wasm32")]
            event_sequence: RefCell::new(0),
            #[cfg(not(target_arch = "wasm32"))]
            event_sequence: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// Set the configuration.
    pub fn with_config(mut self, config: StreamableConfig) -> Self {
        self.config = config;
        self
    }

    /// Handle an incoming HTTP request.
    ///
    /// Routes to the appropriate handler based on HTTP method:
    /// - GET: Establish SSE stream
    /// - POST: Handle JSON-RPC request
    /// - DELETE: Terminate session
    /// - OPTIONS: CORS preflight
    pub async fn handle(&self, req: Request) -> worker::Result<Response> {
        // SECURITY: Check Content-Length header BEFORE reading body to prevent DoS.
        // This prevents attackers from exhausting memory with large request bodies.
        if req.method() == worker::Method::Post
            && let Some(content_length) = req.headers().get("content-length").ok().flatten()
            && let Ok(length) = content_length.parse::<usize>()
            && length > self.config.max_body_size
        {
            let resp = StreamableResponse::from(StreamableError::BodyTooLarge {
                size: length,
                max: self.config.max_body_size,
            });
            // Early return before origin is parsed - use None (defaults to *)
            return self.build_response(resp, None);
        }

        // Parse the request
        let streamable_req = self.parse_request(&req).await?;
        let request_origin = streamable_req.origin.as_deref();

        // Validate origin if configured
        let origin_validation =
            OriginValidation::validate(request_origin, &self.config.allowed_origins);

        if !origin_validation.passed(self.config.require_origin) {
            let resp = match origin_validation {
                OriginValidation::Missing => {
                    StreamableResponse::forbidden("Origin header required")
                }
                OriginValidation::Invalid(o) => {
                    StreamableResponse::forbidden(format!("Origin not allowed: {o}"))
                }
                OriginValidation::Valid => unreachable!(),
            };
            // For error responses, still include proper CORS headers using the request origin
            return self.build_response(resp, request_origin);
        }

        // Route based on HTTP method
        let response = match streamable_req.method {
            HttpMethod::Get => self.handle_get(&streamable_req).await,
            HttpMethod::Post => self.handle_post(&streamable_req).await,
            HttpMethod::Delete => self.handle_delete(&streamable_req).await,
            HttpMethod::Options => return self.cors_preflight_response(request_origin),
        };

        self.build_response(response, request_origin)
    }

    /// Parse an incoming Worker request into a StreamableRequest.
    async fn parse_request(&self, req: &Request) -> worker::Result<StreamableRequest> {
        let method = match req.method() {
            worker::Method::Get => HttpMethod::Get,
            worker::Method::Post => HttpMethod::Post,
            worker::Method::Delete => HttpMethod::Delete,
            worker::Method::Options => HttpMethod::Options,
            _ => {
                return Ok(StreamableRequest::default());
            }
        };

        let worker_headers = req.headers();

        let session_id = worker_headers.get(headers::MCP_SESSION_ID).ok().flatten();

        let last_event_id = worker_headers.get(headers::LAST_EVENT_ID).ok().flatten();

        let origin = worker_headers.get("Origin").ok().flatten();

        let accept = worker_headers.get("Accept").ok().flatten();

        // Extract common headers for context
        let mut extracted_headers = HashMap::new();
        for key in [
            "authorization",
            "content-type",
            "user-agent",
            "x-request-id",
            "x-session-id",
            "x-client-id",
            "mcp-session-id",
            "origin",
            "referer",
        ] {
            if let Ok(Some(value)) = worker_headers.get(key) {
                extracted_headers.insert(key.to_string(), value);
            }
        }

        // Body is only read for POST requests
        let body = if method == HttpMethod::Post {
            // Clone the request to read body (Request is consumed on text())
            let mut req_clone = req.clone()?;
            req_clone.text().await.ok()
        } else {
            None
        };

        Ok(StreamableRequest {
            method,
            session_id,
            last_event_id,
            origin,
            accept,
            body,
            headers: extracted_headers,
        })
    }

    /// Handle GET request (establish SSE stream).
    async fn handle_get(&self, req: &StreamableRequest) -> StreamableResponse {
        // GET requires a session ID
        let session_id = match &req.session_id {
            Some(id) => SessionId::from_string(id.clone()),
            None => {
                return StreamableResponse::bad_request("Mcp-Session-Id header required for GET");
            }
        };

        // Verify session exists
        let session = match self.session_store.get(&session_id).await {
            Ok(Some(s)) => s,
            Ok(None) => {
                return StreamableResponse::from(StreamableError::SessionNotFound(
                    session_id.into_string(),
                ));
            }
            Err(_) => return StreamableResponse::internal_error("Session store error"),
        };

        if !session.can_accept_requests() {
            return StreamableResponse::from(StreamableError::SessionTerminated(
                session_id.into_string(),
            ));
        }

        // Handle replay if Last-Event-ID is present
        let replay_events = if let Some(last_event_id) = &req.last_event_id {
            match self
                .session_store
                .replay_from(&session_id, last_event_id)
                .await
            {
                Ok(events) => events
                    .into_iter()
                    .map(|e| SseEncoder::encode_string(&SseEvent::with_id(e.id, e.data)))
                    .collect(),
                Err(_) => Vec::new(),
            }
        } else {
            Vec::new()
        };

        StreamableResponse::sse_with_replay(session_id.into_string(), replay_events)
    }

    /// Handle POST request (JSON-RPC).
    async fn handle_post(&self, req: &StreamableRequest) -> StreamableResponse {
        // Check body size
        let body = match &req.body {
            Some(b) if b.len() > self.config.max_body_size => {
                return StreamableResponse::from(StreamableError::BodyTooLarge {
                    size: b.len(),
                    max: self.config.max_body_size,
                });
            }
            Some(b) if b.is_empty() => {
                return StreamableResponse::bad_request("Empty request body");
            }
            Some(b) => b,
            None => return StreamableResponse::bad_request("Missing request body"),
        };

        // Parse JSON-RPC request
        let rpc_request: JsonRpcRequest = match serde_json::from_str(body) {
            Ok(r) => r,
            Err(e) => {
                let response = JsonRpcResponse::error(
                    None,
                    error_codes::PARSE_ERROR,
                    format!("Parse error: {e}"),
                );
                // SAFETY: JsonRpcResponse with simple error string is always serializable
                let json = serde_json::to_string(&response).unwrap_or_else(|_| {
                    r#"{"jsonrpc":"2.0","error":{"code":-32700,"message":"Parse error"}}"#
                        .to_string()
                });
                return StreamableResponse::json(json);
            }
        };

        // Handle session creation for initialize
        let (session_id, is_new_session) = if rpc_request.method == "initialize" {
            // Create new session for initialize requests
            match self.session_store.create().await {
                Ok(id) => (Some(id), true),
                Err(_) => return StreamableResponse::internal_error("Failed to create session"),
            }
        } else if let Some(id) = &req.session_id {
            // Use existing session
            let session_id = SessionId::from_string(id.clone());
            match self.session_store.get(&session_id).await {
                Ok(Some(s)) if s.can_accept_requests() => (Some(session_id), false),
                Ok(Some(_)) => {
                    return StreamableResponse::from(StreamableError::SessionTerminated(
                        id.clone(),
                    ));
                }
                Ok(None) => {
                    return StreamableResponse::from(StreamableError::SessionNotFound(id.clone()));
                }
                Err(_) => return StreamableResponse::internal_error("Session store error"),
            }
        } else {
            // No session - stateless request
            (None, false)
        };

        // Route the request with headers for context
        let response = self.route_request(&rpc_request, &req.headers).await;

        // If this was an initialize request and it succeeded, activate the session
        if is_new_session
            && response.error.is_none()
            && let Some(ref sid) = session_id
            && let Ok(Some(mut session)) = self.session_store.get(sid).await
        {
            session.activate();
            let _ = self.session_store.update(&session).await;
        }

        // Build response with session ID if present
        let json = match serde_json::to_string(&response) {
            Ok(j) => j,
            Err(_) => return StreamableResponse::internal_error("Failed to serialize response"),
        };
        match session_id {
            Some(id) => StreamableResponse::json_with_session(json, id.into_string()),
            None => StreamableResponse::json(json),
        }
    }

    /// Handle DELETE request (terminate session).
    async fn handle_delete(&self, req: &StreamableRequest) -> StreamableResponse {
        // DELETE requires a session ID
        let session_id = match &req.session_id {
            Some(id) => SessionId::from_string(id.clone()),
            None => {
                return StreamableResponse::bad_request(
                    "Mcp-Session-Id header required for DELETE",
                );
            }
        };

        // Verify session exists
        match self.session_store.get(&session_id).await {
            Ok(Some(mut session)) => {
                session.terminate();
                let _ = self.session_store.update(&session).await;
            }
            Ok(None) => {
                return StreamableResponse::from(StreamableError::SessionNotFound(
                    session_id.into_string(),
                ));
            }
            Err(_) => return StreamableResponse::internal_error("Session store error"),
        }

        // Optionally destroy the session completely
        let _ = self.session_store.destroy(&session_id).await;

        StreamableResponse::empty()
    }

    /// Route a JSON-RPC request to the appropriate handler.
    ///
    /// The `headers` parameter contains extracted request headers for context injection.
    async fn route_request(
        &self,
        req: &JsonRpcRequest,
        headers: &HashMap<String, String>,
    ) -> JsonRpcResponse {
        match req.method.as_str() {
            "initialize" => self.handle_initialize(req),
            "notifications/initialized" => {
                JsonRpcResponse::success(req.id.clone(), serde_json::json!({}))
            }
            "ping" => JsonRpcResponse::success(req.id.clone(), serde_json::json!({})),
            "tools/list" => self.handle_tools_list(req),
            "tools/call" => self.handle_tools_call(req, headers).await,
            "resources/list" => self.handle_resources_list(req),
            "resources/templates/list" => self.handle_resource_templates_list(req),
            "resources/read" => self.handle_resources_read(req, headers).await,
            "prompts/list" => self.handle_prompts_list(req),
            "prompts/get" => self.handle_prompts_get(req, headers).await,
            "logging/setLevel" => JsonRpcResponse::success(req.id.clone(), serde_json::json!({})),
            _ => JsonRpcResponse::error(
                req.id.clone(),
                error_codes::METHOD_NOT_FOUND,
                format!("Method not found: {}", req.method),
            ),
        }
    }

    /// Create a RequestContext from extracted headers.
    ///
    /// Extracts session ID, request ID, and other context from HTTP headers.
    fn create_context_from_headers(headers: &HashMap<String, String>) -> RequestContext {
        // Extract session ID from headers
        let session_id = headers
            .get("mcp-session-id")
            .or_else(|| headers.get("x-session-id"))
            .cloned();

        // Extract request ID from headers
        let request_id = headers.get("x-request-id").cloned();

        RequestContext::from_worker_request(request_id, session_id, headers.clone())
    }

    /// Handle initialize request.
    fn handle_initialize(&self, req: &JsonRpcRequest) -> JsonRpcResponse {
        use turbomcp_core::PROTOCOL_VERSION;
        use turbomcp_core::types::initialization::InitializeResult;

        let result = InitializeResult {
            protocol_version: PROTOCOL_VERSION.into(),
            capabilities: self.server.capabilities.clone(),
            server_info: self.server.server_info.clone(),
            instructions: self.server.instructions.clone(),
            _meta: None,
        };

        match serde_json::to_value(&result) {
            Ok(value) => JsonRpcResponse::success(req.id.clone(), value),
            Err(e) => JsonRpcResponse::error(
                req.id.clone(),
                error_codes::INTERNAL_ERROR,
                format!("Failed to serialize result: {e}"),
            ),
        }
    }

    /// Handle tools/list request.
    fn handle_tools_list(&self, req: &JsonRpcRequest) -> JsonRpcResponse {
        let tools: Vec<_> = self.server.tools.values().map(|r| &r.tool).collect();
        let result = serde_json::json!({
            "tools": tools
        });
        JsonRpcResponse::success(req.id.clone(), result)
    }

    /// Handle tools/call request.
    async fn handle_tools_call(
        &self,
        req: &JsonRpcRequest,
        headers: &HashMap<String, String>,
    ) -> JsonRpcResponse {
        #[derive(serde::Deserialize)]
        struct CallToolParams {
            name: String,
            #[serde(default)]
            arguments: Option<Value>,
        }

        let params: CallToolParams = match req.params.as_ref() {
            Some(p) => match serde_json::from_value(p.clone()) {
                Ok(params) => params,
                Err(e) => {
                    return JsonRpcResponse::error(
                        req.id.clone(),
                        error_codes::INVALID_PARAMS,
                        format!("Invalid params: {e}"),
                    );
                }
            },
            None => {
                return JsonRpcResponse::error(
                    req.id.clone(),
                    error_codes::INVALID_PARAMS,
                    "Missing params: expected {name, arguments?}",
                );
            }
        };

        let registered_tool = match self.server.tools.get(&params.name) {
            Some(tool) => tool,
            None => {
                return JsonRpcResponse::error(
                    req.id.clone(),
                    error_codes::METHOD_NOT_FOUND,
                    format!("Tool not found: {}", params.name),
                );
            }
        };

        let args = params.arguments.unwrap_or(serde_json::json!({}));

        // Create a context from request headers
        let ctx = Arc::new(Self::create_context_from_headers(headers));

        // Dispatch to handler based on whether it needs context
        let tool_result = match &registered_tool.handler {
            ToolHandlerKind::NoCtx(handler) => handler(args).await,
            ToolHandlerKind::WithCtx(handler) => handler(ctx, args).await,
        };

        match serde_json::to_value(&tool_result) {
            Ok(value) => JsonRpcResponse::success(req.id.clone(), value),
            Err(e) => JsonRpcResponse::error(
                req.id.clone(),
                error_codes::INTERNAL_ERROR,
                format!("Failed to serialize result: {e}"),
            ),
        }
    }

    /// Handle resources/list request.
    fn handle_resources_list(&self, req: &JsonRpcRequest) -> JsonRpcResponse {
        let resources: Vec<_> = self
            .server
            .resources
            .values()
            .map(|r| &r.resource)
            .collect();
        let result = serde_json::json!({
            "resources": resources
        });
        JsonRpcResponse::success(req.id.clone(), result)
    }

    /// Handle resources/templates/list request.
    fn handle_resource_templates_list(&self, req: &JsonRpcRequest) -> JsonRpcResponse {
        let templates: Vec<_> = self
            .server
            .resource_templates
            .values()
            .map(|r| &r.template)
            .collect();
        let result = serde_json::json!({
            "resourceTemplates": templates
        });
        JsonRpcResponse::success(req.id.clone(), result)
    }

    /// Handle resources/read request.
    async fn handle_resources_read(
        &self,
        req: &JsonRpcRequest,
        headers: &HashMap<String, String>,
    ) -> JsonRpcResponse {
        #[derive(serde::Deserialize)]
        struct ReadResourceParams {
            uri: String,
        }

        let params: ReadResourceParams = match req.params.as_ref() {
            Some(p) => match serde_json::from_value(p.clone()) {
                Ok(params) => params,
                Err(e) => {
                    return JsonRpcResponse::error(
                        req.id.clone(),
                        error_codes::INVALID_PARAMS,
                        format!("Invalid params: {e}"),
                    );
                }
            },
            None => {
                return JsonRpcResponse::error(
                    req.id.clone(),
                    error_codes::INVALID_PARAMS,
                    "Missing params: expected {uri}",
                );
            }
        };

        // Create a context from request headers
        let ctx = Arc::new(Self::create_context_from_headers(headers));

        // Try exact match first
        if let Some(registered_resource) = self.server.resources.get(&params.uri) {
            let result = match &registered_resource.handler {
                ResourceHandlerKind::NoCtx(handler) => handler(params.uri.clone()).await,
                ResourceHandlerKind::WithCtx(handler) => {
                    handler(ctx.clone(), params.uri.clone()).await
                }
            };
            return match result {
                Ok(resource_result) => match serde_json::to_value(&resource_result) {
                    Ok(value) => JsonRpcResponse::success(req.id.clone(), value),
                    Err(e) => JsonRpcResponse::error(
                        req.id.clone(),
                        error_codes::INTERNAL_ERROR,
                        format!("Failed to serialize result: {e}"),
                    ),
                },
                Err(e) => JsonRpcResponse::error(req.id.clone(), error_codes::INTERNAL_ERROR, e),
            };
        }

        // Try template matching
        for (template_uri, registered_template) in &self.server.resource_templates {
            if self.matches_template(template_uri, &params.uri) {
                let result = match &registered_template.handler {
                    ResourceHandlerKind::NoCtx(handler) => handler(params.uri.clone()).await,
                    ResourceHandlerKind::WithCtx(handler) => {
                        handler(ctx.clone(), params.uri.clone()).await
                    }
                };
                return match result {
                    Ok(resource_result) => match serde_json::to_value(&resource_result) {
                        Ok(value) => JsonRpcResponse::success(req.id.clone(), value),
                        Err(e) => JsonRpcResponse::error(
                            req.id.clone(),
                            error_codes::INTERNAL_ERROR,
                            format!("Failed to serialize result: {e}"),
                        ),
                    },
                    Err(e) => {
                        JsonRpcResponse::error(req.id.clone(), error_codes::INTERNAL_ERROR, e)
                    }
                };
            }
        }

        JsonRpcResponse::error(
            req.id.clone(),
            error_codes::INVALID_PARAMS,
            format!("Resource not found: {}", params.uri),
        )
    }

    /// Handle prompts/list request.
    fn handle_prompts_list(&self, req: &JsonRpcRequest) -> JsonRpcResponse {
        let prompts: Vec<_> = self.server.prompts.values().map(|r| &r.prompt).collect();
        let result = serde_json::json!({
            "prompts": prompts
        });
        JsonRpcResponse::success(req.id.clone(), result)
    }

    /// Handle prompts/get request.
    async fn handle_prompts_get(
        &self,
        req: &JsonRpcRequest,
        headers: &HashMap<String, String>,
    ) -> JsonRpcResponse {
        #[derive(serde::Deserialize)]
        struct GetPromptParams {
            name: String,
            #[serde(default)]
            arguments: Option<Value>,
        }

        let params: GetPromptParams = match req.params.as_ref() {
            Some(p) => match serde_json::from_value(p.clone()) {
                Ok(params) => params,
                Err(e) => {
                    return JsonRpcResponse::error(
                        req.id.clone(),
                        error_codes::INVALID_PARAMS,
                        format!("Invalid params: {e}"),
                    );
                }
            },
            None => {
                return JsonRpcResponse::error(
                    req.id.clone(),
                    error_codes::INVALID_PARAMS,
                    "Missing params: expected {name, arguments?}",
                );
            }
        };

        let registered_prompt = match self.server.prompts.get(&params.name) {
            Some(prompt) => prompt,
            None => {
                return JsonRpcResponse::error(
                    req.id.clone(),
                    error_codes::INVALID_PARAMS,
                    format!("Prompt not found: {}", params.name),
                );
            }
        };

        // Create a context from request headers
        let ctx = Arc::new(Self::create_context_from_headers(headers));

        // Dispatch to handler based on whether it needs context
        let result = match &registered_prompt.handler {
            PromptHandlerKind::NoCtx(handler) => handler(params.arguments).await,
            PromptHandlerKind::WithCtx(handler) => handler(ctx, params.arguments).await,
        };

        match result {
            Ok(prompt_result) => match serde_json::to_value(&prompt_result) {
                Ok(value) => JsonRpcResponse::success(req.id.clone(), value),
                Err(e) => JsonRpcResponse::error(
                    req.id.clone(),
                    error_codes::INTERNAL_ERROR,
                    format!("Failed to serialize result: {e}"),
                ),
            },
            Err(e) => JsonRpcResponse::error(req.id.clone(), error_codes::INTERNAL_ERROR, e),
        }
    }

    /// Simple template matching for resource URIs.
    fn matches_template(&self, template: &str, uri: &str) -> bool {
        let template_parts: Vec<&str> = template.split('/').collect();
        let uri_parts: Vec<&str> = uri.split('/').collect();

        if template_parts.len() != uri_parts.len() {
            return false;
        }

        for (t, u) in template_parts.iter().zip(uri_parts.iter()) {
            if t.starts_with('{') && t.ends_with('}') {
                if u.is_empty() {
                    return false;
                }
                continue;
            }
            if t != u {
                return false;
            }
        }

        true
    }

    /// Build a Worker Response from a StreamableResponse.
    ///
    /// The `origin` parameter is the validated Origin header from the request,
    /// used to set the correct CORS `Access-Control-Allow-Origin` header.
    fn build_response(
        &self,
        resp: StreamableResponse,
        origin: Option<&str>,
    ) -> worker::Result<Response> {
        match resp {
            StreamableResponse::Json {
                status,
                session_id,
                body,
            } => {
                let headers = self.response_headers(
                    session_id.as_deref(),
                    headers::CONTENT_TYPE_JSON,
                    origin,
                );
                let response = Response::ok(body)?
                    .with_status(status)
                    .with_headers(headers);
                Ok(response)
            }
            StreamableResponse::Sse {
                session_id,
                initial_events,
            } => {
                // Note: Full SSE streaming requires different handling in Workers
                // This creates an initial response with replay events
                let headers =
                    self.response_headers(session_id.as_deref(), headers::CONTENT_TYPE_SSE, origin);
                let _ = headers.set("Cache-Control", "no-cache");
                let _ = headers.set("Connection", "keep-alive");

                let body = initial_events.join("");
                let response = Response::ok(body)?.with_headers(headers);
                Ok(response)
            }
            StreamableResponse::Empty { status } => {
                let headers = self.response_headers(None, headers::CONTENT_TYPE_JSON, origin);
                Response::empty().map(|r| r.with_status(status).with_headers(headers))
            }
            StreamableResponse::Error { status, message } => {
                let headers = self.response_headers(None, headers::CONTENT_TYPE_JSON, origin);
                let body = serde_json::json!({
                    "error": message
                });
                Response::ok(body.to_string()).map(|r| r.with_status(status).with_headers(headers))
            }
        }
    }

    /// Create response headers with CORS and optional session ID.
    ///
    /// The `origin` parameter is the validated Origin header from the request.
    /// If `allowed_origins` is configured and the origin is valid, we echo it back.
    /// If `allowed_origins` is empty (allow all), we use `*`.
    fn response_headers(
        &self,
        session_id: Option<&str>,
        content_type: &str,
        origin: Option<&str>,
    ) -> Headers {
        let headers = Headers::new();
        // Use the validated origin if specific origins are configured, otherwise use wildcard
        let cors_origin = if self.config.allowed_origins.is_empty() {
            "*".to_string()
        } else {
            origin.unwrap_or("*").to_string()
        };
        let _ = headers.set("Access-Control-Allow-Origin", &cors_origin);
        let _ = headers.set("Access-Control-Allow-Methods", "GET, POST, DELETE, OPTIONS");
        let _ = headers.set(
            "Access-Control-Allow-Headers",
            "Content-Type, Authorization, X-Request-ID, Mcp-Session-Id, Last-Event-ID",
        );
        let _ = headers.set("Access-Control-Expose-Headers", "Mcp-Session-Id");
        let _ = headers.set("Access-Control-Max-Age", "86400");
        let _ = headers.set("Content-Type", content_type);

        if let Some(id) = session_id {
            let _ = headers.set(headers::MCP_SESSION_ID, id);
        }

        headers
    }

    /// Create a CORS preflight response.
    fn cors_preflight_response(&self, origin: Option<&str>) -> worker::Result<Response> {
        let headers = self.response_headers(None, headers::CONTENT_TYPE_JSON, origin);
        Response::empty().map(|r| r.with_status(204).with_headers(headers))
    }

    /// Store an event for replay support.
    ///
    /// Call this when sending server-initiated messages to enable
    /// client reconnection with `Last-Event-ID`.
    pub async fn store_event(&self, session_id: &SessionId, data: &str) -> Option<String> {
        #[cfg(target_arch = "wasm32")]
        let seq = {
            let mut seq = self.event_sequence.borrow_mut();
            *seq += 1;
            *seq
        };
        #[cfg(not(target_arch = "wasm32"))]
        let seq = self
            .event_sequence
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1;

        let event_id = turbomcp_transport_streamable::sse::generate_event_id(seq);

        let event = StoredEvent::new(event_id.clone(), data);
        if self
            .session_store
            .store_event(session_id, event)
            .await
            .is_ok()
        {
            Some(event_id)
        } else {
            None
        }
    }
}

/// Extension trait to add streamable HTTP support to `McpServer`.
pub trait StreamableExt {
    /// Convert this server into a streamable HTTP handler.
    fn into_streamable(self) -> StreamableHandler<MemorySessionStore>;
}

impl StreamableExt for McpServer {
    fn into_streamable(self) -> StreamableHandler<MemorySessionStore> {
        StreamableHandler::new(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use turbomcp_transport_streamable::SessionState;

    #[tokio::test]
    async fn test_memory_session_store() {
        let store = MemorySessionStore::new();

        // Create a session
        let id = store.create().await.unwrap();
        assert!(id.as_str().starts_with("mcp-"));

        // Get the session
        let session = store.get(&id).await.unwrap().unwrap();
        assert_eq!(session.state, SessionState::Pending);

        // Update the session
        let mut session = session;
        session.activate();
        store.update(&session).await.unwrap();

        let updated = store.get(&id).await.unwrap().unwrap();
        assert_eq!(updated.state, SessionState::Active);

        // Store and replay events
        let event1 = StoredEvent::new("evt-1", "data1");
        let event2 = StoredEvent::new("evt-2", "data2");
        store.store_event(&id, event1).await.unwrap();
        store.store_event(&id, event2).await.unwrap();

        let replayed = store.replay_from(&id, "evt-1").await.unwrap();
        assert_eq!(replayed.len(), 1);
        assert_eq!(replayed[0].id, "evt-2");

        // Destroy the session
        store.destroy(&id).await.unwrap();
        assert!(store.get(&id).await.unwrap().is_none());
    }
}
