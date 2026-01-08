//! # `TurboMCP` Client
//!
//! MCP (Model Context Protocol) client implementation for connecting to MCP servers
//! and consuming their capabilities (tools, prompts, resources, and sampling).
//!
//! ## Features
//!
//! - Connection management with automatic reconnection
//! - Error handling and recovery mechanisms
//! - Support for all MCP capabilities including bidirectional sampling
//! - Elicitation response handling for server-initiated user input requests
//! - Transport-agnostic design (works with any `Transport` implementation)
//! - Type-safe protocol communication
//! - Request/response correlation tracking
//! - Timeout and cancellation support
//! - Automatic capability negotiation
//! - Handler support for server-initiated requests (sampling and elicitation)
//!
//! ## Architecture
//!
//! The client follows a layered architecture:
//!
//! ```text
//! Application Layer
//!        ↓
//! Client API (this crate)
//!        ↓  
//! Protocol Layer (turbomcp-protocol)
//!        ↓
//! Transport Layer (turbomcp-transport)
//! ```
//!
//! ## Usage
//!
//! ```rust,no_run
//! use turbomcp_client::{Client, ClientBuilder};
//! use turbomcp_transport::stdio::StdioTransport;
//!
//! # async fn example() -> turbomcp_protocol::Result<()> {
//! // Create a client with stdio transport
//! let transport = StdioTransport::new();
//! let mut client = Client::new(transport);
//!
//! // Initialize connection and negotiate capabilities
//! let result = client.initialize().await?;
//! println!("Connected to: {}", result.server_info.name);
//!
//! // List and call tools
//! let tools = client.list_tools().await?;
//! for tool in tools {
//!     println!("Tool: {} - {}", tool.name, tool.description.as_deref().unwrap_or("No description"));
//! }
//!
//! // Access resources
//! let resources = client.list_resources().await?;
//! for resource in resources {
//!     println!("Resource: {} ({})", resource.name, resource.uri);
//! }
//! # Ok(())
//! # }
//! ```
//!
//! ## Elicitation Response Handling
//!
//! The client supports handling server-initiated elicitation requests:
//!
//! ```rust,no_run
//! use turbomcp_client::Client;
//! use std::collections::HashMap;
//!
//! // Simple elicitation handling example
//! async fn handle_server_elicitation() {
//!     // When server requests user input, you would:
//!     // 1. Present the schema to the user
//!     // 2. Collect their input  
//!     // 3. Send response back to server
//!     
//!     let user_preferences: HashMap<String, String> = HashMap::new();
//!     // Your UI/CLI interaction logic here
//!     println!("Server requesting user preferences");
//! }
//! ```
//!
//! ## Sampling Support
//!
//! Handle server-initiated sampling requests for LLM capabilities:
//!
//! ```rust,no_run
//! use turbomcp_client::Client;
//! use turbomcp_client::sampling::SamplingHandler;
//! use turbomcp_protocol::types::{CreateMessageRequest, CreateMessageResult, Role, Content, StopReason, TextContent};
//! use async_trait::async_trait;
//!
//! #[derive(Debug)]
//! struct MySamplingHandler {
//!     // Your LLM client would go here
//! }
//!
//! #[async_trait]
//! impl SamplingHandler for MySamplingHandler {
//!     async fn handle_create_message(
//!         &self,
//!         request_id: String,
//!         request: CreateMessageRequest
//!     ) -> Result<CreateMessageResult, Box<dyn std::error::Error + Send + Sync>> {
//!         // Forward to your LLM provider (OpenAI, Anthropic, etc.)
//!         // Use request_id for correlation tracking
//!         // Allows the server to request LLM sampling through the client
//!         
//!         Ok(CreateMessageResult {
//!             role: Role::Assistant,
//!             content: Content::Text(
//!                 TextContent {
//!                     text: "Response from LLM".to_string(),
//!                     annotations: None,
//!                     meta: None,
//!                 }
//!             ),
//!             model: "gpt-4".to_string(),
//!             stop_reason: Some(StopReason::EndTurn),
//!             _meta: None,
//!         })
//!     }
//! }
//! ```
//!
//! ## Error Handling
//!
//! The client provides comprehensive error handling with automatic retry logic:
//!
//! ```rust,no_run
//! # use turbomcp_client::Client;
//! # use turbomcp_transport::stdio::StdioTransport;
//! # async fn example() -> turbomcp_protocol::Result<()> {
//! # let mut client = Client::new(StdioTransport::new());
//! match client.call_tool("my_tool", None).await {
//!     Ok(result) => println!("Tool result: {:?}", result),
//!     Err(e) => eprintln!("Tool call failed: {}", e),
//! }
//! # Ok(())
//! # }
//! ```

