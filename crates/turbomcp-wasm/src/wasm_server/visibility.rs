//! Progressive disclosure through component visibility control.
//!
//! This module provides the ability to dynamically show/hide tools, resources,
//! and prompts based on tags. This enables patterns like:
//!
//! - Hiding admin tools until explicitly unlocked
//! - Progressive disclosure of advanced features
//! - Role-based component visibility
//!
//! # Security
//!
//! The visibility layer includes secure CORS handling:
//!
//! - Echoes the request `Origin` header instead of using wildcard `*`
//! - Adds `Vary: Origin` header for proper caching behavior
//! - Falls back to `*` only for non-browser clients (no Origin header)
//!
//! # Example
//!
//! ```ignore
//! use turbomcp_wasm::wasm_server::{McpServer, VisibilityLayer};
//!
//! // Create a server
//! let server = McpServer::builder("my-server", "1.0.0")
//!     .tool("public_tool", "Public tool", public_handler)
//!     .tool("admin_tool", "Admin tool", admin_handler) // tagged with "admin"
//!     .build();
//!
//! // Create a visibility layer that hides admin tools by default
//! let layer = VisibilityLayer::new(server)
//!     .disable_tags(["admin"]);
//!
//! // Enable admin tools for a specific session
//! layer.enable_for_session("session123", &["admin".to_string()]);
//!
//! // Handle requests through the layer
//! layer.handle(request).await
//! ```

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

use worker::{Headers, Request, Response};

use super::context::RequestContext;
use super::server::McpServer;
use super::types::{JsonRpcRequest, JsonRpcResponse};

/// A simple tag-based component filter.
#[derive(Debug, Clone, Default)]
pub struct ComponentFilter {
    /// Tags to match
    pub tags: HashSet<String>,
}

impl ComponentFilter {
    /// Create an empty filter that matches nothing.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a filter that matches components with any of the given tags.
    pub fn with_tags<I, S>(tags: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            tags: tags.into_iter().map(Into::into).collect(),
        }
    }

    /// Check if this filter matches the given tags.
    pub fn matches(&self, component_tags: &[String]) -> bool {
        component_tags.iter().any(|t| self.tags.contains(t))
    }
}

/// RAII guard that automatically cleans up session visibility state when dropped.
///
/// This is the recommended way to manage session visibility lifetime.
#[derive(Debug)]
pub struct VisibilitySessionGuard {
    session_id: String,
    session_enabled: Arc<RwLock<HashMap<String, HashSet<String>>>>,
    session_disabled: Arc<RwLock<HashMap<String, HashSet<String>>>>,
}

impl VisibilitySessionGuard {
    /// Get the session ID this guard is managing.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }
}

impl Drop for VisibilitySessionGuard {
    fn drop(&mut self) {
        if let Ok(mut enabled) = self.session_enabled.write() {
            enabled.remove(&self.session_id);
        }
        if let Ok(mut disabled) = self.session_disabled.write() {
            disabled.remove(&self.session_id);
        }
    }
}

/// A visibility layer that wraps an `McpServer` and filters components.
///
/// This allows per-session control over which tools, resources, and prompts
/// are visible to clients through the `list_*` methods.
///
/// # Example
///
/// ```ignore
/// let layer = VisibilityLayer::new(server)
///     .with_tool_tags("admin_tool", &["admin"])
///     .disable_tags(["admin"]);
///
/// // Enable admin for a specific session
/// layer.enable_for_session("session123", &["admin".to_string()]);
///
/// // Handle requests
/// layer.handle(request).await
/// ```
#[derive(Clone)]
pub struct VisibilityLayer {
    /// The wrapped server
    inner: McpServer,
    /// Globally disabled component filters
    global_disabled: Arc<RwLock<Vec<ComponentFilter>>>,
    /// Session-specific enabled tags (keyed by session_id)
    session_enabled: Arc<RwLock<HashMap<String, HashSet<String>>>>,
    /// Session-specific disabled tags (keyed by session_id)
    session_disabled: Arc<RwLock<HashMap<String, HashSet<String>>>>,
    /// Tool tags mapping (tool_name -> tags)
    tool_tags: Arc<RwLock<HashMap<String, Vec<String>>>>,
    /// Resource tags mapping (uri -> tags)
    resource_tags: Arc<RwLock<HashMap<String, Vec<String>>>>,
    /// Prompt tags mapping (prompt_name -> tags)
    prompt_tags: Arc<RwLock<HashMap<String, Vec<String>>>>,
}

