//! Elicitation system for interactive user input in MCP tools
//!
//! This module provides comprehensive capabilities for MCP servers to request
//! interactive input from clients during tool execution, supporting various
//! input types, validation, and user experience patterns.

use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

use crate::{McpError, McpResult};

/// Client transport trait for sending elicitation requests to MCP clients
#[async_trait::async_trait]
pub trait ClientTransport: Send + Sync {
    /// Send an elicitation request to the connected MCP client
    async fn send_elicitation_request(&self, request: &ElicitationRequest) -> McpResult<()>;
}

/// Elicitation request ID for tracking user input requests
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct ElicitationId(pub String);

impl ElicitationId {
    /// Create a new elicitation ID
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }

    /// Create from string
    #[must_use]
    pub const fn from_string(id: String) -> Self {
        Self(id)
    }

    /// Get as string reference
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for ElicitationId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for ElicitationId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Type of input being requested from the user
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum InputType {
    /// Simple text input
    Text {
        /// Placeholder text
        placeholder: Option<String>,
        /// Maximum length
        max_length: Option<usize>,
        /// Whether to mask input (for passwords)
        masked: bool,
    },
    /// Numeric input
    Number {
        /// Minimum value
        min: Option<f64>,
        /// Maximum value
        max: Option<f64>,
        /// Number of decimal places
        decimal_places: Option<u8>,
    },
    /// Boolean choice (yes/no, true/false)
    Boolean {
        /// Text for true option
        true_label: Option<String>,
        /// Text for false option
        false_label: Option<String>,
    },
    /// Single choice from multiple options
    Choice {
        /// Available options
        options: Vec<ChoiceOption>,
        /// Whether to allow custom input
        allow_custom: bool,
    },
    /// Multiple choices from options
    MultiChoice {
        /// Available options
        options: Vec<ChoiceOption>,
        /// Minimum number of selections
        min_selections: Option<usize>,
        /// Maximum number of selections
        max_selections: Option<usize>,
    },
    /// Date input
    Date {
        /// Minimum allowed date (ISO 8601 format)
        min_date: Option<String>,
        /// Maximum allowed date (ISO 8601 format)
        max_date: Option<String>,
        /// Include time component
        include_time: bool,
    },
    /// File selection/upload
    File {
        /// Allowed file extensions
        allowed_extensions: Vec<String>,
        /// Maximum file size in bytes
        max_size: Option<u64>,
        /// Allow multiple files
        multiple: bool,
    },
}

/// Option for choice-based inputs
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChoiceOption {
    /// Option value
    pub value: String,
    /// Display label
    pub label: String,
    /// Optional description
    pub description: Option<String>,
    /// Whether this option is disabled
    pub disabled: bool,
}

impl ChoiceOption {
    /// Create a simple choice option
    pub fn new<V, L>(value: V, label: L) -> Self
    where
        V: Into<String>,
        L: Into<String>,
    {
        Self {
            value: value.into(),
            label: label.into(),
            description: None,
            disabled: false,
        }
    }

    /// Add description to the option
    pub fn with_description<D>(mut self, description: D) -> Self
    where
        D: Into<String>,
    {
        self.description = Some(description.into());
        self
    }

    /// Mark option as disabled
    #[must_use]
    pub const fn disabled(mut self) -> Self {
        self.disabled = true;
        self
    }
}

/// Priority level for elicitation requests
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    /// Low priority - can be deferred
    Low,
    /// Normal priority - default
    Normal,
    /// High priority - should be shown prominently
    High,
    /// Critical - requires immediate attention
    Critical,
}

impl Default for Priority {
    fn default() -> Self {
        Self::Normal
    }
}

/// Context information for the elicitation request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElicitationContext {
    /// Tool name that initiated the request
    pub tool_name: String,
    /// Step in the process (for multi-step workflows)
    pub step: Option<String>,
    /// Total number of steps (for progress indication)
    pub total_steps: Option<u32>,
    /// Current step number
    pub current_step: Option<u32>,
    /// Additional context metadata
    pub metadata: HashMap<String, serde_json::Value>,
}