/// TurboMCP Client version from Cargo.toml
///
/// This constant provides easy programmatic access to the current version.
///
/// # Example
///
/// ```rust
/// println!("TurboMCP Client version: {}", turbomcp_client::VERSION);
/// ```
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// TurboMCP Client crate name
pub const CRATE_NAME: &str = env!("CARGO_PKG_NAME");

pub mod client;
pub mod handlers;
pub mod integration;
pub mod plugins;
pub mod prelude;
pub mod sampling;

// Tower middleware integration
#[cfg(feature = "tower")]
#[cfg_attr(docsrs, doc(cfg(feature = "tower")))]
pub mod tower;

// Re-export key types for convenience
pub use client::{ConnectionInfo, ConnectionState, ManagerConfig, ServerGroup, SessionManager};

use std::sync::Arc;

// Re-export Transport trait for generic bounds in integrations
pub use turbomcp_transport::Transport;

// ============================================================================
// TOP-LEVEL RE-EXPORTS FOR ERGONOMIC IMPORTS
// ============================================================================

// Result/Error types - re-export from protocol for consistency
pub use turbomcp_protocol::{Error, Result};

// Handler types (most commonly used)
pub use handlers::{
    // Cancellation (MCP 2025-06-18 spec compliant)
    CancellationHandler,
    CancelledNotification,
    ElicitationAction,
    // Elicitation
    ElicitationHandler,
    ElicitationRequest,
    ElicitationResponse,
    // Error handling
    HandlerError,
    HandlerResult,
    // Logging (MCP 2025-06-18 spec compliant)
    LogHandler,
    LoggingNotification,
    PromptListChangedHandler,
    // List changed handlers (MCP 2025-06-18 spec compliant)
    ResourceListChangedHandler,
    // Resource updates (MCP 2025-06-18 spec compliant)
    ResourceUpdateHandler,
    ResourceUpdatedNotification,
    // Roots
    RootsHandler,
    ToolListChangedHandler,
};

// Sampling types
pub use sampling::{SamplingHandler, ServerInfo, UserInteractionHandler};

// Plugin system
pub use plugins::{
    CachePlugin, ClientPlugin, MetricsPlugin, PluginConfig, PluginContext, PluginError,
    PluginResult, RetryPlugin,
};

// Common protocol types
pub use turbomcp_protocol::types::{
    // Resource content types (for processing embedded resources)
    BlobResourceContents,
    // Tool result types (for LLM integrations like rig)
    CallToolResult,
    // Core types
    Content,
    ContentBlock,
    EmbeddedResource,
    LogLevel,
    Prompt,
    Resource,
    ResourceContent,
    ResourceContents,
    Role,
    TextResourceContents,
    Tool,
};

// Transport re-exports (with feature gates)
#[cfg(feature = "stdio")]
pub use turbomcp_transport::stdio::StdioTransport;

#[cfg(feature = "http")]
pub use turbomcp_transport::streamable_http_client::{
    RetryPolicy, StreamableHttpClientConfig, StreamableHttpClientTransport,
};

#[cfg(feature = "tcp")]
pub use turbomcp_transport::tcp::{TcpTransport, TcpTransportBuilder};

#[cfg(feature = "unix")]
pub use turbomcp_transport::unix::{UnixTransport, UnixTransportBuilder};

#[cfg(feature = "websocket")]
pub use turbomcp_transport::websocket_bidirectional::{
    WebSocketBidirectionalConfig, WebSocketBidirectionalTransport,
};

/// Client capability configuration
///
/// Defines the capabilities that this client supports when connecting to MCP servers.
/// These capabilities are sent during the initialization handshake to negotiate
/// which features will be available during the session.
///
/// # Examples
///
/// ```
/// use turbomcp_client::ClientCapabilities;
///
/// let capabilities = ClientCapabilities {
///     tools: true,
///     prompts: true,
///     resources: true,
///     sampling: false,
///     max_concurrent_handlers: 100,
/// };
/// ```
#[derive(Debug, Clone)]
pub struct ClientCapabilities {
    /// Whether the client supports tool calling
    pub tools: bool,

    /// Whether the client supports prompts
    pub prompts: bool,

    /// Whether the client supports resources
    pub resources: bool,

    /// Whether the client supports sampling
    pub sampling: bool,

