//! MCP Server builder for WASM environments
//!
//! Provides an ergonomic builder API for creating MCP servers with automatic
//! schema generation and type-safe handlers.
//!
//! # Example
//!
//! ```ignore
//! use turbomcp_wasm::wasm_server::*;
//!
//! #[derive(Deserialize, JsonSchema)]
//! struct GreetArgs {
//!     name: String,
//! }
//!
//! // Simple async function - just works!
//! async fn greet(args: GreetArgs) -> String {
//!     format!("Hello, {}!", args.name)
//! }
//!
//! // With error handling using ?
//! async fn fetch(args: FetchArgs) -> Result<Json<Data>, ToolError> {
//!     let data = do_fetch(&args.url).await?;
//!     Ok(Json(data))
//! }
//!
//! let server = McpServer::builder("my-server", "1.0.0")
//!     .tool("greet", "Say hello", greet)
//!     .tool("fetch", "Fetch data", fetch)
//!     .build();
//! ```

use hashbrown::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde_json::Value;
use turbomcp_core::error::{McpError, McpResult};
use turbomcp_core::handler::McpHandler;
use turbomcp_core::uri_template::UriTemplate;
use turbomcp_core::{MaybeSend, MaybeSync};
use turbomcp_protocol::types::{
    PromptsCapabilities, ResourcesCapabilities, ServerCapabilities, ToolsCapabilities,
};
use turbomcp_types::{Implementation, Prompt, Resource, ResourceTemplate, Tool, ToolInputSchema};

use super::context::{RequestContext, shared_context};
use super::handler_traits::{
    IntoPromptHandler, IntoPromptHandlerWithCtx, IntoResourceHandler, IntoResourceHandlerWithCtx,
    IntoToolHandler, IntoToolHandlerWithCtx, NoArgs, PromptNoArgs, PromptWithCtxNoArgs, RawArgs,
    WithCtxOnly, WithCtxRaw,
};
use super::response::IntoToolResponse;
use super::traits::IntoPromptResponse;
use super::types::{PromptResult, ResourceResult, ToolResult};

#[cfg(not(target_arch = "wasm32"))]
type BoxedFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

#[cfg(target_arch = "wasm32")]
type BoxedFuture<T> = Pin<Box<dyn Future<Output = T>>>;

/// Type alias for async tool handlers (no context)
#[cfg(not(target_arch = "wasm32"))]
pub type ToolHandler = Arc<dyn Fn(serde_json::Value) -> BoxedFuture<ToolResult> + Send + Sync>;

/// Type alias for async tool handlers (no context)
#[cfg(target_arch = "wasm32")]
pub type ToolHandler = Arc<dyn Fn(serde_json::Value) -> BoxedFuture<ToolResult>>;

/// Type alias for async tool handlers with context
#[cfg(not(target_arch = "wasm32"))]
pub type ToolHandlerWithCtx =
    Arc<dyn Fn(Arc<RequestContext>, serde_json::Value) -> BoxedFuture<ToolResult> + Send + Sync>;

/// Type alias for async tool handlers with context
#[cfg(target_arch = "wasm32")]
pub type ToolHandlerWithCtx =
    Arc<dyn Fn(Arc<RequestContext>, serde_json::Value) -> BoxedFuture<ToolResult>>;

/// Type alias for async resource handlers (no context)
#[cfg(not(target_arch = "wasm32"))]
pub type ResourceHandlerFn =
    Arc<dyn Fn(String) -> BoxedFuture<Result<ResourceResult, String>> + Send + Sync>;

/// Type alias for async resource handlers (no context)
#[cfg(target_arch = "wasm32")]
pub type ResourceHandlerFn = Arc<dyn Fn(String) -> BoxedFuture<Result<ResourceResult, String>>>;

/// Type alias for async resource handlers with context
#[cfg(not(target_arch = "wasm32"))]
pub type ResourceHandlerWithCtxFn = Arc<
    dyn Fn(Arc<RequestContext>, String) -> BoxedFuture<Result<ResourceResult, String>>
        + Send
        + Sync,
>;

/// Type alias for async resource handlers with context
#[cfg(target_arch = "wasm32")]
pub type ResourceHandlerWithCtxFn =
    Arc<dyn Fn(Arc<RequestContext>, String) -> BoxedFuture<Result<ResourceResult, String>>>;

/// Type alias for async prompt handlers (no context)
#[cfg(not(target_arch = "wasm32"))]
pub type PromptHandlerFn = Arc<
    dyn Fn(Option<serde_json::Value>) -> BoxedFuture<Result<PromptResult, McpError>> + Send + Sync,
>;

/// Type alias for async prompt handlers (no context)
#[cfg(target_arch = "wasm32")]
pub type PromptHandlerFn =
    Arc<dyn Fn(Option<serde_json::Value>) -> BoxedFuture<Result<PromptResult, McpError>>>;

/// Type alias for async prompt handlers with context
#[cfg(not(target_arch = "wasm32"))]
pub type PromptHandlerWithCtxFn = Arc<
    dyn Fn(
            Arc<RequestContext>,
            Option<serde_json::Value>,
        ) -> BoxedFuture<Result<PromptResult, McpError>>
        + Send
        + Sync,
>;

/// Type alias for async prompt handlers with context
#[cfg(target_arch = "wasm32")]
pub type PromptHandlerWithCtxFn = Arc<
    dyn Fn(
        Arc<RequestContext>,
        Option<serde_json::Value>,
    ) -> BoxedFuture<Result<PromptResult, McpError>>,
>;

