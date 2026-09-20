//! Server composition through handler mounting.
//!
//! This module enables composing multiple MCP handlers into a single server,
//! with automatic namespacing through prefixes. This allows building modular
//! servers from smaller, focused handlers.
//!
//! # Example
//!
//! ```rust,ignore
//! use turbomcp_server::composite::CompositeHandler;
//!
//! // Create individual handlers
//! let weather = WeatherServer::new();
//! let news = NewsServer::new();
//!
//! // Compose into a single handler
//! let server = CompositeHandler::new("main-server", "1.0.0")
//!     .mount(weather, "weather")  // weather_get_forecast
//!     .mount(news, "news");       // news_get_headlines
//!
//! // All tools are namespaced: "weather_get_forecast", "news_get_headlines"
//! ```

use std::sync::Arc;

use turbomcp_core::context::RequestContext;
use turbomcp_core::error::{McpError, McpResult};
use turbomcp_core::handler::McpHandler;
use turbomcp_types::{
    Prompt, PromptResult, Resource, ResourceResult, ResourceTemplate, ServerCapabilities,
    ServerInfo, Tool, ToolResult,
};

/// A composite handler that mounts multiple handlers with prefixes.
///
/// This enables modular server design by combining multiple handlers into
/// a single namespace. Each mounted handler's tools, resources, and prompts
/// are automatically prefixed to avoid naming conflicts.
///
/// # Namespacing Rules
///
/// - **Tools**: `{prefix}_{tool_name}` (e.g., `weather_get_forecast`)
/// - **Resources**: `{prefix}://{original_uri}` (e.g., `weather://api/forecast`)
/// - **Prompts**: `{prefix}_{prompt_name}` (e.g., `weather_forecast_prompt`)
///
/// # Thread Safety
///
/// `CompositeHandler` is `Send + Sync` when all mounted handlers are.
#[derive(Clone)]
pub struct CompositeHandler {
    name: String,
    version: String,
    description: Option<String>,
    instructions: Option<String>,
    handlers: Arc<Vec<MountedHandler>>,
}

impl std::fmt::Debug for CompositeHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompositeHandler")
            .field("name", &self.name)
            .field("version", &self.version)
            .field("description", &self.description)
            .field("handler_count", &self.handlers.len())
            .finish()
    }
}

/// Wrapper struct for type erasure of McpHandler.
struct HandlerWrapper<H: McpHandler> {
    handler: H,
}

impl<H: McpHandler> HandlerWrapper<H> {
    fn new(handler: H) -> Self {
        Self { handler }
    }

    fn list_tools(&self) -> Vec<Tool> {
        self.handler.list_tools()
    }

    fn list_resources(&self) -> Vec<Resource> {
        self.handler.list_resources()
    }

    fn list_resource_templates(&self) -> Vec<ResourceTemplate> {
        self.handler.list_resource_templates()
    }

    fn list_prompts(&self) -> Vec<Prompt> {
        self.handler.list_prompts()
    }

    async fn call_tool(
        &self,
        name: &str,
        args: serde_json::Value,
        ctx: &RequestContext,
    ) -> McpResult<ToolResult> {
        self.handler.call_tool(name, args, ctx).await
    }

    async fn read_resource(&self, uri: &str, ctx: &RequestContext) -> McpResult<ResourceResult> {
        self.handler.read_resource(uri, ctx).await
    }

    async fn get_prompt(
        &self,
        name: &str,
        args: Option<serde_json::Value>,
        ctx: &RequestContext,
    ) -> McpResult<PromptResult> {
        self.handler.get_prompt(name, args, ctx).await
    }
}

impl<H: McpHandler> Clone for HandlerWrapper<H> {
    fn clone(&self) -> Self {
        Self {
            handler: self.handler.clone(),
        }
    }
}

/// Dynamic dispatch trait for type-erased handlers.
trait DynHandler: Send + Sync {
    fn dyn_clone(&self) -> Box<dyn DynHandler>;
    fn dyn_server_capabilities(&self) -> ServerCapabilities;
    fn dyn_list_tools(&self) -> Vec<Tool>;
    fn dyn_list_resources(&self) -> Vec<Resource>;
    fn dyn_list_resource_templates(&self) -> Vec<ResourceTemplate>;
    fn dyn_list_prompts(&self) -> Vec<Prompt>;
    fn dyn_call_tool<'a>(
        &'a self,
        name: &'a str,
        args: serde_json::Value,
        ctx: &'a RequestContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = McpResult<ToolResult>> + Send + 'a>>;
    fn dyn_read_resource<'a>(
        &'a self,
        uri: &'a str,
        ctx: &'a RequestContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = McpResult<ResourceResult>> + Send + 'a>>;
    fn dyn_get_prompt<'a>(
        &'a self,
        name: &'a str,
        args: Option<serde_json::Value>,
        ctx: &'a RequestContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = McpResult<PromptResult>> + Send + 'a>>;
    fn dyn_subscribe<'a>(
        &'a self,
        uri: &'a str,
        ctx: &'a RequestContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = McpResult<()>> + Send + 'a>>;
    fn dyn_unsubscribe<'a>(
        &'a self,
        uri: &'a str,
        ctx: &'a RequestContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = McpResult<()>> + Send + 'a>>;
    fn dyn_set_log_level<'a>(
        &'a self,
        level: &'a str,
        ctx: &'a RequestContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = McpResult<()>> + Send + 'a>>;
    fn dyn_complete<'a>(
        &'a self,
        params: serde_json::Value,
        ctx: &'a RequestContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = McpResult<serde_json::Value>> + Send + 'a>,
    >;
    fn dyn_on_roots_list_changed<'a>(
        &'a self,
        ctx: &'a RequestContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = McpResult<()>> + Send + 'a>>;
}