    /// Maximum concurrent request/notification handlers (default: 100)
    ///
    /// This limits how many server-initiated requests/notifications can be processed simultaneously.
    /// Provides automatic backpressure when the limit is reached.
    ///
    /// **Tuning Guide:**
    /// - Low-resource clients: 50
    /// - Standard clients: 100 (default)
    /// - High-performance: 200-500
    /// - Maximum recommended: 1000
    pub max_concurrent_handlers: usize,
}

impl Default for ClientCapabilities {
    fn default() -> Self {
        Self {
            tools: false,
            prompts: false,
            resources: false,
            sampling: false,
            max_concurrent_handlers: 100,
        }
    }
}

impl ClientCapabilities {
    /// All capabilities enabled (tools, prompts, resources, sampling)
    ///
    /// This is the most comprehensive configuration, enabling full MCP protocol support.
    ///
    /// # Example
    ///
    /// ```rust
    /// use turbomcp_client::ClientCapabilities;
    ///
    /// let capabilities = ClientCapabilities::all();
    /// assert!(capabilities.tools);
    /// assert!(capabilities.prompts);
    /// assert!(capabilities.resources);
    /// assert!(capabilities.sampling);
    /// ```
    #[must_use]
    pub fn all() -> Self {
        Self {
            tools: true,
            prompts: true,
            resources: true,
            sampling: true,
            max_concurrent_handlers: 100,
        }
    }

    /// Core capabilities without sampling (tools, prompts, resources)
    ///
    /// This is the recommended default for most applications. It enables
    /// all standard MCP features except server-initiated sampling requests.
    ///
    /// # Example
    ///
    /// ```rust
    /// use turbomcp_client::ClientCapabilities;
    ///
    /// let capabilities = ClientCapabilities::core();
    /// assert!(capabilities.tools);
    /// assert!(capabilities.prompts);
    /// assert!(capabilities.resources);
    /// assert!(!capabilities.sampling);
    /// ```
    #[must_use]
    pub fn core() -> Self {
        Self {
            tools: true,
            prompts: true,
            resources: true,
            sampling: false,
            max_concurrent_handlers: 100,
        }
    }

    /// Minimal capabilities (tools only)
    ///
    /// Use this for simple tool-calling clients that don't need prompts,
    /// resources, or sampling support.
    ///
    /// # Example
    ///
    /// ```rust
    /// use turbomcp_client::ClientCapabilities;
    ///
    /// let capabilities = ClientCapabilities::minimal();
    /// assert!(capabilities.tools);
    /// assert!(!capabilities.prompts);
    /// assert!(!capabilities.resources);
    /// assert!(!capabilities.sampling);
    /// ```
    #[must_use]
    pub fn minimal() -> Self {
        Self {
            tools: true,
            prompts: false,
            resources: false,
            sampling: false,
            max_concurrent_handlers: 100,
        }
    }

    /// Only tools enabled
    ///
    /// Same as `minimal()`, provided for clarity.
    #[must_use]
    pub fn only_tools() -> Self {
        Self::minimal()
    }

    /// Only resources enabled
    ///
    /// Use this for resource-focused clients that don't need tools or prompts.
    ///
    /// # Example
    ///
    /// ```rust
    /// use turbomcp_client::ClientCapabilities;
    ///
    /// let capabilities = ClientCapabilities::only_resources();
    /// assert!(!capabilities.tools);
    /// assert!(!capabilities.prompts);
    /// assert!(capabilities.resources);
    /// ```
    #[must_use]
    pub fn only_resources() -> Self {
        Self {
            tools: false,
            prompts: false,
            resources: true,
            sampling: false,
            max_concurrent_handlers: 100,
        }
    }

    /// Only prompts enabled
    ///
    /// Use this for prompt-focused clients that don't need tools or resources.
    ///
    /// # Example
    ///
    /// ```rust
    /// use turbomcp_client::ClientCapabilities;
    ///
    /// let capabilities = ClientCapabilities::only_prompts();
    /// assert!(!capabilities.tools);
    /// assert!(capabilities.prompts);
    /// assert!(!capabilities.resources);
    /// ```
    #[must_use]
    pub fn only_prompts() -> Self {
        Self {
            tools: false,
            prompts: true,
            resources: false,
            sampling: false,
            max_concurrent_handlers: 100,
        }
    }

    /// Only sampling enabled
    ///
    /// Use this for clients that exclusively handle server-initiated sampling requests.
    #[must_use]
    pub fn only_sampling() -> Self {
        Self {
            tools: false,
            prompts: false,
            resources: false,
            sampling: true,
            max_concurrent_handlers: 100,
        }
    }
}