/// Enum wrapping both context-aware and non-context-aware tool handlers
#[derive(Clone)]
pub(crate) enum ToolHandlerKind {
    /// Handler without context
    NoCtx(ToolHandler),
    /// Handler with context
    WithCtx(ToolHandlerWithCtx),
}

/// Enum wrapping both context-aware and non-context-aware resource handlers
#[derive(Clone)]
pub(crate) enum ResourceHandlerKind {
    /// Handler without context
    NoCtx(ResourceHandlerFn),
    /// Handler with context
    WithCtx(ResourceHandlerWithCtxFn),
}

/// Enum wrapping both context-aware and non-context-aware prompt handlers
#[derive(Clone)]
pub(crate) enum PromptHandlerKind {
    /// Handler without context
    NoCtx(PromptHandlerFn),
    /// Handler with context
    WithCtx(PromptHandlerWithCtxFn),
}

/// Registered tool with metadata and handler
#[derive(Clone)]
pub(crate) struct RegisteredTool {
    pub tool: Tool,
    pub handler: ToolHandlerKind,
}

/// Registered resource with metadata and handler
#[derive(Clone)]
pub(crate) struct RegisteredResource {
    pub resource: Resource,
    pub handler: ResourceHandlerKind,
}

/// Registered resource template
#[derive(Clone)]
pub(crate) struct RegisteredResourceTemplate {
    pub template: ResourceTemplate,
    pub handler: ResourceHandlerKind,
}

/// Registered prompt with metadata and handler
#[derive(Clone)]
pub(crate) struct RegisteredPrompt {
    pub prompt: Prompt,
    pub handler: PromptHandlerKind,
}

/// Build a tool definition from the JSON Schema generated for its arguments.
///
/// The whole schema is kept. The builder used to copy only `properties` and
/// `required`, which dropped `$defs`: any argument type with a nested struct or
/// enum is emitted by schemars as a `$ref` into the root `$defs`, so every such
/// tool advertised a schema full of dangling references, and strict clients
/// (llama.cpp among them) reject the whole tool list over one. schemars builds
/// the schema with a single generator, so all definitions sit in that one root
/// `$defs` and the references resolve once it is carried over.
fn tool_definition(name: &str, description: String, schema: serde_json::Value) -> Tool {
    let mut input_schema = ToolInputSchema::from_value(schema);
    // MCP requires an object schema; a handler whose argument type is not a
    // struct still takes a JSON object on the wire.
    if input_schema.schema_type.is_none() {
        input_schema.schema_type = Some("object".into());
    }
    Tool {
        name: name.to_string(),
        description: Some(description),
        title: None,
        icons: None,
        input_schema,
        annotations: None,
        execution: None,
        output_schema: None,
        meta: None,
    }
}

/// Builder for creating an MCP server
///
/// # Example
///
/// ```ignore
/// let server = McpServer::builder("my-server", "1.0.0")
///     .description("A helpful MCP server")
///     .tool("greet", "Greet someone", greet_handler)
///     .resource("config://app", "Config", "App configuration", read_config)
///     .prompt("greeting", "Generate greeting", greeting_prompt)
///     .build();
/// ```
pub struct McpServerBuilder {
    name: String,
    version: String,
    description: Option<String>,
    tools: HashMap<String, RegisteredTool>,
    resources: HashMap<String, RegisteredResource>,
    resource_templates: HashMap<String, RegisteredResourceTemplate>,
    prompts: HashMap<String, RegisteredPrompt>,
    instructions: Option<String>,
}

