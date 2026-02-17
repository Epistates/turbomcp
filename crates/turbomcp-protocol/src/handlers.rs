//! Handler traits for extensible MCP protocol support
//!
//! This module provides trait definitions for handling various MCP protocol
//! features including elicitation, completion, resource templates, and ping.
//!
//! ## Handler Types
//!
//! ### [`ElicitationHandler`]
//! Handle server-initiated user input requests. Useful for asking users for
//! additional information during tool execution.
//!
//! ### [`CompletionProvider`]
//! Provide argument completion suggestions for tools and commands. Implements
//! autocomplete functionality in MCP clients.
//!
//! ### [`ResourceTemplateHandler`]
//! Manage dynamic resource templates with parameter substitution. Enables
//! pattern-based resource access (e.g., `file:///{path}`).
//!
//! ### [`PingHandler`]
//! Handle bidirectional ping/pong for connection health monitoring.
//!
//! ## Example: Implementing an Elicitation Handler
//!
//! ```rust
//! use turbomcp_protocol::{ElicitationHandler, ElicitationContext, ElicitationResponse};
//! use turbomcp_protocol::Result;
//! use std::collections::HashMap;
//! use std::future::Future;
//! use std::pin::Pin;
//!
//! struct MyElicitationHandler;
//!
//! impl ElicitationHandler for MyElicitationHandler {
//!     fn handle_elicitation(
//!         &self,
//!         context: &ElicitationContext
//!     ) -> Pin<Box<dyn Future<Output = Result<ElicitationResponse>> + Send + '_>> {
//!         let can_handle = self.can_handle(context);
//!         Box::pin(async move {
//!             // Check if we can handle this elicitation type
//!             if !can_handle {
//!                 return Ok(ElicitationResponse {
//!                     accepted: false,
//!                     content: None,
//!                     decline_reason: Some("Unsupported elicitation type".to_string()),
//!                 });
//!             }
//!
//!             // Process the elicitation (e.g., prompt user)
//!             let mut response_data = HashMap::new();
//!             response_data.insert(
//!                 "user_input".to_string(),
//!                 serde_json::json!("User provided value")
//!             );
//!
//!             Ok(ElicitationResponse {
//!                 accepted: true,
//!                 content: Some(response_data),
//!                 decline_reason: None,
//!             })
//!         })
//!     }
//!
//!     fn can_handle(&self, context: &ElicitationContext) -> bool {
//!         // Check if elicitation has required input
//!         context.required && !context.message.is_empty()
//!     }
//!
//!     fn priority(&self) -> i32 {
//!         100 // Higher priority than default (0)
//!     }
//! }
//! ```
//!
//! ## Example: Implementing a Completion Provider
//!
//! ```rust
//! use turbomcp_protocol::{CompletionProvider, CompletionContext, CompletionItem};
//! use turbomcp_protocol::Result;
//! use std::future::Future;
//! use std::pin::Pin;
//!
//! struct FilePathCompletionProvider;
//!
//! impl CompletionProvider for FilePathCompletionProvider {
//!     fn provide_completions(
//!         &self,
//!         context: &CompletionContext
//!     ) -> Pin<Box<dyn Future<Output = Result<Vec<CompletionItem>>> + Send + '_>> {
//!         Box::pin(async move {
//!             // Provide file path completions
//!             let completions = vec![
//!                 CompletionItem {
//!                     value: "/home/user/documents".to_string(),
//!                     label: Some("Documents".to_string()),
//!                     documentation: Some("User documents folder".to_string()),
//!                     sort_priority: Some(1),
//!                     insert_text: None,
//!                     metadata: Default::default(),
//!                 },
//!                 CompletionItem {
//!                     value: "/home/user/downloads".to_string(),
//!                     label: Some("Downloads".to_string()),
//!                     documentation: Some("Downloads folder".to_string()),
//!                     sort_priority: Some(2),
//!                     insert_text: None,
//!                     metadata: Default::default(),
//!                 },
//!             ];
//!
//!             Ok(completions)
//!         })
//!     }
//!
//!     fn can_provide(&self, context: &CompletionContext) -> bool {
//!         // Only provide completions for "path" arguments
//!         context.argument_name.as_deref() == Some("path")
//!     }
//! }
//! ```

use serde_json::Value;
use std::collections::HashMap;

use std::future::Future;
use std::pin::Pin;

use crate::Result;
use crate::context::{CompletionContext, ElicitationContext, ServerInitiatedContext};

/// Handler for server-initiated elicitation requests
pub trait ElicitationHandler: Send + Sync {
    /// Handle an elicitation request from the server
    fn handle_elicitation(
        &self,
        context: &ElicitationContext,
    ) -> Pin<Box<dyn Future<Output = Result<ElicitationResponse>> + Send + '_>>;

    /// Check if this handler can process the given elicitation
    fn can_handle(&self, context: &ElicitationContext) -> bool;