/// JSON-RPC protocol handler for MCP communication
// Note: ProtocolClient implementation moved to client/protocol.rs for better modularity
/// MCP client for communicating with servers
///
/// The `Client` struct provides an ergonomic interface for interacting with MCP servers.
/// It handles protocol complexity internally, exposing clean, type-safe methods.
///
/// # Type Parameters
///
/// * `T` - The transport implementation used for communication
///
/// # Examples
///
/// ```rust,no_run
/// use turbomcp_client::Client;
/// use turbomcp_transport::stdio::StdioTransport;
///
/// # async fn example() -> turbomcp_protocol::Result<()> {
/// let transport = StdioTransport::new();
/// let mut client = Client::new(transport);
///
/// // Initialize and start using the client
/// client.initialize().await?;
/// # Ok(())
/// # }
/// ```
// Re-export Client from the core module
pub use client::core::Client;

// Thread-safe wrapper for sharing Client across async tasks
//
// This wrapper encapsulates the Arc/Mutex complexity and provides a clean API
// for concurrent access to MCP client functionality. It addresses the limitations
// identified in PR feedback where Client requires `&mut self` for all operations
// but needs to be shared across multiple async tasks.
//
// # Design Rationale
//
// All Client methods require `&mut self` because:
// - MCP connections maintain state (initialized flag, connection status)
// - Request correlation tracking for JSON-RPC requires mutation
// - Handler and plugin registries need mutable access
//
// Note: SharedClient has been removed in v2 - Client is now directly cloneable via Arc

// ----------------------------------------------------------------------------
// Re-exports
// ----------------------------------------------------------------------------

#[doc = "Result of client initialization"]
#[doc = ""]
#[doc = "Contains information about the server and the negotiated capabilities"]
#[doc = "after a successful initialization handshake."]
pub use client::config::InitializeResult;

// ServerCapabilities is now imported from turbomcp_protocol::types

/// Connection configuration for the client
#[derive(Debug, Clone)]
pub struct ConnectionConfig {
    /// Request timeout in milliseconds
    pub timeout_ms: u64,

    /// Maximum number of retry attempts
    pub max_retries: u32,

    /// Retry delay in milliseconds
    pub retry_delay_ms: u64,

    /// Keep-alive interval in milliseconds
    pub keepalive_ms: u64,
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self {
            timeout_ms: 30_000,    // 30 seconds
            max_retries: 3,        // 3 attempts
            retry_delay_ms: 1_000, // 1 second
            keepalive_ms: 60_000,  // 60 seconds
        }
    }
}

/// Builder for configuring and creating MCP clients
///
/// Provides a fluent interface for configuring client options before creation.
/// The enhanced builder pattern supports comprehensive configuration including:
/// - Protocol capabilities
/// - Plugin registration
/// - Handler registration
/// - Connection settings
/// - Resilience configuration
///
/// # Examples
///
/// Basic usage:
/// ```rust,no_run
/// use turbomcp_client::ClientBuilder;
/// use turbomcp_transport::stdio::StdioTransport;
///
/// # async fn example() -> turbomcp_protocol::Result<()> {
/// let client = ClientBuilder::new()
///     .with_tools(true)
///     .with_prompts(true)
///     .with_resources(false)
///     .build(StdioTransport::new());
/// # Ok(())
/// # }
/// ```
///
/// Advanced configuration:
/// ```rust,no_run
/// use turbomcp_client::{ClientBuilder, ConnectionConfig};
/// use turbomcp_client::plugins::{MetricsPlugin, PluginConfig};
/// use turbomcp_transport::stdio::StdioTransport;
/// use std::sync::Arc;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
/// let client = ClientBuilder::new()
///     .with_tools(true)
///     .with_prompts(true)
///     .with_resources(true)
///     .with_sampling(true)
///     .with_connection_config(ConnectionConfig {
///         timeout_ms: 60_000,
///         max_retries: 5,
///         retry_delay_ms: 2_000,
///         keepalive_ms: 30_000,
///     })
///     .with_plugin(Arc::new(MetricsPlugin::new(PluginConfig::Metrics)))
///     .build(StdioTransport::new())
///     .await?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Default)]
pub struct ClientBuilder {
    capabilities: ClientCapabilities,
    connection_config: ConnectionConfig,
    plugins: Vec<Arc<dyn crate::plugins::ClientPlugin>>,
    elicitation_handler: Option<Arc<dyn crate::handlers::ElicitationHandler>>,
    log_handler: Option<Arc<dyn crate::handlers::LogHandler>>,
    resource_update_handler: Option<Arc<dyn crate::handlers::ResourceUpdateHandler>>,
    // Robustness configuration
    enable_resilience: bool,
    retry_config: Option<turbomcp_transport::resilience::RetryConfig>,
    circuit_breaker_config: Option<turbomcp_transport::resilience::CircuitBreakerConfig>,
    health_check_config: Option<turbomcp_transport::resilience::HealthCheckConfig>,
}