impl McpServerBuilder {
    /// Create a new server builder
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            description: None,
            tools: HashMap::new(),
            resources: HashMap::new(),
            resource_templates: HashMap::new(),
            prompts: HashMap::new(),
            instructions: None,
        }
    }

    /// Set the server description
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Set server instructions (shown to clients)
    pub fn instructions(mut self, instructions: impl Into<String>) -> Self {
        self.instructions = Some(instructions.into());
        self
    }

    // ========================================================================
    // Tool Registration - Ergonomic API
    // ========================================================================

    /// Register a tool with typed arguments.
    ///
    /// This is the primary way to register tools. The handler can be any async function
    /// that takes a typed argument (implementing `Deserialize + JsonSchema`) and returns
    /// anything implementing `IntoToolResponse`.
    ///
    /// # Example
    ///
    /// ```ignore
    /// #[derive(Deserialize, JsonSchema)]
    /// struct AddArgs { a: i64, b: i64 }
    ///
    /// // Simple return
    /// async fn add(args: AddArgs) -> String {
    ///     format!("{}", args.a + args.b)
    /// }
    ///
    /// // With error handling
    /// async fn divide(args: DivideArgs) -> Result<String, ToolError> {
    ///     if args.b == 0 {
    ///         return Err(ToolError::new("Cannot divide by zero"));
    ///     }
    ///     Ok(format!("{}", args.a / args.b))
    /// }
    ///
    /// // With JSON response
    /// async fn get_user(args: GetUserArgs) -> Result<Json<User>, ToolError> {
    ///     let user = fetch_user(args.id).await?;
    ///     Ok(Json(user))
    /// }
    ///
    /// server
    ///     .tool("add", "Add numbers", add)
    ///     .tool("divide", "Divide numbers", divide)
    ///     .tool("get_user", "Get user by ID", get_user)
    /// ```
    pub fn tool<A, M, H>(
        mut self,
        name: impl Into<String>,
        description: impl Into<String>,
        handler: H,
    ) -> Self
    where
        H: IntoToolHandler<A, M>,
    {
        let name = name.into();
        let tool = tool_definition(&name, description.into(), H::schema());

        let boxed_handler = handler.into_handler();
        let wrapped_handler: ToolHandler = Arc::from(boxed_handler);

        self.tools.insert(
            name.clone(),
            RegisteredTool {
                tool,
                handler: ToolHandlerKind::NoCtx(wrapped_handler),
            },
        );

        self
    }

    /// Register a tool with context injection.
    ///
    /// The handler receives `Arc<RequestContext>` as its first argument,
    /// providing access to request metadata, session info, and headers.
    ///
    /// # Example
    ///
    /// ```ignore
    /// #[derive(Deserialize, JsonSchema)]
    /// struct AuthArgs { token: String }
    ///
    /// async fn auth_tool(ctx: Arc<RequestContext>, args: AuthArgs) -> Result<String, ToolError> {
    ///     if !ctx.is_authenticated() {
    ///         return Err(ToolError::new("Unauthorized"));
    ///     }
    ///     Ok(format!("Session: {:?}", ctx.session_id()))
    /// }
    ///
    /// server.tool_with_ctx("auth", "Authenticated tool", auth_tool)
    /// ```
    pub fn tool_with_ctx<A, M, H>(
        mut self,
        name: impl Into<String>,
        description: impl Into<String>,
        handler: H,
    ) -> Self
    where
        H: IntoToolHandlerWithCtx<A, M>,
    {
        let name = name.into();
        let tool = tool_definition(&name, description.into(), H::schema());

        let boxed_handler = handler.into_handler_with_ctx();
        let wrapped_handler: ToolHandlerWithCtx = Arc::from(boxed_handler);

        self.tools.insert(
            name.clone(),
            RegisteredTool {
                tool,
                handler: ToolHandlerKind::WithCtx(wrapped_handler),
            },
        );

        self
    }

    /// Register a tool that takes no arguments.
    ///
    /// # Example
    ///
    /// ```ignore
    /// async fn get_time() -> String {
    ///     chrono::Utc::now().to_string()
    /// }
    ///
    /// server.tool_no_args("time", "Get current time", get_time)
    /// ```
    pub fn tool_no_args<H, Fut, Res>(
        self,
        name: impl Into<String>,
        description: impl Into<String>,
        handler: H,
    ) -> Self
    where
        H: Fn() -> Fut + Clone + MaybeSend + MaybeSync + 'static,
        Fut: Future<Output = Res> + MaybeSend + 'static,
        Res: IntoToolResponse + 'static,
    {
        self.tool::<(), NoArgs, _>(name, description, handler)
    }

    /// Register a tool with raw JSON arguments (no schema validation).
    ///
    /// Use this when you need to handle arbitrary JSON or when the schema
    /// can't be expressed with schemars.
    ///
    /// # Example
    ///
    /// ```ignore
    /// async fn dynamic_tool(args: serde_json::Value) -> String {
    ///     format!("Received: {}", args)
    /// }
    ///
    /// server.tool_raw("dynamic", "Handle any JSON", dynamic_tool)
    /// ```
    pub fn tool_raw<H, Fut, Res>(
        self,
        name: impl Into<String>,
        description: impl Into<String>,
        handler: H,
    ) -> Self
    where
        H: Fn(serde_json::Value) -> Fut + Clone + MaybeSend + MaybeSync + 'static,
        Fut: Future<Output = Res> + MaybeSend + 'static,
        Res: IntoToolResponse + 'static,
    {
        self.tool::<serde_json::Value, RawArgs, _>(name, description, handler)
    }

    /// Register a tool with context that takes no other arguments.
    ///
    /// # Example
    ///
    /// ```ignore
    /// async fn session_info(ctx: Arc<RequestContext>) -> String {
    ///     format!("Session: {:?}", ctx.session_id())
    /// }
    ///
    /// server.tool_with_ctx_no_args("session", "Get session info", session_info)
    /// ```
    pub fn tool_with_ctx_no_args<H, Fut, Res>(
        self,
        name: impl Into<String>,
        description: impl Into<String>,
        handler: H,
    ) -> Self
    where
        H: Fn(Arc<RequestContext>) -> Fut + Clone + MaybeSend + MaybeSync + 'static,
        Fut: Future<Output = Res> + MaybeSend + 'static,
        Res: IntoToolResponse + 'static,
    {
        self.tool_with_ctx::<(), WithCtxOnly, _>(name, description, handler)
    }

    /// Register a tool with context and raw JSON arguments.
    ///
    /// # Example
    ///
    /// ```ignore
    /// async fn dynamic_auth_tool(ctx: Arc<RequestContext>, args: serde_json::Value) -> String {
    ///     if !ctx.is_authenticated() {
    ///         return "Unauthorized".to_string();
    ///     }
    ///     format!("Received: {}", args)
    /// }
    ///
    /// server.tool_with_ctx_raw("dynamic_auth", "Handle any JSON with auth", dynamic_auth_tool)
    /// ```
    pub fn tool_with_ctx_raw<H, Fut, Res>(
        self,
        name: impl Into<String>,
        description: impl Into<String>,
        handler: H,
    ) -> Self
    where
        H: Fn(Arc<RequestContext>, serde_json::Value) -> Fut
            + Clone
            + MaybeSend
            + MaybeSync
            + 'static,
        Fut: Future<Output = Res> + MaybeSend + 'static,
        Res: IntoToolResponse + 'static,
    {
        self.tool_with_ctx::<serde_json::Value, WithCtxRaw, _>(name, description, handler)
    }

    // ========================================================================
    // Resource Registration
    // ========================================================================

    /// Register a static resource.
    ///
    /// # Example
    ///
    /// ```ignore
    /// async fn read_config(uri: String) -> Result<ResourceResult, ToolError> {
    ///     let content = fetch_config().await?;
    ///     Ok(ResourceResult::text(uri, content))
    /// }
    ///
    /// server.resource("config://app", "Config", "Application config", read_config)
    /// ```
    pub fn resource<H, M>(
        mut self,
        uri: impl Into<String>,
        name: impl Into<String>,
        description: impl Into<String>,
        handler: H,
    ) -> Self
    where
        H: IntoResourceHandler<M>,
    {
        let uri = uri.into();
        let name = name.into();
        let description = description.into();

        let resource = Resource {
            uri: uri.clone(),
            name,
            description: Some(description),
            title: None,
            icons: None,
            mime_type: None,
            size: None,
            annotations: None,
            meta: None,
        };

        let boxed_handler = handler.into_handler();
        let wrapped_handler: ResourceHandlerFn = Arc::from(boxed_handler);

        self.resources.insert(
            uri.clone(),
            RegisteredResource {
                resource,
                handler: ResourceHandlerKind::NoCtx(wrapped_handler),
            },
        );

        self
    }

    /// Register a static resource with context injection.
    ///
    /// # Example
    ///
    /// ```ignore
    /// async fn read_user_config(ctx: Arc<RequestContext>, uri: String) -> Result<ResourceResult, ToolError> {
    ///     let user_id = ctx.user_id().ok_or_else(|| ToolError::new("No user"))?;
    ///     let content = fetch_user_config(user_id).await?;
    ///     Ok(ResourceResult::text(uri, content))
    /// }
    ///
    /// server.resource_with_ctx("config://user", "User Config", "User-specific config", read_user_config)
    /// ```
    pub fn resource_with_ctx<H, M>(
        mut self,
        uri: impl Into<String>,
        name: impl Into<String>,
        description: impl Into<String>,
        handler: H,
    ) -> Self
    where
        H: IntoResourceHandlerWithCtx<M>,
    {
        let uri = uri.into();
        let name = name.into();
        let description = description.into();

        let resource = Resource {
            uri: uri.clone(),
            name,
            description: Some(description),
            title: None,
            icons: None,
            mime_type: None,
            size: None,
            annotations: None,
            meta: None,
        };

        let boxed_handler = handler.into_handler_with_ctx();
        let wrapped_handler: ResourceHandlerWithCtxFn = Arc::from(boxed_handler);

        self.resources.insert(
            uri.clone(),
            RegisteredResource {
                resource,
                handler: ResourceHandlerKind::WithCtx(wrapped_handler),
            },
        );

        self
    }

    /// Register a resource template (for dynamic resources).
    ///
    /// # Example
    ///
    /// ```ignore
    /// async fn read_user(uri: String) -> Result<ResourceResult, ToolError> {
    ///     let id = extract_id_from_uri(&uri)?;
    ///     let user = fetch_user(id).await?;
    ///     Ok(ResourceResult::json(uri, &user)?)
    /// }
    ///
    /// server.resource_template("user://{id}", "User", "User data", read_user)
    /// ```
    pub fn resource_template<H, M>(
        mut self,
        uri_template: impl Into<String>,
        name: impl Into<String>,
        description: impl Into<String>,
        handler: H,
    ) -> Self
    where
        H: IntoResourceHandler<M>,
    {
        let uri_template = uri_template.into();
        let name = name.into();
        let description = description.into();

        let template = ResourceTemplate {
            uri_template: uri_template.clone(),
            name,
            description: Some(description),
            title: None,
            icons: None,
            mime_type: None,
            annotations: None,
            meta: None,
        };

        let boxed_handler = handler.into_handler();
        let wrapped_handler: ResourceHandlerFn = Arc::from(boxed_handler);

        self.resource_templates.insert(
            uri_template.clone(),
            RegisteredResourceTemplate {
                template,
                handler: ResourceHandlerKind::NoCtx(wrapped_handler),
            },
        );

        self
    }

    /// Register a resource template with context injection.
    ///
    /// # Example
    ///
    /// ```ignore
    /// async fn read_user_data(ctx: Arc<RequestContext>, uri: String) -> Result<ResourceResult, ToolError> {
    ///     if !ctx.is_authenticated() {
    ///         return Err(ToolError::new("Unauthorized"));
    ///     }
    ///     let id = extract_id_from_uri(&uri)?;
    ///     let user = fetch_user(id).await?;
    ///     Ok(ResourceResult::json(uri, &user)?)
    /// }
    ///
    /// server.resource_template_with_ctx("user://{id}", "User", "User data", read_user_data)
    /// ```
    pub fn resource_template_with_ctx<H, M>(
        mut self,
        uri_template: impl Into<String>,
        name: impl Into<String>,
        description: impl Into<String>,
        handler: H,
    ) -> Self
    where
        H: IntoResourceHandlerWithCtx<M>,
    {
        let uri_template = uri_template.into();
        let name = name.into();
        let description = description.into();

        let template = ResourceTemplate {
            uri_template: uri_template.clone(),
            name,
            description: Some(description),
            title: None,
            icons: None,
            mime_type: None,
            annotations: None,
            meta: None,
        };

        let boxed_handler = handler.into_handler_with_ctx();
        let wrapped_handler: ResourceHandlerWithCtxFn = Arc::from(boxed_handler);

        self.resource_templates.insert(
            uri_template.clone(),
            RegisteredResourceTemplate {
                template,
                handler: ResourceHandlerKind::WithCtx(wrapped_handler),
            },
        );

        self
    }

    // ========================================================================
    // Prompt Registration
    // ========================================================================

    /// Register a prompt with typed arguments.
    ///
    /// # Example
    ///
    /// ```ignore
    /// #[derive(Deserialize, JsonSchema)]
    /// struct GreetingArgs {
    ///     name: String,
    /// }
    ///
    /// async fn greeting_prompt(args: Option<GreetingArgs>) -> PromptResult {
    ///     let name = args.map(|a| a.name).unwrap_or_else(|| "World".into());
    ///     PromptResult::user(format!("Hello, {}!", name))
    /// }
    ///
    /// server.prompt("greeting", "Generate greeting", greeting_prompt)
    /// ```
    pub fn prompt<A, M, H>(
        mut self,
        name: impl Into<String>,
        description: impl Into<String>,
        handler: H,
    ) -> Self
    where
        H: IntoPromptHandler<A, M>,
    {
        let name = name.into();
        let description = description.into();

        let arguments = H::arguments();

        let prompt = Prompt {
            name: name.clone(),
            description: Some(description),
            title: None,
            icons: None,
            arguments: if arguments.is_empty() {
                None
            } else {
                Some(arguments)
            },
            meta: None,
        };

        let boxed_handler = handler.into_handler();
        let wrapped_handler: PromptHandlerFn = Arc::from(boxed_handler);

        self.prompts.insert(
            name.clone(),
            RegisteredPrompt {
                prompt,
                handler: PromptHandlerKind::NoCtx(wrapped_handler),
            },
        );

        self
    }

    /// Register a prompt with context injection.
    ///
    /// # Example
    ///
    /// ```ignore
    /// #[derive(Deserialize, JsonSchema)]
    /// struct GreetingArgs {
    ///     name: String,
    /// }
    ///
    /// async fn greeting_prompt(ctx: Arc<RequestContext>, args: Option<GreetingArgs>) -> PromptResult {
    ///     let user = ctx.user_id().unwrap_or("Guest");
    ///     let name = args.map(|a| a.name).unwrap_or_else(|| user.into());
    ///     PromptResult::user(format!("Hello, {}!", name))
    /// }
    ///
    /// server.prompt_with_ctx("greeting", "Generate greeting", greeting_prompt)
    /// ```
    pub fn prompt_with_ctx<A, M, H>(
        mut self,
        name: impl Into<String>,
        description: impl Into<String>,
        handler: H,
    ) -> Self
    where
        H: IntoPromptHandlerWithCtx<A, M>,
    {
        let name = name.into();
        let description = description.into();

        let arguments = H::arguments();

        let prompt = Prompt {
            name: name.clone(),
            description: Some(description),
            title: None,
            icons: None,
            arguments: if arguments.is_empty() {
                None
            } else {
                Some(arguments)
            },
            meta: None,
        };

        let boxed_handler = handler.into_handler_with_ctx();
        let wrapped_handler: PromptHandlerWithCtxFn = Arc::from(boxed_handler);

        self.prompts.insert(
            name.clone(),
            RegisteredPrompt {
                prompt,
                handler: PromptHandlerKind::WithCtx(wrapped_handler),
            },
        );

        self
    }

    /// Register a prompt with no arguments.
    ///
    /// # Example
    ///
    /// ```ignore
    /// async fn default_greeting() -> PromptResult {
    ///     PromptResult::user("Hello! How can I help you?")
    /// }
    ///
    /// server.prompt_no_args("greeting", "Default greeting", default_greeting)
    /// ```
    pub fn prompt_no_args<H, Fut, Res>(
        self,
        name: impl Into<String>,
        description: impl Into<String>,
        handler: H,
    ) -> Self
    where
        H: Fn() -> Fut + Clone + MaybeSend + MaybeSync + 'static,
        Fut: Future<Output = Res> + MaybeSend + 'static,
        Res: IntoPromptResponse + 'static,
    {
        self.prompt::<(), PromptNoArgs, _>(name, description, handler)
    }

    /// Register a prompt with context but no other arguments.
    ///
    /// # Example
    ///
    /// ```ignore
    /// async fn user_greeting(ctx: Arc<RequestContext>) -> PromptResult {
    ///     let user = ctx.user_id().unwrap_or("Guest");
    ///     PromptResult::user(format!("Hello, {}! How can I help?", user))
    /// }
    ///
    /// server.prompt_with_ctx_no_args("greeting", "User greeting", user_greeting)
    /// ```
    pub fn prompt_with_ctx_no_args<H, Fut, Res>(
        self,
        name: impl Into<String>,
        description: impl Into<String>,
        handler: H,
    ) -> Self
    where
        H: Fn(Arc<RequestContext>) -> Fut + Clone + MaybeSend + MaybeSync + 'static,
        Fut: Future<Output = Res> + MaybeSend + 'static,
        Res: IntoPromptResponse + 'static,
    {
        self.prompt_with_ctx::<(), PromptWithCtxNoArgs, _>(name, description, handler)
    }

    // ========================================================================
    // Build
    // ========================================================================

    /// Build the MCP server
    pub fn build(self) -> McpServer {
        let capabilities = ServerCapabilities {
            extensions: None,
            experimental: None,
            logging: None,
            completions: None,
            tasks: None,
            prompts: if self.prompts.is_empty() {
                None
            } else {
                Some(PromptsCapabilities {
                    list_changed: Some(false),
                })
            },
            resources: if self.resources.is_empty() && self.resource_templates.is_empty() {
                None
            } else {
                Some(ResourcesCapabilities {
                    subscribe: Some(false),
                    list_changed: Some(false),
                })
            },
            tools: if self.tools.is_empty() {
                None
            } else {
                Some(ToolsCapabilities {
                    list_changed: Some(false),
                })
            },
        };

        let server_info = Implementation {
            name: self.name,
            title: None,
            description: self.description,
            version: self.version,
            icons: None,
            website_url: None,
        };

        McpServer {
            server_info,
            capabilities,
            tools: self.tools,
            resources: self.resources,
            resource_templates: self.resource_templates,
            prompts: self.prompts,
            instructions: self.instructions,
        }
    }
}

