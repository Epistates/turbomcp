//! Handler traits for bidirectional communication in MCP client
//!
//! This module provides handler traits and registration mechanisms for processing
//! server-initiated requests. The MCP protocol is bidirectional, meaning servers
//! can also send requests to clients for various purposes like elicitation,
//! progress reporting, logging, and resource updates.
//!
//! ## Handler Types
//!
//! - **ElicitationHandler**: Handle user input requests from servers
//! - **ProgressHandler**: Process progress notifications from long-running operations
//! - **LogHandler**: Route server log messages to client logging systems
//! - **ResourceUpdateHandler**: Handle notifications when resources change
//!
//! ## Usage
//!
//! ```rust,no_run
//! use turbomcp_client::handlers::{ElicitationHandler, ElicitationRequest, ElicitationResponse, HandlerError};
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
//!         // Present schema to user and collect input
//!         // Return user's response
//!         todo!("Implement user interaction")
//!     }
//! }
//! ```

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tracing::{debug, error, info, warn};
use turbomcp_protocol::types::{
    LogLevel, ProgressNotification as ProtocolProgressNotification, ResourceContents,
};

// ============================================================================
// ERROR TYPES FOR HANDLER OPERATIONS
// ============================================================================

/// Errors that can occur during handler operations
#[derive(Error, Debug)]
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

pub type HandlerResult<T> = Result<T, HandlerError>;

// ============================================================================
// ELICITATION HANDLER TRAIT
// ============================================================================