    /// Get handler priority (higher = higher priority)
    fn priority(&self) -> i32 {
        0
    }
}

/// Response to an elicitation request
#[derive(Debug, Clone)]
pub struct ElicitationResponse {
    /// Whether the elicitation was accepted
    pub accepted: bool,
    /// The response content if accepted
    pub content: Option<HashMap<String, Value>>,
    /// Optional reason for declining
    pub decline_reason: Option<String>,
}

/// Provider for argument completion
pub trait CompletionProvider: Send + Sync {
    /// Provide completions for the given context
    fn provide_completions(
        &self,
        context: &CompletionContext,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<CompletionItem>>> + Send + '_>>;

    /// Check if this provider can handle the completion request
    fn can_provide(&self, context: &CompletionContext) -> bool;

    /// Get provider priority
    fn priority(&self) -> i32 {
        0
    }
}

/// A single completion item
#[derive(Debug, Clone)]
pub struct CompletionItem {
    /// The completion value
    pub value: String,
    /// Human-readable label
    pub label: Option<String>,
    /// Additional documentation
    pub documentation: Option<String>,
    /// Sort priority (lower = higher priority)
    pub sort_priority: Option<i32>,
    /// Text to insert
    pub insert_text: Option<String>,
    /// Item metadata
    pub metadata: HashMap<String, Value>,
}

/// Handler for resource templates
pub trait ResourceTemplateHandler: Send + Sync {
    /// List available resource templates
    fn list_templates(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<ResourceTemplate>>> + Send + '_>>;

    /// Get a specific resource template
    fn get_template(
        &self,
        name: &str,
    ) -> Pin<Box<dyn Future<Output = Result<Option<ResourceTemplate>>> + Send + '_>>;

    /// Resolve template parameters
    fn resolve_template(
        &self,
        template: &ResourceTemplate,
        params: HashMap<String, Value>,
    ) -> Pin<Box<dyn Future<Output = Result<ResolvedResource>> + Send + '_>>;
}

/// Resource template definition
#[derive(Debug, Clone)]
pub struct ResourceTemplate {
    /// Template name
    pub name: String,
    /// Template description
    pub description: Option<String>,
    /// URI template pattern
    pub uri_template: String,
    /// Template parameters
    pub parameters: Vec<TemplateParam>,
    /// Template metadata
    pub metadata: HashMap<String, Value>,
}

/// Template parameter definition
#[derive(Debug, Clone)]
pub struct TemplateParam {
    /// Parameter name
    pub name: String,
    /// Parameter description
    pub description: Option<String>,
    /// Whether the parameter is required
    pub required: bool,
    /// Parameter type
    pub param_type: String,
    /// Default value
    pub default_value: Option<Value>,
}

/// Resolved resource from template
#[derive(Debug, Clone)]
pub struct ResolvedResource {
    /// Resolved URI
    pub uri: String,
    /// Resource name
    pub name: String,
    /// Resource description
    pub description: Option<String>,
    /// Resource content
    pub content: Option<Value>,
    /// Resource metadata
    pub metadata: HashMap<String, Value>,
}

/// Handler for bidirectional ping requests
pub trait PingHandler: Send + Sync {
    /// Handle a ping request
    fn handle_ping(
        &self,
        context: &ServerInitiatedContext,
    ) -> Pin<Box<dyn Future<Output = Result<PingResponse>> + Send + '_>>;

    /// Send a ping to the remote party
    fn send_ping(
        &self,
        target: &str,
    ) -> Pin<Box<dyn Future<Output = Result<PingResponse>> + Send + '_>>;
}

/// Response to a ping request
#[derive(Debug, Clone)]
pub struct PingResponse {
    /// Whether the ping was successful
    pub success: bool,
    /// Round-trip time in milliseconds
    pub rtt_ms: Option<u64>,
    /// Additional metadata
    pub metadata: HashMap<String, Value>,
}

/// Capabilities for server-initiated features
#[derive(Debug, Clone, Default)]
pub struct ServerInitiatedCapabilities {
    /// Supports sampling/message creation
    pub sampling: bool,
    /// Supports roots listing
    pub roots: bool,
    /// Supports elicitation
    pub elicitation: bool,
    /// Maximum concurrent requests
    pub max_concurrent_requests: usize,
    /// Supported experimental features
    pub experimental: HashMap<String, bool>,
}

/// Handler capability tracking
#[derive(Debug, Clone, Default)]
pub struct HandlerCapabilities {
    /// Supports elicitation
    pub elicitation: bool,
    /// Supports completion
    pub completion: bool,
    /// Supports resource templates
    pub templates: bool,
    /// Supports bidirectional ping
    pub ping: bool,
    /// Server-initiated capabilities
    pub server_initiated: ServerInitiatedCapabilities,
}

impl HandlerCapabilities {
    /// Create new handler capabilities
    pub fn new() -> Self {
        Self::default()
    }

