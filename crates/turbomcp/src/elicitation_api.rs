//! Ergonomic Elicitation API for TurboMCP
//!
//! This module provides a high-level, type-safe API for elicitation that integrates
//! with our Context system and compile-time routing architecture.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{RwLock, oneshot};

use turbomcp_core::{RequestContext, ServerCapabilities};
use turbomcp_protocol::elicitation::{
    ElicitationAction, ElicitationCreateRequest, ElicitationCreateResult, ElicitationSchema,
    ElicitationValue, PrimitiveSchemaDefinition,
};

use crate::{McpError, McpResult};

/// Elicitation builder for creating type-safe elicitation requests
pub struct ElicitationBuilder {
    message: String,
    schema: ElicitationSchema,
}

impl ElicitationBuilder {
    /// Create a new elicitation builder
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            schema: ElicitationSchema::new(),
        }
    }

    /// Add a field to the elicitation schema
    pub fn field(
        mut self,
        name: impl Into<String>,
        schema: impl Into<PrimitiveSchemaDefinition>,
    ) -> Self {
        self.schema.properties.insert(name.into(), schema.into());
        self
    }

    /// Mark fields as required
    pub fn require(mut self, names: Vec<impl Into<String>>) -> Self {
        self.schema.required = Some(names.into_iter().map(Into::into).collect());
        self
    }

    /// Send the elicitation request through the context
    pub async fn send(self, ctx: &RequestContext) -> McpResult<ElicitationResult> {
        // Get server capabilities from context
        let capabilities = ctx
            .server_capabilities()
            .ok_or_else(|| McpError::Protocol("No server capabilities in context".to_string()))?;

        // Create the request
        let request = ElicitationCreateRequest {
            message: self.message,
            requested_schema: self.schema,
        };

        // Serialize the request to JSON
        let request_json = serde_json::to_value(request).map_err(|e| {
            McpError::Protocol(format!("Failed to serialize elicitation request: {}", e))
        })?;

        // Send through server capabilities and get JSON response
        let response_json = capabilities
            .elicit(request_json)
            .await
            .map_err(|e| McpError::Protocol(format!("Elicitation failed: {}", e)))?;

        // Deserialize the response
        let response: ElicitationCreateResult =
            serde_json::from_value(response_json).map_err(|e| {
                McpError::Protocol(format!("Failed to deserialize elicitation response: {}", e))
            })?;

        // Convert to API result type
        Ok(response.into())
    }
}

/// Result of an elicitation request
pub enum ElicitationResult {
    /// User accepted and provided data
    Accept(ElicitationData),
    /// User explicitly declined with optional reason
    Decline(Option<String>),
    /// User cancelled/dismissed
    Cancel,
}

impl ElicitationResult {
    /// Get data if accepted
    pub fn as_accept(&self) -> Option<&ElicitationData> {
        match self {
            ElicitationResult::Accept(data) => Some(data),
            _ => None,
        }
    }
}

/// Type-safe access to elicitation data
pub struct ElicitationData {
    content: HashMap<String, ElicitationValue>,
}

impl ElicitationData {
    /// Create from protocol response
    pub fn from_content(content: HashMap<String, ElicitationValue>) -> Self {
        Self { content }
    }

    /// Get a string field
    pub fn get_string(&self, key: &str) -> McpResult<String> {
        self.content
            .get(key)
            .and_then(|v| v.as_string())
            .cloned()
            .ok_or_else(|| McpError::Protocol(format!("Field '{}' not found or not a string", key)))
    }

    /// Get an integer field
    pub fn get_integer(&self, key: &str) -> McpResult<i64> {
        self.content
            .get(key)
            .and_then(|v| v.as_integer())
            .ok_or_else(|| {
                McpError::Protocol(format!("Field '{}' not found or not an integer", key))
            })
    }

    /// Get a boolean field
    pub fn get_boolean(&self, key: &str) -> McpResult<bool> {
        self.content
            .get(key)
            .and_then(|v| v.as_boolean())
            .ok_or_else(|| {
                McpError::Protocol(format!("Field '{}' not found or not a boolean", key))
            })
    }