impl<H: McpHandler> DynHandler for HandlerWrapper<H> {
    fn dyn_clone(&self) -> Box<dyn DynHandler> {
        Box::new(self.clone())
    }

    fn dyn_server_capabilities(&self) -> ServerCapabilities {
        self.handler.server_capabilities()
    }

    fn dyn_list_tools(&self) -> Vec<Tool> {
        self.list_tools()
    }

    fn dyn_list_resources(&self) -> Vec<Resource> {
        self.list_resources()
    }

    fn dyn_list_resource_templates(&self) -> Vec<ResourceTemplate> {
        self.list_resource_templates()
    }

    fn dyn_list_prompts(&self) -> Vec<Prompt> {
        self.list_prompts()
    }

    fn dyn_call_tool<'a>(
        &'a self,
        name: &'a str,
        args: serde_json::Value,
        ctx: &'a RequestContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = McpResult<ToolResult>> + Send + 'a>>
    {
        Box::pin(self.call_tool(name, args, ctx))
    }

    fn dyn_read_resource<'a>(
        &'a self,
        uri: &'a str,
        ctx: &'a RequestContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = McpResult<ResourceResult>> + Send + 'a>>
    {
        Box::pin(self.read_resource(uri, ctx))
    }

    fn dyn_get_prompt<'a>(
        &'a self,
        name: &'a str,
        args: Option<serde_json::Value>,
        ctx: &'a RequestContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = McpResult<PromptResult>> + Send + 'a>>
    {
        Box::pin(self.get_prompt(name, args, ctx))
    }

    fn dyn_subscribe<'a>(
        &'a self,
        uri: &'a str,
        ctx: &'a RequestContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = McpResult<()>> + Send + 'a>> {
        Box::pin(self.handler.subscribe(uri, ctx))
    }

    fn dyn_unsubscribe<'a>(
        &'a self,
        uri: &'a str,
        ctx: &'a RequestContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = McpResult<()>> + Send + 'a>> {
        Box::pin(self.handler.unsubscribe(uri, ctx))
    }

    fn dyn_set_log_level<'a>(
        &'a self,
        level: &'a str,
        ctx: &'a RequestContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = McpResult<()>> + Send + 'a>> {
        Box::pin(self.handler.set_log_level(level, ctx))
    }

    fn dyn_complete<'a>(
        &'a self,
        params: serde_json::Value,
        ctx: &'a RequestContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = McpResult<serde_json::Value>> + Send + 'a>,
    > {
        Box::pin(self.handler.complete(params, ctx))
    }

    fn dyn_on_roots_list_changed<'a>(
        &'a self,
        ctx: &'a RequestContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = McpResult<()>> + Send + 'a>> {
        Box::pin(self.handler.on_roots_list_changed(ctx))
    }
}

/// Internal struct to hold a mounted handler with its prefix.
struct MountedHandler {
    prefix: String,
    handler: Box<dyn DynHandler>,
}

impl Clone for MountedHandler {
    fn clone(&self) -> Self {
        Self {
            prefix: self.prefix.clone(),
            handler: self.handler.dyn_clone(),
        }
    }
}

