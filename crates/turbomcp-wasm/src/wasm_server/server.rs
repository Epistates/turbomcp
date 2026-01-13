//! MCP Server builder for Cloudflare Workers

use hashbrown::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde::de::DeserializeOwned;
use turbomcp_core::types::capabilities::ServerCapabilities;
use turbomcp_core::types::core::Implementation;
use turbomcp_core::types::prompts::{Prompt, PromptArgument};
use turbomcp_core::types::resources::{Resource, ResourceTemplate};
use turbomcp_core::types::tools::{Tool, ToolInputSchema};

use super::handler::McpHandler;
use super::types::{PromptResult, ResourceResult, ToolResult};

/// Type alias for async tool handlers
pub type ToolHandler = Arc<
    dyn Fn(serde_json::Value) -> Pin<Box<dyn Future<Output = Result<ToolResult, String>> + Send>>
        + Send
        + Sync,
>;

/// Type alias for async resource handlers
pub type ResourceHandler = Arc<
    dyn Fn(String) -> Pin<Box<dyn Future<Output = Result<ResourceResult, String>> + Send>>
        + Send
        + Sync,
>;

/// Type alias for async prompt handlers
pub type PromptHandler = Arc<
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
    pub handler: ResourceHandler,
}

/// Registered resource template
pub(crate) struct RegisteredResourceTemplate {
    pub template: ResourceTemplate,
    pub handler: ResourceHandler,
}

/// Registered prompt with metadata and handler
pub(crate) struct RegisteredPrompt {
    pub prompt: Prompt,
    pub handler: PromptHandler,
}

/// Builder for creating an MCP server
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

    /// Register a simple prompt (no arguments)
    ///
    /// For prompts that require arguments, use `with_prompt` instead.
    ///
    /// # Example
    ///
    /// ```ignore
    /// server.with_simple_prompt("greeting", "Generate a greeting", || async move {
    ///     Ok(PromptResult::user("Hello! How can I help you today?"))
    /// })
    /// ```
    pub fn with_simple_prompt<F, Fut>(
        self,
        name: impl Into<String>,
        description: impl Into<String>,
        handler: F,
    ) -> Self
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<PromptResult, String>> + Send + 'static,
    {
        let handler = Arc::new(handler);
        self.with_prompt(name, description, vec![], move |_args| {
            let handler = handler.clone();
            async move { handler().await }
        })
    }

    /// Register a tool with typed arguments
    ///
    /// The argument type must implement `DeserializeOwned` and `JsonSchema` for
    /// automatic schema generation.
    ///
    /// # Example
    ///
    /// ```ignore
    /// #[derive(Deserialize, JsonSchema)]
    /// struct AddArgs { a: i64, b: i64 }
    ///
    /// server.with_tool("add", "Add two numbers", |args: AddArgs| async move {
    ///     Ok(ToolResult::text(format!("{}", args.a + args.b)))
    /// })
    /// ```
    pub fn with_tool<A, F, Fut>(
        mut self,
        name: impl Into<String>,
        description: impl Into<String>,
        handler: F,
    ) -> Self
    where
        A: DeserializeOwned + schemars::JsonSchema + 'static,
        F: Fn(A) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<ToolResult, String>> + Send + 'static,
    {
        let name = name.into();
        let description = description.into();

        // Generate JSON schema from the argument type using schemars 1.x API
        let schema = schemars::schema_for!(A);
        let schema_value = serde_json::to_value(&schema).unwrap_or_default();

        // Extract properties - schemars 1.x puts them directly in the root object
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

        // Wrap the typed handler
        let handler = Arc::new(handler);
        let wrapped_handler: ToolHandler = Arc::new(move |params: serde_json::Value| {
            let handler = handler.clone();
            Box::pin(async move {
                let args: A = serde_json::from_value(params)
                    .map_err(|e| format!("Failed to parse arguments: {e}"))?;
                handler(args).await
            })
        });

        self.tools.insert(
            name.clone(),
            RegisteredTool {
                tool,
                handler: wrapped_handler,
            },
        );

        self
    }

    /// Register a tool with raw JSON arguments (no schema validation)
    pub fn with_raw_tool<F, Fut>(
        mut self,
        name: impl Into<String>,
        description: impl Into<String>,
        handler: F,
    ) -> Self
    where
        F: Fn(serde_json::Value) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<ToolResult, String>> + Send + 'static,
    {
        let name = name.into();
        let description = description.into();

        let tool = Tool {
            name: name.clone(),
            description: Some(description),
            title: None,
            icon: None,
            input_schema: ToolInputSchema {
                schema_type: "object".to_string(),
                properties: None,
                required: None,
                additional_properties: Some(true),
            },
            annotations: None,
        };

        let handler = Arc::new(handler);
        let wrapped_handler: ToolHandler = Arc::new(move |params: serde_json::Value| {
            let handler = handler.clone();
            Box::pin(async move { handler(params).await })
        });

        self.tools.insert(
            name.clone(),
            RegisteredTool {
                tool,
                handler: wrapped_handler,
            },
        );

        self
    }

    /// Register a static resource
    pub fn with_resource<F, Fut>(
        mut self,
        uri: impl Into<String>,
        name: impl Into<String>,
        description: impl Into<String>,
        handler: F,
    ) -> Self
    where
        F: Fn(String) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<ResourceResult, String>> + Send + 'static,
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

        let handler = Arc::new(handler);
        let wrapped_handler: ResourceHandler = Arc::new(move |uri: String| {
            let handler = handler.clone();
            Box::pin(async move { handler(uri).await })
        });

        self.resources.insert(
            uri.clone(),
            RegisteredResource {
                resource,
                handler: wrapped_handler,
            },
        );

        self
    }

    /// Register a resource template (for dynamic resources)
    pub fn with_resource_template<F, Fut>(
        mut self,
        uri_template: impl Into<String>,
        name: impl Into<String>,
        description: impl Into<String>,
        handler: F,
    ) -> Self
    where
        F: Fn(String) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<ResourceResult, String>> + Send + 'static,
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

        let handler = Arc::new(handler);
        let wrapped_handler: ResourceHandler = Arc::new(move |uri: String| {
            let handler = handler.clone();
            Box::pin(async move { handler(uri).await })
        });

        self.resource_templates.insert(
            uri_template.clone(),
            RegisteredResourceTemplate {
                template,
                handler: wrapped_handler,
            },
        );

        self
    }

    /// Register a prompt
    pub fn with_prompt<F, Fut>(
        mut self,
        name: impl Into<String>,
        description: impl Into<String>,
        arguments: Vec<PromptArgument>,
        handler: F,
    ) -> Self
    where
        F: Fn(Option<serde_json::Value>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<PromptResult, String>> + Send + 'static,
    {
        let name = name.into();
        let description = description.into();

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

        let handler = Arc::new(handler);
        let wrapped_handler: PromptHandler = Arc::new(move |args: Option<serde_json::Value>| {
            let handler = handler.clone();
            Box::pin(async move { handler(args).await })
        });

        self.prompts.insert(
            name.clone(),
            RegisteredPrompt {
                prompt,
                handler: wrapped_handler,
            },
        );

        self
    }

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

/// MCP Server for Cloudflare Workers
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
    ///     .with_tool("hello", "Say hello", handler)
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
