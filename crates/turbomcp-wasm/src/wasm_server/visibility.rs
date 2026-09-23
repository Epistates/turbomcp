//! Progressive disclosure through component visibility control.
//!
//! This module provides the ability to dynamically show/hide tools, resources,
//! and prompts based on tags. This enables patterns like:
//!
//! - Hiding admin tools until explicitly unlocked
//! - Progressive disclosure of advanced features
//! - Role-based component visibility
//!
//! The layer is an [`McpHandler`] wrapping another one, so every request is
//! dispatched by the core router like any other WASM entry point. A hidden
//! component is indistinguishable from one that does not exist: it is left out
//! of listings, and calling it gets the same "not found" error an unknown name
//! would.
//!
//! # Example
//!
//! ```ignore
//! use turbomcp_wasm::wasm_server::{McpServer, VisibilityLayer};
//!
//! // Create a server
//! let server = McpServer::builder("my-server", "1.0.0")
//!     .tool("public_tool", "Public tool", public_handler)
//!     .tool("admin_tool", "Admin tool", admin_handler)
//!     .build();
//!
//! // Create a visibility layer that hides admin tools by default
//! let layer = VisibilityLayer::new(server)
//!     .with_tool_tags("admin_tool", ["admin"])
//!     .disable_tags(["admin"]);
//!
//! // Enable admin tools for a specific session
//! layer.enable_for_session("session123", &["admin".to_string()]);
//!
//! // Handle requests for that session through the layer
//! layer.handle_with_session(request, Some("session123")).await
//! ```

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::sync::{Arc, RwLock};

use serde_json::Value;
use turbomcp_core::MaybeSend;
use turbomcp_core::error::{McpError, McpResult};
use turbomcp_core::handler::McpHandler;
use turbomcp_core::uri_template::UriTemplate;
use turbomcp_types::{
    Implementation, Prompt, PromptResult, Resource, ResourceResult, ResourceTemplate,
    ServerCapabilities, Tool, ToolResult,
};
use worker::{Request, Response};

use super::context::RequestContext;
use super::server::McpServer;

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

/// Per-session tag overrides, keyed by session id.
type SessionTags = Arc<RwLock<HashMap<String, HashSet<String>>>>;

/// RAII guard that automatically cleans up session visibility state when dropped.
///
/// This is the recommended way to manage session visibility lifetime.
#[derive(Debug)]
pub struct VisibilitySessionGuard {
    session_id: String,
    session_enabled: SessionTags,
    session_disabled: SessionTags,
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

/// A visibility layer that wraps a handler and filters its components.
///
/// This allows per-session control over which tools, resources, and prompts
/// are visible to clients. It wraps an [`McpServer`] by default, and any other
/// [`McpHandler`] — a middleware stack, a composite server — just as well.
///
/// Per-session overrides apply to requests served through
/// [`handle_with_session`](Self::handle_with_session). The layer's own
/// `McpHandler` implementation (what `handle` and Streamable HTTP use) applies
/// the global rules only: listings carry no request context to take a session
/// from, and a component that is listed must be callable and vice versa.
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
/// layer.handle_with_session(request, Some("session123")).await
/// ```
#[derive(Clone)]
pub struct VisibilityLayer<H: McpHandler = McpServer> {
    /// The wrapped handler
    inner: H,
    /// Session whose overrides this view applies; `None` for the global rules
    session: Option<Arc<str>>,
    /// Globally disabled component filters
    global_disabled: Arc<RwLock<Vec<ComponentFilter>>>,
    /// Session-specific enabled tags (keyed by session_id)
    session_enabled: SessionTags,
    /// Session-specific disabled tags (keyed by session_id)
    session_disabled: SessionTags,
    /// Tool tags mapping (tool_name -> tags)
    tool_tags: Arc<RwLock<HashMap<String, Vec<String>>>>,
    /// Resource tags mapping (uri or uri template -> tags)
    resource_tags: Arc<RwLock<HashMap<String, Vec<String>>>>,
    /// Prompt tags mapping (prompt_name -> tags)
    prompt_tags: Arc<RwLock<HashMap<String, Vec<String>>>>,
}

impl<H: McpHandler> std::fmt::Debug for VisibilityLayer<H> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let global_count = self.global_disabled.read().map(|g| g.len()).unwrap_or(0);
        let enabled_count = self.session_enabled.read().map(|e| e.len()).unwrap_or(0);
        let disabled_count = self.session_disabled.read().map(|d| d.len()).unwrap_or(0);