    /// Get a field with type inference
    pub fn get<T: ElicitationExtract>(&self, key: &str) -> McpResult<T> {
        T::extract(self, key)
    }

    /// Get the underlying map as object (for iteration)
    pub fn as_object(&self) -> impl Iterator<Item = (&String, &ElicitationValue)> {
        self.content.iter()
    }
}

/// Trait for extracting typed values from elicitation data
pub trait ElicitationExtract: Sized {
    /// Extract a value of this type from elicitation data
    fn extract(data: &ElicitationData, key: &str) -> McpResult<Self>;
}

impl ElicitationExtract for String {
    fn extract(data: &ElicitationData, key: &str) -> McpResult<Self> {
        data.get_string(key)
    }
}

impl ElicitationExtract for i64 {
    fn extract(data: &ElicitationData, key: &str) -> McpResult<Self> {
        data.get_integer(key)
    }
}

impl ElicitationExtract for i32 {
    fn extract(data: &ElicitationData, key: &str) -> McpResult<Self> {
        data.get_integer(key).map(|v| v as i32)
    }
}

impl ElicitationExtract for bool {
    fn extract(data: &ElicitationData, key: &str) -> McpResult<Self> {
        data.get_boolean(key)
    }
}

impl ElicitationExtract for f64 {
    fn extract(data: &ElicitationData, key: &str) -> McpResult<Self> {
        // Try as integer first, then as number
        data.content
            .get(key)
            .and_then(|v| v.as_integer().map(|i| i as f64).or_else(|| v.as_number()))
            .ok_or_else(|| McpError::Protocol(format!("Field '{}' not found or not a number", key)))
    }
}

/// Extension trait for Context to add elicitation support
pub trait ContextElicitation {
    /// Start building an elicitation request
    fn elicit(&self) -> ElicitationBuilder;
}

impl ContextElicitation for RequestContext {
    fn elicit(&self) -> ElicitationBuilder {
        ElicitationBuilder::new("")
    }
}

/// Server capabilities extension for elicitation
#[async_trait::async_trait]
pub trait ServerElicitation: ServerCapabilities {
    /// Send an elicitation request to the client
    async fn elicit(&self, request: ElicitationCreateRequest) -> McpResult<ElicitationResult>;
}

/// Default implementation for ServerCapabilities
#[async_trait::async_trait]
impl<T: ServerCapabilities + ?Sized> ServerElicitation for T {
    async fn elicit(&self, _request: ElicitationCreateRequest) -> McpResult<ElicitationResult> {
        // Current implementation: Works in test mode, returns appropriate error in production
        // Transport-level elicitation can be enhanced when full bidirectional support is added
        // For testing/demo purposes, we can simulate a response
        if cfg!(test) {
            // Return a mock response for testing
            return Ok(ElicitationResult::Accept(ElicitationData {
                content: HashMap::new(),
            }));
        }

        Err(McpError::Protocol(
            "Elicitation not configured for this transport".to_string(),
        ))
    }
}

/// Manager for tracking pending elicitation requests with timeout support
pub struct ElicitationManager {
    pending: Arc<RwLock<HashMap<String, ElicitationHandle>>>,
    timeout: std::time::Duration,
}

/// Handle for a pending elicitation request
struct ElicitationHandle {
    sender: oneshot::Sender<ElicitationCreateResult>,
    created_at: std::time::Instant,
    _tool_name: Option<String>,
    _request_id: String,
}

impl ElicitationManager {
    /// Create a new elicitation manager
    pub fn new() -> Self {
        Self {
            pending: Arc::new(RwLock::new(HashMap::new())),
            timeout: std::time::Duration::from_secs(60), // Default 60 second timeout
        }
    }

    /// Create with custom timeout
    pub fn with_timeout(timeout: std::time::Duration) -> Self {
        Self {
            pending: Arc::new(RwLock::new(HashMap::new())),
            timeout,
        }
    }