impl CompositeHandler {
    /// Create a new composite handler with the given name and version.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let server = CompositeHandler::new("my-server", "1.0.0");
    /// ```
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            description: None,
            instructions: None,
            handlers: Arc::new(Vec::new()),
        }
    }

    /// Set the server description.
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Set the `instructions` string returned by the `initialize` handshake.
    ///
    /// Mounted handlers' own instructions are deliberately not merged: each was
    /// written to describe a standalone server, and concatenating several of
    /// them yields guidance that contradicts the composite's prefixed tool
    /// names. Describe the composed surface here instead.
    #[must_use]
    pub fn with_instructions(mut self, instructions: impl Into<String>) -> Self {
        self.instructions = Some(instructions.into());
        self
    }

    /// Check a prefix before mounting.
    ///
    /// Beyond rejecting an exact duplicate, this rejects prefixes that *nest*.
    /// Mounted names are minted as `{prefix}_{name}`, so mounting at `x`
    /// (exposing a tool `y_z`) alongside `x_y` (exposing `z`) produces two
    /// entries both called `x_y_z`. `tools/list` then advertises a duplicate
    /// name, and because routing picks the longest matching prefix, every call
    /// for it reaches the second mount — the first tool becomes permanently
    /// unreachable, with no error anywhere.
    ///
    /// The charset check keeps minted names inside the one MCP allows for tool
    /// names, so a prefix cannot produce a name a client will reject.
    fn validate_prefix(&self, prefix: &str) -> Result<(), String> {
        if prefix.is_empty() || prefix.len() > 64 {
            return Err(format!(
                "prefix '{prefix}' must be between 1 and 64 characters"
            ));
        }
        if !prefix
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'.')
        {
            return Err(format!(
                "prefix '{prefix}' may contain only A-Z a-z 0-9 _ - . so that the \
                 names it mints stay valid MCP tool names"
            ));
        }

        for mounted in self.handlers.iter() {
            let other = mounted.prefix.as_str();
            if other == prefix {
                return Err(format!(
                    "duplicate prefix '{prefix}' - each mounted handler must have a unique prefix"
                ));
            }
            let nests = prefix
                .strip_prefix(other)
                .is_some_and(|rest| rest.starts_with('_'))
                || other
                    .strip_prefix(prefix)
                    .is_some_and(|rest| rest.starts_with('_'));
            if nests {
                return Err(format!(
                    "prefix '{prefix}' nests with already-mounted '{other}'; one could mint \
                     the same tool name as the other and silently shadow it"
                ));
            }
        }
        Ok(())
    }

    /// Mount a handler with the given prefix (panicking variant).
    ///
    /// All tools, resources, and prompts from the handler will be namespaced
    /// with the prefix. Prefer [`try_mount`](Self::try_mount) in production
    /// code — it returns a `Result` instead of panicking on duplicate prefixes,
    /// which is easier to reason about under dynamic configuration.
    ///
    /// This method is retained for builder-chain ergonomics in static setups
    /// (tests, examples, small servers where the prefix set is known at compile
    /// time). It may be deprecated in a future major version — new code should
    /// prefer `try_mount`.
    ///
    /// # Panics
    ///
    /// Panics if a handler with the same prefix is already mounted. This prevents
    /// silent shadowing of tools/resources/prompts which could lead to confusing
    /// runtime behavior.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let server = CompositeHandler::new("main", "1.0.0")
    ///     .mount(weather_handler, "weather")
    ///     .mount(news_handler, "news");
    /// ```
    #[must_use]
    pub fn mount<H: McpHandler>(mut self, handler: H, prefix: impl Into<String>) -> Self {
        let prefix = prefix.into();

        if let Err(e) = self.validate_prefix(&prefix) {
            panic!("CompositeHandler: {e}");
        }

        let handlers = Arc::make_mut(&mut self.handlers);
        handlers.push(MountedHandler {
            prefix,
            handler: Box::new(HandlerWrapper::new(handler)),
        });
        self
    }

    /// Try to mount a handler with the given prefix, returning an error on duplicate.
    ///
    /// This is the fallible version of [`mount`](Self::mount) and the
    /// recommended entry point for production code — duplicate prefixes
    /// become a recoverable error instead of a panic, which matters for
    /// servers that register handlers from user configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if a handler with the same prefix is already mounted.
    pub fn try_mount<H: McpHandler>(
        mut self,
        handler: H,
        prefix: impl Into<String>,
    ) -> Result<Self, String> {
        let prefix = prefix.into();

        self.validate_prefix(&prefix)?;

        let handlers = Arc::make_mut(&mut self.handlers);
        handlers.push(MountedHandler {
            prefix,
            handler: Box::new(HandlerWrapper::new(handler)),
        });
        Ok(self)
    }

    /// Get the number of mounted handlers.
    pub fn handler_count(&self) -> usize {
        self.handlers.len()
    }

    /// Get all mounted prefixes.
    pub fn prefixes(&self) -> Vec<&str> {
        self.handlers.iter().map(|h| h.prefix.as_str()).collect()
    }

    // ===== Internal Helpers =====

    /// Prefix a tool name.
    fn prefix_tool_name(prefix: &str, name: &str) -> String {
        format!("{}_{}", prefix, name)
    }

    /// Prefix a resource URI.
    fn prefix_resource_uri(prefix: &str, uri: &str) -> String {
        format!("{}://{}", prefix, uri)
    }

    /// Prefix a resource URI template.
    fn prefix_resource_template_uri(prefix: &str, uri_template: &str) -> String {
        format!("{}://{}", prefix, uri_template)
    }

    /// Rewrite resource URIs inside outgoing content blocks.
    ///
    /// `list_resources` presents a mount's URIs prefixed, and `read_resource`
    /// strips the prefix back off on the way in — but results travelling *out*
    /// were passed through verbatim. A `resource_link` in a tool result, or an
    /// embedded resource in a prompt message, therefore carried the mount's
    /// own unprefixed URI, which the client cannot read back: sending it to
    /// `resources/read` fails to match any mount.
    fn prefix_uris_in_content(prefix: &str, blocks: &mut [turbomcp_types::Content]) {
        use turbomcp_types::Content;
        for block in blocks {
            match block {
                Content::ResourceLink(link) => {
                    link.uri = Self::prefix_resource_uri(prefix, &link.uri);
                }
                Content::Resource(embedded) => match &mut embedded.resource {
                    turbomcp_types::ResourceContents::Text(text) => {
                        text.uri = Self::prefix_resource_uri(prefix, &text.uri);
                    }
                    turbomcp_types::ResourceContents::Blob(blob) => {
                        blob.uri = Self::prefix_resource_uri(prefix, &blob.uri);
                    }
                },
                _ => {}
            }
        }
    }

    /// Prefix a prompt name.
    fn prefix_prompt_name(prefix: &str, name: &str) -> String {
        format!("{}_{}", prefix, name)
    }

    /// Parse a prefixed tool name into (prefix, original_name).
    ///
    /// Pre-3.1 used `split_once('_')` which mis-split prefixes containing `_`
    /// (e.g., prefix `my_weather` + tool `get_forecast` → joined `my_weather_get_forecast`
    /// would split as `("my", "weather_get_forecast")`). The fix is to look up the
    /// matching mounted prefix using the registered handler list, longest-first so
    /// nested prefixes route correctly.
    fn parse_prefixed_tool<'a>(&self, name: &'a str) -> Option<(&'a str, &'a str)> {
        self.match_prefix(name, "_")
    }

    /// Parse a prefixed resource URI into (prefix, original_uri).
    fn parse_prefixed_uri<'a>(&self, uri: &'a str) -> Option<(&'a str, &'a str)> {
        self.match_prefix(uri, "://")
    }

    /// Parse a prefixed prompt name into (prefix, original_name).
    fn parse_prefixed_prompt<'a>(&self, name: &'a str) -> Option<(&'a str, &'a str)> {
        self.match_prefix(name, "_")
    }

    /// Find a registered prefix that `s` starts with, followed by the given separator.
    /// Returns `(prefix, remainder)`. Longest prefix wins to handle nested mount points.
    /// Avoids per-iteration string allocation (the previous `format!("{prefix}{sep}")`
    /// was O(n × prefix.len) on every routed call).
    fn match_prefix<'a>(&self, s: &'a str, sep: &str) -> Option<(&'a str, &'a str)> {
        let sep_bytes = sep.as_bytes();
        let mut best: Option<(&str, &'a str)> = None;
        for h in self.handlers.iter() {
            let prefix_len = h.prefix.len();
            let total = prefix_len + sep.len();
            if s.len() < total {
                continue;
            }
            if !s.is_char_boundary(prefix_len) || !s.is_char_boundary(total) {
                continue;
            }
            if &s.as_bytes()[..prefix_len] != h.prefix.as_bytes() {
                continue;
            }
            if &s.as_bytes()[prefix_len..total] != sep_bytes {
                continue;
            }
            let prefix_slice = &s[..prefix_len];
            let rest = &s[total..];
            match best {
                Some((p, _)) if p.len() >= prefix_len => {}
                _ => best = Some((prefix_slice, rest)),
            }
        }
        best
    }

    /// Find a handler by prefix.
    fn find_handler(&self, prefix: &str) -> Option<&MountedHandler> {
        self.handlers.iter().find(|h| h.prefix == prefix)
    }
}