/// Request structure for elicitation operations
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ElicitationRequest {
    /// Unique identifier for this elicitation request
    pub id: String,

    /// Human-readable prompt for the user
    pub prompt: String,

    /// JSON schema defining the expected response structure
    pub schema: serde_json::Value,

    /// Optional timeout in seconds
    pub timeout: Option<u64>,

    /// Additional metadata for the request
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Response structure for elicitation operations
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ElicitationResponse {
    /// The elicitation request ID this responds to
    pub id: String,

    /// User's response data (must conform to the request schema)
    pub data: serde_json::Value,

    /// Whether the user cancelled the operation
    pub cancelled: bool,
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
/// use turbomcp_client::handlers::{ElicitationHandler, ElicitationRequest, ElicitationResponse, HandlerResult};
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
///         println!("Server request: {}", request.prompt);
///         
///         // In a real implementation, you would:
///         // 1. Parse the schema to understand what input is needed
///         // 2. Present an appropriate UI (CLI prompts, GUI forms, etc.)
///         // 3. Validate the user's input against the schema
///         // 4. Return the structured response
///         
///         Ok(ElicitationResponse {
///             id: request.id,
///             data: json!({ "user_choice": "example_value" }),
///             cancelled: false,
///         })
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
// PROGRESS HANDLER TRAIT
// ============================================================================

/// Progress notification from server operations
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProgressNotification {
    /// Unique identifier for the operation being tracked
    pub operation_id: String,

    /// Current progress information
    pub progress: ProtocolProgressNotification,

    /// Human-readable status message
    pub message: Option<String>,

    /// Whether the operation has completed
    pub completed: bool,

    /// Optional error information if the operation failed
    pub error: Option<String>,
}

/// Handler for server progress notifications
///
/// Progress handlers receive notifications about long-running server operations.
/// This allows clients to display progress bars, status updates, or other
/// feedback to users during operations that take significant time.
///
/// # Examples
///
/// ```rust,no_run
/// use turbomcp_client::handlers::{ProgressHandler, ProgressNotification, HandlerResult};
/// use async_trait::async_trait;
///
/// #[derive(Debug)]
/// struct ProgressBarHandler;
///
/// #[async_trait]
/// impl ProgressHandler for ProgressBarHandler {
///     async fn handle_progress(&self, notification: ProgressNotification) -> HandlerResult<()> {
///         let progress_val = notification.progress.progress;
///         if let Some(total) = notification.progress.total {
///             let percentage = (progress_val / total) * 100.0;
///             println!("Progress: {:.1}% - {}", percentage,
///                 notification.message.unwrap_or_default());
///         } else {
///             println!("Progress: {} - {}", progress_val,
///                 notification.message.unwrap_or_default());
///         }
///         
///         if notification.completed {
///             if let Some(error) = notification.error {
///                 println!("Operation failed: {}", error);
///             } else {
///                 println!("Operation completed successfully!");
///             }
///         }
///         
///         Ok(())
///     }
/// }
/// ```
#[async_trait]
pub trait ProgressHandler: Send + Sync + std::fmt::Debug {
    /// Handle a progress notification from the server
    ///
    /// This method is called when the server sends progress updates for
    /// long-running operations.
    ///
    /// # Arguments
    ///
    /// * `notification` - Progress information including current status and completion state
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if the notification was processed successfully.
    async fn handle_progress(&self, notification: ProgressNotification) -> HandlerResult<()>;
}

// ============================================================================
// LOG HANDLER TRAIT
// ============================================================================

/// Log message from the server
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LogMessage {
    /// Log level (Error, Warning, Info, Debug)
    pub level: LogLevel,

    /// The log message content
    pub message: String,

    /// Optional logger name/category
    pub logger: Option<String>,

    /// Timestamp when the log was created (ISO 8601 format)
    pub timestamp: String,

    /// Additional structured data
    pub data: Option<serde_json::Value>,
}

/// Handler for server log messages
///
/// Log handlers receive log messages from the server and can route them to
/// the client's logging system. This is useful for debugging, monitoring,
/// and maintaining a unified log across client and server.
///
/// # Examples
///
/// ```rust,no_run
/// use turbomcp_client::handlers::{LogHandler, LogMessage, HandlerResult};
/// use turbomcp_protocol::types::LogLevel;
/// use async_trait::async_trait;
///
/// #[derive(Debug)]
/// struct TraceLogHandler;
///
/// #[async_trait]
/// impl LogHandler for TraceLogHandler {
///     async fn handle_log(&self, log: LogMessage) -> HandlerResult<()> {
///         match log.level {
///             LogLevel::Error => tracing::error!("Server: {}", log.message),
///             LogLevel::Warning => tracing::warn!("Server: {}", log.message),
///             LogLevel::Info => tracing::info!("Server: {}", log.message),
///             LogLevel::Debug => tracing::debug!("Server: {}", log.message),
///             LogLevel::Notice => tracing::info!("Server: {}", log.message),
///             LogLevel::Critical => tracing::error!("Server CRITICAL: {}", log.message),
///             LogLevel::Alert => tracing::error!("Server ALERT: {}", log.message),
///             LogLevel::Emergency => tracing::error!("Server EMERGENCY: {}", log.message),
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
    /// * `log` - The log message with level, content, and metadata
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if the log message was processed successfully.
    async fn handle_log(&self, log: LogMessage) -> HandlerResult<()>;
}

// ============================================================================
// RESOURCE UPDATE HANDLER TRAIT
// ============================================================================

/// Resource update notification
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ResourceUpdateNotification {
    /// URI of the resource that changed
    pub uri: String,

    /// Type of change (created, modified, deleted)
    pub change_type: ResourceChangeType,

    /// Updated resource content (for create/modify operations)
    pub content: Option<ResourceContents>,

    /// Timestamp of the change
    pub timestamp: String,

    /// Additional metadata about the change
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Types of resource changes
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ResourceChangeType {
    /// Resource was created
    Created,
    /// Resource was modified
    Modified,
    /// Resource was deleted
    Deleted,
}

/// Handler for resource update notifications
///
/// Resource update handlers receive notifications when resources that the
/// client has subscribed to are modified. This enables reactive updates
/// to cached data or UI refreshes when server-side resources change.
///
/// # Examples
///
/// ```rust,no_run
/// use turbomcp_client::handlers::{ResourceUpdateHandler, ResourceUpdateNotification, HandlerResult};
/// use async_trait::async_trait;
///
/// #[derive(Debug)]
/// struct CacheInvalidationHandler;
///
/// #[async_trait]
/// impl ResourceUpdateHandler for CacheInvalidationHandler {
///     async fn handle_resource_update(
///         &self,
///         notification: ResourceUpdateNotification,
///     ) -> HandlerResult<()> {
///         println!("Resource {} was {:?}",
///             notification.uri,
///             notification.change_type);
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
        notification: ResourceUpdateNotification,
    ) -> HandlerResult<()>;
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
    /// Elicitation handler for user input requests
    pub elicitation: Option<Arc<dyn ElicitationHandler>>,

    /// Progress handler for operation updates
    pub progress: Option<Arc<dyn ProgressHandler>>,

    /// Log handler for server log messages
    pub log: Option<Arc<dyn LogHandler>>,

    /// Resource update handler for resource change notifications
    pub resource_update: Option<Arc<dyn ResourceUpdateHandler>>,
}

impl HandlerRegistry {
    /// Create a new empty handler registry
    pub fn new() -> Self {
        Self::default()
    }

    /// Register an elicitation handler
    pub fn set_elicitation_handler(&mut self, handler: Arc<dyn ElicitationHandler>) {
        debug!("Registering elicitation handler");
        self.elicitation = Some(handler);
    }