impl std::fmt::Debug for VisibilityLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let global_count = self.global_disabled.read().map(|g| g.len()).unwrap_or(0);
        let enabled_count = self.session_enabled.read().map(|e| e.len()).unwrap_or(0);
        let disabled_count = self.session_disabled.read().map(|d| d.len()).unwrap_or(0);

        f.debug_struct("VisibilityLayer")
            .field("server_name", &self.inner.server_info.name)
            .field("global_disabled_count", &global_count)
            .field("session_enabled_count", &enabled_count)
            .field("session_disabled_count", &disabled_count)
            .finish()
    }
}

impl VisibilityLayer {
    /// Create a new visibility layer wrapping the given server.
    pub fn new(inner: McpServer) -> Self {
        Self {
            inner,
            global_disabled: Arc::new(RwLock::new(Vec::new())),
            session_enabled: Arc::new(RwLock::new(HashMap::new())),
            session_disabled: Arc::new(RwLock::new(HashMap::new())),
            tool_tags: Arc::new(RwLock::new(HashMap::new())),
            resource_tags: Arc::new(RwLock::new(HashMap::new())),
            prompt_tags: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Assign tags to a tool for visibility filtering.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let layer = VisibilityLayer::new(server)
    ///     .with_tool_tags("admin_tool", &["admin"])
    ///     .with_tool_tags("dangerous_tool", &["admin", "dangerous"])
    ///     .disable_tags(["admin"]);
    /// ```
    #[must_use]
    pub fn with_tool_tags<I, S>(self, tool_name: &str, tags: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        if let Ok(mut map) = self.tool_tags.write() {
            map.insert(
                tool_name.to_string(),
                tags.into_iter().map(Into::into).collect(),
            );
        }
        self
    }

    /// Assign tags to a resource for visibility filtering.
    #[must_use]
    pub fn with_resource_tags<I, S>(self, uri: &str, tags: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        if let Ok(mut map) = self.resource_tags.write() {
            map.insert(uri.to_string(), tags.into_iter().map(Into::into).collect());
        }
        self
    }

    /// Assign tags to a prompt for visibility filtering.
    #[must_use]
    pub fn with_prompt_tags<I, S>(self, prompt_name: &str, tags: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        if let Ok(mut map) = self.prompt_tags.write() {
            map.insert(
                prompt_name.to_string(),
                tags.into_iter().map(Into::into).collect(),
            );
        }
        self
    }

    /// Disable components matching the filter globally.
    ///
    /// This affects all sessions unless explicitly enabled per-session.
    #[must_use]
    pub fn with_disabled(self, filter: ComponentFilter) -> Self {
        if let Ok(mut global) = self.global_disabled.write() {
            global.push(filter);
        }
        self
    }

    /// Disable components with the given tags globally.
    #[must_use]
    pub fn disable_tags<I, S>(self, tags: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.with_disabled(ComponentFilter::with_tags(tags))
    }

    /// Enable components with the given tags for a specific session.
    pub fn enable_for_session(&self, session_id: &str, tags: &[String]) {
        if let Ok(mut enabled) = self.session_enabled.write() {
            let entry = enabled.entry(session_id.to_string()).or_default();
            entry.extend(tags.iter().cloned());
        }

        // Remove from disabled if present
        if let Ok(mut disabled) = self.session_disabled.write()
            && let Some(disabled_tags) = disabled.get_mut(session_id)
        {
            for tag in tags {
                disabled_tags.remove(tag);
            }
        }
    }

    /// Disable components with the given tags for a specific session.
    pub fn disable_for_session(&self, session_id: &str, tags: &[String]) {
        if let Ok(mut disabled) = self.session_disabled.write() {
            let entry = disabled.entry(session_id.to_string()).or_default();
            entry.extend(tags.iter().cloned());
        }

        // Remove from enabled if present
        if let Ok(mut enabled) = self.session_enabled.write()
            && let Some(enabled_tags) = enabled.get_mut(session_id)
        {
            for tag in tags {
                enabled_tags.remove(tag);
            }
        }
    }

    /// Clear all session-specific overrides.
    pub fn clear_session(&self, session_id: &str) {
        if let Ok(mut enabled) = self.session_enabled.write() {
            enabled.remove(session_id);
        }
        if let Ok(mut disabled) = self.session_disabled.write() {
            disabled.remove(session_id);
        }
    }

    /// Create an RAII guard that automatically cleans up session state on drop.
    ///
    /// This is the recommended way to manage session visibility lifetime.
    pub fn session_guard(&self, session_id: impl Into<String>) -> VisibilitySessionGuard {
        VisibilitySessionGuard {
            session_id: session_id.into(),
            session_enabled: Arc::clone(&self.session_enabled),
            session_disabled: Arc::clone(&self.session_disabled),
        }
    }

    /// Get the number of active sessions with visibility overrides.
    pub fn active_sessions_count(&self) -> usize {
        let mut sessions = HashSet::new();

        if let Ok(enabled) = self.session_enabled.read() {
            sessions.extend(enabled.keys().cloned());
        }
        if let Ok(disabled) = self.session_disabled.read() {
            sessions.extend(disabled.keys().cloned());
        }

        sessions.len()
    }

    /// Get a reference to the inner server.
    pub fn inner(&self) -> &McpServer {
        &self.inner
    }

    /// Unwrap the layer and return the inner server.
    pub fn into_inner(self) -> McpServer {
        self.inner
    }

    /// Check if a component is visible given its tags and session.
    fn is_visible(&self, component_tags: &[String], session_id: Option<&str>) -> bool {
        // Check global disabled filters
        let globally_hidden = self
            .global_disabled
            .read()
            .map(|global| global.iter().any(|filter| filter.matches(component_tags)))
            .unwrap_or(false);

        if !globally_hidden {
            // Not globally hidden - check if session explicitly disabled it
            if let Some(sid) = session_id
                && let Ok(disabled) = self.session_disabled.read()
                && let Some(disabled_tags) = disabled.get(sid)
                && component_tags.iter().any(|t| disabled_tags.contains(t))
            {
                return false;
            }
            return true;
        }

        // Globally hidden - check if session explicitly enabled it
        if let Some(sid) = session_id
            && let Ok(enabled) = self.session_enabled.read()
            && let Some(enabled_tags) = enabled.get(sid)
            && component_tags.iter().any(|t| enabled_tags.contains(t))
        {
            return true;
        }

        false
    }

    /// Get tags for a tool from the stored mapping.
    fn get_tool_tags(&self, tool_name: &str) -> Vec<String> {
        self.tool_tags
            .read()
            .ok()
            .and_then(|map| map.get(tool_name).cloned())
            .unwrap_or_default()
    }

    /// Get tags for a resource from the stored mapping.
    fn get_resource_tags(&self, uri: &str) -> Vec<String> {
        self.resource_tags
            .read()
            .ok()
            .and_then(|map| map.get(uri).cloned())
            .unwrap_or_default()
    }

    /// Get tags for a prompt from the stored mapping.
    fn get_prompt_tags(&self, prompt_name: &str) -> Vec<String> {
        self.prompt_tags
            .read()
            .ok()
            .and_then(|map| map.get(prompt_name).cloned())
            .unwrap_or_default()
    }

    /// Handle an incoming Cloudflare Worker request.
    ///
    /// This routes requests through the visibility layer, filtering
    /// tools/resources/prompts based on visibility rules.
    pub async fn handle(&self, req: Request) -> worker::Result<Response> {
        self.handle_with_session(req, None).await
    }

    /// Handle an incoming request with session context.
    ///
    /// This allows session-specific visibility overrides to take effect.
    pub async fn handle_with_session(
        &self,
        mut req: Request,
        session_id: Option<&str>,
    ) -> worker::Result<Response> {
        // SECURITY: Extract Origin header early for CORS responses.
        // We echo this back instead of using wildcard "*".
        let request_origin = req.headers().get("origin").ok().flatten();
        let origin_ref = request_origin.as_deref();

        // Handle CORS preflight
        if req.method() == worker::Method::Options {
            return self.cors_preflight_response(origin_ref);
        }

        // Parse JSON-RPC request
        let body = req.text().await?;
        let rpc_request: JsonRpcRequest = match serde_json::from_str(&body) {
            Ok(r) => r,
            Err(e) => {
                return self.json_rpc_error_response(
                    None,
                    -32700,
                    &format!("Parse error: {}", e),
                    origin_ref,
                );
            }
        };

        let id = rpc_request.id.clone();

        // Route based on method
        let result = match rpc_request.method.as_str() {
            "initialize" => self.handle_initialize(&rpc_request).await,
            "tools/list" => self.handle_list_tools(session_id),
            "tools/call" => self.handle_call_tool(&rpc_request, session_id).await,
            "resources/list" => self.handle_list_resources(session_id),
            "resources/read" => self.handle_read_resource(&rpc_request, session_id).await,
            "resources/templates/list" => self.handle_list_resource_templates(session_id),
            "prompts/list" => self.handle_list_prompts(session_id),
            "prompts/get" => self.handle_get_prompt(&rpc_request, session_id).await,
            method => {
                return self.json_rpc_error_response(
                    id.clone(),
                    -32601,
                    &format!("Method not found: {}", method),
                    origin_ref,
                );
            }
        };

        match result {
            Ok(value) => self.json_rpc_success_response(id, value, origin_ref),
            Err(e) => self.json_rpc_error_response(id, -32603, &e, origin_ref),
        }
    }

    // =========================================================================
    // Request Handlers (with visibility filtering)
    // =========================================================================

    async fn handle_initialize(&self, _req: &JsonRpcRequest) -> Result<serde_json::Value, String> {
        Ok(serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": self.inner.capabilities,
            "serverInfo": self.inner.server_info
        }))
    }