fn merge_optional_bool(left: Option<bool>, right: Option<bool>) -> Option<bool> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left || right),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}

fn merge_server_capabilities(target: &mut ServerCapabilities, source: ServerCapabilities) {
    if let Some(source_tools) = source.tools {
        if let Some(target_tools) = target.tools.as_mut() {
            target_tools.list_changed =
                merge_optional_bool(target_tools.list_changed, source_tools.list_changed);
        } else {
            target.tools = Some(source_tools);
        }
    }

    if let Some(source_resources) = source.resources {
        if let Some(target_resources) = target.resources.as_mut() {
            target_resources.subscribe =
                merge_optional_bool(target_resources.subscribe, source_resources.subscribe);
            target_resources.list_changed =
                merge_optional_bool(target_resources.list_changed, source_resources.list_changed);
        } else {
            target.resources = Some(source_resources);
        }
    }

    if let Some(source_prompts) = source.prompts {
        if let Some(target_prompts) = target.prompts.as_mut() {
            target_prompts.list_changed =
                merge_optional_bool(target_prompts.list_changed, source_prompts.list_changed);
        } else {
            target.prompts = Some(source_prompts);
        }
    }

    if target.logging.is_none() {
        target.logging = source.logging;
    }
    if target.completions.is_none() {
        target.completions = source.completions;
    }
    if target.tasks.is_none() {
        target.tasks = source.tasks;
    }

    if let Some(source_extensions) = source.extensions {
        target
            .extensions
            .get_or_insert_with(Default::default)
            .extend(source_extensions);
    }
    if let Some(source_experimental) = source.experimental {
        target
            .experimental
            .get_or_insert_with(Default::default)
            .extend(source_experimental);
    }
}

#[allow(clippy::manual_async_fn)]
impl McpHandler for CompositeHandler {
    fn server_info(&self) -> ServerInfo {
        let mut info = ServerInfo::new(&self.name, &self.version);
        if let Some(ref desc) = self.description {
            info = info.with_description(desc);
        }
        info
    }

    fn instructions(&self) -> Option<String> {
        self.instructions.clone()
    }

    fn server_capabilities(&self) -> ServerCapabilities {
        let mut capabilities = ServerCapabilities::default();
        for mounted in self.handlers.iter() {
            merge_server_capabilities(&mut capabilities, mounted.handler.dyn_server_capabilities());
        }
        capabilities
    }

    fn list_tools(&self) -> Vec<Tool> {
        let mut tools = Vec::new();
        for mounted in self.handlers.iter() {
            for mut tool in mounted.handler.dyn_list_tools() {
                tool.name = Self::prefix_tool_name(&mounted.prefix, &tool.name);
                tools.push(tool);
            }
        }
        tools
    }

