//! Progressive disclosure through component visibility control.
//!
//! This module provides the ability to dynamically show/hide tools, resources,
//! and prompts based on tags, exact component names, and tool annotations. This
//! enables patterns like:
//!
//! - Hiding admin tools until explicitly unlocked
//! - Progressive disclosure of advanced features
//! - Role-based component visibility
//! - Smaller `tools/list` responses for clients with tool-count or context limits
//! - Read-only MCP profiles that hide write/destructive tools from LLM clients
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
//! use turbomcp_server::{VisibilityConfig, VisibilityLayer, VisibilitySessionGuard};
//! use turbomcp_types::component::ComponentFilter;
//!
//! // Create a visibility layer that hides admin tools by default
//! let layer = VisibilityLayer::new(server)
//!     .with_disabled(ComponentFilter::with_tags(["admin"]))
//!     .with_disabled_tools(["delete_all", "reset_database"]);
//!
//! // Or apply a config loaded by a consumer such as TurboVault
//! let layer = VisibilityLayer::new(server)
//!     .with_visibility_config(
//!         VisibilityConfig::new()
//!             .with_allowed_tools(["search", "read_note", "list_notes"])
//!             .require_read_only_tools(),
//!     );
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

use std::collections::{BTreeSet, HashSet};
use std::sync::Arc;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use turbomcp_core::context::RequestContext;
use turbomcp_core::error::{McpError, McpResult};
use turbomcp_core::handler::McpHandler;
use turbomcp_types::{
    ComponentFilter, ComponentMeta, Prompt, PromptResult, Resource, ResourceResult,
    ResourceTemplate, Tool, ToolResult,
};

/// Type alias for session visibility maps to reduce complexity.
type SessionVisibilityMap = Arc<dashmap::DashMap<String, HashSet<String>>>;

/// Exact-name visibility rules for one MCP component family.
///
/// Matching is case-sensitive and exact. Deny rules win over allow rules. When
/// `allow` is `Some`, only matching identifiers are enabled; when it is `None`,
/// every identifier is enabled unless it appears in `deny`. Hidden identifiers
/// remain callable/readable/gettable, but are omitted from `list_*` responses.
///
/// Resources and resource templates are matched by both `name` and URI/URI
/// template, so config authors can use whichever identifier is most stable for
/// their server.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ComponentVisibilityRules {
    /// Exact identifiers to enable. `None` means no allowlist is configured.
    ///
    /// An empty set inside `Some` intentionally disables the entire component
    /// family.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow: Option<BTreeSet<String>>,

    /// Exact identifiers to disable from both listing and direct use.
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub deny: BTreeSet<String>,

    /// Exact identifiers to omit from lists while still permitting direct use.
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub hide: BTreeSet<String>,
}

impl ComponentVisibilityRules {
    /// Create rules that enable and list everything unless denied or hidden.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Create rules that enable only the given exact identifiers.
    #[must_use]
    pub fn allow<I, S>(names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            allow: Some(collect_names(names)),
            deny: BTreeSet::new(),
            hide: BTreeSet::new(),
        }
    }

    /// Create rules that disable the given exact identifiers.
    #[must_use]
    pub fn deny<I, S>(names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            allow: None,
            deny: collect_names(names),
            hide: BTreeSet::new(),
        }
    }

    /// Create rules that omit the given exact identifiers from lists.
    #[must_use]
    pub fn hide<I, S>(names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            allow: None,
            deny: BTreeSet::new(),
            hide: collect_names(names),
        }
    }

    /// Replace the allowlist with the given exact identifiers.
    #[must_use]
    pub fn with_allowed<I, S>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.allow = Some(collect_names(names));
        self
    }

    /// Replace the denylist with the given exact identifiers.
    #[must_use]
    pub fn with_disabled<I, S>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.deny = collect_names(names);
        self
    }

    /// Replace the hidden list with the given exact identifiers.
    #[must_use]
    pub fn with_hidden<I, S>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.hide = collect_names(names);
        self
    }

    /// Check whether a single exact identifier is enabled for direct use.
    #[must_use]
    pub fn is_enabled(&self, identifier: &str) -> bool {
        self.is_enabled_any([identifier])
    }

    /// Check whether any identifier for the same component is enabled for direct use.
    ///
    /// Denying any identifier disables the component. When an allowlist is
    /// present, at least one identifier must be allowlisted.
    #[must_use]
    pub fn is_enabled_any<'a, I>(&self, identifiers: I) -> bool
    where
        I: IntoIterator<Item = &'a str>,
    {
        let identifiers = identifiers.into_iter().collect::<Vec<_>>();

        if identifiers
            .iter()
            .any(|identifier| self.deny.contains(*identifier))
        {
            return false;
        }

        self.allow.as_ref().is_none_or(|allow| {
            identifiers
                .iter()
                .any(|identifier| allow.contains(*identifier))
        })
    }

    /// Check whether a single exact identifier should appear in list responses.
    #[must_use]
    pub fn is_listed(&self, identifier: &str) -> bool {
        self.is_listed_any([identifier])
    }

    /// Check whether any identifier for the same component should appear in lists.
    #[must_use]
    pub fn is_listed_any<'a, I>(&self, identifiers: I) -> bool
    where
        I: IntoIterator<Item = &'a str>,
    {
        let identifiers = identifiers.into_iter().collect::<Vec<_>>();

        self.is_enabled_any(identifiers.iter().copied())
            && !identifiers
                .iter()
                .any(|identifier| self.hide.contains(*identifier))
    }
}