    /// Register a pending elicitation request
    pub async fn register(
        &self,
        id: String,
        tool_name: Option<String>,
    ) -> oneshot::Receiver<ElicitationCreateResult> {
        let (tx, rx) = oneshot::channel();
        let handle = ElicitationHandle {
            sender: tx,
            created_at: std::time::Instant::now(),
            _tool_name: tool_name,
            _request_id: id.clone(),
        };

        let cleanup_id = id.clone();
        self.pending.write().await.insert(id, handle);

        // Start cleanup task for expired requests
        let pending = self.pending.clone();
        let timeout = self.timeout;
        tokio::spawn(async move {
            tokio::time::sleep(timeout).await;
            let mut pending = pending.write().await;
            if let Some(handle) = pending.remove(&cleanup_id) {
                // Send timeout error
                let _ = handle.sender.send(ElicitationCreateResult {
                    action: ElicitationAction::Cancel,
                    content: None,
                    meta: Some(HashMap::from([(
                        "error".to_string(),
                        serde_json::json!("Elicitation request timed out"),
                    )])),
                });
            }
        });

        rx
    }

    /// Complete a pending elicitation request
    pub async fn complete(&self, id: String, result: ElicitationCreateResult) -> McpResult<()> {
        if let Some(handle) = self.pending.write().await.remove(&id) {
            handle
                .sender
                .send(result)
                .map_err(|_| McpError::Tool("Failed to send elicitation result".to_string()))?;
        } else {
            // Request might have timed out or doesn't exist
            return Err(McpError::Protocol(format!(
                "No pending elicitation with id: {}",
                id
            )));
        }
        Ok(())
    }

    /// Clean up expired requests
    pub async fn cleanup_expired(&self) {
        let now = std::time::Instant::now();
        let mut pending = self.pending.write().await;
        let expired: Vec<String> = pending
            .iter()
            .filter(|(_, handle)| now.duration_since(handle.created_at) > self.timeout)
            .map(|(id, _)| id.clone())
            .collect();

        for id in expired {
            if let Some(handle) = pending.remove(&id) {
                // Send timeout error
                let _ = handle.sender.send(ElicitationCreateResult {
                    action: ElicitationAction::Cancel,
                    content: None,
                    meta: Some(HashMap::from([(
                        "error".to_string(),
                        serde_json::json!("Elicitation request timed out"),
                    )])),
                });
            }
        }
    }

    /// Get the number of pending requests
    pub async fn pending_count(&self) -> usize {
        self.pending.read().await.len()
    }
}

impl Default for ElicitationManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Convert protocol result to API result
impl From<ElicitationCreateResult> for ElicitationResult {
    fn from(result: ElicitationCreateResult) -> Self {
        match result.action {
            ElicitationAction::Accept => {
                if let Some(content) = result.content {
                    ElicitationResult::Accept(ElicitationData::from_content(content))
                } else {
                    ElicitationResult::Accept(ElicitationData {
                        content: HashMap::new(),
                    })
                }
            }
            ElicitationAction::Decline => {
                // Extract decline reason from meta if available
                let reason = result
                    .meta
                    .as_ref()
                    .and_then(|meta| meta.get("reason"))
                    .and_then(|v| v.as_str())
                    .map(String::from);
                ElicitationResult::Decline(reason)
            }
            ElicitationAction::Cancel => ElicitationResult::Cancel,
        }
    }
}

// Re-export specific types from protocol for convenience
pub use turbomcp_protocol::elicitation::StringFormat;

/// Builder functions that wrap the protocol builders
/// These avoid import conflicts while providing the same ergonomic API
/// Create a string field with title - beautiful ergonomics!
pub fn string(title: impl Into<String>) -> turbomcp_protocol::elicitation::StringSchemaBuilder {
    turbomcp_protocol::elicitation::string(title)
}

/// Create a string schema without title (for advanced usage)
pub fn string_builder() -> turbomcp_protocol::elicitation::StringSchemaBuilder {
    turbomcp_protocol::elicitation::string_builder()
}

/// Create an integer field with title - beautiful ergonomics!
pub fn integer(title: impl Into<String>) -> turbomcp_protocol::elicitation::NumberSchemaBuilder {
    turbomcp_protocol::elicitation::integer(title)
}

