//! Progressive disclosure through component visibility control.
//!
//! This module provides the ability to dynamically show/hide tools, resources,
//! and prompts based on tags. This enables patterns like:
//!
//! - Hiding admin tools until explicitly unlocked
//! - Progressive disclosure of advanced features
//! - Role-based component visibility
//!
//! # Memory Management
//!
//! Session visibility overrides are stored in a per-layer map keyed by session ID.
//! **IMPORTANT**: You must ensure cleanup happens when sessions end to prevent
//! memory leaks. Use one of these approaches:
//!
//! 1. **Recommended**: Use [`VisibilitySessionGuard`] which automatically cleans up on drop
//! 2. **Manual**: Call [`VisibilityLayer::clear_session`] when a session disconnects
//!
//! # Example
//!
//! ```rust,ignore
//! use turbomcp_server::visibility::{VisibilityLayer, VisibilitySessionGuard};
//! use turbomcp_types::component::ComponentFilter;
//!
//! // Create a visibility layer that hides admin tools by default
//! let layer = VisibilityLayer::new(server)
//!     .with_disabled(ComponentFilter::with_tags(["admin"]));
//!
//! // Tools, resources, and prompts tagged with "admin" won't appear
//! // until explicitly enabled via the RequestContext
//!
//! async fn handle_session(layer: &VisibilityLayer<MyHandler>, session_id: &str) {
//!     // Guard ensures cleanup when it goes out of scope
//!     let _guard = layer.session_guard(session_id);
//!
//!     // Enable admin tools for this session
//!     layer.enable_for_session(session_id, &["admin".to_string()]);
//!
//!     // ... handle requests ...
//!
//! } // Guard dropped here, session state automatically cleaned up
//! ```

use std::collections::HashSet;
use std::sync::Arc;

use parking_lot::RwLock;
use turbomcp_core::context::RequestContext;
use turbomcp_core::error::{McpError, McpResult};
use turbomcp_core::handler::McpHandler;
use turbomcp_types::{
    ComponentFilter, ComponentMeta, Prompt, PromptResult, Resource, ResourceResult, Tool,
    ToolResult,
};

/// Type alias for session visibility maps to reduce complexity.
type SessionVisibilityMap = Arc<dashmap::DashMap<String, HashSet<String>>>;

/// RAII guard that automatically cleans up session visibility state when dropped.
///
/// This is the recommended way to manage session visibility lifetime. Create a guard
/// at the start of a session and let it clean up automatically when the session ends.
///
/// # Example
///
/// ```rust,ignore
/// use turbomcp_server::visibility::VisibilityLayer;
///
/// async fn handle_connection<H: McpHandler>(layer: &VisibilityLayer<H>, session_id: &str) {
///     let _guard = layer.session_guard(session_id);
///
///     // Enable admin tools for this session
///     layer.enable_for_session(session_id, &["admin".to_string()]);
///
///     // ... handle requests ...
///
/// } // State automatically cleaned up here
/// ```
#[derive(Debug)]
pub struct VisibilitySessionGuard {
    session_id: String,
    session_enabled: SessionVisibilityMap,
    session_disabled: SessionVisibilityMap,
}

impl VisibilitySessionGuard {
    /// Get the session ID this guard is managing.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }
}

impl Drop for VisibilitySessionGuard {
    fn drop(&mut self) {
        self.session_enabled.remove(&self.session_id);
        self.session_disabled.remove(&self.session_id);
    }
}

/// A visibility layer that wraps an `McpHandler` and filters components.
///
/// This allows per-session control over which tools, resources, and prompts
/// are visible to clients through the `list_*` methods.
///
/// **Warning**: Session overrides stored in this layer must be manually cleaned up
/// via [`clear_session`](Self::clear_session) or by using a [`VisibilitySessionGuard`]
/// to prevent unbounded memory growth.
#[derive(Clone)]
pub struct VisibilityLayer<H> {
    /// The wrapped handler
    inner: H,
    /// Globally disabled component filters
    global_disabled: Arc<RwLock<Vec<ComponentFilter>>>,
    /// Session-specific visibility overrides (keyed by session_id)
    ///
    /// **Warning**: Entries must be manually cleaned up via [`clear_session`](Self::clear_session)
    /// or [`session_guard`](Self::session_guard) to prevent unbounded memory growth.
    session_enabled: SessionVisibilityMap,
    session_disabled: SessionVisibilityMap,
}

