//! Handler traits for bidirectional communication in MCP client
//!
//! This module provides handler traits and registration mechanisms for processing
//! server-initiated requests. The MCP protocol is bidirectional, meaning servers
//! can also send requests to clients for various purposes like elicitation,
//! logging, and resource updates.
//!
//! ## Handler Types
//!
//! - **ElicitationHandler**: Handle user input requests from servers
//! - **LogHandler**: Route server log messages to client logging systems
//! - **ResourceUpdateHandler**: Handle notifications when resources change
//!
//! ## Usage
//!
//! ```rust,no_run
//! use turbomcp_client::handlers::{ElicitationHandler, ElicitationRequest, ElicitationResponse, ElicitationAction, HandlerError};
//! use async_trait::async_trait;
//!
//! // Implement elicitation handler
//! #[derive(Debug)]
//! struct MyElicitationHandler;
//!
//! #[async_trait]
//! impl ElicitationHandler for MyElicitationHandler {
//!     async fn handle_elicitation(
//!         &self,
//!         request: ElicitationRequest,
//!     ) -> Result<ElicitationResponse, HandlerError> {
//!         // Display the prompt to the user
//!         eprintln!("\n{}", request.message());
//!         eprintln!("---");
//!
//!         // Access the typed schema (not serde_json::Value!)
//!         let mut content = std::collections::HashMap::new();
//!         if let Some(schema) = request.schema() {
//!             for (field_name, field_def) in &schema.properties {
//!                 eprint!("{}: ", field_name);
//!
//!                 let mut input = String::new();
//!                 std::io::stdin().read_line(&mut input)
//!                     .map_err(|e| HandlerError::Generic {
//!                         message: e.to_string()
//!                     })?;
//!
//!                 let input = input.trim();
//!
//!                 // Parse input based on field type (from typed schema!)
//!                 use turbomcp_protocol::types::PrimitiveSchemaDefinition;
//!                 let value: serde_json::Value = match field_def {
//!                     PrimitiveSchemaDefinition::Boolean { .. } => {
//!                         serde_json::json!(input == "true" || input == "yes" || input == "1")
//!                     }
//!                     PrimitiveSchemaDefinition::Number { .. } | PrimitiveSchemaDefinition::Integer { .. } => {
//!                         input.parse::<f64>()
//!                             .map(|n| serde_json::json!(n))
//!                             .unwrap_or_else(|_| serde_json::json!(input))
//!                     }
//!                     _ => serde_json::json!(input),
//!                 };
//!
//!                 content.insert(field_name.clone(), value);
//!             }
//!         }
//!
//!         Ok(ElicitationResponse::accept(content))
//!     }
//! }
//! ```

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tracing::{debug, error, info, warn};
use turbomcp_protocol::MessageId;
use turbomcp_protocol::jsonrpc::JsonRpcError;
use turbomcp_protocol::types::LogLevel;

// Re-export MCP protocol notification types directly (MCP spec compliance)
pub use turbomcp_protocol::types::{
    CancelledNotification,       // MCP 2025-06-18 spec
    LoggingNotification,         // MCP 2025-06-18 spec
    ResourceUpdatedNotification, // MCP 2025-06-18 spec
};

// ============================================================================
// ERROR TYPES FOR HANDLER OPERATIONS
// ============================================================================

