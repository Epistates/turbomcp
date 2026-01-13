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

use turbomcp_core::types::capabilities::ServerCapabilities;
use turbomcp_core::types::core::Implementation;
use turbomcp_core::types::prompts::Prompt;
use turbomcp_core::types::resources::{Resource, ResourceTemplate};
use turbomcp_core::types::tools::{Tool, ToolInputSchema};

use super::handler::McpHandler;
use super::handler_traits::{
    IntoPromptHandler, IntoResourceHandler, IntoToolHandler, NoArgs, PromptNoArgs, RawArgs,
};
use super::response::IntoToolResponse;
use super::traits::IntoPromptResponse;
use super::types::{PromptResult, ResourceResult, ToolResult};

/// Type alias for async tool handlers
pub type ToolHandler = Arc<
    dyn Fn(serde_json::Value) -> Pin<Box<dyn Future<Output = ToolResult> + Send>> + Send + Sync,
>;

/// Type alias for async resource handlers
pub type ResourceHandlerFn = Arc<
    dyn Fn(String) -> Pin<Box<dyn Future<Output = Result<ResourceResult, String>> + Send>>
        + Send
        + Sync,
>;

/// Type alias for async prompt handlers
pub type PromptHandlerFn = Arc<
    dyn Fn(
            Option<serde_json::Value>,
        ) -> Pin<Box<dyn Future<Output = Result<PromptResult, String>> + Send>>
        + Send
        + Sync,
>;

/// Registered tool with metadata and handler
pub(crate) struct RegisteredTool {
    pub tool: Tool,
    pub handler: ToolHandler,
}

/// Registered resource with metadata and handler
pub(crate) struct RegisteredResource {
    pub resource: Resource,
    pub handler: ResourceHandlerFn,
}

/// Registered resource template
pub(crate) struct RegisteredResourceTemplate {
    pub template: ResourceTemplate,
    pub handler: ResourceHandlerFn,
}

/// Registered prompt with metadata and handler
pub(crate) struct RegisteredPrompt {
    pub prompt: Prompt,
    pub handler: PromptHandlerFn,
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
        let description = description.into();

        // Get schema from the handler trait
        let schema_value = H::schema();

        // Extract properties and required fields
        let properties = schema_value
            .get("properties")
            .and_then(|p| p.as_object())
            .map(|obj| {
                obj.iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect::<HashMap<String, serde_json::Value>>()
            });

        let required = schema_value
            .get("required")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            });

        let tool = Tool {
            name: name.clone(),
            description: Some(description),
            title: None,
            icon: None,
            input_schema: ToolInputSchema {
                schema_type: "object".to_string(),
                properties,
                required,
                additional_properties: None,
            },
            annotations: None,
        };

        let boxed_handler = handler.into_handler();
        let wrapped_handler: ToolHandler = Arc::from(boxed_handler);

        self.tools.insert(
            name.clone(),
            RegisteredTool {
                tool,
                handler: wrapped_handler,
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
        H: Fn() -> Fut + Clone + Send + Sync + 'static,
        Fut: Future<Output = Res> + Send + 'static,
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
        H: Fn(serde_json::Value) -> Fut + Clone + Send + Sync + 'static,
        Fut: Future<Output = Res> + Send + 'static,
        Res: IntoToolResponse + 'static,
    {
        self.tool::<serde_json::Value, RawArgs, _>(name, description, handler)
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
            icon: None,
            mime_type: None,
            size: None,
            annotations: None,
        };

        let boxed_handler = handler.into_handler();
        let wrapped_handler: ResourceHandlerFn = Arc::from(boxed_handler);

        self.resources.insert(
            uri.clone(),
            RegisteredResource {
                resource,
                handler: wrapped_handler,
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
            icon: None,
            mime_type: None,
            annotations: None,
        };

        let boxed_handler = handler.into_handler();
        let wrapped_handler: ResourceHandlerFn = Arc::from(boxed_handler);

        self.resource_templates.insert(
            uri_template.clone(),
            RegisteredResourceTemplate {
                template,
                handler: wrapped_handler,
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
            icon: None,
            arguments: if arguments.is_empty() {
                None
            } else {
                Some(arguments)
            },
        };

        let boxed_handler = handler.into_handler();
        let wrapped_handler: PromptHandlerFn = Arc::from(boxed_handler);

        self.prompts.insert(
            name.clone(),
            RegisteredPrompt {
                prompt,
                handler: wrapped_handler,
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
        H: Fn() -> Fut + Clone + Send + Sync + 'static,
        Fut: Future<Output = Res> + Send + 'static,
        Res: IntoPromptResponse + 'static,
    {
        self.prompt::<(), PromptNoArgs, _>(name, description, handler)
    }

    // ========================================================================
    // Build
    // ========================================================================

    /// Build the MCP server
    pub fn build(self) -> McpServer {
        let capabilities = ServerCapabilities {
            experimental: None,
            logging: None,
            tasks: None,
            prompts: if self.prompts.is_empty() {
                None
            } else {
                Some(turbomcp_core::types::capabilities::PromptsCapability {
                    list_changed: Some(false),
                })
            },
            resources: if self.resources.is_empty() && self.resource_templates.is_empty() {
                None
            } else {
                Some(turbomcp_core::types::capabilities::ResourcesCapability {
                    subscribe: Some(false),
                    list_changed: Some(false),
                })
            },
            tools: if self.tools.is_empty() {
                None
            } else {
                Some(turbomcp_core::types::capabilities::ToolsCapability {
                    list_changed: Some(false),
                })
            },
        };

        let server_info = Implementation {
            name: self.name,
            title: None,
            description: self.description,
            version: self.version,
            icon: None,
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
    /// This is the main entry point for your Worker's fetch handler.
    pub async fn handle(&self, req: worker::Request) -> worker::Result<worker::Response> {
        McpHandler::new(self).handle(req).await
    }

    /// Get the list of registered tools
    pub fn tools(&self) -> Vec<&Tool> {
        self.tools.values().map(|r| &r.tool).collect()
    }

    /// Get the list of registered resources
    pub fn resources(&self) -> Vec<&Resource> {
        self.resources.values().map(|r| &r.resource).collect()
    }

    /// Get the list of registered resource templates
    pub fn resource_templates(&self) -> Vec<&ResourceTemplate> {
        self.resource_templates
            .values()
            .map(|r| &r.template)
            .collect()
    }

    /// Get the list of registered prompts
    pub fn prompts(&self) -> Vec<&Prompt> {
        self.prompts.values().map(|r| &r.prompt).collect()
    }
}