impl ElicitationContext {
    /// Create basic context
    pub fn new<T>(tool_name: T) -> Self
    where
        T: Into<String>,
    {
        Self {
            tool_name: tool_name.into(),
            step: None,
            total_steps: None,
            current_step: None,
            metadata: HashMap::new(),
        }
    }

    /// Add step information
    pub fn with_step<S>(mut self, step: S, current: u32, total: u32) -> Self
    where
        S: Into<String>,
    {
        self.step = Some(step.into());
        self.current_step = Some(current);
        self.total_steps = Some(total);
        self
    }

    /// Add metadata
    pub fn with_metadata<K, V>(mut self, key: K, value: V) -> Self
    where
        K: Into<String>,
        V: Serialize,
    {
        if let Ok(json_value) = serde_json::to_value(value) {
            self.metadata.insert(key.into(), json_value);
        }
        self
    }
}

/// Request for user input
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElicitationRequest {
    /// Unique identifier for this request
    pub id: ElicitationId,
    /// Type of input being requested
    pub input_type: InputType,
    /// Prompt/question to show the user
    pub prompt: String,
    /// Optional detailed description
    pub description: Option<String>,
    /// Default value (if applicable)
    pub default_value: Option<serde_json::Value>,
    /// Whether this input is required
    pub required: bool,
    /// Priority level
    pub priority: Priority,
    /// Timeout for the request
    pub timeout: Option<Duration>,
    /// Context information
    pub context: ElicitationContext,
    /// Validation rules (JSON Schema or custom)
    pub validation: Option<serde_json::Value>,
}

impl ElicitationRequest {
    /// Create a new elicitation request
    pub fn new<P>(input_type: InputType, prompt: P) -> Self
    where
        P: Into<String>,
    {
        Self {
            id: ElicitationId::new(),
            input_type,
            prompt: prompt.into(),
            description: None,
            default_value: None,
            required: true,
            priority: Priority::Normal,
            timeout: None,
            context: ElicitationContext::new("unknown"),
            validation: None,
        }
    }

    /// Add description
    pub fn with_description<D>(mut self, description: D) -> Self
    where
        D: Into<String>,
    {
        self.description = Some(description.into());
        self
    }

    /// Set default value
    pub fn with_default<V>(mut self, value: V) -> Self
    where
        V: Serialize,
    {
        if let Ok(json_value) = serde_json::to_value(value) {
            self.default_value = Some(json_value);
        }
        self
    }

    /// Mark as optional
    #[must_use]
    pub const fn optional(mut self) -> Self {
        self.required = false;
        self
    }

    /// Set priority
    #[must_use]
    pub const fn with_priority(mut self, priority: Priority) -> Self {
        self.priority = priority;
        self
    }

    /// Set timeout
    #[must_use]
    pub const fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Set context
    #[must_use]
    pub fn with_context(mut self, context: ElicitationContext) -> Self {
        self.context = context;
        self
    }
}