/// Errors that can occur during handler operations
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum HandlerError {
    /// Handler operation failed due to user cancellation
    #[error("User cancelled the operation")]
    UserCancelled,

    /// Handler operation timed out
    #[error("Handler operation timed out after {timeout_seconds} seconds")]
    Timeout { timeout_seconds: u64 },

    /// Input validation failed
    #[error("Invalid input: {details}")]
    InvalidInput { details: String },

    /// Handler configuration error
    #[error("Handler configuration error: {message}")]
    Configuration { message: String },

    /// Generic handler error
    #[error("Handler error: {message}")]
    Generic { message: String },

    /// External system error (e.g., UI framework, database)
    #[error("External system error: {source}")]
    External {
        #[from]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

impl HandlerError {
    /// Convert handler error to JSON-RPC error
    ///
    /// This method centralizes the mapping between handler errors and
    /// JSON-RPC error codes, ensuring consistency across all handlers.
    ///
    /// # Error Code Mapping
    ///
    /// - **-1**: User rejected sampling request (MCP 2025-06-18 spec)
    /// - **-32801**: Handler operation timed out
    /// - **-32602**: Invalid input (bad request)
    /// - **-32601**: Handler configuration error (method not found)
    /// - **-32603**: Generic/external handler error (internal error)
    ///
    /// # Examples
    ///
    /// ```rust
    /// use turbomcp_client::handlers::HandlerError;
    ///
    /// let error = HandlerError::UserCancelled;
    /// let jsonrpc_error = error.into_jsonrpc_error();
    /// assert_eq!(jsonrpc_error.code, -1);
    /// assert!(jsonrpc_error.message.contains("User rejected"));
    /// ```
    #[must_use]
    pub fn into_jsonrpc_error(&self) -> JsonRpcError {
        let (code, message) = match self {
            HandlerError::UserCancelled => (-1, "User rejected sampling request".to_string()),
            HandlerError::Timeout { timeout_seconds } => (
                -32801,
                format!(
                    "Handler operation timed out after {} seconds",
                    timeout_seconds
                ),
            ),
            HandlerError::InvalidInput { details } => {
                (-32602, format!("Invalid input: {}", details))
            }
            HandlerError::Configuration { message } => {
                (-32601, format!("Handler configuration error: {}", message))
            }
            HandlerError::Generic { message } => (-32603, format!("Handler error: {}", message)),
            HandlerError::External { source } => {
                (-32603, format!("External system error: {}", source))
            }
        };

        JsonRpcError {
            code,
            message,
            data: None,
        }
    }
}

pub type HandlerResult<T> = Result<T, HandlerError>;

// ============================================================================
// ELICITATION HANDLER TRAIT
// ============================================================================

/// Ergonomic wrapper around protocol ElicitRequest with request ID
///
/// This type wraps the protocol-level `ElicitRequest` and adds the request ID
/// from the JSON-RPC envelope. It provides ergonomic accessors while preserving
/// full type safety from the protocol layer.
///
/// # Design Philosophy
///
/// Rather than duplicating protocol types, we wrap them. This ensures:
/// - Type safety is preserved (ElicitationSchema stays typed!)
/// - No data loss (Duration instead of lossy integer seconds)
/// - Single source of truth (protocol crate defines MCP types)
/// - Automatic sync (protocol changes propagate automatically)
///
/// # Examples
///
/// ```rust,no_run
/// use turbomcp_client::handlers::ElicitationRequest;
///
/// async fn handle(request: ElicitationRequest) {
///     // Access request ID
///     println!("ID: {:?}", request.id());
///
///     // Access message
///     println!("Message: {}", request.message());
///
///     // Access typed schema (not Value!)
///     if let Some(schema) = request.schema() {
///         for (name, property) in &schema.properties {
///             println!("Field: {}", name);
///         }
///     }
///
///     // Access timeout as Duration
///     if let Some(timeout) = request.timeout() {
///         println!("Timeout: {:?}", timeout);
///     }
/// }
/// ```
#[derive(Debug, Clone)]
pub struct ElicitationRequest {
    id: MessageId,
    inner: turbomcp_protocol::types::ElicitRequest,
}

impl ElicitationRequest {
    /// Create a new elicitation request wrapper
    ///
    /// # Arguments
    ///
    /// * `id` - Request ID from JSON-RPC envelope
    /// * `request` - Protocol-level elicit request
    #[must_use]
    pub fn new(id: MessageId, request: turbomcp_protocol::types::ElicitRequest) -> Self {
        Self { id, inner: request }
    }

    /// Get request ID from JSON-RPC envelope
    #[must_use]
    pub fn id(&self) -> &MessageId {
        &self.id
    }

    /// Get human-readable message for the user
    ///
    /// This is the primary prompt/question being asked of the user.
    #[must_use]
    pub fn message(&self) -> &str {
        self.inner.params.message()
    }

    /// Get schema defining expected response structure
    ///
    /// Returns the typed `ElicitationSchema` which provides:
    /// - Type-safe access to properties
    /// - Required field information
    /// - Validation constraints
    ///
    /// # Note
    ///
    /// This returns a TYPED schema, not `serde_json::Value`.
    /// You can inspect the schema structure type-safely:
    ///
    /// ```rust,no_run
    /// # use turbomcp_client::handlers::ElicitationRequest;
    /// # use turbomcp_protocol::types::PrimitiveSchemaDefinition;
    /// # async fn example(request: ElicitationRequest) {
    /// if let Some(schema) = request.schema() {
    ///     for (name, definition) in &schema.properties {
    ///         match definition {
    ///             PrimitiveSchemaDefinition::String { description, .. } => {
    ///                 println!("String field: {}", name);
    ///             }
    ///             PrimitiveSchemaDefinition::Number { minimum, maximum, .. } => {
    ///                 println!("Number field: {} ({:?}-{:?})", name, minimum, maximum);
    ///             }
    ///             _ => {}
    ///         }
    ///     }
    /// }
    /// # }
    /// ```
    #[must_use]
    pub fn schema(&self) -> Option<&turbomcp_protocol::types::ElicitationSchema> {
        #[allow(unreachable_patterns)]
        match &self.inner.params {
            turbomcp_protocol::types::ElicitRequestParams::Form(form) => Some(&form.schema),
            _ => None, // URL elicitation (when mcp-url-elicitation feature is enabled)
        }
    }

    /// Get optional timeout as Duration
    ///
    /// Converts milliseconds from the protocol to ergonomic `Duration` type.
    /// No data loss occurs (unlike converting to integer seconds).
    #[must_use]
    pub fn timeout(&self) -> Option<Duration> {
        #[allow(unreachable_patterns)]
        match &self.inner.params {
            turbomcp_protocol::types::ElicitRequestParams::Form(form) => {
                form.timeout_ms.map(|ms| Duration::from_millis(ms as u64))
            }
            _ => None, // URL elicitation (when mcp-url-elicitation feature is enabled)
        }
    }

    /// Check if request can be cancelled by the user
    #[must_use]
    pub fn is_cancellable(&self) -> bool {
        #[allow(unreachable_patterns)]
        match &self.inner.params {
            turbomcp_protocol::types::ElicitRequestParams::Form(form) => {
                form.cancellable.unwrap_or(false)
            }
            _ => false, // URL elicitation (when mcp-url-elicitation feature is enabled)
        }
    }

    /// Get access to underlying protocol request if needed
    ///
    /// For advanced use cases where you need the raw protocol type.
    #[must_use]
    pub fn as_protocol(&self) -> &turbomcp_protocol::types::ElicitRequest {
        &self.inner
    }

    /// Consume wrapper and return protocol request
    #[must_use]
    pub fn into_protocol(self) -> turbomcp_protocol::types::ElicitRequest {
        self.inner
    }
}

// Re-export protocol action enum (no need to duplicate)
pub use turbomcp_protocol::types::ElicitationAction;

/// Elicitation response builder
///
/// Wrapper around protocol `ElicitResult` with ergonomic factory methods.
///
/// # Examples
///
/// ```rust
/// use turbomcp_client::handlers::ElicitationResponse;
/// use std::collections::HashMap;
///
/// // Accept with content
/// let mut content = HashMap::new();
/// content.insert("name".to_string(), serde_json::json!("Alice"));
/// let response = ElicitationResponse::accept(content);
///
/// // Decline
/// let response = ElicitationResponse::decline();
///
/// // Cancel
/// let response = ElicitationResponse::cancel();
/// ```
#[derive(Debug, Clone)]
pub struct ElicitationResponse {
    inner: turbomcp_protocol::types::ElicitResult,
}

impl ElicitationResponse {
    /// Create response with accept action and user content
    ///
    /// # Arguments
    ///
    /// * `content` - User-submitted data conforming to the request schema
    #[must_use]
    pub fn accept(content: HashMap<String, serde_json::Value>) -> Self {
        Self {
            inner: turbomcp_protocol::types::ElicitResult {
                action: ElicitationAction::Accept,
                content: Some(content),
                _meta: None,
            },
        }
    }

    /// Create response with decline action (user explicitly declined)
    #[must_use]
    pub fn decline() -> Self {
        Self {
            inner: turbomcp_protocol::types::ElicitResult {
                action: ElicitationAction::Decline,
                content: None,
                _meta: None,
            },
        }
    }

    /// Create response with cancel action (user dismissed without choice)
    #[must_use]
    pub fn cancel() -> Self {
        Self {
            inner: turbomcp_protocol::types::ElicitResult {
                action: ElicitationAction::Cancel,
                content: None,
                _meta: None,
            },
        }
    }

    /// Get the action from this response
    #[must_use]
    pub fn action(&self) -> ElicitationAction {
        self.inner.action
    }

    /// Get the content from this response
    #[must_use]
    pub fn content(&self) -> Option<&HashMap<String, serde_json::Value>> {
        self.inner.content.as_ref()
    }

    /// Convert to protocol type for sending over the wire
    pub(crate) fn into_protocol(self) -> turbomcp_protocol::types::ElicitResult {
        self.inner
    }
}

/// Handler for server-initiated elicitation requests
///
/// Elicitation is a mechanism where servers can request user input during
/// operations. For example, a server might need user preferences, authentication
/// credentials, or configuration choices to complete a task.
///
/// Implementations should:
/// - Present the schema/prompt to the user in an appropriate UI
/// - Validate user input against the provided schema
/// - Handle user cancellation gracefully
/// - Respect timeout constraints
///
/// # Examples
///
/// ```rust,no_run
/// use turbomcp_client::handlers::{ElicitationAction, ElicitationHandler, ElicitationRequest, ElicitationResponse, HandlerResult};
/// use async_trait::async_trait;
/// use serde_json::json;
///
/// #[derive(Debug)]
/// struct CLIElicitationHandler;
///
/// #[async_trait]
/// impl ElicitationHandler for CLIElicitationHandler {
///     async fn handle_elicitation(
///         &self,
///         request: ElicitationRequest,
///     ) -> HandlerResult<ElicitationResponse> {
///         println!("Server request: {}", request.message());
///
///         // In a real implementation, you would:
///         // 1. Inspect the typed schema to understand what input is needed
///         // 2. Present an appropriate UI (CLI prompts, GUI forms, etc.)
///         // 3. Validate the user's input against the schema
///         // 4. Return the structured response
///
///         let mut content = std::collections::HashMap::new();
///         content.insert("user_choice".to_string(), json!("example_value"));
///         Ok(ElicitationResponse::accept(content))
///     }
/// }
/// ```
#[async_trait]
pub trait ElicitationHandler: Send + Sync + std::fmt::Debug {
    /// Handle an elicitation request from the server
    ///
    /// This method is called when a server needs user input. The implementation
    /// should present the request to the user and collect their response.
    ///
    /// # Arguments
    ///
    /// * `request` - The elicitation request containing prompt, schema, and metadata
    ///
    /// # Returns
    ///
    /// Returns the user's response or an error if the operation failed.
    async fn handle_elicitation(
        &self,
        request: ElicitationRequest,
    ) -> HandlerResult<ElicitationResponse>;
}

// ============================================================================

// ============================================================================
// LOG HANDLER TRAIT
// ============================================================================

// LoggingNotification is re-exported from protocol (see imports above)
// This ensures MCP 2025-06-18 spec compliance

/// Handler for server log messages
///
/// Log handlers receive log messages from the server and can route them to
/// the client's logging system. This is useful for debugging, monitoring,
/// and maintaining a unified log across client and server.
///
/// # Examples
///
/// ```rust,no_run
/// use turbomcp_client::handlers::{LogHandler, LoggingNotification, HandlerResult};
/// use turbomcp_protocol::types::LogLevel;
/// use async_trait::async_trait;
///
/// #[derive(Debug)]
/// struct TraceLogHandler;
///
/// #[async_trait]
/// impl LogHandler for TraceLogHandler {
///     async fn handle_log(&self, log: LoggingNotification) -> HandlerResult<()> {
///         // MCP spec: data can be any JSON type (string, object, etc.)
///         let message = log.data.to_string();
///         match log.level {
///             LogLevel::Error => tracing::error!("Server: {}", message),
///             LogLevel::Warning => tracing::warn!("Server: {}", message),
///             LogLevel::Info => tracing::info!("Server: {}", message),
///             LogLevel::Debug => tracing::debug!("Server: {}", message),
///             LogLevel::Notice => tracing::info!("Server: {}", message),
///             LogLevel::Critical => tracing::error!("Server CRITICAL: {}", message),
///             LogLevel::Alert => tracing::error!("Server ALERT: {}", message),
///             LogLevel::Emergency => tracing::error!("Server EMERGENCY: {}", message),
///         }
///         Ok(())
///     }
/// }
/// ```
#[async_trait]
pub trait LogHandler: Send + Sync + std::fmt::Debug {
    /// Handle a log message from the server
    ///
    /// This method is called when the server sends log messages to the client.
    /// Implementations can route these to the client's logging system.
    ///
    /// # Arguments
    ///
    /// * `log` - The log notification with level and data (per MCP 2025-06-18 spec)
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if the log message was processed successfully.
    async fn handle_log(&self, log: LoggingNotification) -> HandlerResult<()>;
}

// ============================================================================
// RESOURCE UPDATE HANDLER TRAIT
// ============================================================================

// ResourceUpdatedNotification is re-exported from protocol (see imports above)
// This ensures MCP 2025-06-18 spec compliance
//
// Per MCP spec: This notification ONLY contains the URI of the changed resource.
// Clients must call resources/read to get the updated content.

/// Handler for resource update notifications
///
/// Resource update handlers receive notifications when resources that the
/// client has subscribed to are modified. This enables reactive updates
/// to cached data or UI refreshes when server-side resources change.
///
/// # Examples
///
/// ```rust,no_run
/// use turbomcp_client::handlers::{ResourceUpdateHandler, ResourceUpdatedNotification, HandlerResult};
/// use async_trait::async_trait;
///
/// #[derive(Debug)]
/// struct CacheInvalidationHandler;
///
/// #[async_trait]
/// impl ResourceUpdateHandler for CacheInvalidationHandler {
///     async fn handle_resource_update(
///         &self,
///         notification: ResourceUpdatedNotification,
///     ) -> HandlerResult<()> {
///         // Per MCP spec: notification only contains URI
///         // Client must call resources/read to get updated content
///         println!("Resource {} was updated", notification.uri);
///
///         // In a real implementation, you might:
///         // - Invalidate cached data for this resource
///         // - Refresh UI components that display this resource
///         // - Log the change for audit purposes
///         // - Trigger dependent computations
///         
///         Ok(())
///     }
/// }
/// ```
#[async_trait]
pub trait ResourceUpdateHandler: Send + Sync + std::fmt::Debug {
    /// Handle a resource update notification
    ///
    /// This method is called when a subscribed resource changes on the server.
    ///
    /// # Arguments
    ///
    /// * `notification` - Information about the resource change
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if the notification was processed successfully.
    async fn handle_resource_update(
        &self,
        notification: ResourceUpdatedNotification,
    ) -> HandlerResult<()>;
}

// ============================================================================
// ROOTS HANDLER TRAIT
// ============================================================================

/// Roots handler for responding to server requests for filesystem roots
///
/// Per MCP 2025-06-18 specification, `roots/list` is a SERVER->CLIENT request.
/// Servers ask clients what filesystem roots (directories/files) they have access to.
/// This is commonly used when servers need to understand their operating boundaries,
/// such as which repositories or project directories they can access.
///
/// # Examples
///
/// ```rust,no_run
/// use turbomcp_client::handlers::{RootsHandler, HandlerResult};
/// use turbomcp_protocol::types::Root;
/// use async_trait::async_trait;
///
/// #[derive(Debug)]
/// struct MyRootsHandler {
///     project_dirs: Vec<String>,
/// }
///
/// #[async_trait]
/// impl RootsHandler for MyRootsHandler {
///     async fn handle_roots_request(&self) -> HandlerResult<Vec<Root>> {
///         Ok(self.project_dirs
///             .iter()
///             .map(|dir| Root {
///                 uri: format!("file://{}", dir).into(),
///                 name: Some(dir.split('/').last().unwrap_or("").to_string()),
///             })
///             .collect())
///     }
/// }
/// ```
#[async_trait]
pub trait RootsHandler: Send + Sync + std::fmt::Debug {
    /// Handle a roots/list request from the server
    ///
    /// This method is called when the server wants to know which filesystem roots
    /// the client has available. The implementation should return a list of Root
    /// objects representing directories or files the server can operate on.
    ///
    /// # Returns
    ///
    /// Returns a vector of Root objects, each with a URI (must start with file://)
    /// and optional human-readable name.
    ///
    /// # Note
    ///
    /// Per MCP specification, URIs must start with `file://` for now. This restriction
    /// may be relaxed in future protocol versions.
    async fn handle_roots_request(&self) -> HandlerResult<Vec<turbomcp_protocol::types::Root>>;
}

// ============================================================================
// CANCELLATION HANDLER TRAIT
// ============================================================================

/// Cancellation handler for processing cancellation notifications
///
/// Per MCP 2025-06-18 specification, `notifications/cancelled` can be sent by
/// either side to indicate cancellation of a previously-issued request.
///
/// When the server sends a cancellation notification, it indicates that a request
/// the client sent is being cancelled and the result will be unused. The client
/// SHOULD cease any associated processing.
///
/// # MCP Specification
///
/// From the MCP spec:
/// - "The request SHOULD still be in-flight, but due to communication latency,
///    it is always possible that this notification MAY arrive after the request
///    has already finished."
/// - "A client MUST NOT attempt to cancel its `initialize` request."
///
/// # Examples
///
/// ```rust,no_run
/// use turbomcp_client::handlers::{CancellationHandler, CancelledNotification, HandlerResult};
/// use async_trait::async_trait;
///
/// #[derive(Debug)]
/// struct MyCancellationHandler;
///
/// #[async_trait]
/// impl CancellationHandler for MyCancellationHandler {
///     async fn handle_cancellation(&self, notification: CancelledNotification) -> HandlerResult<()> {
///         println!("Request {} was cancelled", notification.request_id);
///         if let Some(reason) = &notification.reason {
///             println!("Reason: {}", reason);
///         }
///
///         // In a real implementation:
///         // - Look up the in-flight request by notification.request_id
///         // - Signal cancellation (e.g., via CancellationToken)
///         // - Clean up any resources
///
///         Ok(())
///     }
/// }
/// ```
#[async_trait]
pub trait CancellationHandler: Send + Sync + std::fmt::Debug {
    /// Handle a cancellation notification
    ///
    /// This method is called when the server cancels a request that the client
    /// previously issued.
    ///
    /// # Arguments
    ///
    /// * `notification` - The cancellation notification containing request ID and optional reason
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if the cancellation was processed successfully.
    async fn handle_cancellation(&self, notification: CancelledNotification) -> HandlerResult<()>;
}

// ============================================================================
// LIST CHANGED HANDLER TRAITS
// ============================================================================

/// Handler for resource list changes
///
/// Per MCP 2025-06-18 specification, `notifications/resources/list_changed` is
/// an optional notification from the server to the client, informing it that the
/// list of resources it can read from has changed.
///
/// This notification has no parameters - it simply signals that the client should
/// re-query the server's resource list if needed.
///
/// # Examples
///
/// ```rust,no_run
/// use turbomcp_client::handlers::{ResourceListChangedHandler, HandlerResult};
/// use async_trait::async_trait;
///
/// #[derive(Debug)]
/// struct MyResourceListHandler;
///
/// #[async_trait]
/// impl ResourceListChangedHandler for MyResourceListHandler {
///     async fn handle_resource_list_changed(&self) -> HandlerResult<()> {
///         println!("Server's resource list changed - refreshing...");
///         // In a real implementation, re-query: client.list_resources().await
///         Ok(())
///     }
/// }
/// ```
#[async_trait]
pub trait ResourceListChangedHandler: Send + Sync + std::fmt::Debug {
    /// Handle a resource list changed notification
    ///
    /// This method is called when the server's available resource list changes.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if the notification was processed successfully.
    async fn handle_resource_list_changed(&self) -> HandlerResult<()>;
}

/// Handler for prompt list changes
///
/// Per MCP 2025-06-18 specification, `notifications/prompts/list_changed` is
/// an optional notification from the server to the client, informing it that the
/// list of prompts it offers has changed.
///
/// # Examples
///
/// ```rust,no_run
/// use turbomcp_client::handlers::{PromptListChangedHandler, HandlerResult};
/// use async_trait::async_trait;
///
/// #[derive(Debug)]
/// struct MyPromptListHandler;
///
/// #[async_trait]
/// impl PromptListChangedHandler for MyPromptListHandler {
///     async fn handle_prompt_list_changed(&self) -> HandlerResult<()> {
///         println!("Server's prompt list changed - refreshing...");
///         Ok(())
///     }
/// }
/// ```
#[async_trait]
pub trait PromptListChangedHandler: Send + Sync + std::fmt::Debug {
    /// Handle a prompt list changed notification
    ///
    /// This method is called when the server's available prompt list changes.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if the notification was processed successfully.
    async fn handle_prompt_list_changed(&self) -> HandlerResult<()>;
}

/// Handler for tool list changes
///
/// Per MCP 2025-06-18 specification, `notifications/tools/list_changed` is
/// an optional notification from the server to the client, informing it that the
/// list of tools it offers has changed.
///
/// # Examples
///
/// ```rust,no_run
/// use turbomcp_client::handlers::{ToolListChangedHandler, HandlerResult};
/// use async_trait::async_trait;
///
/// #[derive(Debug)]
/// struct MyToolListHandler;
///
/// #[async_trait]
/// impl ToolListChangedHandler for MyToolListHandler {
///     async fn handle_tool_list_changed(&self) -> HandlerResult<()> {
///         println!("Server's tool list changed - refreshing...");
///         Ok(())
///     }
/// }
/// ```
#[async_trait]
pub trait ToolListChangedHandler: Send + Sync + std::fmt::Debug {
    /// Handle a tool list changed notification
    ///
    /// This method is called when the server's available tool list changes.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if the notification was processed successfully.
    async fn handle_tool_list_changed(&self) -> HandlerResult<()>;
}

// ============================================================================
// HANDLER REGISTRY FOR CLIENT
// ============================================================================

/// Registry for managing client-side handlers
///
/// This registry holds all the handler implementations and provides methods
/// for registering and invoking them. It's used internally by the Client
/// to dispatch server-initiated requests to the appropriate handlers.
#[derive(Debug, Default)]
pub struct HandlerRegistry {
    /// Roots handler for filesystem root requests
    pub roots: Option<Arc<dyn RootsHandler>>,

    /// Elicitation handler for user input requests
    pub elicitation: Option<Arc<dyn ElicitationHandler>>,

    /// Log handler for server log messages
    pub log: Option<Arc<dyn LogHandler>>,

    /// Resource update handler for resource change notifications
    pub resource_update: Option<Arc<dyn ResourceUpdateHandler>>,

    /// Cancellation handler for cancellation notifications
    pub cancellation: Option<Arc<dyn CancellationHandler>>,

    /// Resource list changed handler
    pub resource_list_changed: Option<Arc<dyn ResourceListChangedHandler>>,

    /// Prompt list changed handler
    pub prompt_list_changed: Option<Arc<dyn PromptListChangedHandler>>,

    /// Tool list changed handler
    pub tool_list_changed: Option<Arc<dyn ToolListChangedHandler>>,
}

impl HandlerRegistry {
    /// Create a new empty handler registry
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a roots handler
    pub fn set_roots_handler(&mut self, handler: Arc<dyn RootsHandler>) {
        debug!("Registering roots handler");
        self.roots = Some(handler);
    }

    /// Register an elicitation handler
    pub fn set_elicitation_handler(&mut self, handler: Arc<dyn ElicitationHandler>) {
        debug!("Registering elicitation handler");
        self.elicitation = Some(handler);
    }

    /// Register a log handler
    pub fn set_log_handler(&mut self, handler: Arc<dyn LogHandler>) {
        debug!("Registering log handler");
        self.log = Some(handler);
    }

    /// Register a resource update handler
    pub fn set_resource_update_handler(&mut self, handler: Arc<dyn ResourceUpdateHandler>) {
        debug!("Registering resource update handler");
        self.resource_update = Some(handler);
    }

    /// Register a cancellation handler
    pub fn set_cancellation_handler(&mut self, handler: Arc<dyn CancellationHandler>) {
        debug!("Registering cancellation handler");
        self.cancellation = Some(handler);
    }

    /// Register a resource list changed handler
    pub fn set_resource_list_changed_handler(
        &mut self,
        handler: Arc<dyn ResourceListChangedHandler>,
    ) {
        debug!("Registering resource list changed handler");
        self.resource_list_changed = Some(handler);
    }

    /// Register a prompt list changed handler
    pub fn set_prompt_list_changed_handler(&mut self, handler: Arc<dyn PromptListChangedHandler>) {
        debug!("Registering prompt list changed handler");
        self.prompt_list_changed = Some(handler);
    }

    /// Register a tool list changed handler
    pub fn set_tool_list_changed_handler(&mut self, handler: Arc<dyn ToolListChangedHandler>) {
        debug!("Registering tool list changed handler");
        self.tool_list_changed = Some(handler);
    }

    /// Check if a roots handler is registered
    #[must_use]
    pub fn has_roots_handler(&self) -> bool {
        self.roots.is_some()
    }

    /// Check if an elicitation handler is registered
    #[must_use]
    pub fn has_elicitation_handler(&self) -> bool {
        self.elicitation.is_some()
    }

    /// Check if a log handler is registered
    #[must_use]
    pub fn has_log_handler(&self) -> bool {
        self.log.is_some()
    }

    /// Check if a resource update handler is registered
    #[must_use]
    pub fn has_resource_update_handler(&self) -> bool {
        self.resource_update.is_some()
    }

    /// Get the log handler if registered
    #[must_use]
    pub fn get_log_handler(&self) -> Option<Arc<dyn LogHandler>> {
        self.log.clone()
    }

    /// Get the resource update handler if registered
    #[must_use]
    pub fn get_resource_update_handler(&self) -> Option<Arc<dyn ResourceUpdateHandler>> {
        self.resource_update.clone()
    }

    /// Get the cancellation handler if registered
    #[must_use]
    pub fn get_cancellation_handler(&self) -> Option<Arc<dyn CancellationHandler>> {
        self.cancellation.clone()
    }

    /// Get the resource list changed handler if registered
    #[must_use]
    pub fn get_resource_list_changed_handler(&self) -> Option<Arc<dyn ResourceListChangedHandler>> {
        self.resource_list_changed.clone()
    }

    /// Get the prompt list changed handler if registered
    #[must_use]
    pub fn get_prompt_list_changed_handler(&self) -> Option<Arc<dyn PromptListChangedHandler>> {
        self.prompt_list_changed.clone()
    }

    /// Get the tool list changed handler if registered
    #[must_use]
    pub fn get_tool_list_changed_handler(&self) -> Option<Arc<dyn ToolListChangedHandler>> {
        self.tool_list_changed.clone()
    }

    /// Handle a roots/list request from the server
    pub async fn handle_roots_request(&self) -> HandlerResult<Vec<turbomcp_protocol::types::Root>> {
        match &self.roots {
            Some(handler) => {
                info!("Processing roots/list request from server");
                handler.handle_roots_request().await
            }
            None => {
                warn!("No roots handler registered, returning empty roots list");
                // Return empty list per MCP spec - client has no roots available
                Ok(Vec::new())
            }
        }
    }

    /// Handle an elicitation request
    pub async fn handle_elicitation(
        &self,
        request: ElicitationRequest,
    ) -> HandlerResult<ElicitationResponse> {
        match &self.elicitation {
            Some(handler) => {
                info!("Processing elicitation request: {}", request.id);
                handler.handle_elicitation(request).await
            }
            None => {
                warn!("No elicitation handler registered, declining request");
                Err(HandlerError::Configuration {
                    message: "No elicitation handler registered".to_string(),
                })
            }
        }
    }

    /// Handle a log message
    pub async fn handle_log(&self, log: LoggingNotification) -> HandlerResult<()> {
        match &self.log {
            Some(handler) => handler.handle_log(log).await,
            None => {
                debug!("No log handler registered, ignoring log message");
                Ok(())
            }
        }
    }

    /// Handle a resource update notification
    pub async fn handle_resource_update(
        &self,
        notification: ResourceUpdatedNotification,
    ) -> HandlerResult<()> {
        match &self.resource_update {
            Some(handler) => {
                debug!("Processing resource update: {}", notification.uri);
                handler.handle_resource_update(notification).await
            }
            None => {
                debug!("No resource update handler registered, ignoring notification");
                Ok(())
            }
        }
    }
}

// ============================================================================
// DEFAULT HANDLER IMPLEMENTATIONS
// ============================================================================

/// Default elicitation handler that declines all requests
#[derive(Debug)]
pub struct DeclineElicitationHandler;

#[async_trait]
impl ElicitationHandler for DeclineElicitationHandler {
    async fn handle_elicitation(
        &self,
        request: ElicitationRequest,
    ) -> HandlerResult<ElicitationResponse> {
        warn!("Declining elicitation request: {}", request.message());
        Ok(ElicitationResponse::decline())
    }
}

/// Default log handler that routes server logs to tracing
#[derive(Debug)]
pub struct TracingLogHandler;

#[async_trait]
impl LogHandler for TracingLogHandler {
    async fn handle_log(&self, log: LoggingNotification) -> HandlerResult<()> {
        let logger_prefix = log.logger.as_deref().unwrap_or("server");

        // Per MCP spec: data can be any JSON type (string, object, etc.)
        let message = log.data.to_string();
        match log.level {
            LogLevel::Error => error!("[{}] {}", logger_prefix, message),
            LogLevel::Warning => warn!("[{}] {}", logger_prefix, message),
            LogLevel::Info => info!("[{}] {}", logger_prefix, message),
            LogLevel::Debug => debug!("[{}] {}", logger_prefix, message),
            LogLevel::Notice => info!("[{}] [NOTICE] {}", logger_prefix, message),
            LogLevel::Critical => error!("[{}] [CRITICAL] {}", logger_prefix, message),
            LogLevel::Alert => error!("[{}] [ALERT] {}", logger_prefix, message),
            LogLevel::Emergency => error!("[{}] [EMERGENCY] {}", logger_prefix, message),
        }

        Ok(())
    }
}

/// Default resource update handler that logs changes
#[derive(Debug)]
pub struct LoggingResourceUpdateHandler;

#[async_trait]
impl ResourceUpdateHandler for LoggingResourceUpdateHandler {
    async fn handle_resource_update(
        &self,
        notification: ResourceUpdatedNotification,
    ) -> HandlerResult<()> {
        // Per MCP spec: notification only contains URI
        info!("Resource {} was updated", notification.uri);
        Ok(())
    }
}

/// Default cancellation handler that logs cancellation notifications
#[derive(Debug)]
pub struct LoggingCancellationHandler;

#[async_trait]
impl CancellationHandler for LoggingCancellationHandler {
    async fn handle_cancellation(&self, notification: CancelledNotification) -> HandlerResult<()> {
        if let Some(reason) = &notification.reason {
            info!(
                "Request {} was cancelled: {}",
                notification.request_id, reason
            );
        } else {
            info!("Request {} was cancelled", notification.request_id);
        }
        Ok(())
    }
}

/// Default resource list changed handler that logs changes
#[derive(Debug)]
pub struct LoggingResourceListChangedHandler;

#[async_trait]
impl ResourceListChangedHandler for LoggingResourceListChangedHandler {
    async fn handle_resource_list_changed(&self) -> HandlerResult<()> {
        info!("Server's resource list changed");
        Ok(())
    }
}

/// Default prompt list changed handler that logs changes
#[derive(Debug)]
pub struct LoggingPromptListChangedHandler;

#[async_trait]
impl PromptListChangedHandler for LoggingPromptListChangedHandler {
    async fn handle_prompt_list_changed(&self) -> HandlerResult<()> {
        info!("Server's prompt list changed");
        Ok(())
    }
}

/// Default tool list changed handler that logs changes
#[derive(Debug)]
pub struct LoggingToolListChangedHandler;

#[async_trait]
impl ToolListChangedHandler for LoggingToolListChangedHandler {
    async fn handle_tool_list_changed(&self) -> HandlerResult<()> {
        info!("Server's tool list changed");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tokio;

    // Test handler implementations
    #[derive(Debug)]
    struct TestElicitationHandler;

    #[async_trait]
    impl ElicitationHandler for TestElicitationHandler {
        async fn handle_elicitation(
            &self,
            _request: ElicitationRequest,
        ) -> HandlerResult<ElicitationResponse> {
            let mut content = HashMap::new();
            content.insert("test".to_string(), json!("response"));
            Ok(ElicitationResponse::accept(content))
        }
    }

    #[tokio::test]
    async fn test_handler_registry_creation() {
        let registry = HandlerRegistry::new();
        assert!(!registry.has_elicitation_handler());
        assert!(!registry.has_log_handler());
        assert!(!registry.has_resource_update_handler());
    }

    #[tokio::test]
    async fn test_elicitation_handler_registration() {
        let mut registry = HandlerRegistry::new();
        let handler = Arc::new(TestElicitationHandler);

        registry.set_elicitation_handler(handler);
        assert!(registry.has_elicitation_handler());
    }

    #[tokio::test]
    async fn test_elicitation_request_handling() {
        let mut registry = HandlerRegistry::new();
        let handler = Arc::new(TestElicitationHandler);
        registry.set_elicitation_handler(handler);

        // Create protocol request
        let proto_request = turbomcp_protocol::types::ElicitRequest {
            params: turbomcp_protocol::types::ElicitRequestParams::form(
                "Test prompt".to_string(),
                turbomcp_protocol::types::ElicitationSchema::new(),
                None,
                None,
            ),
            task: None,
            _meta: None,
        };

        // Wrap for handler
        let request = ElicitationRequest::new(
            turbomcp_protocol::MessageId::String("test-123".to_string()),
            proto_request,
        );

        let response = registry.handle_elicitation(request).await.unwrap();
        assert_eq!(response.action(), ElicitationAction::Accept);
        assert!(response.content().is_some());
    }

    #[tokio::test]
    async fn test_default_handlers() {
        let decline_handler = DeclineElicitationHandler;

        // Create protocol request
        let proto_request = turbomcp_protocol::types::ElicitRequest {
            params: turbomcp_protocol::types::ElicitRequestParams::form(
                "Test".to_string(),
                turbomcp_protocol::types::ElicitationSchema::new(),
                None,
                None,
            ),
            task: None,
            _meta: None,
        };

        // Wrap for handler
        let request = ElicitationRequest::new(
            turbomcp_protocol::MessageId::String("test".to_string()),
            proto_request,
        );

        let response = decline_handler.handle_elicitation(request).await.unwrap();
        assert_eq!(response.action(), ElicitationAction::Decline);
    }

    #[tokio::test]
    async fn test_handler_error_types() {
        let error = HandlerError::UserCancelled;
        assert!(error.to_string().contains("User cancelled"));

        let timeout_error = HandlerError::Timeout {
            timeout_seconds: 30,
        };
        assert!(timeout_error.to_string().contains("30 seconds"));
    }

    // ========================================================================
    // JSON-RPC Error Mapping Tests
    // ========================================================================

    #[test]
    fn test_user_cancelled_error_mapping() {
        let error = HandlerError::UserCancelled;
        let jsonrpc_error = error.into_jsonrpc_error();

        assert_eq!(
            jsonrpc_error.code, -1,
            "User cancelled should map to -1 per MCP 2025-06-18 spec"
        );
        assert!(jsonrpc_error.message.contains("User rejected"));
        assert!(jsonrpc_error.data.is_none());
    }

    #[test]
    fn test_timeout_error_mapping() {
        let error = HandlerError::Timeout {
            timeout_seconds: 30,
        };
        let jsonrpc_error = error.into_jsonrpc_error();

        assert_eq!(jsonrpc_error.code, -32801, "Timeout should map to -32801");
        assert!(jsonrpc_error.message.contains("30 seconds"));
        assert!(jsonrpc_error.data.is_none());
    }

    #[test]
    fn test_invalid_input_error_mapping() {
        let error = HandlerError::InvalidInput {
            details: "Missing required field".to_string(),
        };
        let jsonrpc_error = error.into_jsonrpc_error();

        assert_eq!(
            jsonrpc_error.code, -32602,
            "Invalid input should map to -32602"
        );
        assert!(jsonrpc_error.message.contains("Invalid input"));
        assert!(jsonrpc_error.message.contains("Missing required field"));
        assert!(jsonrpc_error.data.is_none());
    }

    #[test]
    fn test_configuration_error_mapping() {
        let error = HandlerError::Configuration {
            message: "Handler not registered".to_string(),
        };
        let jsonrpc_error = error.into_jsonrpc_error();

        assert_eq!(
            jsonrpc_error.code, -32601,
            "Configuration error should map to -32601"
        );
        assert!(
            jsonrpc_error
                .message
                .contains("Handler configuration error")
        );
        assert!(jsonrpc_error.message.contains("Handler not registered"));
        assert!(jsonrpc_error.data.is_none());
    }

    #[test]
    fn test_generic_error_mapping() {
        let error = HandlerError::Generic {
            message: "Something went wrong".to_string(),
        };
        let jsonrpc_error = error.into_jsonrpc_error();

        assert_eq!(
            jsonrpc_error.code, -32603,
            "Generic error should map to -32603"
        );
        assert!(jsonrpc_error.message.contains("Handler error"));
        assert!(jsonrpc_error.message.contains("Something went wrong"));
        assert!(jsonrpc_error.data.is_none());
    }

    #[test]
    fn test_external_error_mapping() {
        let external_err = Box::new(std::io::Error::other("Database connection failed"));
        let error = HandlerError::External {
            source: external_err,
        };
        let jsonrpc_error = error.into_jsonrpc_error();

        assert_eq!(
            jsonrpc_error.code, -32603,
            "External error should map to -32603"
        );
        assert!(jsonrpc_error.message.contains("External system error"));
        assert!(jsonrpc_error.message.contains("Database connection failed"));
        assert!(jsonrpc_error.data.is_none());
    }

    #[test]
    fn test_error_code_uniqueness() {
        // Verify that user-facing errors have unique codes
        let user_cancelled = HandlerError::UserCancelled.into_jsonrpc_error().code;
        let timeout = HandlerError::Timeout { timeout_seconds: 1 }
            .into_jsonrpc_error()
            .code;
        let invalid_input = HandlerError::InvalidInput {
            details: "test".to_string(),
        }
        .into_jsonrpc_error()
        .code;
        let configuration = HandlerError::Configuration {
            message: "test".to_string(),
        }
        .into_jsonrpc_error()
        .code;

        // These should all be different
        assert_ne!(user_cancelled, timeout);
        assert_ne!(user_cancelled, invalid_input);
        assert_ne!(user_cancelled, configuration);
        assert_ne!(timeout, invalid_input);
        assert_ne!(timeout, configuration);
        assert_ne!(invalid_input, configuration);
    }

    #[test]
    fn test_error_messages_are_informative() {
        // Verify all error messages contain useful information
        let errors = vec![
            HandlerError::UserCancelled,
            HandlerError::Timeout {
                timeout_seconds: 42,
            },
            HandlerError::InvalidInput {
                details: "test detail".to_string(),
            },
            HandlerError::Configuration {
                message: "test config".to_string(),
            },
            HandlerError::Generic {
                message: "test generic".to_string(),
            },
        ];

        for error in errors {
            let jsonrpc_error = error.into_jsonrpc_error();
            assert!(
                !jsonrpc_error.message.is_empty(),
                "Error message should not be empty"
            );
            assert!(
                jsonrpc_error.message.len() > 10,
                "Error message should be descriptive"
            );
        }
    }
}