/// Create an integer schema without title (for advanced usage)
pub fn integer_builder() -> turbomcp_protocol::elicitation::NumberSchemaBuilder {
    turbomcp_protocol::elicitation::integer_builder()
}

/// Create a number field with title - beautiful ergonomics!
pub fn number(title: impl Into<String>) -> turbomcp_protocol::elicitation::NumberSchemaBuilder {
    turbomcp_protocol::elicitation::number(title)
}

/// Create a number schema without title (for advanced usage)
pub fn number_builder() -> turbomcp_protocol::elicitation::NumberSchemaBuilder {
    turbomcp_protocol::elicitation::number_builder()
}

/// Create a boolean field with title - beautiful ergonomics!
pub fn boolean(title: impl Into<String>) -> turbomcp_protocol::elicitation::BooleanSchemaBuilder {
    turbomcp_protocol::elicitation::boolean(title)
}

/// Create a boolean schema without title (for advanced usage)
pub fn boolean_builder() -> turbomcp_protocol::elicitation::BooleanSchemaBuilder {
    turbomcp_protocol::elicitation::boolean_builder()
}

/// Create an enum schema
pub fn enum_of(values: Vec<String>) -> turbomcp_protocol::elicitation::EnumSchemaBuilder {
    turbomcp_protocol::elicitation::enum_of(values)
}

/// World-class DX: Create enum schema from array slice (no Vec required!)
///
/// Creates an enum schema with the specified options. Perfect for dropdowns and choice lists.
/// Uses zero-allocation array slices instead of requiring Vec allocation.
///
/// # Examples
/// ```rust
/// use turbomcp::elicitation_api::options;
///
/// // Simple options list
/// let size_field = options(&["small", "medium", "large"]).title("Size");
///
/// // Options with description  
/// let priority_field = options(&["low", "medium", "high"])
///     .title("Priority Level")
///     .description("Choose task priority");
/// ```
pub fn options<T: AsRef<str>>(values: &[T]) -> turbomcp_protocol::elicitation::EnumSchemaBuilder {
    turbomcp_protocol::elicitation::options(values)
}

/// Alias for options() - terser naming
///
/// Identical to [`options`] but with a shorter name for concise code.
///
/// # Examples
/// ```rust
/// use turbomcp::elicitation_api::choices;
///
/// let answer_field = choices(&["yes", "no", "maybe"]).title("Your Answer");
/// ```
pub fn choices<T: AsRef<str>>(values: &[T]) -> turbomcp_protocol::elicitation::EnumSchemaBuilder {
    turbomcp_protocol::elicitation::choices(values)
}

/// World-class DX: Create text field with title - beautiful ergonomics!
///
/// Creates a string schema with the specified title. Perfect for user input fields.
///
/// # Examples
/// ```rust
/// use turbomcp::elicitation_api::text;
///
/// // Simple text field
/// let name_field = text("Full Name");
///
/// // Text field with validation
/// let email_field = text("Email Address").email().min_length(5);
///
/// // Text field with enum options (becomes a dropdown)
/// let theme_field = text("UI Theme").options(&["light", "dark", "auto"]);
/// ```
pub fn text(title: impl Into<String>) -> turbomcp_protocol::elicitation::StringSchemaBuilder {
    turbomcp_protocol::elicitation::text(title)
}

/// World-class DX: Create integer field with title
///
/// Creates an integer number schema with the specified title. Perfect for counts, ages, etc.
///
/// # Examples
/// ```rust
/// use turbomcp::elicitation_api::integer_field;
///
/// // Simple integer field
/// let age_field = integer_field("Age");
///
/// // Integer field with range validation
/// let count_field = integer_field("Item Count").range(1.0, 100.0);
/// ```
pub fn integer_field(
    title: impl Into<String>,
) -> turbomcp_protocol::elicitation::NumberSchemaBuilder {
    turbomcp_protocol::elicitation::integer_field(title)
}