// Default implementation is now derived

impl ClientBuilder {
    /// Create a new client builder
    ///
    /// Returns a new builder with default configuration.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    // ============================================================================
    // CAPABILITY CONFIGURATION
    // ============================================================================

    /// Enable or disable tool support
    ///
    /// # Arguments
    ///
    /// * `enabled` - Whether to enable tool support
    #[must_use]
    pub fn with_tools(mut self, enabled: bool) -> Self {
        self.capabilities.tools = enabled;
        self
    }

    /// Enable or disable prompt support
    ///
    /// # Arguments
    ///
    /// * `enabled` - Whether to enable prompt support
    #[must_use]
    pub fn with_prompts(mut self, enabled: bool) -> Self {
        self.capabilities.prompts = enabled;
        self
    }

    /// Enable or disable resource support
    ///
    /// # Arguments
    ///
    /// * `enabled` - Whether to enable resource support
    #[must_use]
    pub fn with_resources(mut self, enabled: bool) -> Self {
        self.capabilities.resources = enabled;
        self
    }

    /// Enable or disable sampling support
    ///
    /// # Arguments
    ///
    /// * `enabled` - Whether to enable sampling support
    #[must_use]
    pub fn with_sampling(mut self, enabled: bool) -> Self {
        self.capabilities.sampling = enabled;
        self
    }

    /// Set maximum concurrent request/notification handlers
    ///
    /// This limits how many server-initiated requests/notifications can be processed simultaneously.
    /// Provides automatic backpressure when the limit is reached.
    ///
    /// # Arguments
    ///
    /// * `limit` - Maximum concurrent handlers (default: 100)
    ///
    /// # Tuning Guide
    ///
    /// - Low-resource clients: 50
    /// - Standard clients: 100 (default)
    /// - High-performance: 200-500
    /// - Maximum recommended: 1000
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use turbomcp_client::ClientBuilder;
    /// # use turbomcp_transport::StdioTransport;
    ///
    /// let builder = ClientBuilder::new()
    ///     .with_max_concurrent_handlers(200);
    /// ```
    #[must_use]
    pub fn with_max_concurrent_handlers(mut self, limit: usize) -> Self {
        self.capabilities.max_concurrent_handlers = limit;
        self
    }

    /// Configure all capabilities at once
    ///
    /// # Arguments
    ///
    /// * `capabilities` - The capabilities configuration
    #[must_use]
    pub fn with_capabilities(mut self, capabilities: ClientCapabilities) -> Self {
        self.capabilities = capabilities;
        self
    }

    // ============================================================================
    // CONNECTION CONFIGURATION
    // ============================================================================

    /// Configure connection settings
    ///
    /// # Arguments
    ///
    /// * `config` - The connection configuration
    #[must_use]
    pub fn with_connection_config(mut self, config: ConnectionConfig) -> Self {
        self.connection_config = config;
        self
    }

    /// Set request timeout
    ///
    /// # Arguments
    ///
    /// * `timeout_ms` - Timeout in milliseconds
    #[must_use]
    pub fn with_timeout(mut self, timeout_ms: u64) -> Self {
        self.connection_config.timeout_ms = timeout_ms;
        self
    }

    /// Set maximum retry attempts
    ///
    /// # Arguments
    ///
    /// * `max_retries` - Maximum number of retries
    #[must_use]
    pub fn with_max_retries(mut self, max_retries: u32) -> Self {
        self.connection_config.max_retries = max_retries;
        self
    }

    /// Set retry delay
    ///
    /// # Arguments
    ///
    /// * `delay_ms` - Retry delay in milliseconds
    #[must_use]
    pub fn with_retry_delay(mut self, delay_ms: u64) -> Self {
        self.connection_config.retry_delay_ms = delay_ms;
        self
    }

    /// Set keep-alive interval
    ///
    /// # Arguments
    ///
    /// * `interval_ms` - Keep-alive interval in milliseconds
    #[must_use]
    pub fn with_keepalive(mut self, interval_ms: u64) -> Self {
        self.connection_config.keepalive_ms = interval_ms;
        self
    }