/// MCP Server for WASM environments
///
/// Handles incoming HTTP requests and routes them to registered handlers.
#[derive(Clone)]
pub struct McpServer {
    pub(crate) server_info: Implementation,
    pub(crate) capabilities: ServerCapabilities,
    pub(crate) tools: HashMap<String, RegisteredTool>,
    pub(crate) resources: HashMap<String, RegisteredResource>,
    pub(crate) resource_templates: HashMap<String, RegisteredResourceTemplate>,
    pub(crate) prompts: HashMap<String, RegisteredPrompt>,
    pub(crate) instructions: Option<String>,
}

impl McpServer {
    /// Create a new server builder
    ///
    /// # Example
    ///
    /// ```ignore
    /// let server = McpServer::builder("my-server", "1.0.0")
    ///     .tool("hello", "Say hello", handler)
    ///     .build();
    /// ```
    pub fn builder(name: impl Into<String>, version: impl Into<String>) -> McpServerBuilder {
        McpServerBuilder::new(name, version)
    }

    /// Handle an incoming Cloudflare Worker request
    ///
    /// This is the main entry point for your Worker's fetch handler. It serves
    /// the stateless JSON-RPC endpoint with the default [`EndpointConfig`]
    /// (loopback browser origins only); use
    /// [`WasmHandlerExt::handle_worker_request_with_config`] to allow other
    /// origins, or `into_streamable()` (feature `streamable`) for sessions and
    /// SSE.
    ///
    /// [`EndpointConfig`]: super::EndpointConfig
    /// [`WasmHandlerExt::handle_worker_request_with_config`]: super::WasmHandlerExt::handle_worker_request_with_config
    pub async fn handle(&self, req: worker::Request) -> worker::Result<worker::Response> {
        super::endpoint::serve(
            self,
            req,
            &super::EndpointConfig::default(),
            |ctx| ctx,
            super::endpoint::admit_all,
        )
        .await
    }

