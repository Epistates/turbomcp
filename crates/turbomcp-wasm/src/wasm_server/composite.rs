//! Server composition through handler mounting.
//!
//! This module enables composing multiple MCP servers into a single server,
//! with automatic namespacing through prefixes. This allows building modular
//! servers from smaller, focused handlers.
//!
//! The composite is an [`McpHandler`], so requests reach it through the core
//! router like every other WASM entry point, and it follows the native
//! `CompositeHandler`'s naming and prefix rules so a server composed on either
//! side exposes the same names.
//!
//! # Example
//!
//! ```ignore
//! use turbomcp_wasm::wasm_server::{McpServer, CompositeServer};
//!
//! // Create individual servers
//! let weather = McpServer::builder("weather", "1.0.0")
//!     .tool("get_forecast", "Get weather forecast", get_forecast)
//!     .build();
//!
//! let news = McpServer::builder("news", "1.0.0")
//!     .tool("get_headlines", "Get news headlines", get_headlines)
//!     .build();
//!
//! // Compose into a single server
//! let server = CompositeServer::builder("main-server", "1.0.0")
//!     .mount(weather, "weather")  // weather_get_forecast
//!     .mount(news, "news")        // news_get_headlines
//!     .build();
//!
//! // All tools are namespaced: "weather_get_forecast", "news_get_headlines"
//! server.handle(req).await
//! ```

use std::future::Future;
use std::sync::Arc;

use serde_json::Value;
use turbomcp_core::MaybeSend;
use turbomcp_core::error::{ErrorKind, McpError, McpResult};
use turbomcp_core::handler::McpHandler;
use turbomcp_core::uri_template::UriTemplate;
use turbomcp_protocol::types::{
    PromptsCapabilities, ResourcesCapabilities, ServerCapabilities, ToolsCapabilities,
};
use turbomcp_types::{
    Content, Implementation, Prompt, PromptResult, Resource, ResourceContents, ResourceResult,
    ResourceTemplate, Tool, ToolResult,
};
use worker::{Request, Response};

use super::context::{RequestContext, shared_context};
use super::server::{McpServer, into_core_tool_result};

/// A composite server that mounts multiple MCP servers with prefixes.
///
/// This enables modular server design by combining multiple servers into
/// a single namespace. Each mounted server's tools, resources, and prompts
/// are automatically prefixed to avoid naming conflicts.
///
/// # Namespacing Rules
///
/// - **Tools**: `{prefix}_{tool_name}` (e.g., `weather_get_forecast`)
/// - **Resources**: `{prefix}://{original_uri}` (e.g., `weather://api/forecast`)
/// - **Prompts**: `{prefix}_{prompt_name}` (e.g., `weather_forecast_prompt`)
///
/// A name is routed to the mount whose prefix it starts with — the longest
/// one, should two qualify — rather than by splitting at the first `_`, so a
/// prefix may itself contain `_`.
///
/// # Example
///
/// ```ignore
/// let composite = CompositeServer::builder("my-gateway", "1.0.0")
///     .mount(weather_server, "weather")
///     .mount(news_server, "news")
///     .build();
///
/// // Handle incoming request
/// let response = composite.handle(request).await?;
/// ```
#[derive(Clone)]
pub struct CompositeServer {
    name: String,
    version: String,
    description: Option<String>,
    instructions: Option<String>,
    mounted: Arc<Vec<MountedServer>>,
}

impl std::fmt::Debug for CompositeServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompositeServer")
            .field("name", &self.name)
            .field("version", &self.version)
            .field("description", &self.description)
            .field("mounted_count", &self.mounted.len())
            .finish()
    }
}

/// Internal struct to hold a mounted server with its prefix.
#[derive(Clone)]
struct MountedServer {
    prefix: String,
    server: McpServer,
}