    // ============================================================================
    // ROBUSTNESS & RESILIENCE CONFIGURATION
    // ============================================================================

    /// Enable resilient transport with circuit breaker, retry, and health checking
    ///
    /// When enabled, the transport layer will automatically:
    /// - Retry failed operations with exponential backoff
    /// - Use circuit breaker pattern to prevent cascade failures
    /// - Perform periodic health checks
    /// - Deduplicate messages
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use turbomcp_client::ClientBuilder;
    /// use turbomcp_transport::stdio::StdioTransport;
    ///
    /// let client = ClientBuilder::new()
    ///     .enable_resilience()
    ///     .build(StdioTransport::new());
    /// ```
    #[must_use]
    pub fn enable_resilience(mut self) -> Self {
        self.enable_resilience = true;
        self
    }

    /// Configure retry behavior for resilient transport
    ///
    /// # Arguments
    ///
    /// * `config` - Retry configuration
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use turbomcp_client::ClientBuilder;
    /// use turbomcp_transport::resilience::RetryConfig;
    /// use turbomcp_transport::stdio::StdioTransport;
    /// use std::time::Duration;
    ///
    /// let client = ClientBuilder::new()
    ///     .enable_resilience()
    ///     .with_retry_config(RetryConfig {
    ///         max_attempts: 5,
    ///         base_delay: Duration::from_millis(100),
    ///         max_delay: Duration::from_secs(30),
    ///         backoff_multiplier: 2.0,
    ///         jitter_factor: 0.1,
    ///         retry_on_connection_error: true,
    ///         retry_on_timeout: true,
    ///         custom_retry_conditions: Vec::new(),
    ///     })
    ///     .build(StdioTransport::new());
    /// ```
    #[must_use]
    pub fn with_retry_config(
        mut self,
        config: turbomcp_transport::resilience::RetryConfig,
    ) -> Self {
        self.retry_config = Some(config);
        self.enable_resilience = true; // Auto-enable resilience
        self
    }

    /// Configure circuit breaker for resilient transport
    ///
    /// # Arguments
    ///
    /// * `config` - Circuit breaker configuration
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use turbomcp_client::ClientBuilder;
    /// use turbomcp_transport::resilience::CircuitBreakerConfig;
    /// use turbomcp_transport::stdio::StdioTransport;
    /// use std::time::Duration;
    ///
    /// let client = ClientBuilder::new()
    ///     .enable_resilience()
    ///     .with_circuit_breaker_config(CircuitBreakerConfig {
    ///         failure_threshold: 5,
    ///         success_threshold: 2,
    ///         timeout: Duration::from_secs(60),
    ///         rolling_window_size: 100,
    ///         minimum_requests: 10,
    ///     })
    ///     .build(StdioTransport::new());
    /// ```
    #[must_use]
    pub fn with_circuit_breaker_config(
        mut self,
        config: turbomcp_transport::resilience::CircuitBreakerConfig,
    ) -> Self {
        self.circuit_breaker_config = Some(config);
        self.enable_resilience = true; // Auto-enable resilience
        self
    }

    /// Configure health checking for resilient transport
    ///
    /// # Arguments
    ///
    /// * `config` - Health check configuration
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use turbomcp_client::ClientBuilder;
    /// use turbomcp_transport::resilience::HealthCheckConfig;
    /// use turbomcp_transport::stdio::StdioTransport;
    /// use std::time::Duration;
    ///
    /// let client = ClientBuilder::new()
    ///     .enable_resilience()
    ///     .with_health_check_config(HealthCheckConfig {
    ///         interval: Duration::from_secs(30),
    ///         timeout: Duration::from_secs(5),
    ///         failure_threshold: 3,
    ///         success_threshold: 1,
    ///         custom_check: None,
    ///     })
    ///     .build(StdioTransport::new());
    /// ```
    #[must_use]
    pub fn with_health_check_config(
        mut self,
        config: turbomcp_transport::resilience::HealthCheckConfig,
    ) -> Self {
        self.health_check_config = Some(config);
        self.enable_resilience = true; // Auto-enable resilience
        self
    }

    // ============================================================================
    // PLUGIN SYSTEM CONFIGURATION
    // ============================================================================