    /// Enable elicitation support
    pub fn with_elicitation(mut self) -> Self {
        self.elicitation = true;
        self
    }

    /// Enable completion support
    pub fn with_completion(mut self) -> Self {
        self.completion = true;
        self
    }

    /// Enable template support
    pub fn with_templates(mut self) -> Self {
        self.templates = true;
        self
    }

    /// Enable ping support
    pub fn with_ping(mut self) -> Self {
        self.ping = true;
        self
    }

    /// Set server-initiated capabilities
    pub fn with_server_initiated(mut self, capabilities: ServerInitiatedCapabilities) -> Self {
        self.server_initiated = capabilities;
        self
    }
}

/// Handler for JSON-RPC requests - Core abstraction for MCP protocol implementation
///
/// This trait provides a transport-agnostic interface for handling MCP JSON-RPC requests.
/// Implementations of this trait can work seamlessly across all transport layers
/// (HTTP, STDIO, WebSocket, etc.) without transport-specific code.
///
/// # Architecture
///
/// The `JsonRpcHandler` trait serves as the bridge between:
/// - **Protocol Logic**: Tools, resources, prompts dispatch (typically macro-generated)
/// - **Transport Layer**: HTTP, STDIO, WebSocket protocol details
///
/// This separation enables:
/// - Clean, testable handler implementations
/// - Transport-agnostic server code
/// - Full MCP 2025-06-18 compliance in transport layer
/// - Compile-time dispatch optimizations in handlers
///
/// # Example: Macro-Generated Implementation
///
/// ```rust,ignore
/// use turbomcp_protocol::JsonRpcHandler;
/// use serde_json::Value;
///
/// #[derive(Clone)]
/// struct WeatherServer;
///
/// impl JsonRpcHandler for WeatherServer {
///     async fn handle_request(&self, req: Value) -> Value {
///         // Parse method and dispatch
///         let method = req["method"].as_str().unwrap_or("");
///         match method {
///             "initialize" => { /* ... */ },
///             "tools/call" => { /* dispatch to tools */ },
///             "resources/read" => { /* dispatch to resources */ },
///             _ => serde_json::json!({"error": "method not found"}),
///         }
///     }
///
///     fn server_info(&self) -> ServerInfo {
///         ServerInfo {
///             name: "Weather Server".to_string(),
///             version: "1.0.0".to_string(),
///         }
///     }
/// }
/// ```
///
/// # Usage with Transports
///
/// ```rust,ignore
/// // HTTP Transport
/// use turbomcp_transport::streamable_http::StreamableHttpTransport;
///
/// let handler = Arc::new(WeatherServer);
/// let transport = StreamableHttpTransport::new(config, handler);
/// transport.run().await?;
///
/// // STDIO Transport
/// use turbomcp_transport::stdio::StdioTransport;
///
/// let handler = Arc::new(WeatherServer);
/// let transport = StdioTransport::new(handler);
/// transport.run().await?;
/// ```
pub trait JsonRpcHandler: Send + Sync + 'static {
    /// Handle a JSON-RPC request and return a response
    ///
    /// This method receives a JSON-RPC request as a `serde_json::Value` and must return
    /// a valid JSON-RPC response. The implementation should:
    /// - Route the request based on the `method` field
    /// - Validate parameters
    /// - Execute the appropriate handler logic
    /// - Return a success response with results or an error response
    ///
    /// # Arguments
    ///
    /// * `request` - The JSON-RPC request as a JSON value
    ///
    /// # Returns
    ///
    /// A JSON-RPC response as a JSON value containing either:
    /// - `result`: For successful operations
    /// - `error`: For failed operations with error details
    ///
    /// # Note
    ///
    /// The request and response are `serde_json::Value` to avoid tight coupling with
    /// protocol types. Transport layers handle conversion to/from typed structs.
    fn handle_request(
        &self,
        request: serde_json::Value,
    ) -> Pin<Box<dyn Future<Output = serde_json::Value> + Send + '_>>;

    /// Get server metadata
    ///
    /// Returns information about the server including name and version.
    /// This is used during the MCP initialization handshake.
    ///
    /// # Returns
    ///
    /// Server information including name and version
    fn server_info(&self) -> ServerInfo {
        ServerInfo {
            name: "TurboMCP Server".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }

    /// Get server capabilities
    ///
    /// Returns the capabilities supported by this server.
    /// Override this to advertise custom capabilities to clients.
    ///
    /// # Returns
    ///
    /// JSON value describing server capabilities
    fn capabilities(&self) -> serde_json::Value {
        serde_json::json!({})
    }
}

/// Server metadata information
#[derive(Debug, Clone)]
pub struct ServerInfo {
    /// Server name
    pub name: String,
    /// Server version
    pub version: String,
}