/// Complete runtime visibility configuration for an MCP server.
///
/// This type is intentionally serializable so applications can deserialize a
/// user-facing config file and pass it directly to
/// [`VisibilityLayer::with_visibility_config`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct VisibilityConfig {
    /// Exact-name rules for tools.
    pub tools: ComponentVisibilityRules,
    /// Exact-name rules for resources. Matches resource `name` or `uri`.
    pub resources: ComponentVisibilityRules,
    /// Exact-name rules for resource templates. Matches `name` or `uriTemplate`.
    pub resource_templates: ComponentVisibilityRules,
    /// Exact-name rules for prompts.
    pub prompts: ComponentVisibilityRules,
    /// Hide every tool that is not explicitly annotated `readOnlyHint: true`.
    ///
    /// Tools marked `destructiveHint: true` are hidden even if they also carry a
    /// read-only hint, because conflicting safety hints should fail closed.
    #[serde(skip_serializing_if = "is_false")]
    pub require_read_only_tools: bool,
}

impl VisibilityConfig {
    /// Create an empty visibility config.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Enable only the named tools.
    #[must_use]
    pub fn with_allowed_tools<I, S>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.tools = self.tools.with_allowed(names);
        self
    }

    /// Disable the named tools from both listing and direct calls.
    #[must_use]
    pub fn with_disabled_tools<I, S>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.tools = self.tools.with_disabled(names);
        self
    }

    /// Hide the named tools from `tools/list` while still permitting direct calls.
    #[must_use]
    pub fn with_hidden_tools<I, S>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.tools = self.tools.with_hidden(names);
        self
    }

    /// Enable only the named resources. Names and URIs both match.
    #[must_use]
    pub fn with_allowed_resources<I, S>(mut self, identifiers: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.resources = self.resources.with_allowed(identifiers);
        self
    }

    /// Disable the named resources from both listing and direct reads.
    #[must_use]
    pub fn with_disabled_resources<I, S>(mut self, identifiers: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.resources = self.resources.with_disabled(identifiers);
        self
    }

    /// Hide the named resources from list responses while still permitting reads.
    #[must_use]
    pub fn with_hidden_resources<I, S>(mut self, identifiers: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.resources = self.resources.with_hidden(identifiers);
        self
    }

    /// Enable only the named resource templates. Names and URI templates both match.
    #[must_use]
    pub fn with_allowed_resource_templates<I, S>(mut self, identifiers: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.resource_templates = self.resource_templates.with_allowed(identifiers);
        self
    }

    /// Disable the named resource templates from list responses.
    #[must_use]
    pub fn with_disabled_resource_templates<I, S>(mut self, identifiers: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.resource_templates = self.resource_templates.with_disabled(identifiers);
        self
    }

    /// Hide the named resource templates from list responses.
    #[must_use]
    pub fn with_hidden_resource_templates<I, S>(mut self, identifiers: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.resource_templates = self.resource_templates.with_hidden(identifiers);
        self
    }

    /// Enable only the named prompts.
    #[must_use]
    pub fn with_allowed_prompts<I, S>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.prompts = self.prompts.with_allowed(names);
        self
    }

    /// Disable the named prompts from both listing and direct gets.
    #[must_use]
    pub fn with_disabled_prompts<I, S>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.prompts = self.prompts.with_disabled(names);
        self
    }

    /// Hide the named prompts from `prompts/list` while still permitting gets.
    #[must_use]
    pub fn with_hidden_prompts<I, S>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.prompts = self.prompts.with_hidden(names);
        self
    }

    /// Hide every tool that is not explicitly annotated read-only.
    #[must_use]
    pub fn require_read_only_tools(mut self) -> Self {
        self.require_read_only_tools = true;
        self
    }
}