    /// Register a plugin with the client
    ///
    /// Plugins provide middleware functionality for request/response processing,
    /// metrics collection, retry logic, caching, and other cross-cutting concerns.
    ///
    /// # Arguments
    ///
    /// * `plugin` - The plugin implementation
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use turbomcp_client::{ClientBuilder, ConnectionConfig};
    /// use turbomcp_client::plugins::{MetricsPlugin, RetryPlugin, PluginConfig, RetryConfig};
    /// use std::sync::Arc;
    ///
    /// let client = ClientBuilder::new()
    ///     .with_plugin(Arc::new(MetricsPlugin::new(PluginConfig::Metrics)))
    ///     .with_plugin(Arc::new(RetryPlugin::new(PluginConfig::Retry(RetryConfig {
    ///         max_retries: 5,
    ///         base_delay_ms: 1000,
    ///         max_delay_ms: 30000,
    ///         backoff_multiplier: 2.0,
    ///         retry_on_timeout: true,
    ///         retry_on_connection_error: true,
    ///     }))));
    /// ```
    pub fn with_plugin(mut self, plugin: Arc<dyn crate::plugins::ClientPlugin>) -> Self {
        self.plugins.push(plugin);
        self
    }

    /// Register multiple plugins at once
    ///
    /// # Arguments
    ///
    /// * `plugins` - Vector of plugin implementations
    #[must_use]
    pub fn with_plugins(mut self, plugins: Vec<Arc<dyn crate::plugins::ClientPlugin>>) -> Self {
        self.plugins.extend(plugins);
        self
    }

    // ============================================================================
    // HANDLER REGISTRATION
    // ============================================================================

    /// Register an elicitation handler for processing user input requests
    ///
    /// # Arguments
    ///
    /// * `handler` - The elicitation handler implementation
    pub fn with_elicitation_handler(
        mut self,
        handler: Arc<dyn crate::handlers::ElicitationHandler>,
    ) -> Self {
        self.elicitation_handler = Some(handler);
        self
    }

    /// Register a log handler for processing server log messages
    ///
    /// # Arguments
    ///
    /// * `handler` - The log handler implementation
    pub fn with_log_handler(mut self, handler: Arc<dyn crate::handlers::LogHandler>) -> Self {
        self.log_handler = Some(handler);
        self
    }

    /// Register a resource update handler for processing resource change notifications
    ///
    /// # Arguments
    ///
    /// * `handler` - The resource update handler implementation
    pub fn with_resource_update_handler(
        mut self,
        handler: Arc<dyn crate::handlers::ResourceUpdateHandler>,
    ) -> Self {
        self.resource_update_handler = Some(handler);
        self
    }

    // ============================================================================
    // BUILD METHODS
    // ============================================================================

    /// Build a client with the configured options
    ///
    /// Creates a new client instance with all the configured options. The client
    /// will be initialized with the registered plugins, handlers, and providers.
    ///
    /// # Arguments
    ///
    /// * `transport` - The transport to use for the client
    ///
    /// # Returns
    ///
    /// Returns a configured `Client` instance wrapped in a Result for async setup.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use turbomcp_client::ClientBuilder;
    /// use turbomcp_transport::stdio::StdioTransport;
    ///
    /// # async fn example() -> turbomcp_protocol::Result<()> {
    /// let client = ClientBuilder::new()
    ///     .with_tools(true)
    ///     .with_prompts(true)
    ///     .build(StdioTransport::new())
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn build<T: Transport + 'static>(self, transport: T) -> Result<Client<T>> {
        // Create base client with capabilities
        let client = Client::with_capabilities(transport, self.capabilities);

        // Register handlers
        if let Some(handler) = self.elicitation_handler {
            client.set_elicitation_handler(handler);
        }
        if let Some(handler) = self.log_handler {
            client.set_log_handler(handler);
        }
        if let Some(handler) = self.resource_update_handler {
            client.set_resource_update_handler(handler);
        }

        // Apply connection configuration (store for future use in actual connections)
        // Note: The current Client doesn't expose connection config setters,
        // so we'll store this for when the transport supports it

        // Register plugins with the client
        let has_plugins = !self.plugins.is_empty();
        for plugin in self.plugins {
            client.register_plugin(plugin).await?;
        }

        // Initialize plugins after registration
        if has_plugins {
            client.initialize_plugins().await?;
        }

