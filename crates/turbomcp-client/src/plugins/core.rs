//! Core plugin system traits and types
//!
//! Defines the fundamental abstractions for the plugin system including the ClientPlugin trait,
//! context objects, error types, and configuration structures.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fmt;
use std::time::Duration;
use thiserror::Error;
use turbomcp_protocol::jsonrpc::JsonRpcRequest;

// ============================================================================
// ERROR TYPES
// ============================================================================

/// Errors that can occur during plugin operations
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum PluginError {
    /// Plugin initialization failed
    #[error("Plugin initialization failed: {message}")]
    Initialization { message: String },

    /// Plugin configuration is invalid
    #[error("Invalid plugin configuration: {message}")]
    Configuration { message: String },

    /// Error during request processing
    #[error("Request processing error: {message}")]
    RequestProcessing { message: String },

    /// Error during response processing
    #[error("Response processing error: {message}")]
    ResponseProcessing { message: String },

    /// Error in custom method handler
    #[error("Custom handler error: {message}")]
    CustomHandler { message: String },

    /// Plugin dependency not available
    #[error("Plugin dependency '{dependency}' not available")]
    DependencyNotAvailable { dependency: String },

    /// Plugin version compatibility issue
    #[error("Plugin version incompatibility: {message}")]
    VersionIncompatible { message: String },

    /// Resource access error
    #[error("Resource access error: {resource} - {message}")]
    ResourceAccess { resource: String, message: String },

    /// External system error
    #[error("External system error: {source}")]
    External {
        #[from]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

impl PluginError {
    /// Create an initialization error
    pub fn initialization(message: impl Into<String>) -> Self {
        Self::Initialization {
            message: message.into(),
        }
    }

    /// Create a configuration error
    pub fn configuration(message: impl Into<String>) -> Self {
        Self::Configuration {
            message: message.into(),
        }
    }

    /// Create a request processing error
    pub fn request_processing(message: impl Into<String>) -> Self {
        Self::RequestProcessing {
            message: message.into(),
        }
    }

    /// Create a response processing error
    pub fn response_processing(message: impl Into<String>) -> Self {
        Self::ResponseProcessing {
            message: message.into(),
        }
    }

    /// Create a custom handler error
    pub fn custom_handler(message: impl Into<String>) -> Self {
        Self::CustomHandler {
            message: message.into(),
        }
    }

    /// Create a dependency error
    pub fn dependency_not_available(dependency: impl Into<String>) -> Self {
        Self::DependencyNotAvailable {
            dependency: dependency.into(),
        }
    }

    /// Create a version incompatibility error
    pub fn version_incompatible(message: impl Into<String>) -> Self {
        Self::VersionIncompatible {
            message: message.into(),
        }
    }

    /// Create a resource access error
    pub fn resource_access(resource: impl Into<String>, message: impl Into<String>) -> Self {
        Self::ResourceAccess {
            resource: resource.into(),
            message: message.into(),
        }
    }
}

pub type PluginResult<T> = Result<T, PluginError>;

// ============================================================================
// CONTEXT TYPES
// ============================================================================

/// Context information available to plugins during initialization
#[derive(Debug, Clone)]
pub struct PluginContext {
    /// Client information
    pub client_name: String,
    pub client_version: String,

    /// Available capabilities
    pub capabilities: HashMap<String, Value>,

    /// Configuration values
    pub config: HashMap<String, Value>,

    /// Registered plugin names (for dependency checking)
    pub available_plugins: Vec<String>,
}

impl PluginContext {
    /// Create a new plugin context
    #[must_use]
    pub fn new(
        client_name: String,
        client_version: String,
        capabilities: HashMap<String, Value>,
        config: HashMap<String, Value>,
        available_plugins: Vec<String>,
    ) -> Self {
        Self {
            client_name,
            client_version,
            capabilities,
            config,
            available_plugins,
        }
    }

    /// Check if a capability is available
    #[must_use]
    pub fn has_capability(&self, capability: &str) -> bool {
        self.capabilities.contains_key(capability)
    }

    /// Get a configuration value
    #[must_use]
    pub fn get_config(&self, key: &str) -> Option<&Value> {
        self.config.get(key)
    }

    /// Check if a plugin dependency is available
    #[must_use]
    pub fn has_plugin(&self, plugin_name: &str) -> bool {
        self.available_plugins.contains(&plugin_name.to_string())
    }
}

/// Context for request processing
#[derive(Debug, Clone)]
pub struct RequestContext {
    /// The JSON-RPC request being processed
    pub request: JsonRpcRequest,

    /// Additional metadata (can be modified by plugins)
    pub metadata: HashMap<String, Value>,

    /// Request timestamp
    pub timestamp: DateTime<Utc>,
}

impl RequestContext {
    /// Create a new request context
    #[must_use]
    pub fn new(request: JsonRpcRequest, metadata: HashMap<String, Value>) -> Self {
        Self {
            request,
            metadata,
            timestamp: Utc::now(),
        }
    }

    /// Get the request method
    #[must_use]
    pub fn method(&self) -> &str {
        &self.request.method
    }

    /// Get request parameters
    #[must_use]
    pub fn params(&self) -> Option<&Value> {
        self.request.params.as_ref()
    }

    /// Add metadata
    pub fn add_metadata(&mut self, key: String, value: Value) {
        self.metadata.insert(key, value);
    }

    /// Get metadata value
    #[must_use]
    pub fn get_metadata(&self, key: &str) -> Option<&Value> {
        self.metadata.get(key)
    }
}

/// Context for response processing
#[derive(Debug, Clone)]
pub struct ResponseContext {
    /// The original request context
    pub request_context: RequestContext,

    /// The response data (if successful)
    pub response: Option<Value>,

    /// Error information (if failed)
    pub error: Option<turbomcp_protocol::Error>,

    /// Request duration
    pub duration: Duration,

    /// Additional metadata (can be modified by plugins)
    pub metadata: HashMap<String, Value>,
}

impl ResponseContext {
    /// Create a new response context
    #[must_use]
    pub fn new(
        request_context: RequestContext,
        response: Option<Value>,
        error: Option<turbomcp_protocol::Error>,
        duration: Duration,
    ) -> Self {
        Self {
            request_context,
            response,
            error,
            duration,
            metadata: HashMap::new(),
        }
    }

    /// Check if the response was successful
    pub fn is_success(&self) -> bool {
        self.error.is_none()
    }

    /// Check if the response was an error
    pub fn is_error(&self) -> bool {
        self.error.is_some()
    }

    /// Get the request method
    pub fn method(&self) -> &str {
        self.request_context.method()
    }

    /// Add metadata
    pub fn add_metadata(&mut self, key: String, value: Value) {
        self.metadata.insert(key, value);
    }

    /// Get metadata value
    pub fn get_metadata(&self, key: &str) -> Option<&Value> {
        self.metadata.get(key)
    }
}

// ============================================================================
// PLUGIN CONFIGURATION
// ============================================================================

/// Plugin configuration variants
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum PluginConfig {
    /// Metrics plugin configuration
    #[serde(rename = "metrics")]
    Metrics,

    /// Retry plugin configuration
    #[serde(rename = "retry")]
    Retry(super::examples::RetryConfig),

    /// Cache plugin configuration
    #[serde(rename = "cache")]
    Cache(super::examples::CacheConfig),

    /// Custom plugin configuration
    #[serde(rename = "custom")]
    Custom {
        name: String,
        config: HashMap<String, Value>,
    },
}

impl fmt::Display for PluginConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PluginConfig::Metrics => write!(f, "Metrics"),
            PluginConfig::Retry(_) => write!(f, "Retry"),
            PluginConfig::Cache(_) => write!(f, "Cache"),
            PluginConfig::Custom { name, .. } => write!(f, "Custom({})", name),
        }
    }
}