/// World-class DX: Create number field with title
///
/// Creates a floating-point number schema with the specified title. Perfect for prices, measurements, etc.
///
/// # Examples  
/// ```rust
/// use turbomcp::elicitation_api::number_field;
///
/// // Simple number field
/// let price_field = number_field("Price");
///
/// // Number field with range validation
/// let temperature_field = number_field("Temperature").range(-273.15, 1000.0);
/// ```
pub fn number_field(
    title: impl Into<String>,
) -> turbomcp_protocol::elicitation::NumberSchemaBuilder {
    turbomcp_protocol::elicitation::number_field(title)
}

/// World-class DX: Create boolean field with title (checkbox semantic)
///
/// Creates a boolean schema with the specified title. Perfect for yes/no questions and toggles.
///
/// # Examples
/// ```rust
/// use turbomcp::elicitation_api::checkbox;
///
/// // Simple checkbox
/// let notifications_field = checkbox("Enable Notifications");
///
/// // Checkbox with default value and description  
/// let auto_save_field = checkbox("Auto Save")
///     .default(true)
///     .description("Automatically save your work");
/// ```
pub fn checkbox(title: impl Into<String>) -> turbomcp_protocol::elicitation::BooleanSchemaBuilder {
    turbomcp_protocol::elicitation::checkbox(title)
}

/// Create an object schema
pub fn object() -> turbomcp_protocol::elicitation::ObjectSchemaBuilder {
    turbomcp_protocol::elicitation::object()
}

/// Create an array schema
pub fn array() -> turbomcp_protocol::elicitation::ArraySchemaBuilder {
    turbomcp_protocol::elicitation::array()
}

/// Convenience function for creating an elicitation request
pub fn elicit(message: impl Into<String>) -> ElicitationBuilder {
    ElicitationBuilder::new(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_elicitation_builder() {
        let builder = elicit("Please configure your project")
            .field("name", string("Project Name").min_length(3).max_length(50))
            .field("port", integer("Port Number").range(1024.0, 65535.0))
            .field("debug", boolean("Debug Mode").default(false))
            .require(vec!["name"]);

        assert_eq!(builder.message, "Please configure your project");
        assert_eq!(builder.schema.properties.len(), 3);
        assert_eq!(builder.schema.required, Some(vec!["name".to_string()]));
    }

    #[test]
    fn test_elicitation_data_extraction() {
        let mut content = HashMap::new();
        content.insert(
            "name".to_string(),
            ElicitationValue::String("my-project".to_string()),
        );
        content.insert("port".to_string(), ElicitationValue::Integer(3000));
        content.insert("debug".to_string(), ElicitationValue::Boolean(true));

        let data = ElicitationData::from_content(content);

        assert_eq!(data.get_string("name").unwrap(), "my-project");
        assert_eq!(data.get_integer("port").unwrap(), 3000);
        assert!(data.get_boolean("debug").unwrap());

        // Test type inference
        let name: String = data.get("name").unwrap();
        assert_eq!(name, "my-project");

        let port: i32 = data.get("port").unwrap();
        assert_eq!(port, 3000);

        let debug: bool = data.get("debug").unwrap();
        assert!(debug);
    }

    #[test]
    fn test_elicitation_result_conversion() {
        // Test accept
        let mut content = HashMap::new();
        content.insert(
            "key".to_string(),
            ElicitationValue::String("value".to_string()),
        );

        let protocol_result = ElicitationCreateResult {
            action: ElicitationAction::Accept,
            content: Some(content),
            meta: None,
        };

        let result: ElicitationResult = protocol_result.into();
        match result {
            ElicitationResult::Accept(data) => {
                assert_eq!(data.get_string("key").unwrap(), "value");
            }
            _ => panic!("Expected Accept result"),
        }

        // Test decline
        let decline_result = ElicitationCreateResult {
            action: ElicitationAction::Decline,
            content: None,
            meta: None,
        };

        let result: ElicitationResult = decline_result.into();
        assert!(matches!(result, ElicitationResult::Decline(_)));

        // Test cancel
        let cancel_result = ElicitationCreateResult {
            action: ElicitationAction::Cancel,
            content: None,
            meta: None,
        };

        let result: ElicitationResult = cancel_result.into();
        assert!(matches!(result, ElicitationResult::Cancel));
    }
}