impl<H: std::fmt::Debug> std::fmt::Debug for VisibilityLayer<H> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VisibilityLayer")
            .field("inner", &self.inner)
            .field("global_disabled_count", &self.global_disabled.read().len())
            .field("session_enabled_count", &self.session_enabled.len())
            .field("session_disabled_count", &self.session_disabled.len())
            .finish()
    }
}

impl<H: McpHandler> VisibilityLayer<H> {
    /// Create a new visibility layer wrapping the given handler.
    pub fn new(inner: H) -> Self {
        Self {
            inner,
            global_disabled: Arc::new(RwLock::new(Vec::new())),
            session_enabled: Arc::new(dashmap::DashMap::new()),
            session_disabled: Arc::new(dashmap::DashMap::new()),
        }
    }

    /// Disable components matching the filter globally.
    ///
    /// This affects all sessions unless explicitly enabled per-session.
    #[must_use]
    pub fn with_disabled(self, filter: ComponentFilter) -> Self {
        self.global_disabled.write().push(filter);
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

    /// Check if a component is visible given its metadata and session.
    fn is_visible(&self, meta: &ComponentMeta, session_id: Option<&str>) -> bool {
        // Check global disabled filters
        let global_disabled = self.global_disabled.read();
        let globally_hidden = global_disabled.iter().any(|filter| filter.matches(meta));

        if !globally_hidden {
            // Not globally hidden - check if session explicitly disabled it
            if let Some(sid) = session_id
                && let Some(disabled) = self.session_disabled.get(sid)
                && meta.tags.iter().any(|t| disabled.contains(t))
            {
                return false;
            }
            return true;
        }

        // Globally hidden - check if session explicitly enabled it
        if let Some(sid) = session_id
            && let Some(enabled) = self.session_enabled.get(sid)
            && meta.tags.iter().any(|t| enabled.contains(t))
        {
            return true;
        }

        false
    }

    /// Enable components with the given tags for a specific session.
    pub fn enable_for_session(&self, session_id: &str, tags: &[String]) {
        let mut entry = self
            .session_enabled
            .entry(session_id.to_string())
            .or_default();
        entry.extend(tags.iter().cloned());

        // Remove from disabled if present
        if let Some(mut disabled) = self.session_disabled.get_mut(session_id) {
            for tag in tags {
                disabled.remove(tag);
            }
        }
    }

    /// Disable components with the given tags for a specific session.
    pub fn disable_for_session(&self, session_id: &str, tags: &[String]) {
        let mut entry = self
            .session_disabled
            .entry(session_id.to_string())
            .or_default();
        entry.extend(tags.iter().cloned());

        // Remove from enabled if present
        if let Some(mut enabled) = self.session_enabled.get_mut(session_id) {
            for tag in tags {
                enabled.remove(tag);
            }
        }
    }

    /// Clear all session-specific overrides.
    pub fn clear_session(&self, session_id: &str) {
        self.session_enabled.remove(session_id);
        self.session_disabled.remove(session_id);
    }

    /// Create an RAII guard that automatically cleans up session state on drop.
    ///
    /// This is the recommended way to manage session visibility lifetime.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// async fn handle_connection<H: McpHandler>(layer: &VisibilityLayer<H>, session_id: &str) {
    ///     let _guard = layer.session_guard(session_id);
    ///
    ///     layer.enable_for_session(session_id, &["admin".to_string()]);
    ///
    ///     // ... handle requests ...
    ///
    /// } // State automatically cleaned up here
    /// ```
    pub fn session_guard(&self, session_id: impl Into<String>) -> VisibilitySessionGuard {
        VisibilitySessionGuard {
            session_id: session_id.into(),
            session_enabled: Arc::clone(&self.session_enabled),
            session_disabled: Arc::clone(&self.session_disabled),
        }
    }

    /// Get the number of active sessions with visibility overrides.
    ///
    /// This is useful for monitoring memory usage.
    pub fn active_sessions_count(&self) -> usize {
        // Count unique session IDs across both maps
        let mut sessions = HashSet::new();
        for entry in self.session_enabled.iter() {
            sessions.insert(entry.key().clone());
        }
        for entry in self.session_disabled.iter() {
            sessions.insert(entry.key().clone());
        }
        sessions.len()
    }

    /// Get a reference to the inner handler.
    pub fn inner(&self) -> &H {
        &self.inner
    }

    /// Get a mutable reference to the inner handler.
    pub fn inner_mut(&mut self) -> &mut H {
        &mut self.inner
    }

    /// Unwrap the layer and return the inner handler.
    pub fn into_inner(self) -> H {
        self.inner
    }
}

#[allow(clippy::manual_async_fn)]
impl<H: McpHandler> McpHandler for VisibilityLayer<H> {
    fn server_info(&self) -> turbomcp_types::ServerInfo {
        self.inner.server_info()
    }