// ============================================================================
// CLIENT PLUGIN TRAIT
// ============================================================================

/// Core trait for client plugins
///
/// Plugins can hook into the client lifecycle at various points:
/// - **initialization**: Called when the plugin is registered
/// - **before_request**: Called before sending requests to the server
/// - **after_response**: Called after receiving responses from the server
/// - **handle_custom**: Called for custom method handling
///
/// All methods are async and return PluginResult to allow for error handling
/// and async operations like network calls, database access, etc.
///
/// # Examples
///
/// ```rust,no_run
/// use turbomcp_client::plugins::{ClientPlugin, PluginContext, RequestContext, ResponseContext, PluginResult};
/// use async_trait::async_trait;
/// use serde_json::Value;
///
/// #[derive(Debug)]
/// struct LoggingPlugin;
///
/// #[async_trait]
/// impl ClientPlugin for LoggingPlugin {
///     fn name(&self) -> &str {
///         "logging"
///     }
///
///     fn version(&self) -> &str {
///         "1.0.0"
///     }
///
///     async fn initialize(&self, context: &PluginContext) -> PluginResult<()> {
///         println!("Logging plugin initialized for client: {}", context.client_name);
///         Ok(())
///     }
///
///     async fn before_request(&self, context: &mut RequestContext) -> PluginResult<()> {
///         println!("Request: {} {}", context.method(),
///             context.params().unwrap_or(&Value::Null));
///         Ok(())
///     }
///
///     async fn after_response(&self, context: &mut ResponseContext) -> PluginResult<()> {
///         println!("Response: {} took {:?}", context.method(), context.duration);
///         Ok(())
///     }
///
///     async fn handle_custom(&self, method: &str, params: Option<Value>) -> PluginResult<Option<Value>> {
///         if method == "logging.get_stats" {
///             Ok(Some(serde_json::json!({"logged_requests": 42})))
///         } else {
///             Ok(None) // Not handled by this plugin
///         }
///     }
/// }
/// ```
#[async_trait]
pub trait ClientPlugin: Send + Sync + fmt::Debug {
    /// Plugin name - must be unique across all registered plugins
    fn name(&self) -> &str;