        f.debug_struct("VisibilityLayer")
            .field("server_name", &self.inner.server_info().name)
            .field("global_disabled_count", &global_count)
            .field("session_enabled_count", &enabled_count)
            .field("session_disabled_count", &disabled_count)
            .finish()
    }
}

impl<H: McpHandler> VisibilityLayer<H> {
    /// Create a new visibility layer wrapping the given handler.
    pub fn new(inner: H) -> Self {
        Self {
            inner,
            session: None,
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
    ///
    /// `uri` may be a resource URI or a resource template's URI template; a
    /// URI read through a template inherits the template's tags.
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

    /// Get a reference to the inner handler.
    pub fn inner(&self) -> &H {
        &self.inner
    }

    /// Unwrap the layer and return the inner handler.
    pub fn into_inner(self) -> H {
        self.inner
    }

    /// This layer, seen by one session: the same rules and state, with that
    /// session's overrides applied.
    fn scoped(&self, session_id: Option<&str>) -> Self {
        Self {
            session: session_id.map(Arc::from),
            ..self.clone()
        }
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

    /// Visibility of a component under this view's session.
    fn shows(&self, component_tags: &[String]) -> bool {
        self.is_visible(component_tags, self.session.as_deref())
    }

    fn tags_of(map: &RwLock<HashMap<String, Vec<String>>>, key: &str) -> Vec<String> {
        map.read()
            .ok()
            .and_then(|map| map.get(key).cloned())
            .unwrap_or_default()
    }

    /// Tags for a resource URI: its own, or else those of the template it is
    /// read through.
    fn resource_tags_for(&self, uri: &str) -> Vec<String> {
        let Ok(map) = self.resource_tags.read() else {
            return Vec::new();
        };
        if let Some(tags) = map.get(uri) {
            return tags.clone();
        }
        self.inner
            .list_resource_templates()
            .iter()
            .find(|template| UriTemplate::parse(&template.uri_template).matches(uri))
            .and_then(|template| map.get(&template.uri_template).cloned())
            .unwrap_or_default()
    }

    /// Handle an incoming Cloudflare Worker request.
    ///
    /// This routes requests through the visibility layer, filtering
    /// tools/resources/prompts by the global rules.
    pub async fn handle(&self, req: Request) -> worker::Result<Response> {
        self.handle_with_session(req, None).await
    }

    /// Handle an incoming request with session context.
    ///
    /// This allows session-specific visibility overrides to take effect. The
    /// session id is the caller's to establish (for example from an
    /// authenticated principal); it is also set on the request context the
    /// handlers see.
    pub async fn handle_with_session(
        &self,
        req: Request,
        session_id: Option<&str>,
    ) -> worker::Result<Response> {
        let scoped = self.scoped(session_id);
        let session = session_id.map(str::to_string);
        super::endpoint::serve(
            &scoped,
            req,
            &super::EndpointConfig::default(),
            move |ctx| match session {
                Some(session) => ctx.with_session_id(session),
                None => ctx,
            },
            super::endpoint::admit_all,
        )
        .await
    }
}

#[allow(clippy::manual_async_fn)]
impl<H: McpHandler> McpHandler for VisibilityLayer<H> {
    fn server_info(&self) -> Implementation {
        self.inner.server_info()
    }

    fn instructions(&self) -> Option<String> {
        self.inner.instructions()
    }

    fn server_capabilities(&self) -> ServerCapabilities {
        self.inner.server_capabilities()
    }

    fn list_tools(&self) -> Vec<Tool> {
        self.inner
            .list_tools()
            .into_iter()
            .filter(|tool| self.shows(&Self::tags_of(&self.tool_tags, &tool.name)))
            .collect()
    }

    fn list_resources(&self) -> Vec<Resource> {
        self.inner
            .list_resources()
            .into_iter()
            .filter(|resource| self.shows(&Self::tags_of(&self.resource_tags, &resource.uri)))
            .collect()
    }

    fn list_resource_templates(&self) -> Vec<ResourceTemplate> {
        self.inner
            .list_resource_templates()
            .into_iter()
            .filter(|template| {
                self.shows(&Self::tags_of(&self.resource_tags, &template.uri_template))
            })
            .collect()
    }

    fn list_prompts(&self) -> Vec<Prompt> {
        self.inner
            .list_prompts()
            .into_iter()
            .filter(|prompt| self.shows(&Self::tags_of(&self.prompt_tags, &prompt.name)))
            .collect()
    }

    fn call_tool<'a>(
        &'a self,
        name: &'a str,
        args: Value,
        ctx: &'a RequestContext,
    ) -> impl Future<Output = McpResult<ToolResult>> + MaybeSend + 'a {
        async move {
            if !self.shows(&Self::tags_of(&self.tool_tags, name)) {
                return Err(McpError::tool_not_found(name));
            }
            self.inner.call_tool(name, args, ctx).await
        }
    }

    fn read_resource<'a>(
        &'a self,
        uri: &'a str,
        ctx: &'a RequestContext,
    ) -> impl Future<Output = McpResult<ResourceResult>> + MaybeSend + 'a {
        async move {
            if !self.shows(&self.resource_tags_for(uri)) {
                return Err(McpError::resource_not_found(uri));
            }
            self.inner.read_resource(uri, ctx).await
        }
    }

    fn get_prompt<'a>(
        &'a self,
        name: &'a str,
        args: Option<Value>,
        ctx: &'a RequestContext,
    ) -> impl Future<Output = McpResult<PromptResult>> + MaybeSend + 'a {
        async move {
            if !self.shows(&Self::tags_of(&self.prompt_tags, name)) {
                return Err(McpError::prompt_not_found(name));
            }
            self.inner.get_prompt(name, args, ctx).await
        }
    }

    fn list_tasks<'a>(
        &'a self,
        cursor: Option<&'a str>,
        limit: Option<usize>,
        ctx: &'a RequestContext,
    ) -> impl Future<Output = McpResult<turbomcp_types::ListTasksResult>> + MaybeSend + 'a {
        self.inner.list_tasks(cursor, limit, ctx)
    }

    fn get_task<'a>(
        &'a self,
        task_id: &'a str,
        ctx: &'a RequestContext,
    ) -> impl Future<Output = McpResult<turbomcp_types::Task>> + MaybeSend + 'a {
        self.inner.get_task(task_id, ctx)
    }

    fn cancel_task<'a>(
        &'a self,
        task_id: &'a str,
        ctx: &'a RequestContext,
    ) -> impl Future<Output = McpResult<turbomcp_types::Task>> + MaybeSend + 'a {
        self.inner.cancel_task(task_id, ctx)
    }

    fn get_task_result<'a>(
        &'a self,
        task_id: &'a str,
        ctx: &'a RequestContext,
    ) -> impl Future<Output = McpResult<Value>> + MaybeSend + 'a {
        self.inner.get_task_result(task_id, ctx)
    }

    fn subscribe<'a>(
        &'a self,
        uri: &'a str,
        ctx: &'a RequestContext,
    ) -> impl Future<Output = McpResult<()>> + MaybeSend + 'a {
        async move {
            if !self.shows(&self.resource_tags_for(uri)) {
                return Err(McpError::resource_not_found(uri));
            }
            self.inner.subscribe(uri, ctx).await
        }
    }

    fn unsubscribe<'a>(
        &'a self,
        uri: &'a str,
        ctx: &'a RequestContext,
    ) -> impl Future<Output = McpResult<()>> + MaybeSend + 'a {
        self.inner.unsubscribe(uri, ctx)
    }

    fn set_log_level<'a>(
        &'a self,
        level: &'a str,
        ctx: &'a RequestContext,
    ) -> impl Future<Output = McpResult<()>> + MaybeSend + 'a {
        self.inner.set_log_level(level, ctx)
    }

    fn complete<'a>(
        &'a self,
        params: Value,
        ctx: &'a RequestContext,
    ) -> impl Future<Output = McpResult<Value>> + MaybeSend + 'a {
        self.inner.complete(params, ctx)
    }

    fn page_size(&self) -> Option<usize> {
        self.inner.page_size()
    }

    fn on_roots_list_changed<'a>(
        &'a self,
        ctx: &'a RequestContext,
    ) -> impl Future<Output = McpResult<()>> + MaybeSend + 'a {
        self.inner.on_roots_list_changed(ctx)
    }

    fn on_initialize(&self) -> impl Future<Output = McpResult<()>> + MaybeSend {
        self.inner.on_initialize()
    }

    fn on_shutdown(&self) -> impl Future<Output = McpResult<()>> + MaybeSend {
        self.inner.on_shutdown()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn route<H: McpHandler>(handler: &H, method: &str, params: Value) -> Value {
        let request = turbomcp_core::jsonrpc::JsonRpcIncoming {
            jsonrpc: "2.0".into(),
            id: Some(serde_json::json!(1)),
            method: method.into(),
            params: Some(params),
        };
        let ctx = crate::wasm_server::context::new_wasm_context();
        serde_json::to_value(super::super::endpoint::route(handler, request, &ctx, None).await)
            .unwrap()
    }

    fn tool_names(response: &Value) -> Vec<String> {
        response["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_string())
            .collect()
    }

    #[tokio::test]
    async fn hidden_tools_are_unlisted_and_indistinguishable_from_unknown() {
        let layer = VisibilityLayer::new(create_test_server())
            .with_tool_tags("admin_tool", ["admin"])
            .disable_tags(["admin"]);

        let listed = route(&layer, "tools/list", serde_json::json!({})).await;
        assert_eq!(tool_names(&listed), ["public_tool"]);

        let hidden = route(
            &layer,
            "tools/call",
            serde_json::json!({"name": "admin_tool"}),
        )
        .await;
        let unknown = route(&layer, "tools/call", serde_json::json!({"name": "no_tool"})).await;
        assert_eq!(hidden["error"]["code"], unknown["error"]["code"]);
        assert_eq!(hidden["error"]["code"], -32602);

        // Results travel through untouched: no `isError: null`, no dropped fields.
        let public = route(
            &layer,
            "tools/call",
            serde_json::json!({"name": "public_tool"}),
        )
        .await;
        let result = public["result"].as_object().unwrap();
        assert!(!result.contains_key("isError"));
        assert_eq!(public["result"]["content"][0]["text"], "public");
    }

    #[tokio::test]
    async fn session_overrides_apply_to_listing_and_calls_alike() {
        let layer = VisibilityLayer::new(create_test_server())
            .with_tool_tags("admin_tool", ["admin"])
            .disable_tags(["admin"]);
        layer.enable_for_session("s1", &["admin".to_string()]);

        let scoped = layer.scoped(Some("s1"));
        let listed = route(&scoped, "tools/list", serde_json::json!({})).await;
        assert_eq!(tool_names(&listed), ["admin_tool", "public_tool"]);
        let called = route(
            &scoped,
            "tools/call",
            serde_json::json!({"name": "admin_tool"}),
        )
        .await;
        assert_eq!(called["result"]["content"][0]["text"], "admin");

        let other = layer.scoped(Some("s2"));
        let listed = route(&other, "tools/list", serde_json::json!({})).await;
        assert_eq!(tool_names(&listed), ["public_tool"]);
    }

    #[tokio::test]
    async fn layer_answers_the_protocol_through_core() {
        let layer = VisibilityLayer::new(create_test_server());
        let init = route(
            &layer,
            "initialize",
            serde_json::json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "c", "version": "1"}
            }),
        )
        .await;
        assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
        assert_eq!(init["result"]["serverInfo"]["name"], "test");

        let ping = route(&layer, "ping", serde_json::json!({})).await;
        assert_eq!(ping["result"], serde_json::json!({}));
    }

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