/// Response from user input
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElicitationResponse {
    /// ID of the original request
    pub request_id: ElicitationId,
    /// User's input value
    pub value: Option<serde_json::Value>,
    /// Whether the request was cancelled
    pub cancelled: bool,
    /// Error message if input was invalid
    pub error: Option<String>,
    /// Timestamp of response
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

impl ElicitationResponse {
    /// Create successful response
    pub fn success<V>(request_id: ElicitationId, value: V) -> McpResult<Self>
    where
        V: Serialize,
    {
        Ok(Self {
            request_id,
            value: Some(serde_json::to_value(value)?),
            cancelled: false,
            error: None,
            timestamp: chrono::Utc::now(),
        })
    }

    /// Create cancelled response
    #[must_use]
    pub fn cancelled(request_id: ElicitationId) -> Self {
        Self {
            request_id,
            value: None,
            cancelled: true,
            error: None,
            timestamp: chrono::Utc::now(),
        }
    }

    /// Create error response
    pub fn error<E>(request_id: ElicitationId, error: E) -> Self
    where
        E: Into<String>,
    {
        Self {
            request_id,
            value: None,
            cancelled: false,
            error: Some(error.into()),
            timestamp: chrono::Utc::now(),
        }
    }

    /// Extract typed value from response
    pub fn get_value<T>(&self) -> McpResult<Option<T>>
    where
        T: for<'de> Deserialize<'de>,
    {
        match &self.value {
            Some(value) => Ok(Some(serde_json::from_value(value.clone())?)),
            None => Ok(None),
        }
    }
}

/// Status of an elicitation request
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ElicitationStatus {
    /// Request is pending user input
    Pending,
    /// Request is being shown to user
    Active,
    /// Request completed successfully
    Completed,
    /// Request was cancelled
    Cancelled,
    /// Request timed out
    TimedOut,
    /// Request failed with error
    Failed(String),
}

/// Elicitation manager for handling user input requests
#[derive(Debug)]
#[allow(dead_code)]
pub struct ElicitationManager {
    /// Pending requests
    pending_requests: std::sync::Arc<tokio::sync::RwLock<HashMap<ElicitationId, PendingRequest>>>,
    /// Response sender for completed requests
    response_sender: mpsc::UnboundedSender<ElicitationResponse>,
    /// Response receiver
    response_receiver:
        std::sync::Arc<tokio::sync::Mutex<mpsc::UnboundedReceiver<ElicitationResponse>>>,
    /// Client transport for sending elicitation requests (production MCP client integration)
    client_transport: Option<std::sync::Arc<dyn ClientTransport>>,
}

/// Internal structure for tracking pending requests
#[derive(Debug)]
struct PendingRequest {
    request: ElicitationRequest,
    status: ElicitationStatus,
    response_sender: oneshot::Sender<ElicitationResponse>,
    created_at: chrono::DateTime<chrono::Utc>,
}

impl ElicitationManager {
    /// Create a new elicitation manager
    #[must_use]
    pub fn new() -> Self {
        let (response_sender, response_receiver) = mpsc::unbounded_channel();

        Self {
            pending_requests: std::sync::Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            response_sender,
            response_receiver: std::sync::Arc::new(tokio::sync::Mutex::new(response_receiver)),
            client_transport: None,
        }
    }
    
    /// Set the client transport for production MCP client integration
    pub fn with_client_transport(mut self, transport: std::sync::Arc<dyn ClientTransport>) -> Self {
        self.client_transport = Some(transport);
        self
    }

    /// Request user input
    pub async fn request_input(
        &self,
        request: ElicitationRequest,
    ) -> McpResult<ElicitationResponse> {
        let request_id = request.id.clone();
        let timeout = request.timeout.unwrap_or(Duration::from_secs(300)); // 5 minutes default

        let (response_sender, response_receiver) = oneshot::channel();

        let pending = PendingRequest {
            request: request.clone(),
            status: ElicitationStatus::Pending,
            response_sender,
            created_at: chrono::Utc::now(),
        };

        // Store the pending request
        self.pending_requests
            .write()
            .await
            .insert(request_id.clone(), pending);

        // Send request to client (in a real implementation, this would be sent via transport)
        tracing::info!(
            "Elicitation request created: {} - {}",
            request_id,
            request.prompt
        );

        // Production: Client interaction handled by registered MCP client transport
        // Emit elicitation request event for MCP client to handle
        if let Some(transport) = &self.client_transport {
            transport.send_elicitation_request(&request).await?;
        }

        // Wait for response with timeout
        match tokio::time::timeout(timeout, response_receiver).await {
            Ok(Ok(response)) => {
                // Clean up pending request
                self.pending_requests.write().await.remove(&request_id);
                Ok(response)
            }
            Ok(Err(_)) => {
                // Channel closed unexpectedly
                self.pending_requests.write().await.remove(&request_id);
                Err(McpError::Tool("Elicitation request cancelled".to_string()))
            }
            Err(_) => {
                // Timeout
                self.mark_request_timed_out(request_id.clone()).await;
                Err(McpError::Tool(format!(
                    "Elicitation request {request_id} timed out"
                )))
            }
        }
    }

    /// Provide response to a pending request
    pub async fn provide_response(&self, response: ElicitationResponse) -> McpResult<()> {
        let request_id = response.request_id.clone();

        if let Some(pending) = self.pending_requests.write().await.remove(&request_id) {
            if pending.response_sender.send(response).is_err() {
                tracing::warn!("Failed to send response for elicitation {}", request_id);
            }
            Ok(())
        } else {
            Err(McpError::Tool(format!(
                "No pending elicitation request with ID {request_id}"
            )))
        }
    }

    /// Cancel a pending request
    pub async fn cancel_request(&self, request_id: ElicitationId) -> McpResult<()> {
        let request_id_clone = request_id.clone();
        if let Some(pending) = self.pending_requests.write().await.remove(&request_id) {
            let response = ElicitationResponse::cancelled(request_id);
            if pending.response_sender.send(response).is_err() {
                tracing::warn!(
                    "Failed to send cancellation for elicitation {}",
                    request_id_clone
                );
            }
            Ok(())
        } else {
            Err(McpError::Tool(format!(
                "No pending elicitation request with ID {request_id_clone}"
            )))
        }
    }

    /// Get status of a request
    pub async fn get_status(&self, request_id: &ElicitationId) -> Option<ElicitationStatus> {
        self.pending_requests
            .read()
            .await
            .get(request_id)
            .map(|req| req.status.clone())
    }

    /// List all pending requests
    pub async fn list_pending(&self) -> Vec<ElicitationRequest> {
        self.pending_requests
            .read()
            .await
            .values()
            .map(|pending| pending.request.clone())
            .collect()
    }

    /// Clean up timed out requests
    pub async fn cleanup_expired(&self, max_age: Duration) {
        let now = chrono::Utc::now();
        let mut to_remove = Vec::new();

        {
            let requests = self.pending_requests.read().await;
            for (id, pending) in requests.iter() {
                if now.signed_duration_since(pending.created_at)
                    > chrono::Duration::from_std(max_age).unwrap_or_default()
                {
                    to_remove.push(id.clone());
                }
            }
        }

        let mut requests = self.pending_requests.write().await;
        for id in to_remove {
            if let Some(pending) = requests.remove(&id) {
                let response = ElicitationResponse::error(id, "Request expired");
                let _ = pending.response_sender.send(response);
            }
        }
    }

    /// Mark request as timed out
    async fn mark_request_timed_out(&self, request_id: ElicitationId) {
        if let Some(mut pending) = self.pending_requests.write().await.remove(&request_id) {
            pending.status = ElicitationStatus::TimedOut;
            let response = ElicitationResponse::error(request_id, "Request timed out");
            let _ = pending.response_sender.send(response);
        }
    }

    /// Clone for use in async tasks
    #[allow(dead_code)]
    fn clone_for_task(&self) -> Self {
        Self {
            pending_requests: self.pending_requests.clone(),
            response_sender: self.response_sender.clone(),
            response_receiver: self.response_receiver.clone(),
        }
    }

    /// Simulate user input for testing and development (production systems should use actual client interaction)
    #[allow(dead_code)]
    async fn simulate_user_input(&self, request_id: ElicitationId, request: ElicitationRequest) {
        // Wait a bit to simulate user thinking time
        tokio::time::sleep(Duration::from_millis(1000 + fastrand::u64(1..3000))).await;

        let request_id_clone = request_id.clone();

        let response = match &request.input_type {
            InputType::Text { .. } => ElicitationResponse::success(
                request_id.clone(),
                "User provided text input".to_string(),
            ),
            InputType::Number { min, max, .. } => {
                let value = match (min, max) {
                    (Some(min_val), Some(max_val)) => {
                        min_val + (max_val - min_val) * fastrand::f64()
                    }
                    (Some(min_val), None) => min_val + fastrand::f64() * 100.0,
                    (None, Some(max_val)) => fastrand::f64() * max_val,
                    (None, None) => fastrand::f64() * 100.0,
                };
                ElicitationResponse::success(request_id.clone(), value)
            }
            InputType::Boolean { .. } => {
                ElicitationResponse::success(request_id.clone(), fastrand::bool())
            }
            InputType::Choice { options, .. } => {
                if options.is_empty() {
                    Ok(ElicitationResponse::error(
                        request_id.clone(),
                        "No options available",
                    ))
                } else {
                    let choice = &options[fastrand::usize(0..options.len())];
                    ElicitationResponse::success(request_id.clone(), choice.value.clone())
                }
            }
            InputType::MultiChoice { options, .. } => {
                let selections = if options.is_empty() {
                    vec![]
                } else {
                    let count = fastrand::usize(1..=options.len().min(3));
                    let mut selected = Vec::new();
                    for _ in 0..count {
                        let choice = &options[fastrand::usize(0..options.len())];
                        if !selected.contains(&choice.value) {
                            selected.push(choice.value.clone());
                        }
                    }
                    selected
                };
                ElicitationResponse::success(request_id.clone(), selections)
            }
            InputType::Date { include_time, .. } => {
                let date_str = if *include_time {
                    chrono::Utc::now().to_rfc3339()
                } else {
                    chrono::Utc::now().format("%Y-%m-%d").to_string()
                };
                ElicitationResponse::success(request_id.clone(), date_str)
            }
            InputType::File { .. } => {
                ElicitationResponse::success(request_id, "/path/to/selected/file.txt")
            }
        };

        match response {
            Ok(resp) => {
                if self.provide_response(resp).await.is_err() {
                    tracing::error!(
                        "Failed to provide simulated response for {}",
                        request_id_clone
                    );
                }
            }
            Err(e) => {
                let error_resp = ElicitationResponse::error(
                    request_id_clone.clone(),
                    format!("Simulation error: {e}"),
                );
                if self.provide_response(error_resp).await.is_err() {
                    tracing::error!("Failed to provide error response for {}", request_id_clone);
                }
            }
        }
    }
}

impl Default for ElicitationManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Global elicitation manager instance
static GLOBAL_ELICITATION_MANAGER: once_cell::sync::Lazy<ElicitationManager> =
    once_cell::sync::Lazy::new(ElicitationManager::new);

/// Get the global elicitation manager
#[must_use]
pub fn global_elicitation_manager() -> &'static ElicitationManager {
    &GLOBAL_ELICITATION_MANAGER
}