    /// Get the list of registered tools, ordered by name
    pub fn tools(&self) -> Vec<&Tool> {
        let mut tools: Vec<&Tool> = self.tools.values().map(|r| &r.tool).collect();
        tools.sort_by(|a, b| a.name.cmp(&b.name));
        tools
    }

    /// Get the list of registered resources, ordered by URI
    pub fn resources(&self) -> Vec<&Resource> {
        let mut resources: Vec<&Resource> = self.resources.values().map(|r| &r.resource).collect();
        resources.sort_by(|a, b| a.uri.cmp(&b.uri));
        resources
    }

    /// Get the list of registered resource templates, ordered by URI template
    pub fn resource_templates(&self) -> Vec<&ResourceTemplate> {
        let mut templates: Vec<&ResourceTemplate> = self
            .resource_templates
            .values()
            .map(|r| &r.template)
            .collect();
        templates.sort_by(|a, b| a.uri_template.cmp(&b.uri_template));
        templates
    }

    /// Get the list of registered prompts, ordered by name
    pub fn prompts(&self) -> Vec<&Prompt> {
        let mut prompts: Vec<&Prompt> = self.prompts.values().map(|r| &r.prompt).collect();
        prompts.sort_by(|a, b| a.name.cmp(&b.name));
        prompts
    }