fn collect_names<I, S>(names: I) -> BTreeSet<String>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    names.into_iter().map(Into::into).collect()
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn is_explicit_read_only_tool(tool: &Tool) -> bool {
    tool.annotations.as_ref().is_some_and(|annotations| {
        annotations.read_only_hint == Some(true) && annotations.destructive_hint != Some(true)
    })
}

/// RAII guard that automatically cleans up session visibility state when dropped.
///
/// This is the recommended way to manage session visibility lifetime. Create a guard
/// at the start of a session and let it clean up automatically when the session ends.
///
/// # Example
///
/// ```rust,ignore
/// use turbomcp_server::VisibilityLayer;
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
    /// Exact-name visibility rules for tools
    tool_rules: ComponentVisibilityRules,
    /// Exact-name visibility rules for resources
    resource_rules: ComponentVisibilityRules,
    /// Exact-name visibility rules for resource templates
    resource_template_rules: ComponentVisibilityRules,
    /// Exact-name visibility rules for prompts
    prompt_rules: ComponentVisibilityRules,
    /// When true, only explicitly read-only tools are visible/callable
    read_only_tools_required: bool,
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
            .field(
                "tool_allow_count",
                &self.tool_rules.allow.as_ref().map(BTreeSet::len),
            )
            .field("tool_deny_count", &self.tool_rules.deny.len())
            .field("tool_hide_count", &self.tool_rules.hide.len())
            .field("read_only_tools_required", &self.read_only_tools_required)
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
            tool_rules: ComponentVisibilityRules::new(),
            resource_rules: ComponentVisibilityRules::new(),
            resource_template_rules: ComponentVisibilityRules::new(),
            prompt_rules: ComponentVisibilityRules::new(),
            read_only_tools_required: false,
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

    /// Replace exact-name rules with a complete visibility configuration.
    ///
    /// This is the easiest integration point for applications that expose
    /// user-facing config. Tag/session visibility configured through
    /// [`with_disabled`](Self::with_disabled) and
    /// [`enable_for_session`](Self::enable_for_session) remains independent.
    #[must_use]
    pub fn with_visibility_config(mut self, config: VisibilityConfig) -> Self {
        self.tool_rules = config.tools;
        self.resource_rules = config.resources;
        self.resource_template_rules = config.resource_templates;
        self.prompt_rules = config.prompts;
        self.read_only_tools_required = config.require_read_only_tools;
        self
    }

    /// Return the currently configured exact-name visibility rules.
    #[must_use]
    pub fn visibility_config(&self) -> VisibilityConfig {
        VisibilityConfig {
            tools: self.tool_rules.clone(),
            resources: self.resource_rules.clone(),
            resource_templates: self.resource_template_rules.clone(),
            prompts: self.prompt_rules.clone(),
            require_read_only_tools: self.read_only_tools_required,
        }
    }

    /// Enable only the named tools.
    ///
    /// This filters both `tools/list` and `tools/call`. Exact denies configured
    /// through [`with_disabled_tools`](Self::with_disabled_tools) still win.
    #[must_use]
    pub fn with_allowed_tools<I, S>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.tool_rules = self.tool_rules.with_allowed(names);
        self
    }

    /// Disable the named tools from `tools/list` and reject matching calls as not found.
    #[must_use]
    pub fn with_disabled_tools<I, S>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.tool_rules = self.tool_rules.with_disabled(names);
        self
    }

    /// Hide the named tools from `tools/list` while still permitting direct calls.
    #[must_use]
    pub fn with_hidden_tools<I, S>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.tool_rules = self.tool_rules.with_hidden(names);
        self
    }

    /// Enable only the named resources. Resource names and URIs both match.
    #[must_use]
    pub fn with_allowed_resources<I, S>(mut self, identifiers: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.resource_rules = self.resource_rules.with_allowed(identifiers);
        self
    }

    /// Disable the named resources. Resource names and URIs both match.
    #[must_use]
    pub fn with_disabled_resources<I, S>(mut self, identifiers: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.resource_rules = self.resource_rules.with_disabled(identifiers);
        self
    }

    /// Hide the named resources from list responses while still permitting reads.
    #[must_use]
    pub fn with_hidden_resources<I, S>(mut self, identifiers: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.resource_rules = self.resource_rules.with_hidden(identifiers);
        self
    }

    /// Enable only the named resource templates. Names and URI templates both match.
    #[must_use]
    pub fn with_allowed_resource_templates<I, S>(mut self, identifiers: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.resource_template_rules = self.resource_template_rules.with_allowed(identifiers);
        self
    }

    /// Disable the named resource templates. Names and URI templates both match.
    #[must_use]
    pub fn with_disabled_resource_templates<I, S>(mut self, identifiers: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.resource_template_rules = self.resource_template_rules.with_disabled(identifiers);
        self
    }

    /// Hide the named resource templates from list responses.
    #[must_use]
    pub fn with_hidden_resource_templates<I, S>(mut self, identifiers: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.resource_template_rules = self.resource_template_rules.with_hidden(identifiers);
        self
    }

    /// Enable only the named prompts.
    #[must_use]
    pub fn with_allowed_prompts<I, S>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.prompt_rules = self.prompt_rules.with_allowed(names);
        self
    }

    /// Disable the named prompts.
    #[must_use]
    pub fn with_disabled_prompts<I, S>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.prompt_rules = self.prompt_rules.with_disabled(names);
        self
    }

    /// Hide the named prompts from `prompts/list` while still permitting gets.
    #[must_use]
    pub fn with_hidden_prompts<I, S>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.prompt_rules = self.prompt_rules.with_hidden(names);
        self
    }

    /// Hide every tool that is not explicitly annotated `readOnlyHint: true`.
    ///
    /// This is useful for AI clients that should not be offered mutating
    /// operations. Tools with no annotation are hidden; annotation gaps should
    /// fail closed.
    #[must_use]
    pub fn require_read_only_tools(mut self) -> Self {
        self.read_only_tools_required = true;
        self
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

    /// Check if a tool is enabled/callable under exact-name, annotation, tag, and session rules.
    fn is_tool_enabled(&self, tool: &Tool, session_id: Option<&str>) -> bool {
        if !self.tool_rules.is_enabled(&tool.name) {
            return false;
        }

        if self.read_only_tools_required && !is_explicit_read_only_tool(tool) {
            return false;
        }

        let meta = ComponentMeta::from_meta_value(tool.meta.as_ref());
        self.is_visible(&meta, session_id)
    }

    /// Check if a tool is listed under exact-name, annotation, tag, and session rules.
    fn is_tool_listed(&self, tool: &Tool, session_id: Option<&str>) -> bool {
        self.is_tool_enabled(tool, session_id) && self.tool_rules.is_listed(&tool.name)
    }

    /// Check if an unlisted tool may be called.
    fn is_unlisted_tool_callable(&self, name: &str) -> bool {
        self.tool_rules.is_enabled(name) && !self.read_only_tools_required
    }

    /// Check if a resource is enabled/readable under exact-name, tag, and session rules.
    fn is_resource_enabled(&self, resource: &Resource, session_id: Option<&str>) -> bool {
        if !self
            .resource_rules
            .is_enabled_any([resource.name.as_str(), resource.uri.as_str()])
        {
            return false;
        }

        let meta = ComponentMeta::from_meta_value(resource.meta.as_ref());
        self.is_visible(&meta, session_id)
    }

    /// Check if a resource is listed under exact-name, tag, and session rules.
    fn is_resource_listed(&self, resource: &Resource, session_id: Option<&str>) -> bool {
        self.is_resource_enabled(resource, session_id)
            && self
                .resource_rules
                .is_listed_any([resource.name.as_str(), resource.uri.as_str()])
    }

    /// Check exact-name visibility for a resource read when no listed metadata is available.
    fn is_unlisted_resource_readable(&self, uri: &str) -> bool {
        self.resource_rules.is_enabled(uri)
    }

    /// Check if a resource template is listed under exact-name, tag, and session rules.
    fn is_resource_template_listed(
        &self,
        template: &ResourceTemplate,
        session_id: Option<&str>,
    ) -> bool {
        if !self
            .resource_template_rules
            .is_listed_any([template.name.as_str(), template.uri_template.as_str()])
        {
            return false;
        }

        let meta = ComponentMeta::from_meta_value(template.meta.as_ref());
        self.is_visible(&meta, session_id)
    }

    /// Check if a prompt is enabled/gettable under exact-name, tag, and session rules.
    fn is_prompt_enabled(&self, prompt: &Prompt, session_id: Option<&str>) -> bool {
        if !self.prompt_rules.is_enabled(&prompt.name) {
            return false;
        }

        let meta = ComponentMeta::from_meta_value(prompt.meta.as_ref());
        self.is_visible(&meta, session_id)
    }

    /// Check if a prompt is listed under exact-name, tag, and session rules.
    fn is_prompt_listed(&self, prompt: &Prompt, session_id: Option<&str>) -> bool {
        self.is_prompt_enabled(prompt, session_id) && self.prompt_rules.is_listed(&prompt.name)
    }

    /// Check exact-name visibility for a prompt get when no listed metadata is available.
    fn is_unlisted_prompt_gettable(&self, name: &str) -> bool {
        self.prompt_rules.is_enabled(name)
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
            .filter(|tool| self.is_tool_listed(tool, None))
            .collect()
    }

    fn list_resources(&self) -> Vec<Resource> {
        self.inner
            .list_resources()
            .into_iter()
            .filter(|resource| self.is_resource_listed(resource, None))
            .collect()
    }

    fn list_resource_templates(&self) -> Vec<ResourceTemplate> {
        self.inner
            .list_resource_templates()
            .into_iter()
            .filter(|template| self.is_resource_template_listed(template, None))
            .collect()
    }

    fn list_prompts(&self) -> Vec<Prompt> {
        self.inner
            .list_prompts()
            .into_iter()
            .filter(|prompt| self.is_prompt_listed(prompt, None))
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
            // Check listed tool metadata when available; unlisted dynamic tools
            // can still be governed by exact-name rules.
            let tools = self.inner.list_tools();
            let tool = tools.iter().find(|t| t.name == name);

            if let Some(tool) = tool {
                if !self.is_tool_enabled(tool, ctx.session_id()) {
                    return Err(McpError::tool_not_found(name));
                }
            } else if !self.is_unlisted_tool_callable(name) {
                return Err(McpError::tool_not_found(name));
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
            // Check listed resource metadata when available; unlisted dynamic
            // resources can still be governed by exact-name rules.
            let resources = self.inner.list_resources();
            let resource = resources.iter().find(|r| r.uri == uri);

            if let Some(resource) = resource {
                if !self.is_resource_enabled(resource, ctx.session_id()) {
                    return Err(McpError::resource_not_found(uri));
                }
            } else if !self.is_unlisted_resource_readable(uri) {
                return Err(McpError::resource_not_found(uri));
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
            // Check listed prompt metadata when available; unlisted dynamic
            // prompts can still be governed by exact-name rules.
            let prompts = self.inner.list_prompts();
            let prompt = prompts.iter().find(|p| p.name == name);

            if let Some(prompt) = prompt {
                if !self.is_prompt_enabled(prompt, ctx.session_id()) {
                    return Err(McpError::prompt_not_found(name));
                }
            } else if !self.is_unlisted_prompt_gettable(name) {
                return Err(McpError::prompt_not_found(name));
            }

            self.inner.get_prompt(name, args, ctx).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use turbomcp_types::ToolAnnotations;

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
                    annotations: Some(ToolAnnotations::default().with_read_only(true)),
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
                    annotations: Some(ToolAnnotations::default().with_destructive(true)),
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
            vec![
                Resource {
                    uri: "vault://public".to_string(),
                    name: "public_resource".to_string(),
                    meta: Some({
                        let mut m = std::collections::HashMap::new();
                        m.insert("tags".to_string(), serde_json::json!(["public"]));
                        m
                    }),
                    ..Default::default()
                },
                Resource {
                    uri: "vault://admin".to_string(),
                    name: "admin_resource".to_string(),
                    meta: Some({
                        let mut m = std::collections::HashMap::new();
                        m.insert("tags".to_string(), serde_json::json!(["admin"]));
                        m
                    }),
                    ..Default::default()
                },
            ]
        }

        fn list_resource_templates(&self) -> Vec<ResourceTemplate> {
            vec![ResourceTemplate {
                uri_template: "vault://notes/{id}".to_string(),
                name: "note_template".to_string(),
                meta: Some({
                    let mut m = std::collections::HashMap::new();
                    m.insert("tags".to_string(), serde_json::json!(["public"]));
                    m
                }),
                ..Default::default()
            }]
        }

        fn list_prompts(&self) -> Vec<Prompt> {
            vec![
                Prompt {
                    name: "public_prompt".to_string(),
                    meta: Some({
                        let mut m = std::collections::HashMap::new();
                        m.insert("tags".to_string(), serde_json::json!(["public"]));
                        m
                    }),
                    ..Default::default()
                },
                Prompt {
                    name: "admin_prompt".to_string(),
                    meta: Some({
                        let mut m = std::collections::HashMap::new();
                        m.insert("tags".to_string(), serde_json::json!(["admin"]));
                        m
                    }),
                    ..Default::default()
                },
            ]
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
            uri: &'a str,
            _ctx: &'a RequestContext,
        ) -> impl std::future::Future<Output = McpResult<ResourceResult>>
        + turbomcp_core::marker::MaybeSend
        + 'a {
            async move { Ok(ResourceResult::text(uri, format!("Read {}", uri))) }
        }

        fn get_prompt<'a>(
            &'a self,
            name: &'a str,
            _args: Option<serde_json::Value>,
            _ctx: &'a RequestContext,
        ) -> impl std::future::Future<Output = McpResult<PromptResult>>
        + turbomcp_core::marker::MaybeSend
        + 'a {
            async move { Ok(PromptResult::user(format!("Prompt {}", name))) }
        }
    }

    #[derive(Clone, Debug)]
    struct DynamicHandler;

    #[allow(clippy::manual_async_fn)]
    impl McpHandler for DynamicHandler {
        fn server_info(&self) -> turbomcp_types::ServerInfo {
            turbomcp_types::ServerInfo::new("dynamic", "1.0.0")
        }

        fn list_tools(&self) -> Vec<Tool> {
            vec![]
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
            async move { Ok(ToolResult::text(format!("Dynamic {}", name))) }
        }

        fn read_resource<'a>(
            &'a self,
            uri: &'a str,
            _ctx: &'a RequestContext,
        ) -> impl std::future::Future<Output = McpResult<ResourceResult>>
        + turbomcp_core::marker::MaybeSend
        + 'a {
            async move { Ok(ResourceResult::text(uri, format!("Dynamic {}", uri))) }
        }

        fn get_prompt<'a>(
            &'a self,
            name: &'a str,
            _args: Option<serde_json::Value>,
            _ctx: &'a RequestContext,
        ) -> impl std::future::Future<Output = McpResult<PromptResult>>
        + turbomcp_core::marker::MaybeSend
        + 'a {
            async move { Ok(PromptResult::user(format!("Dynamic {}", name))) }
        }
    }

    fn tool_names(layer: &VisibilityLayer<MockHandler>) -> Vec<String> {
        layer
            .list_tools()
            .into_iter()
            .map(|tool| tool.name)
            .collect()
    }

    #[test]
    fn test_component_visibility_rules_deny_wins() {
        let rules = ComponentVisibilityRules::allow(["search", "delete"]).with_disabled(["delete"]);

        assert!(rules.is_enabled("search"));
        assert!(rules.is_listed("search"));
        assert!(!rules.is_enabled("delete"));
        assert!(!rules.is_listed("delete"));
        assert!(!rules.is_enabled("unknown"));
    }

    #[test]
    fn test_component_visibility_rules_match_aliases() {
        let rules = ComponentVisibilityRules::allow(["vault://public"]);

        assert!(rules.is_enabled_any(["public_resource", "vault://public"]));
        assert!(!rules.is_enabled_any(["public_resource", "vault://private"]));
    }

    #[test]
    fn test_component_visibility_rules_can_hide_without_disabling() {
        let rules = ComponentVisibilityRules::hide(["advanced_tool"]);

        assert!(rules.is_enabled("advanced_tool"));
        assert!(!rules.is_listed("advanced_tool"));
        assert!(rules.is_enabled("public_tool"));
        assert!(rules.is_listed("public_tool"));
    }

    #[test]
    fn test_visibility_config_round_trips_serialization() {
        let config = VisibilityConfig::new()
            .with_allowed_tools(["search", "read_note"])
            .with_disabled_tools(["delete_note"])
            .with_hidden_tools(["advanced_graph"])
            .with_allowed_resources(["vault://public"])
            .with_allowed_prompts(["summarize"])
            .require_read_only_tools();

        let json = serde_json::to_string(&config).expect("visibility config serializes");
        let decoded: VisibilityConfig =
            serde_json::from_str(&json).expect("visibility config deserializes");

        assert_eq!(decoded, config);
    }

    #[test]
    fn test_empty_tool_allowlist_hides_all_tools() {
        let layer =
            VisibilityLayer::new(MockHandler).with_allowed_tools(std::iter::empty::<&str>());

        assert!(layer.list_tools().is_empty());
    }

    #[test]
    fn test_conflicting_read_only_and_destructive_hints_fail_closed() {
        let tool = Tool {
            name: "conflicting_tool".to_string(),
            annotations: Some(
                ToolAnnotations::default()
                    .with_read_only(true)
                    .with_destructive(true),
            ),
            ..Default::default()
        };

        assert!(!is_explicit_read_only_tool(&tool));
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
    fn test_exact_tool_allowlist_reduces_list_surface() {
        let layer = VisibilityLayer::new(MockHandler).with_allowed_tools(["public_tool"]);

        assert_eq!(tool_names(&layer), vec!["public_tool"]);
    }

    #[test]
    fn test_exact_tool_denylist_wins_over_allowlist() {
        let layer = VisibilityLayer::new(MockHandler)
            .with_allowed_tools(["public_tool", "admin_tool"])
            .with_disabled_tools(["public_tool"]);

        assert_eq!(tool_names(&layer), vec!["admin_tool"]);
    }

    #[test]
    fn test_layer_clone_has_independent_exact_rules() {
        let base = VisibilityLayer::new(MockHandler);
        let narrowed = base.clone().with_allowed_tools(["public_tool"]);

        assert_eq!(base.list_tools().len(), 2);
        assert_eq!(tool_names(&narrowed), vec!["public_tool"]);
    }

    #[test]
    fn test_with_disabled_tools_replaces_previous_denylist() {
        let layer = VisibilityLayer::new(MockHandler)
            .with_disabled_tools(["public_tool"])
            .with_disabled_tools(["admin_tool"]);

        assert_eq!(tool_names(&layer), vec!["public_tool"]);
    }

    #[tokio::test]
    async fn test_hidden_tool_is_not_listed_but_remains_callable() {
        let layer = VisibilityLayer::new(MockHandler).with_hidden_tools(["public_tool"]);
        let ctx = RequestContext::default();

        assert_eq!(tool_names(&layer), vec!["admin_tool"]);

        let result = layer
            .call_tool("public_tool", serde_json::json!({}), &ctx)
            .await
            .expect("hidden but enabled tool should remain callable");
        assert_eq!(result.first_text(), Some("Called public_tool"));
    }

    #[tokio::test]
    async fn test_disabled_tool_wins_over_hidden_tool() {
        let layer = VisibilityLayer::new(MockHandler)
            .with_hidden_tools(["public_tool"])
            .with_disabled_tools(["public_tool"]);
        let ctx = RequestContext::default();

        assert!(!tool_names(&layer).contains(&"public_tool".to_string()));

        let err = layer
            .call_tool("public_tool", serde_json::json!({}), &ctx)
            .await
            .expect_err("disabled tool should not be callable even if hidden");
        assert_eq!(err.kind, turbomcp_core::error::ErrorKind::ToolNotFound);
    }

    #[test]
    fn test_tag_disabled_tool_stays_hidden_despite_name_allowlist() {
        let layer = VisibilityLayer::new(MockHandler)
            .with_allowed_tools(["admin_tool"])
            .disable_tags(["admin"]);

        assert!(layer.list_tools().is_empty());
    }

    #[test]
    fn test_visibility_config_getter_reflects_builder_methods() {
        let layer = VisibilityLayer::new(MockHandler)
            .with_allowed_tools(["public_tool"])
            .with_disabled_resources(["vault://admin"])
            .with_hidden_prompts(["admin_prompt"])
            .require_read_only_tools();

        let config = layer.visibility_config();

        assert!(config.tools.allow.unwrap().contains("public_tool"));
        assert!(config.resources.deny.contains("vault://admin"));
        assert!(config.prompts.hide.contains("admin_prompt"));
        assert!(config.require_read_only_tools);
    }

    #[tokio::test]
    async fn test_disabled_tool_call_returns_not_found() {
        let layer = VisibilityLayer::new(MockHandler).with_disabled_tools(["public_tool"]);
        let ctx = RequestContext::default();

        let err = layer
            .call_tool("public_tool", serde_json::json!({}), &ctx)
            .await
            .expect_err("hidden tool calls should be rejected");

        assert_eq!(err.kind, turbomcp_core::error::ErrorKind::ToolNotFound);
    }

    #[tokio::test]
    async fn test_session_enable_allows_hidden_tagged_tool_call() {
        let layer = VisibilityLayer::new(MockHandler).disable_tags(["admin"]);
        let ctx = RequestContext::default().with_session_id("session1");

        let err = layer
            .call_tool("admin_tool", serde_json::json!({}), &ctx)
            .await
            .expect_err("globally hidden tagged tool should be rejected");
        assert_eq!(err.kind, turbomcp_core::error::ErrorKind::ToolNotFound);

        layer.enable_for_session("session1", &["admin".to_string()]);

        let result = layer
            .call_tool("admin_tool", serde_json::json!({}), &ctx)
            .await
            .expect("session-enabled tagged tool should pass through");
        assert_eq!(result.first_text(), Some("Called admin_tool"));
    }

    #[tokio::test]
    async fn test_session_disable_blocks_visible_tagged_tool_call() {
        let layer = VisibilityLayer::new(MockHandler);
        let ctx = RequestContext::default().with_session_id("session1");

        let result = layer
            .call_tool("public_tool", serde_json::json!({}), &ctx)
            .await
            .expect("public tool should initially pass through");
        assert_eq!(result.first_text(), Some("Called public_tool"));

        layer.disable_for_session("session1", &["public".to_string()]);

        let err = layer
            .call_tool("public_tool", serde_json::json!({}), &ctx)
            .await
            .expect_err("session-disabled tagged tool should be rejected");
        assert_eq!(err.kind, turbomcp_core::error::ErrorKind::ToolNotFound);
    }

    #[tokio::test]
    async fn test_exact_tool_policy_blocks_unlisted_dynamic_call() {
        let layer = VisibilityLayer::new(DynamicHandler).with_disabled_tools(["dynamic_tool"]);
        let ctx = RequestContext::default();

        let err = layer
            .call_tool("dynamic_tool", serde_json::json!({}), &ctx)
            .await
            .expect_err("denylisted dynamic tool calls should be rejected");

        assert_eq!(err.kind, turbomcp_core::error::ErrorKind::ToolNotFound);
    }

    #[tokio::test]
    async fn test_exact_tool_allowlist_can_permit_unlisted_dynamic_call() {
        let layer = VisibilityLayer::new(DynamicHandler).with_allowed_tools(["dynamic_tool"]);
        let ctx = RequestContext::default();

        let result = layer
            .call_tool("dynamic_tool", serde_json::json!({}), &ctx)
            .await
            .expect("allowlisted dynamic tool should pass through");

        assert_eq!(result.first_text(), Some("Dynamic dynamic_tool"));
    }

    #[tokio::test]
    async fn test_read_only_policy_blocks_unlisted_dynamic_tool() {
        let layer = VisibilityLayer::new(DynamicHandler)
            .with_allowed_tools(["dynamic_tool"])
            .require_read_only_tools();
        let ctx = RequestContext::default();

        let err = layer
            .call_tool("dynamic_tool", serde_json::json!({}), &ctx)
            .await
            .expect_err("read-only policy should fail closed without annotations");

        assert_eq!(err.kind, turbomcp_core::error::ErrorKind::ToolNotFound);
    }

    #[test]
    fn test_require_read_only_tools_hides_mutating_tools() {
        let layer = VisibilityLayer::new(MockHandler).require_read_only_tools();

        assert_eq!(tool_names(&layer), vec!["public_tool"]);
    }

    #[tokio::test]
    async fn test_disabled_resource_read_returns_not_found() {
        let layer = VisibilityLayer::new(MockHandler).with_disabled_resources(["vault://public"]);
        let ctx = RequestContext::default();

        let err = layer
            .read_resource("vault://public", &ctx)
            .await
            .expect_err("hidden resource reads should be rejected");

        assert_eq!(err.kind, turbomcp_core::error::ErrorKind::ResourceNotFound);
    }

    #[tokio::test]
    async fn test_resource_allowlist_by_name_allows_uri_read() {
        let layer = VisibilityLayer::new(MockHandler).with_allowed_resources(["public_resource"]);
        let ctx = RequestContext::default();

        let result = layer
            .read_resource("vault://public", &ctx)
            .await
            .expect("allowlisted resource name should permit URI read");

        assert_eq!(result.first_text(), Some("Read vault://public"));
    }

    #[tokio::test]
    async fn test_exact_resource_policy_blocks_unlisted_dynamic_read() {
        let layer =
            VisibilityLayer::new(DynamicHandler).with_disabled_resources(["vault://dynamic"]);
        let ctx = RequestContext::default();

        let err = layer
            .read_resource("vault://dynamic", &ctx)
            .await
            .expect_err("denylisted dynamic resources should be rejected");

        assert_eq!(err.kind, turbomcp_core::error::ErrorKind::ResourceNotFound);
    }

    #[tokio::test]
    async fn test_disabled_prompt_get_returns_not_found() {
        let layer = VisibilityLayer::new(MockHandler).with_disabled_prompts(["public_prompt"]);
        let ctx = RequestContext::default();

        let err = layer
            .get_prompt("public_prompt", None, &ctx)
            .await
            .expect_err("hidden prompts should be rejected");

        assert_eq!(err.kind, turbomcp_core::error::ErrorKind::PromptNotFound);
    }

    #[tokio::test]
    async fn test_exact_prompt_policy_blocks_unlisted_dynamic_get() {
        let layer = VisibilityLayer::new(DynamicHandler).with_disabled_prompts(["dynamic_prompt"]);
        let ctx = RequestContext::default();

        let err = layer
            .get_prompt("dynamic_prompt", None, &ctx)
            .await
            .expect_err("denylisted dynamic prompts should be rejected");

        assert_eq!(err.kind, turbomcp_core::error::ErrorKind::PromptNotFound);
    }

    #[test]
    fn test_visibility_config_applies_component_rules() {
        let config = VisibilityConfig::new()
            .with_allowed_tools(["public_tool"])
            .with_disabled_resources(["vault://admin"])
            .with_allowed_prompts(["public_prompt"])
            .with_allowed_resource_templates(["vault://notes/{id}"]);

        let layer = VisibilityLayer::new(MockHandler).with_visibility_config(config);

        assert_eq!(tool_names(&layer), vec!["public_tool"]);

        let resources = layer.list_resources();
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0].name, "public_resource");

        let prompts = layer.list_prompts();
        assert_eq!(prompts.len(), 1);
        assert_eq!(prompts[0].name, "public_prompt");

        let templates = layer.list_resource_templates();
        assert_eq!(templates.len(), 1);
        assert_eq!(templates[0].name, "note_template");
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