    /// Register a progress handler
    pub fn set_progress_handler(&mut self, handler: Arc<dyn ProgressHandler>) {
        debug!("Registering progress handler");
        self.progress = Some(handler);
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

    /// Check if an elicitation handler is registered
    pub fn has_elicitation_handler(&self) -> bool {
        self.elicitation.is_some()
    }

    /// Check if a progress handler is registered
    pub fn has_progress_handler(&self) -> bool {
        self.progress.is_some()
    }

    /// Check if a log handler is registered
    pub fn has_log_handler(&self) -> bool {
        self.log.is_some()
    }

    /// Check if a resource update handler is registered
    pub fn has_resource_update_handler(&self) -> bool {
        self.resource_update.is_some()
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

    /// Handle a progress notification
    pub async fn handle_progress(&self, notification: ProgressNotification) -> HandlerResult<()> {
        match &self.progress {
            Some(handler) => {
                debug!(
                    "Processing progress notification: {}",
                    notification.operation_id
                );
                handler.handle_progress(notification).await
            }
            None => {
                debug!("No progress handler registered, ignoring notification");
                Ok(())
            }
        }
    }

    /// Handle a log message
    pub async fn handle_log(&self, log: LogMessage) -> HandlerResult<()> {
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
        notification: ResourceUpdateNotification,
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
        warn!("Declining elicitation request: {}", request.prompt);
        Ok(ElicitationResponse {
            id: request.id,
            data: serde_json::Value::Null,
            cancelled: true,
        })
    }
}

/// Default progress handler that logs progress to tracing
#[derive(Debug)]
pub struct LoggingProgressHandler;

#[async_trait]
impl ProgressHandler for LoggingProgressHandler {
    async fn handle_progress(&self, notification: ProgressNotification) -> HandlerResult<()> {
        if notification.completed {
            if let Some(error) = &notification.error {
                error!("Operation {} failed: {}", notification.operation_id, error);
            } else {
                info!(
                    "Operation {} completed successfully",
                    notification.operation_id
                );
            }
        } else if let Some(message) = &notification.message {
            info!("Operation {}: {}", notification.operation_id, message);
        }

        Ok(())
    }
}

/// Default log handler that routes server logs to tracing
#[derive(Debug)]
pub struct TracingLogHandler;

#[async_trait]
impl LogHandler for TracingLogHandler {
    async fn handle_log(&self, log: LogMessage) -> HandlerResult<()> {
        let logger_prefix = log.logger.as_deref().unwrap_or("server");

        match log.level {
            LogLevel::Error => error!("[{}] {}", logger_prefix, log.message),
            LogLevel::Warning => warn!("[{}] {}", logger_prefix, log.message),
            LogLevel::Info => info!("[{}] {}", logger_prefix, log.message),
            LogLevel::Debug => debug!("[{}] {}", logger_prefix, log.message),
            LogLevel::Notice => info!("[{}] [NOTICE] {}", logger_prefix, log.message),
            LogLevel::Critical => error!("[{}] [CRITICAL] {}", logger_prefix, log.message),
            LogLevel::Alert => error!("[{}] [ALERT] {}", logger_prefix, log.message),
            LogLevel::Emergency => error!("[{}] [EMERGENCY] {}", logger_prefix, log.message),
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
        notification: ResourceUpdateNotification,
    ) -> HandlerResult<()> {
        info!(
            "Resource {} was {:?} at {}",
            notification.uri, notification.change_type, notification.timestamp
        );
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
            request: ElicitationRequest,
        ) -> HandlerResult<ElicitationResponse> {
            Ok(ElicitationResponse {
                id: request.id,
                data: json!({"test": "response"}),
                cancelled: false,
            })
        }
    }

    #[derive(Debug)]
    struct TestProgressHandler;

    #[async_trait]
    impl ProgressHandler for TestProgressHandler {
        async fn handle_progress(&self, _notification: ProgressNotification) -> HandlerResult<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_handler_registry_creation() {
        let registry = HandlerRegistry::new();
        assert!(!registry.has_elicitation_handler());
        assert!(!registry.has_progress_handler());
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

        let request = ElicitationRequest {
            id: "test-123".to_string(),
            prompt: "Test prompt".to_string(),
            schema: json!({"type": "object"}),
            timeout: None,
            metadata: HashMap::new(),
        };

        let response = registry.handle_elicitation(request).await.unwrap();
        assert_eq!(response.id, "test-123");
        assert!(!response.cancelled);
    }

    #[tokio::test]
    async fn test_progress_handler_registration() {
        let mut registry = HandlerRegistry::new();
        let handler = Arc::new(TestProgressHandler);

        registry.set_progress_handler(handler);
        assert!(registry.has_progress_handler());
    }

    #[tokio::test]
    async fn test_default_handlers() {
        let decline_handler = DeclineElicitationHandler;
        let request = ElicitationRequest {
            id: "test".to_string(),
            prompt: "Test".to_string(),
            schema: json!({}),
            timeout: None,
            metadata: HashMap::new(),
        };

        let response = decline_handler.handle_elicitation(request).await.unwrap();
        assert!(response.cancelled);
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
}