    // ========================================================================
    // Dispatch to the registered handlers
    // ========================================================================

    /// Call a tool handler.
    ///
    /// Shared by the [`McpHandler`] implementation and the middleware chain,
    /// which reaches the handlers after its last hook.
    pub(crate) async fn call_tool_internal(
        &self,
        name: &str,
        args: Value,
        ctx: Arc<RequestContext>,
    ) -> McpResult<ToolResult> {
        let registered = self
            .tools
            .get(name)
            .ok_or_else(|| McpError::tool_not_found(name))?;

        // Core passes `null` when the request omitted `arguments`; typed
        // handlers deserialize from an object, which is what the WASM
        // dispatchers always handed them.
        let args = if args.is_null() {
            Value::Object(serde_json::Map::new())
        } else {
            args
        };

        Ok(match &registered.handler {
            ToolHandlerKind::NoCtx(handler) => handler(args).await,
            ToolHandlerKind::WithCtx(handler) => handler(ctx, args).await,
        })
    }

    /// Read a resource: an exact URI first, then the first template it is an
    /// instance of.
    pub(crate) async fn read_resource_internal(
        &self,
        uri: &str,
        ctx: Arc<RequestContext>,
    ) -> McpResult<ResourceResult> {
        let handler = match self.resources.get(uri) {
            Some(registered) => &registered.handler,
            None => {
                // Templates are tried in a fixed order so that when two could
                // match, the same one answers on every request.
                let mut templates: Vec<_> = self.resource_templates.iter().collect();
                templates.sort_by(|a, b| a.0.cmp(b.0));
                templates
                    .into_iter()
                    .find(|(template, _)| UriTemplate::parse(template).matches(uri))
                    .map(|(_, registered)| &registered.handler)
                    .ok_or_else(|| McpError::resource_not_found(uri))?
            }
        };

        let result = match handler {
            ResourceHandlerKind::NoCtx(handler) => handler(uri.to_string()).await,
            ResourceHandlerKind::WithCtx(handler) => handler(ctx, uri.to_string()).await,
        };
        result.map_err(McpError::internal)
    }