    fn list_tools(&self) -> Vec<Tool> {
        self.inner
            .list_tools()
            .into_iter()
            .filter(|tool| {
                let meta = ComponentMeta::from_meta_value(tool.meta.as_ref());
                self.is_visible(&meta, None)
            })
            .collect()
    }

    fn list_resources(&self) -> Vec<Resource> {
        self.inner
            .list_resources()
            .into_iter()
            .filter(|resource| {
                let meta = ComponentMeta::from_meta_value(resource.meta.as_ref());
                self.is_visible(&meta, None)
            })
            .collect()
    }

    fn list_prompts(&self) -> Vec<Prompt> {
        self.inner
            .list_prompts()
            .into_iter()
            .filter(|prompt| {
                let meta = ComponentMeta::from_meta_value(prompt.meta.as_ref());
                self.is_visible(&meta, None)
            })
            .collect()
    }

    fn call_tool<'a>(
        &'a self,
        name: &'a str,
        args: serde_json::Value,
        ctx: &'a RequestContext,
    ) -> impl std::future::Future<Output = McpResult<ToolResult>> + turbomcp_core::marker::MaybeSend + 'a
    {
        async move {
            // Check if tool is visible for this session
            let tools = self.inner.list_tools();
            let tool = tools.iter().find(|t| t.name == name);

            if let Some(tool) = tool {
                let meta = ComponentMeta::from_meta_value(tool.meta.as_ref());
                if !self.is_visible(&meta, ctx.get_metadata("session_id")) {
                    return Err(McpError::tool_not_found(name));
                }
            }

            self.inner.call_tool(name, args, ctx).await
        }
    }