    fn handle_list_tools(&self, session_id: Option<&str>) -> Result<serde_json::Value, String> {
        let filtered_tools: Vec<_> = self
            .inner
            .tools()
            .into_iter()
            .filter(|tool| {
                let tags = self.get_tool_tags(&tool.name);
                self.is_visible(&tags, session_id)
            })
            .cloned()
            .collect();

        Ok(serde_json::json!({
            "tools": filtered_tools
        }))
    }

    async fn handle_call_tool(
        &self,
        req: &JsonRpcRequest,
        session_id: Option<&str>,
    ) -> Result<serde_json::Value, String> {
        let params = req
            .params
            .as_ref()
            .ok_or_else(|| "Missing params".to_string())?;

        let name = params
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "Missing tool name".to_string())?;

        // Check visibility
        let tags = self.get_tool_tags(name);
        if !self.is_visible(&tags, session_id) {
            return Err(format!("Tool not found: {}", name));
        }

        let args = params
            .get("arguments")
            .cloned()
            .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));

        // Create context with session
        let mut ctx = RequestContext::new();
        if let Some(sid) = session_id {
            ctx = ctx.with_session_id(sid);
        }
        let ctx = Arc::new(ctx);

        // Call the tool
        let result = self.inner.call_tool_internal(name, args, ctx).await?;

        Ok(serde_json::json!({
            "content": result.content,
            "isError": result.is_error
        }))
    }

    fn handle_list_resources(&self, session_id: Option<&str>) -> Result<serde_json::Value, String> {
        let filtered_resources: Vec<_> = self
            .inner
            .resources()
            .into_iter()
            .filter(|resource| {
                let tags = self.get_resource_tags(&resource.uri);
                self.is_visible(&tags, session_id)
            })
            .cloned()
            .collect();

        Ok(serde_json::json!({
            "resources": filtered_resources
        }))
    }

    fn handle_list_resource_templates(
        &self,
        _session_id: Option<&str>,
    ) -> Result<serde_json::Value, String> {
        // For templates, we'd need a similar tag extraction mechanism
        // For now, return all templates (visibility filtering for templates could be added later)
        let templates: Vec<_> = self
            .inner
            .resource_templates()
            .into_iter()
            .cloned()
            .collect();

        Ok(serde_json::json!({
            "resourceTemplates": templates
        }))
    }

    async fn handle_read_resource(
        &self,
        req: &JsonRpcRequest,
        session_id: Option<&str>,
    ) -> Result<serde_json::Value, String> {
        let params = req
            .params
            .as_ref()
            .ok_or_else(|| "Missing params".to_string())?;

        let uri = params
            .get("uri")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "Missing resource URI".to_string())?;

        // Check visibility
        let tags = self.get_resource_tags(uri);
        if !self.is_visible(&tags, session_id) {
            return Err(format!("Resource not found: {}", uri));
        }

        // Create context with session
        let mut ctx = RequestContext::new();
        if let Some(sid) = session_id {
            ctx = ctx.with_session_id(sid);
        }
        let ctx = Arc::new(ctx);

        // Read the resource
        let result = self.inner.read_resource_internal(uri, ctx).await?;

        Ok(serde_json::json!({
            "contents": result.contents
        }))
    }

    fn handle_list_prompts(&self, session_id: Option<&str>) -> Result<serde_json::Value, String> {
        let filtered_prompts: Vec<_> = self
            .inner
            .prompts()
            .into_iter()
            .filter(|prompt| {
                let tags = self.get_prompt_tags(&prompt.name);
                self.is_visible(&tags, session_id)
            })
            .cloned()
            .collect();

        Ok(serde_json::json!({
            "prompts": filtered_prompts
        }))
    }

    async fn handle_get_prompt(
        &self,
        req: &JsonRpcRequest,
        session_id: Option<&str>,
    ) -> Result<serde_json::Value, String> {
        let params = req
            .params
            .as_ref()
            .ok_or_else(|| "Missing params".to_string())?;

        let name = params
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "Missing prompt name".to_string())?;

        // Check visibility
        let tags = self.get_prompt_tags(name);
        if !self.is_visible(&tags, session_id) {
            return Err(format!("Prompt not found: {}", name));
        }

        let args = params.get("arguments").cloned();

        // Create context with session
        let mut ctx = RequestContext::new();
        if let Some(sid) = session_id {
            ctx = ctx.with_session_id(sid);
        }
        let ctx = Arc::new(ctx);

        // Get the prompt
        let result = self.inner.get_prompt_internal(name, args, ctx).await?;

        Ok(serde_json::json!({
            "description": result.description,
            "messages": result.messages
        }))
    }

    // =========================================================================
    // Response Helpers
    // =========================================================================

    /// Create CORS headers for responses.
    ///
    /// SECURITY: Echoes the request Origin header instead of using wildcard `*`.
    fn cors_headers(&self, request_origin: Option<&str>) -> Headers {
        let headers = Headers::new();
        // SECURITY: Echo the request origin instead of using wildcard.
        let origin = request_origin.unwrap_or("*");
        let _ = headers.set("Access-Control-Allow-Origin", origin);
        if request_origin.is_some() {
            let _ = headers.set("Vary", "Origin");
        }
        let _ = headers.set("Access-Control-Allow-Methods", "POST, OPTIONS");
        let _ = headers.set("Access-Control-Allow-Headers", "Content-Type");
        let _ = headers.set("Access-Control-Max-Age", "86400");
        headers
    }

    fn cors_preflight_response(&self, request_origin: Option<&str>) -> worker::Result<Response> {
        Ok(Response::empty()?
            .with_status(204)
            .with_headers(self.cors_headers(request_origin)))
    }

    fn json_rpc_success_response(
        &self,
        id: Option<serde_json::Value>,
        result: serde_json::Value,
        request_origin: Option<&str>,
    ) -> worker::Result<Response> {
        let response = JsonRpcResponse::success(id, result);
        let json =
            serde_json::to_string(&response).map_err(|e| worker::Error::from(e.to_string()))?;

        let headers = self.cors_headers(request_origin);
        let _ = headers.set("Content-Type", "application/json");

        Ok(Response::ok(json)?.with_headers(headers))
    }

    fn json_rpc_error_response(
        &self,
        id: Option<serde_json::Value>,
        code: i32,
        message: &str,
        request_origin: Option<&str>,
    ) -> worker::Result<Response> {
        let response = JsonRpcResponse::error(id, code, message);
        let json =
            serde_json::to_string(&response).map_err(|e| worker::Error::from(e.to_string()))?;

        let headers = self.cors_headers(request_origin);
        let _ = headers.set("Content-Type", "application/json");

        Ok(Response::ok(json)?.with_headers(headers))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_server() -> McpServer {
        McpServer::builder("test", "1.0.0")
            .tool_raw("public_tool", "Public tool", |_args| async {
                "public".to_string()
            })
            .tool_raw("admin_tool", "Admin tool", |_args| async {
                "admin".to_string()
            })
            .build()
    }

    #[test]
    fn test_visibility_layer_creation() {
        let server = create_test_server();
        let layer = VisibilityLayer::new(server);

        assert_eq!(layer.active_sessions_count(), 0);
    }

    #[test]
    fn test_component_filter() {
        let filter = ComponentFilter::with_tags(["admin", "dangerous"]);

        assert!(filter.matches(&["admin".to_string()]));
        assert!(filter.matches(&["dangerous".to_string()]));
        assert!(filter.matches(&["admin".to_string(), "public".to_string()]));
        assert!(!filter.matches(&["public".to_string()]));
        assert!(!filter.matches(&[]));
    }

    #[test]
    fn test_session_enable_override() {
        let server = create_test_server();
        let layer = VisibilityLayer::new(server).disable_tags(["admin"]);

        // Enable for session
        layer.enable_for_session("session1", &["admin".to_string()]);

        // Session should have override
        assert_eq!(layer.active_sessions_count(), 1);

        // Cleanup
        layer.clear_session("session1");
        assert_eq!(layer.active_sessions_count(), 0);
    }

    #[test]
    fn test_session_guard_cleanup() {
        let server = create_test_server();
        let layer = VisibilityLayer::new(server).disable_tags(["admin"]);

        {
            let _guard = layer.session_guard("guard-session");

            // Enable admin for this session
            layer.enable_for_session("guard-session", &["admin".to_string()]);
            layer.disable_for_session("guard-session", &["public".to_string()]);

            // Session state exists
            assert!(layer.active_sessions_count() > 0);
        }

        // After guard drops, session state should be cleaned up
        assert_eq!(layer.active_sessions_count(), 0);
    }

    #[test]
    fn test_active_sessions_count() {
        let server = create_test_server();
        let layer = VisibilityLayer::new(server);

        assert_eq!(layer.active_sessions_count(), 0);

        layer.enable_for_session("session1", &["tag1".to_string()]);
        assert_eq!(layer.active_sessions_count(), 1);

        layer.disable_for_session("session2", &["tag2".to_string()]);
        assert_eq!(layer.active_sessions_count(), 2);

        // Same session, different tag - should not increase count
        layer.enable_for_session("session1", &["tag2".to_string()]);
        assert_eq!(layer.active_sessions_count(), 2);

        layer.clear_session("session1");
        assert_eq!(layer.active_sessions_count(), 1);

        layer.clear_session("session2");
        assert_eq!(layer.active_sessions_count(), 0);
    }

    #[test]
    fn test_is_visible_basic() {
        let server = create_test_server();
        let layer = VisibilityLayer::new(server);

        // Without any filters, everything is visible
        assert!(layer.is_visible(&["admin".to_string()], None));
        assert!(layer.is_visible(&["public".to_string()], None));
    }

    #[test]
    fn test_is_visible_with_global_filter() {
        let server = create_test_server();
        let layer = VisibilityLayer::new(server).disable_tags(["admin"]);

        // Admin is hidden globally
        assert!(!layer.is_visible(&["admin".to_string()], None));
        assert!(layer.is_visible(&["public".to_string()], None));
    }

    #[test]
    fn test_is_visible_with_session_override() {
        let server = create_test_server();
        let layer = VisibilityLayer::new(server).disable_tags(["admin"]);

        // Admin is hidden globally
        assert!(!layer.is_visible(&["admin".to_string()], None));

        // Enable for session
        layer.enable_for_session("session1", &["admin".to_string()]);

        // Admin is visible for session1
        assert!(layer.is_visible(&["admin".to_string()], Some("session1")));

        // Admin still hidden for other sessions
        assert!(!layer.is_visible(&["admin".to_string()], Some("session2")));
        assert!(!layer.is_visible(&["admin".to_string()], None));
    }

    #[test]
    fn test_disable_for_session() {
        let server = create_test_server();
        let layer = VisibilityLayer::new(server);

        // Public is visible by default
        assert!(layer.is_visible(&["public".to_string()], None));
        assert!(layer.is_visible(&["public".to_string()], Some("session1")));

        // Disable for session1
        layer.disable_for_session("session1", &["public".to_string()]);

        // Public is hidden for session1
        assert!(!layer.is_visible(&["public".to_string()], Some("session1")));

        // Public still visible for others
        assert!(layer.is_visible(&["public".to_string()], None));
        assert!(layer.is_visible(&["public".to_string()], Some("session2")));
    }

    #[test]
    fn test_enable_removes_from_disabled() {
        let server = create_test_server();
        let layer = VisibilityLayer::new(server);

        // Disable a tag for session
        layer.disable_for_session("session1", &["tag1".to_string()]);
        assert!(!layer.is_visible(&["tag1".to_string()], Some("session1")));

        // Enable the same tag - should remove from disabled
        layer.enable_for_session("session1", &["tag1".to_string()]);

        // Globally disabled - but enabled for session, so visible
        let layer2 = VisibilityLayer::new(create_test_server()).disable_tags(["tag1"]);
        layer2.enable_for_session("session1", &["tag1".to_string()]);
        assert!(layer2.is_visible(&["tag1".to_string()], Some("session1")));
    }

    #[test]
    fn test_disable_removes_from_enabled() {
        let server = create_test_server();
        let layer = VisibilityLayer::new(server).disable_tags(["admin"]);

        // Enable admin for session
        layer.enable_for_session("session1", &["admin".to_string()]);
        assert!(layer.is_visible(&["admin".to_string()], Some("session1")));

        // Now disable it - should remove from enabled
        layer.disable_for_session("session1", &["admin".to_string()]);
        assert!(!layer.is_visible(&["admin".to_string()], Some("session1")));
    }
}