    /// Plugin version
    fn version(&self) -> &str;

    /// Optional plugin description
    fn description(&self) -> Option<&str> {
        None
    }

    /// Plugin dependencies (other plugins that must be registered first)
    fn dependencies(&self) -> Vec<&str> {
        Vec::new()
    }

    /// Initialize the plugin
    ///
    /// Called once when the plugin is registered with the client.
    /// Use this to set up resources, validate configuration, check dependencies, etc.
    ///
    /// # Arguments
    ///
    /// * `context` - Plugin context with client info, capabilities, and configuration
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if initialization succeeds, or `PluginError` if it fails.
    async fn initialize(&self, context: &PluginContext) -> PluginResult<()>;

    /// Hook called before sending a request to the server
    ///
    /// This allows plugins to:
    /// - Modify request parameters
    /// - Add metadata for tracking
    /// - Implement features like authentication, request logging, etc.
    /// - Abort requests by returning an error
    ///
    /// # Arguments
    ///
    /// * `context` - Mutable request context that can be modified
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` to continue processing, or `PluginError` to abort.
    async fn before_request(&self, context: &mut RequestContext) -> PluginResult<()>;

    /// Hook called after receiving a response from the server
    ///
    /// This allows plugins to:
    /// - Process response data
    /// - Log metrics and performance data
    /// - Implement features like caching, retry logic, etc.
    /// - Modify response metadata
    ///
    /// # Arguments
    ///
    /// * `context` - Mutable response context that can be modified
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if processing succeeds, or `PluginError` if it fails.
    async fn after_response(&self, context: &mut ResponseContext) -> PluginResult<()>;

    /// Handle custom methods not part of the standard MCP protocol
    ///
    /// This allows plugins to implement custom functionality that can be invoked
    /// by clients. Each plugin can handle its own set of custom methods.
    ///
    /// # Arguments
    ///
    /// * `method` - The custom method name (e.g., "metrics.get_stats")
    /// * `params` - Optional parameters for the method
    ///
    /// # Returns
    ///
    /// Returns `Some(Value)` if the method was handled, `None` if not handled by this plugin,
    /// or `PluginError` if handling failed.
    async fn handle_custom(
        &self,
        method: &str,
        params: Option<Value>,
    ) -> PluginResult<Option<Value>>;

    /// Optional cleanup when plugin is unregistered
    ///
    /// Default implementation does nothing. Override to perform cleanup
    /// like closing connections, flushing buffers, etc.
    async fn cleanup(&self) -> PluginResult<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use turbomcp_protocol::MessageId;
    use turbomcp_protocol::jsonrpc::{JsonRpcRequest, JsonRpcVersion};

    #[test]
    fn test_plugin_error_creation() {
        let error = PluginError::initialization("Test error");
        assert!(error.to_string().contains("Plugin initialization failed"));

        let config_error = PluginError::configuration("Invalid config");
        assert!(
            config_error
                .to_string()
                .contains("Invalid plugin configuration")
        );

        let request_error = PluginError::request_processing("Request failed");
        assert!(
            request_error
                .to_string()
                .contains("Request processing error")
        );
    }

    #[test]
    fn test_plugin_context_creation() {
        let capabilities = HashMap::from([
            ("tools".to_string(), json!(true)),
            ("sampling".to_string(), json!(false)),
        ]);

        let config = HashMap::from([
            ("debug".to_string(), json!(true)),
            ("timeout".to_string(), json!(5000)),
        ]);

        let plugins = vec!["metrics".to_string(), "retry".to_string()];

        let context = PluginContext::new(
            "test-client".to_string(),
            "1.0.0".to_string(),
            capabilities,
            config,
            plugins,
        );

        assert_eq!(context.client_name, "test-client");
        assert_eq!(context.client_version, "1.0.0");
        assert!(context.has_capability("tools"));
        assert!(!context.has_capability("nonexistent"));
        assert_eq!(context.get_config("debug"), Some(&json!(true)));
        assert!(context.has_plugin("metrics"));
        assert!(!context.has_plugin("nonexistent"));
    }

    #[test]
    fn test_request_context_creation() {
        let request = JsonRpcRequest {
            jsonrpc: JsonRpcVersion,
            id: MessageId::from("test-123"),
            method: "test/method".to_string(),
            params: Some(json!({"key": "value"})),
        };

        let metadata = HashMap::from([("user_id".to_string(), json!("user123"))]);

        let mut context = RequestContext::new(request, metadata);

        assert_eq!(context.method(), "test/method");
        assert_eq!(context.params(), Some(&json!({"key": "value"})));
        assert_eq!(context.get_metadata("user_id"), Some(&json!("user123")));

        context.add_metadata("request_id".to_string(), json!("req456"));
        assert_eq!(context.get_metadata("request_id"), Some(&json!("req456")));
    }

    #[test]
    fn test_response_context_creation() {
        let request = JsonRpcRequest {
            jsonrpc: JsonRpcVersion,
            id: MessageId::from("test-123"),
            method: "test/method".to_string(),
            params: Some(json!({"key": "value"})),
        };

        let request_context = RequestContext::new(request, HashMap::new());
        let response = Some(json!({"result": "success"}));
        let duration = Duration::from_millis(150);

        let mut context = ResponseContext::new(request_context, response, None, duration);

        assert!(context.is_success());
        assert!(!context.is_error());
        assert_eq!(context.method(), "test/method");
        assert_eq!(context.duration, Duration::from_millis(150));

        context.add_metadata("cache_hit".to_string(), json!(true));
        assert_eq!(context.get_metadata("cache_hit"), Some(&json!(true)));
    }

    #[test]
    fn test_plugin_config_serialization() {
        let config = PluginConfig::Metrics;
        let json_str = serde_json::to_string(&config).unwrap();
        assert!(json_str.contains("metrics"));

        let deserialized: PluginConfig = serde_json::from_str(&json_str).unwrap();
        match deserialized {
            PluginConfig::Metrics => {}
            _ => panic!("Expected Metrics config"),
        }
    }

    #[test]
    fn test_plugin_config_display() {
        let metrics_config = PluginConfig::Metrics;
        assert_eq!(format!("{}", metrics_config), "Metrics");

        let custom_config = PluginConfig::Custom {
            name: "test".to_string(),
            config: HashMap::new(),
        };
        assert_eq!(format!("{}", custom_config), "Custom(test)");
    }
}