    /// Get a prompt, checking its required arguments first.
    pub(crate) async fn get_prompt_internal(
        &self,
        name: &str,
        args: Option<Value>,
        ctx: Arc<RequestContext>,
    ) -> McpResult<PromptResult> {
        let registered = self
            .prompts
            .get(name)
            .ok_or_else(|| McpError::prompt_not_found(name))?;

        check_prompt_arguments(&registered.prompt, args.as_ref())?;

        match &registered.handler {
            PromptHandlerKind::NoCtx(handler) => handler(args).await,
            PromptHandlerKind::WithCtx(handler) => handler(ctx, args).await,
        }
    }
}

/// Hold a `prompts/get` request to the arguments the prompt declared.
///
/// The prompt advertises which arguments are required, so a request missing
/// one is the client's mistake and is answered as invalid params (`-32602`)
/// rather than reaching the handler, which would otherwise either fail with an
/// internal error or quietly render a prompt with a hole in it.
fn check_prompt_arguments(prompt: &Prompt, args: Option<&Value>) -> McpResult<()> {
    let supplied = match args {
        None | Some(Value::Null) => None,
        Some(Value::Object(map)) => Some(map),
        Some(_) => {
            return Err(McpError::invalid_params(
                "prompts/get 'arguments' must be an object",
            ));
        }
    };

    for argument in prompt.arguments.iter().flatten() {
        if argument.required == Some(true)
            && !supplied.is_some_and(|map| map.contains_key(&argument.name))
        {
            return Err(McpError::invalid_params(format!(
                "prompt '{}' requires argument '{}'",
                prompt.name, argument.name
            )));
        }
    }
    Ok(())
}

/// The WASM handlers return the wire-level `CallToolResult`; core deals in
/// its `ToolResult`. Same fields, so nothing is lost crossing over.
pub(crate) fn into_core_tool_result(result: ToolResult) -> turbomcp_types::ToolResult {
    turbomcp_types::ToolResult {
        content: result.content,
        is_error: result.is_error,
        structured_content: result.structured_content,
        meta: result.meta,
    }
}

#[allow(clippy::manual_async_fn)]
impl McpHandler for McpServer {
    fn server_info(&self) -> Implementation {
        self.server_info.clone()
    }

    fn instructions(&self) -> Option<String> {
        self.instructions.clone()
    }

    fn server_capabilities(&self) -> ServerCapabilities {
        self.capabilities.clone()
    }

    fn list_tools(&self) -> Vec<Tool> {
        self.tools().into_iter().cloned().collect()
    }

    fn list_resources(&self) -> Vec<Resource> {
        self.resources().into_iter().cloned().collect()
    }

    fn list_resource_templates(&self) -> Vec<ResourceTemplate> {
        self.resource_templates().into_iter().cloned().collect()
    }

    fn list_prompts(&self) -> Vec<Prompt> {
        self.prompts().into_iter().cloned().collect()
    }