/// Convenience functions for common input types
/// Request text input from user
pub async fn request_text<P>(prompt: P) -> McpResult<String>
where
    P: Into<String>,
{
    let request = ElicitationRequest::new(
        InputType::Text {
            placeholder: None,
            max_length: None,
            masked: false,
        },
        prompt,
    );

    let response = global_elicitation_manager().request_input(request).await?;

    if response.cancelled {
        return Err(McpError::Tool("User cancelled input request".to_string()));
    }

    if let Some(error) = response.error {
        return Err(McpError::Tool(format!("Input error: {error}")));
    }

    response
        .get_value::<String>()?
        .ok_or_else(|| McpError::Tool("No value provided".to_string()))
}

/// Request password input from user
pub async fn request_password<P>(prompt: P) -> McpResult<String>
where
    P: Into<String>,
{
    let request = ElicitationRequest::new(
        InputType::Text {
            placeholder: None,
            max_length: None,
            masked: true,
        },
        prompt,
    );

    let response = global_elicitation_manager().request_input(request).await?;

    if response.cancelled {
        return Err(McpError::Tool(
            "User cancelled password request".to_string(),
        ));
    }

    response
        .get_value::<String>()?
        .ok_or_else(|| McpError::Tool("No password provided".to_string()))
}

/// Request number input from user
pub async fn request_number<P>(prompt: P) -> McpResult<f64>
where
    P: Into<String>,
{
    let request = ElicitationRequest::new(
        InputType::Number {
            min: None,
            max: None,
            decimal_places: None,
        },
        prompt,
    );

    let response = global_elicitation_manager().request_input(request).await?;

    if response.cancelled {
        return Err(McpError::Tool("User cancelled number request".to_string()));
    }

    response
        .get_value::<f64>()?
        .ok_or_else(|| McpError::Tool("No number provided".to_string()))
}