        Ok(client)
    }

    /// Build a client with resilient transport (circuit breaker, retry, health checking)
    ///
    /// When resilience features are enabled via `enable_resilience()` or any resilience
    /// configuration method, this wraps the transport in a `TurboTransport` that provides:
    /// - Automatic retry with exponential backoff
    /// - Circuit breaker pattern for fast failure
    /// - Health checking and monitoring
    /// - Message deduplication
    ///
    /// # Arguments
    ///
    /// * `transport` - The base transport to wrap with resilience features
    ///
    /// # Returns
    ///
    /// Returns a configured `Client<TurboTransport>` instance.
    ///
    /// # Errors
    ///
    /// Returns an error if plugin initialization fails.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use turbomcp_client::ClientBuilder;
    /// use turbomcp_transport::stdio::StdioTransport;
    /// use turbomcp_transport::resilience::{RetryConfig, CircuitBreakerConfig, HealthCheckConfig};
    /// use std::time::Duration;
    ///
    /// # async fn example() -> turbomcp_protocol::Result<()> {
    /// let client = ClientBuilder::new()
    ///     .with_retry_config(RetryConfig {
    ///         max_attempts: 5,
    ///         base_delay: Duration::from_millis(200),
    ///         ..Default::default()
    ///     })
    ///     .with_circuit_breaker_config(CircuitBreakerConfig {
    ///         failure_threshold: 3,
    ///         timeout: Duration::from_secs(30),
    ///         ..Default::default()
    ///     })
    ///     .with_health_check_config(HealthCheckConfig {
    ///         interval: Duration::from_secs(15),
    ///         timeout: Duration::from_secs(5),
    ///         ..Default::default()
    ///     })
    ///     .build_resilient(StdioTransport::new())
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn build_resilient<T: Transport + 'static>(
        self,
        transport: T,
    ) -> Result<Client<turbomcp_transport::resilience::TurboTransport>> {
        use turbomcp_transport::resilience::TurboTransport;

        // Get configurations or use defaults
        let retry_config = self.retry_config.unwrap_or_default();
        let circuit_config = self.circuit_breaker_config.unwrap_or_default();
        let health_config = self.health_check_config.unwrap_or_default();

        // Wrap transport in TurboTransport
        let robust_transport = TurboTransport::new(
            Box::new(transport),
            retry_config,
            circuit_config,
            health_config,
        );

        // Create client with resilient transport
        let client = Client::with_capabilities(robust_transport, self.capabilities);

        // Register handlers
        if let Some(handler) = self.elicitation_handler {
            client.set_elicitation_handler(handler);
        }
        if let Some(handler) = self.log_handler {
            client.set_log_handler(handler);
        }
        if let Some(handler) = self.resource_update_handler {
            client.set_resource_update_handler(handler);
        }

        // Register plugins
        let has_plugins = !self.plugins.is_empty();
        for plugin in self.plugins {
            client.register_plugin(plugin).await?;
        }

        if has_plugins {
            client.initialize_plugins().await?;
        }

        Ok(client)
    }

    /// Build a client synchronously with basic configuration only
    ///
    /// This is a convenience method for simple use cases where no async setup
    /// is required. For advanced features like plugins, use `build()` instead.
    ///
    /// # Arguments
    ///
    /// * `transport` - The transport to use for the client
    ///
    /// # Returns
    ///
    /// Returns a configured `Client` instance.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use turbomcp_client::ClientBuilder;
    /// use turbomcp_transport::stdio::StdioTransport;
    ///
    /// let client = ClientBuilder::new()
    ///     .with_tools(true)
    ///     .build_sync(StdioTransport::new());
    /// ```
    pub fn build_sync<T: Transport + 'static>(self, transport: T) -> Client<T> {
        let client = Client::with_capabilities(transport, self.capabilities);

        // Register synchronous handlers only
        if let Some(handler) = self.elicitation_handler {
            client.set_elicitation_handler(handler);
        }
        if let Some(handler) = self.log_handler {
            client.set_log_handler(handler);
        }
        if let Some(handler) = self.resource_update_handler {
            client.set_resource_update_handler(handler);
        }

        client
    }

    // ============================================================================
    // CONFIGURATION ACCESS
    // ============================================================================

    /// Get the current capabilities configuration
    #[must_use]
    pub fn capabilities(&self) -> &ClientCapabilities {
        &self.capabilities
    }

    /// Get the current connection configuration
    #[must_use]
    pub fn connection_config(&self) -> &ConnectionConfig {
        &self.connection_config
    }

    /// Get the number of registered plugins
    #[must_use]
    pub fn plugin_count(&self) -> usize {
        self.plugins.len()
    }

    /// Check if any handlers are registered
    #[must_use]
    pub fn has_handlers(&self) -> bool {
        self.elicitation_handler.is_some()
            || self.log_handler.is_some()
            || self.resource_update_handler.is_some()
    }
}

// Re-export types for public API
pub use turbomcp_protocol::types::ServerCapabilities as PublicServerCapabilities;