/// Builder for creating a composite server.
pub struct CompositeServerBuilder {
    name: String,
    version: String,
    description: Option<String>,
    instructions: Option<String>,
    mounted: Vec<MountedServer>,
}

impl std::fmt::Debug for CompositeServerBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompositeServerBuilder")
            .field("name", &self.name)
            .field("version", &self.version)
            .field("description", &self.description)
            .field("mounted_count", &self.mounted.len())
            .finish()
    }
}

impl CompositeServerBuilder {
    /// Create a new composite server builder.
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            description: None,
            instructions: None,
            mounted: Vec::new(),
        }
    }

    /// Set the server description.
    #[must_use]
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Set the `instructions` returned by the `initialize` handshake.
    ///
    /// Mounted servers' own instructions are not merged: each was written for a
    /// standalone server and names tools the composite exposes under another
    /// name. Describe the composed surface here instead.
    #[must_use]
    pub fn instructions(mut self, instructions: impl Into<String>) -> Self {
        self.instructions = Some(instructions.into());
        self
    }

    /// Check a prefix before mounting.
    ///
    /// Same rules as the native `CompositeHandler`. Beyond an exact duplicate,
    /// prefixes that *nest* are refused: names are minted as `{prefix}_{name}`,
    /// so mounting at `x` (exposing a tool `y_z`) next to `x_y` (exposing `z`)
    /// produces two tools both called `x_y_z`, one of which can never be
    /// reached. The charset keeps minted names inside the one MCP allows for
    /// tool names.
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

        for mounted in &self.mounted {
            let other = mounted.prefix.as_str();
            if other == prefix {
                return Err(format!(
                    "duplicate prefix '{prefix}' - each mounted server must have a unique prefix"
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

    /// Mount a server with the given prefix.
    ///
    /// All tools, resources, and prompts from the server will be namespaced
    /// with the prefix. Prefer [`try_mount`](Self::try_mount) when prefixes
    /// come from configuration.
    ///
    /// # Panics
    ///
    /// Panics if the prefix is empty, longer than 64 characters, contains
    /// anything outside `A-Z a-z 0-9 _ - .`, duplicates an existing prefix or
    /// nests with one (see [`try_mount`](Self::try_mount)). This prevents
    /// silent shadowing of tools/resources/prompts which could lead to
    /// confusing runtime behavior.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let server = CompositeServer::builder("main", "1.0.0")
    ///     .mount(weather_server, "weather")
    ///     .mount(news_server, "news")
    ///     .build();
    /// ```
    #[must_use]
    pub fn mount(self, server: McpServer, prefix: impl Into<String>) -> Self {
        match self.try_mount(server, prefix) {
            Ok(builder) => builder,
            Err(error) => panic!("CompositeServer: {error}"),
        }
    }

    /// Try to mount a server with the given prefix, returning an error when
    /// the prefix is invalid, duplicated or nests with a mounted one.
    ///
    /// This is the fallible version of [`mount`](Self::mount).
    ///
    /// # Errors
    ///
    /// Returns an error describing why the prefix was refused.
    pub fn try_mount(
        mut self,
        server: McpServer,
        prefix: impl Into<String>,
    ) -> Result<Self, String> {
        let prefix = prefix.into();
        self.validate_prefix(&prefix)?;
        self.mounted.push(MountedServer { prefix, server });
        Ok(self)
    }

    /// Build the composite server.
    // `Arc` so the composite is `Send + Sync` on native targets; on wasm32 the
    // handlers are `!Send` and it is only a reference count.
    #[allow(clippy::arc_with_non_send_sync)]
    pub fn build(self) -> CompositeServer {
        CompositeServer {
            name: self.name,
            version: self.version,
            description: self.description,
            instructions: self.instructions,
            mounted: Arc::new(self.mounted),
        }
    }
}

impl CompositeServer {
    /// Create a new composite server builder.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let server = CompositeServer::builder("my-gateway", "1.0.0")
    ///     .mount(server1, "prefix1")
    ///     .build();
    /// ```
    pub fn builder(name: impl Into<String>, version: impl Into<String>) -> CompositeServerBuilder {
        CompositeServerBuilder::new(name, version)
    }

    /// Get the number of mounted servers.
    pub fn server_count(&self) -> usize {
        self.mounted.len()
    }

    /// Get all mounted prefixes.
    pub fn prefixes(&self) -> Vec<&str> {
        self.mounted.iter().map(|m| m.prefix.as_str()).collect()
    }

    /// Handle an incoming Cloudflare Worker request.
    ///
    /// This routes requests to the appropriate mounted server based on the
    /// namespaced tool/resource/prompt names, through the same stateless
    /// endpoint (and body limit) as [`McpServer::handle`].
    pub async fn handle(&self, req: Request) -> worker::Result<Response> {
        super::endpoint::serve(
            self,
            req,
            &super::EndpointConfig::default(),
            |ctx| ctx,
            super::endpoint::admit_all,
        )
        .await
    }

    // =========================================================================
    // Namespacing Helpers
    // =========================================================================

    /// Prefix a tool or prompt name.
    fn prefix_name(prefix: &str, name: &str) -> String {
        format!("{}_{}", prefix, name)
    }

    /// Prefix a resource URI or URI template.
    fn prefix_uri(prefix: &str, uri: &str) -> String {
        format!("{}://{}", prefix, uri)
    }

    /// Find the mount a prefixed name belongs to, returning it with the name
    /// the mount knows. The longest matching prefix wins.
    fn route<'a>(&self, name: &'a str, separator: &str) -> Option<(&MountedServer, &'a str)> {
        self.mounted
            .iter()
            .filter_map(|mounted| {
                name.strip_prefix(mounted.prefix.as_str())?
                    .strip_prefix(separator)
                    .map(|rest| (mounted, rest))
            })
            .max_by_key(|(mounted, _)| mounted.prefix.len())
    }

    /// A mount reports "not found" for the name it knows; the client asked for
    /// the prefixed one, so that is the one the error names. Every other error
    /// passes through with its kind intact.
    fn as_requested(error: McpError, not_found: ErrorKind, requested: &str) -> McpError {
        if error.kind != not_found {
            return error;
        }
        match not_found {
            ErrorKind::ToolNotFound => McpError::tool_not_found(requested),
            ErrorKind::PromptNotFound => McpError::prompt_not_found(requested),
            _ => McpError::resource_not_found(requested),
        }
    }

    /// Whether `uri` is one the mounted server serves itself.
    fn serves(server: &McpServer, uri: &str) -> bool {
        server.resources.contains_key(uri)
            || server
                .resource_templates
                .keys()
                .any(|template| UriTemplate::parse(template).matches(uri))
    }

    /// Rewrite resource URIs inside outgoing content blocks.
    ///
    /// Listings present a mount's URIs prefixed and `resources/read` strips the
    /// prefix back off, so a `resource_link` or embedded resource carrying the
    /// mount's own URI would name something the client cannot read back. Only
    /// URIs the mount actually serves are rewritten: a link to an external
    /// `https://` page is not a resource of the mount and is left alone.
    fn prefix_uris_in_content(mounted: &MountedServer, blocks: &mut [Content]) {
        for block in blocks {
            let uri = match block {
                Content::ResourceLink(link) => &mut link.uri,
                Content::Resource(embedded) => match &mut embedded.resource {
                    ResourceContents::Text(text) => &mut text.uri,
                    ResourceContents::Blob(blob) => &mut blob.uri,
                },
                _ => continue,
            };
            if Self::serves(&mounted.server, uri) {
                *uri = Self::prefix_uri(&mounted.prefix, uri);
            }
        }
    }

    // =========================================================================
    // Capability Aggregation
    // =========================================================================

    fn aggregate_capabilities(&self) -> ServerCapabilities {
        let has_tools = self.mounted.iter().any(|m| !m.server.tools.is_empty());
        let has_resources = self
            .mounted
            .iter()
            .any(|m| !m.server.resources.is_empty() || !m.server.resource_templates.is_empty());
        let has_prompts = self.mounted.iter().any(|m| !m.server.prompts.is_empty());

        ServerCapabilities {
            extensions: None,
            experimental: None,
            logging: None,
            completions: None,
            tasks: None,
            prompts: has_prompts.then_some(PromptsCapabilities {
                list_changed: Some(false),
            }),
            resources: has_resources.then_some(ResourcesCapabilities {
                subscribe: Some(false),
                list_changed: Some(false),
            }),
            tools: has_tools.then_some(ToolsCapabilities {
                list_changed: Some(false),
            }),
        }
    }
}

#[allow(clippy::manual_async_fn)]
impl McpHandler for CompositeServer {
    fn server_info(&self) -> Implementation {
        Implementation {
            name: self.name.clone(),
            title: None,
            description: self.description.clone(),
            version: self.version.clone(),
            icons: None,
            website_url: None,
        }
    }

    fn instructions(&self) -> Option<String> {
        self.instructions.clone()
    }

    fn server_capabilities(&self) -> ServerCapabilities {
        self.aggregate_capabilities()
    }

    fn list_tools(&self) -> Vec<Tool> {
        self.mounted
            .iter()
            .flat_map(|mounted| {
                mounted.server.list_tools().into_iter().map(|mut tool| {
                    tool.name = Self::prefix_name(&mounted.prefix, &tool.name);
                    tool
                })
            })
            .collect()
    }

    fn list_resources(&self) -> Vec<Resource> {
        self.mounted
            .iter()
            .flat_map(|mounted| {
                mounted
                    .server
                    .list_resources()
                    .into_iter()
                    .map(|mut resource| {
                        resource.uri = Self::prefix_uri(&mounted.prefix, &resource.uri);
                        resource
                    })
            })
            .collect()
    }

    fn list_resource_templates(&self) -> Vec<ResourceTemplate> {
        self.mounted
            .iter()
            .flat_map(|mounted| {
                mounted
                    .server
                    .list_resource_templates()
                    .into_iter()
                    .map(|mut template| {
                        template.uri_template =
                            Self::prefix_uri(&mounted.prefix, &template.uri_template);
                        template
                    })
            })
            .collect()
    }

    fn list_prompts(&self) -> Vec<Prompt> {
        self.mounted
            .iter()
            .flat_map(|mounted| {
                mounted.server.list_prompts().into_iter().map(|mut prompt| {
                    prompt.name = Self::prefix_name(&mounted.prefix, &prompt.name);
                    prompt
                })
            })
            .collect()
    }

    fn call_tool<'a>(
        &'a self,
        name: &'a str,
        args: Value,
        ctx: &'a RequestContext,
    ) -> impl Future<Output = McpResult<ToolResult>> + MaybeSend + 'a {
        async move {
            let (mounted, original) = self
                .route(name, "_")
                .ok_or_else(|| McpError::tool_not_found(name))?;
            let mut result = mounted
                .server
                .call_tool_internal(original, args, shared_context(ctx))
                .await
                .map_err(|error| Self::as_requested(error, ErrorKind::ToolNotFound, name))
                .map(into_core_tool_result)?;
            Self::prefix_uris_in_content(mounted, &mut result.content);
            Ok(result)
        }
    }

    fn read_resource<'a>(
        &'a self,
        uri: &'a str,
        ctx: &'a RequestContext,
    ) -> impl Future<Output = McpResult<ResourceResult>> + MaybeSend + 'a {
        async move {
            let (mounted, original) = self
                .route(uri, "://")
                .ok_or_else(|| McpError::resource_not_found(uri))?;
            let mut result = mounted
                .server
                .read_resource_internal(original, shared_context(ctx))
                .await
                .map_err(|error| Self::as_requested(error, ErrorKind::ResourceNotFound, uri))?;
            // Echo back the URI the client asked for, not the mount's own.
            for entry in &mut result.contents {
                let entry_uri = match entry {
                    ResourceContents::Text(text) => &mut text.uri,
                    ResourceContents::Blob(blob) => &mut blob.uri,
                };
                *entry_uri = Self::prefix_uri(&mounted.prefix, entry_uri);
            }
            Ok(result)
        }
    }

    fn get_prompt<'a>(
        &'a self,
        name: &'a str,
        args: Option<Value>,
        ctx: &'a RequestContext,
    ) -> impl Future<Output = McpResult<PromptResult>> + MaybeSend + 'a {
        async move {
            let (mounted, original) = self
                .route(name, "_")
                .ok_or_else(|| McpError::prompt_not_found(name))?;
            let mut result = mounted
                .server
                .get_prompt_internal(original, args, shared_context(ctx))
                .await
                .map_err(|error| Self::as_requested(error, ErrorKind::PromptNotFound, name))?;
            for message in &mut result.messages {
                Self::prefix_uris_in_content(mounted, std::slice::from_mut(&mut message.content));
            }
            Ok(result)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_weather_server() -> McpServer {
        McpServer::builder("weather", "1.0.0")
            .description("Weather service")
            .tool_raw("get_forecast", "Get weather forecast", |_args| async {
                "Sunny, 72°F".to_string()
            })
            .resource(
                "api/current",
                "current",
                "Current",
                |uri: String| async move { Ok::<_, String>(ResourceResult::text(uri, "sunny")) },
            )
            .build()
    }

    fn create_test_news_server() -> McpServer {
        McpServer::builder("news", "1.0.0")
            .description("News service")
            .tool_raw("get_headlines", "Get news headlines", |_args| async {
                "Breaking: AI advances continue".to_string()
            })
            .build()
    }

    fn composite() -> CompositeServer {
        CompositeServer::builder("main", "1.0.0")
            .mount(create_test_weather_server(), "weather")
            .mount(create_test_news_server(), "news")
            .build()
    }

    async fn route(server: &CompositeServer, method: &str, params: Value) -> Value {
        let request = turbomcp_core::jsonrpc::JsonRpcIncoming {
            jsonrpc: "2.0".into(),
            id: Some(serde_json::json!(1)),
            method: method.into(),
            params: Some(params),
        };
        let ctx = crate::wasm_server::context::new_wasm_context();
        serde_json::to_value(super::super::endpoint::route(server, request, &ctx, None).await)
            .unwrap()
    }

    #[test]
    fn test_composite_builder() {
        let composite = CompositeServer::builder("main", "1.0.0")
            .description("Main gateway")
            .mount(create_test_weather_server(), "weather")
            .mount(create_test_news_server(), "news")
            .build();

        assert_eq!(composite.server_count(), 2);
        assert_eq!(composite.prefixes(), vec!["weather", "news"]);
    }

    #[test]
    fn test_list_tools_prefixed() {
        let tools = composite().list_tools();
        let tool_names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();

        assert_eq!(tool_names.len(), 2);
        assert!(tool_names.contains(&"weather_get_forecast"));
        assert!(tool_names.contains(&"news_get_headlines"));
    }

    #[test]
    #[should_panic(expected = "duplicate prefix 'weather'")]
    fn test_duplicate_prefix_panics() {
        let _composite = CompositeServer::builder("main", "1.0.0")
            .mount(create_test_weather_server(), "weather")
            .mount(create_test_weather_server(), "weather"); // Duplicate!
    }

    #[test]
    fn test_try_mount_duplicate_returns_error() {
        let result = CompositeServer::builder("main", "1.0.0")
            .try_mount(create_test_weather_server(), "weather")
            .unwrap()
            .try_mount(create_test_weather_server(), "weather");

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("duplicate prefix"));
    }

    #[test]
    fn prefixes_that_could_shadow_each_other_are_refused() {
        let builder = CompositeServer::builder("main", "1.0.0")
            .try_mount(create_test_weather_server(), "a")
            .unwrap();
        // `a` exposing `b_c` and `a_b` exposing `c` would both mint `a_b_c`.
        let nested = builder.try_mount(create_test_news_server(), "a_b");
        assert!(nested.unwrap_err().contains("nests"));

        for bad in ["", "has space", "slash/es", &"x".repeat(65)] {
            let result = CompositeServer::builder("main", "1.0.0")
                .try_mount(create_test_weather_server(), bad);
            assert!(result.is_err(), "{bad:?} should be refused");
        }
    }

    #[test]
    fn prefixes_containing_underscores_route_correctly() {
        let composite = CompositeServer::builder("main", "1.0.0")
            .mount(create_test_weather_server(), "my_weather")
            .build();
        let (mounted, rest) = composite.route("my_weather_get_forecast", "_").unwrap();
        assert_eq!(mounted.prefix, "my_weather");
        assert_eq!(rest, "get_forecast");
        assert!(composite.route("my_get_forecast", "_").is_none());
    }

    #[test]
    fn test_try_mount_success() {
        let composite = CompositeServer::builder("main", "1.0.0")
            .try_mount(create_test_weather_server(), "weather")
            .unwrap()
            .try_mount(create_test_news_server(), "news")
            .unwrap()
            .build();

        assert_eq!(composite.server_count(), 2);
    }

    #[tokio::test]
    async fn test_call_tool_routed() {
        let server = composite();
        for name in ["weather_get_forecast", "news_get_headlines"] {
            let response = route(&server, "tools/call", serde_json::json!({"name": name})).await;
            let result = response["result"].as_object().unwrap();
            assert!(!result["content"].as_array().unwrap().is_empty());
            // The typed result is serialized as-is: no `isError: null`.
            assert!(!result.contains_key("isError"), "{response}");
        }
    }

    #[tokio::test]
    async fn test_call_tool_not_found() {
        let server = composite();
        for name in ["unknown_tool", "notool", "weather_nope"] {
            let response = route(&server, "tools/call", serde_json::json!({"name": name})).await;
            assert_eq!(response["error"]["code"], -32602, "{name}");
        }
    }

    #[tokio::test]
    async fn resources_route_and_echo_the_prefixed_uri() {
        let server = composite();
        let response = route(
            &server,
            "resources/read",
            serde_json::json!({"uri": "weather://api/current"}),
        )
        .await;
        assert_eq!(
            response["result"]["contents"][0]["uri"],
            "weather://api/current"
        );

        let missing = route(
            &server,
            "resources/read",
            serde_json::json!({"uri": "weather://api/none"}),
        )
        .await;
        assert_eq!(missing["error"]["code"], -32002);
    }

    #[tokio::test]
    async fn composite_answers_the_protocol_through_core() {
        let server = composite();
        let init = route(
            &server,
            "initialize",
            serde_json::json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "c", "version": "1"}
            }),
        )
        .await;
        assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
        // A 2025-06-18 client gets no 2025-11-25 serverInfo fields.
        assert!(init["result"]["serverInfo"].get("description").is_none());

        let ping = route(&server, "ping", serde_json::json!({})).await;
        assert_eq!(ping["result"], serde_json::json!({}));
    }

    #[test]
    fn test_aggregate_capabilities() {
        let caps = CompositeServer::builder("main", "1.0.0")
            .mount(create_test_news_server(), "news")
            .build()
            .server_capabilities();
        assert!(caps.tools.is_some());
        assert!(caps.resources.is_none()); // No resources in the news server
        assert!(caps.prompts.is_none()); // No prompts in test servers
    }
}