    fn list_resources(&self) -> Vec<Resource> {
        let mut resources = Vec::new();
        for mounted in self.handlers.iter() {
            for mut resource in mounted.handler.dyn_list_resources() {
                resource.uri = Self::prefix_resource_uri(&mounted.prefix, &resource.uri);
                resources.push(resource);
            }
        }
        resources
    }

    fn list_resource_templates(&self) -> Vec<ResourceTemplate> {
        let mut templates = Vec::new();
        for mounted in self.handlers.iter() {
            for mut template in mounted.handler.dyn_list_resource_templates() {
                template.uri_template =
                    Self::prefix_resource_template_uri(&mounted.prefix, &template.uri_template);
                templates.push(template);
            }
        }
        templates
    }

    fn list_prompts(&self) -> Vec<Prompt> {
        let mut prompts = Vec::new();
        for mounted in self.handlers.iter() {
            for mut prompt in mounted.handler.dyn_list_prompts() {
                prompt.name = Self::prefix_prompt_name(&mounted.prefix, &prompt.name);
                prompts.push(prompt);
            }
        }
        prompts
    }

    fn call_tool<'a>(
        &'a self,
        name: &'a str,
        args: serde_json::Value,
        ctx: &'a RequestContext,
    ) -> impl std::future::Future<Output = McpResult<ToolResult>> + turbomcp_core::marker::MaybeSend + 'a
    {
        async move {
            let (prefix, original_name) = self
                .parse_prefixed_tool(name)
                .ok_or_else(|| McpError::tool_not_found(name))?;

            let handler = self
                .find_handler(prefix)
                .ok_or_else(|| McpError::tool_not_found(name))?;

            let mut result = handler
                .handler
                .dyn_call_tool(original_name, args, ctx)
                .await?;
            Self::prefix_uris_in_content(prefix, &mut result.content);
            Ok(result)
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
            let (prefix, original_uri) = self
                .parse_prefixed_uri(uri)
                .ok_or_else(|| McpError::resource_not_found(uri))?;

            let handler = self
                .find_handler(prefix)
                .ok_or_else(|| McpError::resource_not_found(uri))?;

            let mut result = handler.handler.dyn_read_resource(original_uri, ctx).await?;
            // Echo back the URI the client actually asked for, not the mount's
            // internal one — a client comparing them would otherwise see a
            // resource it never requested.
            for entry in &mut result.contents {
                match entry {
                    turbomcp_types::ResourceContents::Text(text) => {
                        text.uri = Self::prefix_resource_uri(prefix, &text.uri);
                    }
                    turbomcp_types::ResourceContents::Blob(blob) => {
                        blob.uri = Self::prefix_resource_uri(prefix, &blob.uri);
                    }
                }
            }
            Ok(result)
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
            let (prefix, original_name) = self
                .parse_prefixed_prompt(name)
                .ok_or_else(|| McpError::prompt_not_found(name))?;

            let handler = self
                .find_handler(prefix)
                .ok_or_else(|| McpError::prompt_not_found(name))?;

            let mut result = handler
                .handler
                .dyn_get_prompt(original_name, args, ctx)
                .await?;
            // Prompt messages can embed resources too; same reasoning as
            // `call_tool`.
            for message in &mut result.messages {
                Self::prefix_uris_in_content(prefix, core::slice::from_mut(&mut message.content));
            }
            Ok(result)
        }
    }

    /// Routed to the mount owning the URI, exactly like `read_resource`.
    fn subscribe<'a>(
        &'a self,
        uri: &'a str,
        ctx: &'a RequestContext,
    ) -> impl std::future::Future<Output = McpResult<()>> + turbomcp_core::marker::MaybeSend + 'a
    {
        async move {
            let (prefix, original_uri) = self
                .parse_prefixed_uri(uri)
                .ok_or_else(|| McpError::resource_not_found(uri))?;
            let handler = self
                .find_handler(prefix)
                .ok_or_else(|| McpError::resource_not_found(uri))?;

            handler.handler.dyn_subscribe(original_uri, ctx).await
        }
    }

    fn unsubscribe<'a>(
        &'a self,
        uri: &'a str,
        ctx: &'a RequestContext,
    ) -> impl std::future::Future<Output = McpResult<()>> + turbomcp_core::marker::MaybeSend + 'a
    {
        async move {
            let (prefix, original_uri) = self
                .parse_prefixed_uri(uri)
                .ok_or_else(|| McpError::resource_not_found(uri))?;
            let handler = self
                .find_handler(prefix)
                .ok_or_else(|| McpError::resource_not_found(uri))?;

            handler.handler.dyn_unsubscribe(original_uri, ctx).await
        }
    }

    /// Broadcast: the log level is a connection-wide setting, so every mount
    /// that implements it gets told. Mounts that do not are not an error —
    /// the composite reports success if any mount accepted the level.
    fn set_log_level<'a>(
        &'a self,
        level: &'a str,
        ctx: &'a RequestContext,
    ) -> impl std::future::Future<Output = McpResult<()>> + turbomcp_core::marker::MaybeSend + 'a
    {
        async move {
            let mut accepted = false;
            let mut last_error = None;

            for mount in self.handlers.iter() {
                match mount.handler.dyn_set_log_level(level, ctx).await {
                    Ok(()) => accepted = true,
                    Err(e) => last_error = Some(e),
                }
            }

            if accepted {
                Ok(())
            } else {
                Err(last_error
                    .unwrap_or_else(|| McpError::capability_not_supported("logging/setLevel")))
            }
        }
    }

    /// Routed by the completion's `ref`: a prompt ref carries a prefixed prompt
    /// name, a resource ref a prefixed URI. Both are rewritten to the mount's
    /// own namespace before being forwarded.
    fn complete<'a>(
        &'a self,
        params: serde_json::Value,
        ctx: &'a RequestContext,
    ) -> impl std::future::Future<Output = McpResult<serde_json::Value>>
    + turbomcp_core::marker::MaybeSend
    + 'a {
        async move {
            let mut params = params;
            let reference = params
                .get("ref")
                .ok_or_else(|| McpError::invalid_params("completion/complete requires `ref`"))?;

            let ref_type = reference.get("type").and_then(|v| v.as_str()).unwrap_or("");

            let prefix = match ref_type {
                "ref/prompt" => {
                    let name = reference
                        .get("name")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| McpError::invalid_params("ref/prompt requires `name`"))?;
                    let (prefix, original) = self
                        .parse_prefixed_prompt(name)
                        .ok_or_else(|| McpError::prompt_not_found(name))?;
                    let original = original.to_string();
                    let prefix = prefix.to_string();
                    params["ref"]["name"] = serde_json::Value::String(original);
                    prefix
                }
                "ref/resource" => {
                    let uri = reference
                        .get("uri")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| McpError::invalid_params("ref/resource requires `uri`"))?;
                    let (prefix, original) = self
                        .parse_prefixed_uri(uri)
                        .ok_or_else(|| McpError::resource_not_found(uri))?;
                    let original = original.to_string();
                    let prefix = prefix.to_string();
                    params["ref"]["uri"] = serde_json::Value::String(original);
                    prefix
                }
                other => {
                    return Err(McpError::invalid_params(format!(
                        "unsupported completion ref type: {other}"
                    )));
                }
            };

            let handler = self
                .find_handler(&prefix)
                .ok_or_else(|| McpError::capability_not_supported("completion/complete"))?;

            handler.handler.dyn_complete(params, ctx).await
        }
    }

    /// Broadcast: every mount may be caching roots independently.
    fn on_roots_list_changed<'a>(
        &'a self,
        ctx: &'a RequestContext,
    ) -> impl std::future::Future<Output = McpResult<()>> + turbomcp_core::marker::MaybeSend + 'a
    {
        async move {
            for mount in self.handlers.iter() {
                let _ = mount.handler.dyn_on_roots_list_changed(ctx).await;
            }
            Ok(())
        }
    }
}