/// Request yes/no confirmation from user
pub async fn request_confirmation<P>(prompt: P) -> McpResult<bool>
where
    P: Into<String>,
{
    let request = ElicitationRequest::new(
        InputType::Boolean {
            true_label: Some("Yes".to_string()),
            false_label: Some("No".to_string()),
        },
        prompt,
    );

    let response = global_elicitation_manager().request_input(request).await?;

    if response.cancelled {
        return Err(McpError::Tool(
            "User cancelled confirmation request".to_string(),
        ));
    }

    response
        .get_value::<bool>()?
        .ok_or_else(|| McpError::Tool("No confirmation provided".to_string()))
}

/// Request choice selection from user
pub async fn request_choice<P>(prompt: P, options: Vec<ChoiceOption>) -> McpResult<String>
where
    P: Into<String>,
{
    let request = ElicitationRequest::new(
        InputType::Choice {
            options,
            allow_custom: false,
        },
        prompt,
    );

    let response = global_elicitation_manager().request_input(request).await?;

    if response.cancelled {
        return Err(McpError::Tool("User cancelled choice request".to_string()));
    }

    response
        .get_value::<String>()?
        .ok_or_else(|| McpError::Tool("No choice made".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_elicitation_id() {
        let id1 = ElicitationId::new();
        let id2 = ElicitationId::new();

        assert_ne!(id1, id2);
        assert!(!id1.as_str().is_empty());
    }

    #[test]
    fn test_choice_option() {
        let option = ChoiceOption::new("value1", "Label 1")
            .with_description("This is option 1")
            .disabled();

        assert_eq!(option.value, "value1");
        assert_eq!(option.label, "Label 1");
        assert!(option.description.is_some());
        assert!(option.disabled);
    }

    #[test]
    fn test_elicitation_request() {
        let request = ElicitationRequest::new(
            InputType::Text {
                placeholder: None,
                max_length: Some(100),
                masked: false,
            },
            "Enter your name",
        )
        .with_description("Please provide your full name")
        .with_default("John Doe")
        .optional()
        .with_priority(Priority::High);

        assert_eq!(request.prompt, "Enter your name");
        assert!(request.description.is_some());
        assert!(request.default_value.is_some());
        assert!(!request.required);
        assert_eq!(request.priority, Priority::High);
    }

    #[tokio::test]
    async fn test_elicitation_manager() {
        let manager = ElicitationManager::new();

        // Test status before request
        let id = ElicitationId::new();
        assert!(manager.get_status(&id).await.is_none());

        // Test listing pending (should be empty)
        let pending = manager.list_pending().await;
        assert!(pending.is_empty());

        // Test cancelling non-existent request
        let cancel_result = manager.cancel_request(id.clone()).await;
        assert!(cancel_result.is_err());
    }

    #[test]
    fn test_elicitation_response() {
        let id = ElicitationId::new();

        // Test success response
        let success = ElicitationResponse::success(id.clone(), "test value").unwrap();
        assert!(!success.cancelled);
        assert!(success.error.is_none());
        assert!(success.value.is_some());

        // Test cancelled response
        let cancelled = ElicitationResponse::cancelled(id.clone());
        assert!(cancelled.cancelled);
        assert!(cancelled.value.is_none());

        // Test error response
        let error = ElicitationResponse::error(id, "Test error");
        assert!(!error.cancelled);
        assert!(error.error.is_some());
        assert!(error.value.is_none());
    }
}