    fn call_tool<'a>(
        &'a self,
        name: &'a str,
        args: Value,
        ctx: &'a RequestContext,
    ) -> impl Future<Output = McpResult<turbomcp_types::ToolResult>> + MaybeSend + 'a {
        async move {
            self.call_tool_internal(name, args, shared_context(ctx))
                .await
                .map(into_core_tool_result)
        }
    }

    fn read_resource<'a>(
        &'a self,
        uri: &'a str,
        ctx: &'a RequestContext,
    ) -> impl Future<Output = McpResult<ResourceResult>> + MaybeSend + 'a {
        async move { self.read_resource_internal(uri, shared_context(ctx)).await }
    }

    fn get_prompt<'a>(
        &'a self,
        name: &'a str,
        args: Option<Value>,
        ctx: &'a RequestContext,
    ) -> impl Future<Output = McpResult<PromptResult>> + MaybeSend + 'a {
        async move {
            self.get_prompt_internal(name, args, shared_context(ctx))
                .await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use turbomcp_core::jsonrpc::JsonRpcIncoming;

    async fn call(server: &McpServer, method: &str, params: Value) -> Value {
        let request = JsonRpcIncoming {
            jsonrpc: "2.0".into(),
            id: Some(serde_json::json!(1)),
            method: method.into(),
            params: Some(params),
        };
        let ctx = crate::wasm_server::context::new_wasm_context();
        let response = super::super::endpoint::route(server, request, &ctx, None).await;
        serde_json::to_value(response).unwrap()
    }

    fn server() -> McpServer {
        McpServer::builder("server-test", "1.0.0")
            .tool_raw(
                "echo",
                "Echo",
                |args: Value| async move { args.to_string() },
            )
            .resource(
                "config://app",
                "config",
                "App config",
                |uri: String| async move { Ok::<_, String>(ResourceResult::text(uri, "config")) },
            )
            .resource_template(
                "db://{table}/rows/{id}.json",
                "row",
                "A row",
                |uri: String| async move { Ok::<_, String>(ResourceResult::text(uri, "row")) },
            )
            .resource_template(
                "file:///{path}",
                "file",
                "A file",
                |uri: String| async move { Ok::<_, String>(ResourceResult::text(uri, "file")) },
            )
            .build()
    }

    #[derive(serde::Deserialize, schemars::JsonSchema)]
    #[allow(dead_code)]
    struct Inner {
        value: u32,
    }

    #[derive(serde::Deserialize, schemars::JsonSchema)]
    #[allow(dead_code)]
    struct Outer {
        inner: Inner,
        mode: Mode,
    }

    #[derive(serde::Deserialize, schemars::JsonSchema)]
    #[allow(dead_code)]
    enum Mode {
        Fast,
        Slow,
    }

    /// Every `$ref` in a tool's input schema must resolve against that schema.
    #[test]
    fn nested_argument_types_keep_their_definitions() {
        async fn handler(_args: Outer) -> String {
            String::new()
        }
        let server = McpServer::builder("schema", "1.0.0")
            .tool("nested", "Nested args", handler)
            .build();
        let schema = serde_json::to_value(&server.tools()[0].input_schema).unwrap();

        let mut refs = Vec::new();
        collect_refs(&schema, &mut refs);
        assert!(!refs.is_empty(), "schemars should reference Inner/Mode");
        for reference in refs {
            let pointer = reference
                .strip_prefix('#')
                .expect("refs are local to the schema");
            assert!(
                schema.pointer(pointer).is_some(),
                "dangling $ref {reference} in {schema}"
            );
        }
        assert_eq!(schema["type"], "object");
    }

    fn collect_refs(value: &Value, out: &mut Vec<String>) {
        match value {
            Value::Object(map) => {
                if let Some(Value::String(reference)) = map.get("$ref") {
                    out.push(reference.clone());
                }
                map.values().for_each(|v| collect_refs(v, out));
            }
            Value::Array(items) => items.iter().for_each(|v| collect_refs(v, out)),
            _ => {}
        }
    }

    #[tokio::test]
    async fn unknown_tool_is_invalid_params() {
        let response = call(&server(), "tools/call", serde_json::json!({"name": "nope"})).await;
        assert_eq!(response["error"]["code"], -32602);
    }

    #[tokio::test]
    async fn missing_arguments_reach_the_tool_as_an_empty_object() {
        let response = call(&server(), "tools/call", serde_json::json!({"name": "echo"})).await;
        assert_eq!(response["result"]["content"][0]["text"], "{}");
    }

    #[tokio::test]
    async fn tool_result_omits_absent_optional_fields() {
        let response = call(&server(), "tools/call", serde_json::json!({"name": "echo"})).await;
        let result = response["result"].as_object().unwrap();
        assert!(!result.contains_key("isError"));
        assert!(!result.contains_key("structuredContent"));
    }

    #[tokio::test]
    async fn unknown_resource_is_resource_not_found() {
        let response = call(
            &server(),
            "resources/read",
            serde_json::json!({"uri": "config://missing"}),
        )
        .await;
        assert_eq!(response["error"]["code"], -32002);
    }

    #[tokio::test]
    async fn templates_route_by_rfc_6570_not_segment_count() {
        // `{id}.json` is not a bare variable segment, and `{path}` takes a
        // multi-segment remainder; the old segment-count matcher refused both.
        for uri in ["db://users/rows/7.json", "file:///src/lib.rs"] {
            let response = call(&server(), "resources/read", serde_json::json!({"uri": uri})).await;
            assert_eq!(response["result"]["contents"][0]["uri"], uri, "{uri}");
        }
        let response = call(
            &server(),
            "resources/read",
            serde_json::json!({"uri": "file:///../etc/passwd"}),
        )
        .await;
        assert_eq!(response["error"]["code"], -32002);
    }

    #[tokio::test]
    async fn listings_are_ordered_and_paginated_by_core() {
        let server = McpServer::builder("order", "1.0.0")
            .tool_raw("b", "B", |_args: Value| async { "b" })
            .tool_raw("a", "A", |_args: Value| async { "a" })
            .tool_raw("c", "C", |_args: Value| async { "c" })
            .build();
        let response = call(&server, "tools/list", serde_json::json!({})).await;
        let names: Vec<_> = response["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(names, ["a", "b", "c"]);

        let response = call(
            &server,
            "tools/list",
            serde_json::json!({"cursor": "bogus"}),
        )
        .await;
        assert_eq!(response["error"]["code"], -32602);
    }

    #[derive(serde::Deserialize, schemars::JsonSchema)]
    #[allow(dead_code)]
    struct GreetingArgs {
        name: String,
        #[serde(default)]
        tone: Option<String>,
    }

    #[tokio::test]
    async fn prompt_arguments_are_checked_before_the_handler_runs() {
        let server = McpServer::builder("prompts", "1.0.0")
            .prompt(
                "greet",
                "Greeting",
                |args: Option<GreetingArgs>| async move {
                    PromptResult::user(format!(
                        "Hello, {}!",
                        args.map(|a| a.name).unwrap_or_default()
                    ))
                },
            )
            .build();

        let missing = call(&server, "prompts/get", serde_json::json!({"name": "greet"})).await;
        assert_eq!(missing["error"]["code"], -32602);

        let wrong_type = call(
            &server,
            "prompts/get",
            serde_json::json!({"name": "greet", "arguments": {"name": 5}}),
        )
        .await;
        assert_eq!(wrong_type["error"]["code"], -32602);

        let ok = call(
            &server,
            "prompts/get",
            serde_json::json!({"name": "greet", "arguments": {"name": "Ada"}}),
        )
        .await;
        assert_eq!(
            ok["result"]["messages"][0]["content"]["text"],
            "Hello, Ada!"
        );

        let unknown = call(&server, "prompts/get", serde_json::json!({"name": "nope"})).await;
        assert_eq!(unknown["error"]["code"], -32602);
    }
}