    fn read_resource<'a>(
        &'a self,
        uri: &'a str,
        ctx: &'a RequestContext,
    ) -> impl std::future::Future<Output = McpResult<ResourceResult>>
    + turbomcp_core::marker::MaybeSend
    + 'a {
        async move {
            // Check if resource is visible for this session
            let resources = self.inner.list_resources();
            let resource = resources.iter().find(|r| r.uri == uri);

            if let Some(resource) = resource {
                let meta = ComponentMeta::from_meta_value(resource.meta.as_ref());
                if !self.is_visible(&meta, ctx.get_metadata("session_id")) {
                    return Err(McpError::resource_not_found(uri));
                }
            }

            self.inner.read_resource(uri, ctx).await
        }
    }

    fn get_prompt<'a>(
        &'a self,
        name: &'a str,
        args: Option<serde_json::Value>,
        ctx: &'a RequestContext,
    ) -> impl std::future::Future<Output = McpResult<PromptResult>> + turbomcp_core::marker::MaybeSend + 'a
    {
        async move {
            // Check if prompt is visible for this session
            let prompts = self.inner.list_prompts();
            let prompt = prompts.iter().find(|p| p.name == name);

            if let Some(prompt) = prompt {
                let meta = ComponentMeta::from_meta_value(prompt.meta.as_ref());
                if !self.is_visible(&meta, ctx.get_metadata("session_id")) {
                    return Err(McpError::prompt_not_found(name));
                }
            }

            self.inner.get_prompt(name, args, ctx).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug)]
    struct MockHandler;

    #[allow(clippy::manual_async_fn)]
    impl McpHandler for MockHandler {
        fn server_info(&self) -> turbomcp_types::ServerInfo {
            turbomcp_types::ServerInfo::new("test", "1.0.0")
        }

        fn list_tools(&self) -> Vec<Tool> {
            vec![
                Tool {
                    name: "public_tool".to_string(),
                    description: Some("Public tool".to_string()),
                    meta: Some({
                        let mut m = std::collections::HashMap::new();
                        m.insert("tags".to_string(), serde_json::json!(["public"]));
                        m
                    }),
                    ..Default::default()
                },
                Tool {
                    name: "admin_tool".to_string(),
                    description: Some("Admin tool".to_string()),
                    meta: Some({
                        let mut m = std::collections::HashMap::new();
                        m.insert("tags".to_string(), serde_json::json!(["admin"]));
                        m
                    }),
                    ..Default::default()
                },
            ]
        }

        fn list_resources(&self) -> Vec<Resource> {
            vec![]
        }

        fn list_prompts(&self) -> Vec<Prompt> {
            vec![]
        }

        fn call_tool<'a>(
            &'a self,
            name: &'a str,
            _args: serde_json::Value,
            _ctx: &'a RequestContext,
        ) -> impl std::future::Future<Output = McpResult<ToolResult>>
        + turbomcp_core::marker::MaybeSend
        + 'a {
            async move { Ok(ToolResult::text(format!("Called {}", name))) }
        }

        fn read_resource<'a>(
            &'a self,
            _uri: &'a str,
            _ctx: &'a RequestContext,
        ) -> impl std::future::Future<Output = McpResult<ResourceResult>>
        + turbomcp_core::marker::MaybeSend
        + 'a {
            async move { Err(McpError::resource_not_found("not found")) }
        }

        fn get_prompt<'a>(
            &'a self,
            _name: &'a str,
            _args: Option<serde_json::Value>,
            _ctx: &'a RequestContext,
        ) -> impl std::future::Future<Output = McpResult<PromptResult>>
        + turbomcp_core::marker::MaybeSend
        + 'a {
            async move { Err(McpError::prompt_not_found("not found")) }
        }
    }

    #[test]
    fn test_visibility_layer_hides_admin() {
        let layer = VisibilityLayer::new(MockHandler).disable_tags(["admin"]);

        let tools = layer.list_tools();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "public_tool");
    }

    #[test]
    fn test_visibility_layer_shows_all_by_default() {
        let layer = VisibilityLayer::new(MockHandler);

        let tools = layer.list_tools();
        assert_eq!(tools.len(), 2);
    }

    #[test]
    fn test_session_enable_override() {
        let layer = VisibilityLayer::new(MockHandler).disable_tags(["admin"]);

        // Initially hidden
        assert_eq!(layer.list_tools().len(), 1);

        // Enable for session
        layer.enable_for_session("session1", &["admin".to_string()]);

        // Still hidden in list_tools (doesn't take session context)
        // but call_tool would work with session context
        assert_eq!(layer.list_tools().len(), 1);

        // Cleanup
        layer.clear_session("session1");
    }

    #[test]
    fn test_session_guard_cleanup() {
        let layer = VisibilityLayer::new(MockHandler).disable_tags(["admin"]);

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
        let layer = VisibilityLayer::new(MockHandler);

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
}