#[cfg(test)]
#[allow(clippy::manual_async_fn)]
mod tests {
    use super::*;
    use core::future::Future;
    use turbomcp_core::marker::MaybeSend;

    #[derive(Clone)]
    struct WeatherHandler;

    impl McpHandler for WeatherHandler {
        fn server_info(&self) -> ServerInfo {
            ServerInfo::new("weather", "1.0.0")
        }

        fn list_tools(&self) -> Vec<Tool> {
            vec![Tool::new("get_forecast", "Get weather forecast")]
        }

        fn list_resources(&self) -> Vec<Resource> {
            vec![Resource::new("api/current", "Current weather")]
        }

        fn list_prompts(&self) -> Vec<Prompt> {
            vec![Prompt::new("forecast_prompt", "Weather forecast prompt")]
        }

        fn call_tool<'a>(
            &'a self,
            name: &'a str,
            _args: serde_json::Value,
            _ctx: &'a RequestContext,
        ) -> impl Future<Output = McpResult<ToolResult>> + MaybeSend + 'a {
            async move {
                match name {
                    "get_forecast" => Ok(ToolResult::text("Sunny, 72°F")),
                    _ => Err(McpError::tool_not_found(name)),
                }
            }
        }

        fn read_resource<'a>(
            &'a self,
            uri: &'a str,
            _ctx: &'a RequestContext,
        ) -> impl Future<Output = McpResult<ResourceResult>> + MaybeSend + 'a {
            let uri = uri.to_string();
            async move {
                if uri == "api/current" {
                    Ok(ResourceResult::text(&uri, "Temperature: 72°F"))
                } else {
                    Err(McpError::resource_not_found(&uri))
                }
            }
        }

        fn get_prompt<'a>(
            &'a self,
            name: &'a str,
            _args: Option<serde_json::Value>,
            _ctx: &'a RequestContext,
        ) -> impl Future<Output = McpResult<PromptResult>> + MaybeSend + 'a {
            let name = name.to_string();
            async move {
                if name == "forecast_prompt" {
                    Ok(PromptResult::user("What is the weather forecast?"))
                } else {
                    Err(McpError::prompt_not_found(&name))
                }
            }
        }
    }

    #[derive(Clone)]
    struct NewsHandler;

    impl McpHandler for NewsHandler {
        fn server_info(&self) -> ServerInfo {
            ServerInfo::new("news", "1.0.0")
        }

        fn list_tools(&self) -> Vec<Tool> {
            vec![Tool::new("get_headlines", "Get news headlines")]
        }

        fn list_resources(&self) -> Vec<Resource> {
            vec![Resource::new("feed/top", "Top news feed")]
        }

        fn list_prompts(&self) -> Vec<Prompt> {
            vec![Prompt::new("summary_prompt", "News summary prompt")]
        }

        fn call_tool<'a>(
            &'a self,
            name: &'a str,
            _args: serde_json::Value,
            _ctx: &'a RequestContext,
        ) -> impl Future<Output = McpResult<ToolResult>> + MaybeSend + 'a {
            async move {
                match name {
                    "get_headlines" => Ok(ToolResult::text("Breaking: AI advances continue")),
                    _ => Err(McpError::tool_not_found(name)),
                }
            }
        }

        fn read_resource<'a>(
            &'a self,
            uri: &'a str,
            _ctx: &'a RequestContext,
        ) -> impl Future<Output = McpResult<ResourceResult>> + MaybeSend + 'a {
            let uri = uri.to_string();
            async move {
                if uri == "feed/top" {
                    Ok(ResourceResult::text(&uri, "Top news stories"))
                } else {
                    Err(McpError::resource_not_found(&uri))
                }
            }
        }

        fn get_prompt<'a>(
            &'a self,
            name: &'a str,
            _args: Option<serde_json::Value>,
            _ctx: &'a RequestContext,
        ) -> impl Future<Output = McpResult<PromptResult>> + MaybeSend + 'a {
            let name = name.to_string();
            async move {
                if name == "summary_prompt" {
                    Ok(PromptResult::user("Summarize the news"))
                } else {
                    Err(McpError::prompt_not_found(&name))
                }
            }
        }
    }

    #[test]
    fn test_composite_server_info() {
        let server = CompositeHandler::new("main", "1.0.0").with_description("Main server");

        let info = server.server_info();
        assert_eq!(info.name, "main");
        assert_eq!(info.version, "1.0.0");
    }

    #[test]
    fn test_mount_handlers() {
        let server = CompositeHandler::new("main", "1.0.0")
            .mount(WeatherHandler, "weather")
            .mount(NewsHandler, "news");

        assert_eq!(server.handler_count(), 2);
        assert_eq!(server.prefixes(), vec!["weather", "news"]);
    }

    #[test]
    fn test_list_tools_prefixed() {
        let server = CompositeHandler::new("main", "1.0.0")
            .mount(WeatherHandler, "weather")
            .mount(NewsHandler, "news");

        let tools = server.list_tools();
        assert_eq!(tools.len(), 2);

        let tool_names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(tool_names.contains(&"weather_get_forecast"));
        assert!(tool_names.contains(&"news_get_headlines"));
    }

    #[test]
    fn test_list_resources_prefixed() {
        let server = CompositeHandler::new("main", "1.0.0")
            .mount(WeatherHandler, "weather")
            .mount(NewsHandler, "news");

        let resources = server.list_resources();
        assert_eq!(resources.len(), 2);

        let uris: Vec<&str> = resources.iter().map(|r| r.uri.as_str()).collect();
        assert!(uris.contains(&"weather://api/current"));
        assert!(uris.contains(&"news://feed/top"));
    }

    #[test]
    fn test_list_prompts_prefixed() {
        let server = CompositeHandler::new("main", "1.0.0")
            .mount(WeatherHandler, "weather")
            .mount(NewsHandler, "news");

        let prompts = server.list_prompts();
        assert_eq!(prompts.len(), 2);

        let prompt_names: Vec<&str> = prompts.iter().map(|p| p.name.as_str()).collect();
        assert!(prompt_names.contains(&"weather_forecast_prompt"));
        assert!(prompt_names.contains(&"news_summary_prompt"));
    }

    #[test]
    fn test_hidden_mounted_handler_still_contributes_capabilities() {
        let hidden_weather = crate::VisibilityLayer::new(WeatherHandler)
            .with_hidden_tools(["get_forecast"])
            .with_hidden_resources(["api/current"])
            .with_hidden_prompts(["forecast_prompt"]);
        let server = CompositeHandler::new("main", "1.0.0").mount(hidden_weather, "weather");

        assert!(server.list_tools().is_empty());
        assert!(server.list_resources().is_empty());
        assert!(server.list_prompts().is_empty());

        let capabilities = server.server_capabilities();
        assert!(capabilities.tools.is_some());
        assert!(capabilities.resources.is_some());
        assert!(capabilities.prompts.is_some());
    }

    #[tokio::test]
    async fn test_call_tool_routed() {
        let server = CompositeHandler::new("main", "1.0.0")
            .mount(WeatherHandler, "weather")
            .mount(NewsHandler, "news");

        let ctx = RequestContext::default();

        // Call weather tool
        let result = server
            .call_tool("weather_get_forecast", serde_json::json!({}), &ctx)
            .await
            .unwrap();
        assert_eq!(result.first_text(), Some("Sunny, 72°F"));

        // Call news tool
        let result = server
            .call_tool("news_get_headlines", serde_json::json!({}), &ctx)
            .await
            .unwrap();
        assert_eq!(result.first_text(), Some("Breaking: AI advances continue"));
    }

    #[tokio::test]
    async fn test_call_tool_not_found() {
        let server = CompositeHandler::new("main", "1.0.0").mount(WeatherHandler, "weather");

        let ctx = RequestContext::default();

        // Unknown prefix
        let result = server
            .call_tool("unknown_tool", serde_json::json!({}), &ctx)
            .await;
        assert!(result.is_err());

        // No underscore
        let result = server
            .call_tool("notool", serde_json::json!({}), &ctx)
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_read_resource_routed() {
        let server = CompositeHandler::new("main", "1.0.0")
            .mount(WeatherHandler, "weather")
            .mount(NewsHandler, "news");

        let ctx = RequestContext::default();

        // Read weather resource
        let result = server
            .read_resource("weather://api/current", &ctx)
            .await
            .unwrap();
        assert!(!result.contents.is_empty());

        // Read news resource
        let result = server.read_resource("news://feed/top", &ctx).await.unwrap();
        assert!(!result.contents.is_empty());
    }

    #[tokio::test]
    async fn test_get_prompt_routed() {
        let server = CompositeHandler::new("main", "1.0.0")
            .mount(WeatherHandler, "weather")
            .mount(NewsHandler, "news");

        let ctx = RequestContext::default();

        // Get weather prompt
        let result = server
            .get_prompt("weather_forecast_prompt", None, &ctx)
            .await
            .unwrap();
        assert!(!result.messages.is_empty());

        // Get news prompt
        let result = server
            .get_prompt("news_summary_prompt", None, &ctx)
            .await
            .unwrap();
        assert!(!result.messages.is_empty());
    }

    /// A URI a mount returns must be one the client can hand straight back to
    /// `resources/read`. `list_resources` presents them prefixed and
    /// `read_resource` strips the prefix on the way in, but results travelling
    /// *out* used to pass through verbatim — so a read echoed the mount's own
    /// unprefixed URI, which then matched no mount on the next request.
    #[tokio::test]
    async fn returned_resource_uris_can_be_read_back() {
        let composite = CompositeHandler::new("test", "1.0.0")
            .try_mount(WeatherHandler, "weather")
            .unwrap();
        let ctx = RequestContext::new();

        // What the catalogue advertises.
        let listed = composite.list_resources();
        let advertised = listed
            .iter()
            .find(|r| r.uri.contains("api/current"))
            .expect("resource should be listed");
        assert_eq!(advertised.uri, "weather://api/current");

        // Reading it echoes the same URI back, not the mount's internal one.
        let result = composite
            .read_resource(&advertised.uri, &ctx)
            .await
            .expect("advertised URI must be readable");
        let echoed = match &result.contents[0] {
            turbomcp_types::ResourceContents::Text(text) => text.uri.clone(),
            turbomcp_types::ResourceContents::Blob(blob) => blob.uri.clone(),
        };
        assert_eq!(
            echoed, advertised.uri,
            "the URI a read returns must be the one the client asked for"
        );

        // And it round-trips: feeding it back in works.
        composite
            .read_resource(&echoed, &ctx)
            .await
            .expect("the echoed URI must itself be readable");
    }

    /// Mounted names are `{prefix}_{name}`, so nested prefixes can mint the
    /// same name twice. `tools/list` would then advertise a duplicate and
    /// routing — longest prefix wins — would send every call to one mount,
    /// leaving the other's tool permanently unreachable with no error.
    #[test]
    fn test_nested_prefixes_are_rejected() {
        let result = CompositeHandler::new("test", "1.0.0")
            .try_mount(WeatherHandler, "x")
            .unwrap()
            .try_mount(WeatherHandler, "x_y");
        let err = result.unwrap_err();
        assert!(err.contains("nests"), "got: {err}");

        // Rejected in either mount order.
        let result = CompositeHandler::new("test", "1.0.0")
            .try_mount(WeatherHandler, "x_y")
            .unwrap()
            .try_mount(WeatherHandler, "x");
        assert!(result.unwrap_err().contains("nests"));
    }

    /// A shared leading substring is only a problem at an underscore boundary;
    /// `xy` and `x` cannot collide, so they must still be allowed.
    #[test]
    fn test_merely_overlapping_prefixes_are_allowed() {
        let composite = CompositeHandler::new("test", "1.0.0")
            .try_mount(WeatherHandler, "x")
            .unwrap()
            .try_mount(WeatherHandler, "xy")
            .unwrap();
        assert_eq!(composite.handlers.len(), 2);
    }

    /// A prefix outside the tool-name charset would mint names clients reject.
    #[test]
    fn test_invalid_prefix_charset_is_rejected() {
        for bad in ["we ather", "weather!", "wéather", ""] {
            let result = CompositeHandler::new("test", "1.0.0").try_mount(WeatherHandler, bad);
            assert!(result.is_err(), "prefix {bad:?} should have been rejected");
        }
    }

    #[test]
    #[should_panic(expected = "duplicate prefix 'weather'")]
    fn test_duplicate_prefix_panics() {
        let _server = CompositeHandler::new("main", "1.0.0")
            .mount(WeatherHandler, "weather")
            .mount(NewsHandler, "weather"); // Duplicate!
    }

    #[test]
    fn test_try_mount_duplicate_returns_error() {
        let server = CompositeHandler::new("main", "1.0.0").mount(WeatherHandler, "weather");

        let result = server.try_mount(NewsHandler, "weather");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("duplicate prefix"));
    }

    #[test]
    fn test_try_mount_success() {
        let server = CompositeHandler::new("main", "1.0.0")
            .try_mount(WeatherHandler, "weather")
            .unwrap()
            .try_mount(NewsHandler, "news")
            .unwrap();

        assert_eq!(server.handler_count(), 2);
    }
}
