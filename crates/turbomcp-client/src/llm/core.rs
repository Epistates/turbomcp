//! Core LLM abstractions and types
//!
//! Defines the fundamental traits and types for the LLM system, providing a
//! provider-agnostic interface for different LLM backends.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use thiserror::Error;

// ============================================================================
// ERROR TYPES
// ============================================================================

/// Errors that can occur during LLM operations
#[derive(Error, Debug)]
pub enum LLMError {
    /// Configuration errors
    #[error("Configuration error: {message}")]
    Configuration { message: String },

    /// Authentication failures
    #[error("Authentication failed: {details}")]
    Authentication { details: String },

    /// Network and connectivity issues
    #[error("Network error: {message}")]
    Network { message: String },

    /// API rate limiting
    #[error("Rate limited. Retry after: {retry_after_seconds}s")]
    RateLimit { retry_after_seconds: u64 },

    /// Model or parameter validation errors
    #[error("Invalid parameters: {details}")]
    InvalidParameters { details: String },

    /// Provider-specific errors
    #[error("Provider error [{code}]: {message}")]
    ProviderError { code: i32, message: String },

    /// Request timeout
    #[error("Request timed out after {seconds}s")]
    Timeout { seconds: u64 },

    /// Content processing errors
    #[error("Content processing error: {details}")]
    ContentProcessing { details: String },

    /// Token limit exceeded
    #[error("Token limit exceeded: {used} > {limit}")]
    TokenLimitExceeded { used: usize, limit: usize },

    /// Model not available
    #[error("Model '{model}' not available")]
    ModelNotAvailable { model: String },

    /// Provider not found
    #[error("Provider '{provider}' not found")]
    ProviderNotFound { provider: String },

    /// Session management errors
    #[error("Session error: {message}")]
    Session { message: String },

    /// Generic errors
    #[error("LLM error: {message}")]
    Generic { message: String },
}

impl LLMError {
    /// Create a configuration error
    pub fn configuration(message: impl Into<String>) -> Self {
        Self::Configuration {
            message: message.into(),
        }
    }

    /// Create an authentication error
    pub fn authentication(details: impl Into<String>) -> Self {
        Self::Authentication {
            details: details.into(),
        }
    }

    /// Create a network error
    pub fn network(message: impl Into<String>) -> Self {
        Self::Network {
            message: message.into(),
        }
    }

    /// Create an invalid parameters error
    pub fn invalid_parameters(details: impl Into<String>) -> Self {
        Self::InvalidParameters {
            details: details.into(),
        }
    }

    /// Create a provider error
    pub fn provider_error(code: i32, message: impl Into<String>) -> Self {
        Self::ProviderError {
            code,
            message: message.into(),
        }
    }

    /// Create a timeout error
    pub fn timeout(seconds: u64) -> Self {
        Self::Timeout { seconds }
    }

    /// Create a content processing error
    pub fn content_processing(details: impl Into<String>) -> Self {
        Self::ContentProcessing {
            details: details.into(),
        }
    }

    /// Create a token limit error
    pub fn token_limit_exceeded(used: usize, limit: usize) -> Self {
        Self::TokenLimitExceeded { used, limit }
    }

    /// Create a model not available error
    pub fn model_not_available(model: impl Into<String>) -> Self {
        Self::ModelNotAvailable {
            model: model.into(),
        }
    }

    /// Create a provider not found error
    pub fn provider_not_found(provider: impl Into<String>) -> Self {
        Self::ProviderNotFound {
            provider: provider.into(),
        }
    }

    /// Create a session error
    pub fn session(message: impl Into<String>) -> Self {
        Self::Session {
            message: message.into(),
        }
    }

    /// Create a generic error
    pub fn generic(message: impl Into<String>) -> Self {
        Self::Generic {
            message: message.into(),
        }
    }
}

pub type LLMResult<T> = Result<T, LLMError>;

// ============================================================================
// MESSAGE TYPES
// ============================================================================

/// Role of a message in a conversation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    /// User message
    User,
    /// Assistant/AI response
    Assistant,
    /// System instruction
    System,
    /// Function/tool call
    Function,
}

/// Content type for messages
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum MessageContent {
    /// Plain text content
    #[serde(rename = "text")]
    Text { text: String },

    /// Image content
    #[serde(rename = "image")]
    Image { url: String, detail: Option<String> },

    /// Tool call content
    #[serde(rename = "tool_call")]
    ToolCall {
        id: String,
        function: String,
        arguments: serde_json::Value,
    },

    /// Tool result content
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_call_id: String,
        result: serde_json::Value,
        is_error: bool,
    },
}

impl MessageContent {
    /// Create text content
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text { text: text.into() }
    }

    /// Create image content
    pub fn image(url: impl Into<String>, detail: Option<String>) -> Self {
        Self::Image {
            url: url.into(),
            detail,
        }
    }

    /// Extract text content if available
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text { text } => Some(text),
            _ => None,
        }
    }

    /// Check if content is text
    pub fn is_text(&self) -> bool {
        matches!(self, Self::Text { .. })
    }

    /// Check if content is an image
    pub fn is_image(&self) -> bool {
        matches!(self, Self::Image { .. })
    }
}

/// A message in an LLM conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMMessage {
    /// Message role
    pub role: MessageRole,

    /// Message content
    pub content: MessageContent,

    /// Optional message metadata
    pub metadata: HashMap<String, serde_json::Value>,

    /// Timestamp
    pub timestamp: DateTime<Utc>,
}

impl LLMMessage {
    /// Create a user message
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::User,
            content: MessageContent::text(content),
            metadata: HashMap::new(),
            timestamp: Utc::now(),
        }
    }

    /// Create an assistant message
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::Assistant,
            content: MessageContent::text(content),
            metadata: HashMap::new(),
            timestamp: Utc::now(),
        }
    }

    /// Create a system message
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::System,
            content: MessageContent::text(content),
            metadata: HashMap::new(),
            timestamp: Utc::now(),
        }
    }

    /// Add metadata to the message
    pub fn with_metadata(mut self, key: String, value: serde_json::Value) -> Self {
        self.metadata.insert(key, value);
        self
    }

    /// Get metadata value
    pub fn get_metadata(&self, key: &str) -> Option<&serde_json::Value> {
        self.metadata.get(key)
    }
}

// ============================================================================
// REQUEST AND RESPONSE TYPES
// ============================================================================

/// LLM generation parameters
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GenerationParams {
    /// Temperature (0.0 to 2.0)
    pub temperature: Option<f32>,

    /// Top-p sampling (0.0 to 1.0)
    pub top_p: Option<f32>,

    /// Top-k sampling
    pub top_k: Option<i32>,

    /// Maximum tokens to generate
    pub max_tokens: Option<i32>,

    /// Stop sequences
    pub stop_sequences: Option<Vec<String>>,

    /// Frequency penalty
    pub frequency_penalty: Option<f32>,

    /// Presence penalty
    pub presence_penalty: Option<f32>,

    /// Random seed for reproducibility
    pub seed: Option<i64>,
}

// Default implementation is now derived

/// Request to an LLM provider
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMRequest {
    /// Model to use for generation
    pub model: String,

    /// Conversation messages
    pub messages: Vec<LLMMessage>,

    /// Generation parameters
    pub params: GenerationParams,

    /// Enable streaming response
    pub stream: bool,

    /// Request metadata
    pub metadata: HashMap<String, serde_json::Value>,

    /// Timeout for the request
    pub timeout: Option<Duration>,
}

impl LLMRequest {
    /// Create a new LLM request
    pub fn new(model: impl Into<String>, messages: Vec<LLMMessage>) -> Self {
        Self {
            model: model.into(),
            messages,
            params: GenerationParams::default(),
            stream: false,
            metadata: HashMap::new(),
            timeout: None,
        }
    }

    /// Set generation parameters
    pub fn with_params(mut self, params: GenerationParams) -> Self {
        self.params = params;
        self
    }

    /// Enable streaming
    pub fn with_streaming(mut self, stream: bool) -> Self {
        self.stream = stream;
        self
    }

    /// Set timeout
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Add metadata
    pub fn with_metadata(mut self, key: String, value: serde_json::Value) -> Self {
        self.metadata.insert(key, value);
        self
    }

    /// Get the first user message
    pub fn get_user_message(&self) -> Option<&str> {
        self.messages
            .iter()
            .find(|msg| msg.role == MessageRole::User)
            .and_then(|msg| msg.content.as_text())
    }

    /// Count total messages
    pub fn message_count(&self) -> usize {
        self.messages.len()
    }
}

/// Token usage information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsage {
    /// Input tokens consumed
    pub prompt_tokens: usize,

    /// Output tokens generated
    pub completion_tokens: usize,

    /// Total tokens used
    pub total_tokens: usize,
}

impl TokenUsage {
    /// Create new token usage
    pub fn new(prompt_tokens: usize, completion_tokens: usize) -> Self {
        Self {
            prompt_tokens,
            completion_tokens,
            total_tokens: prompt_tokens + completion_tokens,
        }
    }

    /// Create empty token usage
    pub fn empty() -> Self {
        Self::new(0, 0)
    }
}

/// Response from an LLM provider
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMResponse {
    /// Generated message
    pub message: LLMMessage,

    /// Model used for generation
    pub model: String,

    /// Token usage information
    pub usage: TokenUsage,

    /// Stop reason
    pub stop_reason: Option<String>,

    /// Response metadata
    pub metadata: HashMap<String, serde_json::Value>,

    /// Generation timestamp
    pub timestamp: DateTime<Utc>,
}

impl LLMResponse {
    /// Create a new LLM response
    pub fn new(message: LLMMessage, model: impl Into<String>, usage: TokenUsage) -> Self {
        Self {
            message,
            model: model.into(),
            usage,
            stop_reason: None,
            metadata: HashMap::new(),
            timestamp: Utc::now(),
        }
    }

    /// Set stop reason
    pub fn with_stop_reason(mut self, stop_reason: impl Into<String>) -> Self {
        self.stop_reason = Some(stop_reason.into());
        self
    }

    /// Add metadata
    pub fn with_metadata(mut self, key: String, value: serde_json::Value) -> Self {
        self.metadata.insert(key, value);
        self
    }

    /// Get response text
    pub fn text(&self) -> Option<&str> {
        self.message.content.as_text()
    }

    /// Check if response is complete
    pub fn is_complete(&self) -> bool {
        self.stop_reason.is_some()
    }
}

// ============================================================================
// PROVIDER CONFIGURATION
// ============================================================================

/// Configuration for an LLM provider
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMProviderConfig {
    /// API key or credentials
    pub api_key: String,

    /// Base URL for API requests
    pub base_url: Option<String>,

    /// Default model to use
    pub model: String,

    /// Request timeout in seconds
    pub timeout_seconds: u64,

    /// Maximum retry attempts
    pub max_retries: u32,

    /// Custom headers
    pub headers: HashMap<String, String>,

    /// Provider-specific options
    pub options: HashMap<String, serde_json::Value>,
}

impl Default for LLMProviderConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            base_url: None,
            model: String::new(),
            timeout_seconds: 30,
            max_retries: 3,
            headers: HashMap::new(),
            options: HashMap::new(),
        }
    }
}

impl LLMProviderConfig {
    /// Create a new provider config
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            model: model.into(),
            ..Default::default()
        }
    }

    /// Set base URL
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = Some(base_url.into());
        self
    }

    /// Set timeout
    pub fn with_timeout(mut self, timeout_seconds: u64) -> Self {
        self.timeout_seconds = timeout_seconds;
        self
    }

    /// Set max retries
    pub fn with_max_retries(mut self, max_retries: u32) -> Self {
        self.max_retries = max_retries;
        self
    }

    /// Add custom header
    pub fn with_header(mut self, key: String, value: String) -> Self {
        self.headers.insert(key, value);
        self
    }

    /// Add custom option
    pub fn with_option(mut self, key: String, value: serde_json::Value) -> Self {
        self.options.insert(key, value);
        self
    }

    /// Validate the configuration
    pub fn validate(&self) -> LLMResult<()> {
        if self.api_key.trim().is_empty() {
            return Err(LLMError::configuration("API key cannot be empty"));
        }

        if self.model.trim().is_empty() {
            return Err(LLMError::configuration("Model cannot be empty"));
        }

        if self.timeout_seconds == 0 {
            return Err(LLMError::configuration("Timeout must be greater than 0"));
        }

        Ok(())
    }
}

// ============================================================================
// PROVIDER CAPABILITIES AND MODEL INFO
// ============================================================================

/// Capabilities of an LLM provider
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMCapabilities {
    /// Supports streaming responses
    pub streaming: bool,

    /// Supports image inputs
    pub vision: bool,

    /// Supports function/tool calling
    pub function_calling: bool,

    /// Supports JSON mode
    pub json_mode: bool,

    /// Maximum context window size
    pub max_context_tokens: Option<usize>,

    /// Maximum output tokens
    pub max_output_tokens: Option<usize>,

    /// Supported content types
    pub content_types: Vec<String>,
}

impl Default for LLMCapabilities {
    fn default() -> Self {
        Self {
            streaming: false,
            vision: false,
            function_calling: false,
            json_mode: false,
            max_context_tokens: None,
            max_output_tokens: None,
            content_types: vec!["text".to_string()],
        }
    }
}

/// Information about a specific model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    /// Model name/identifier
    pub name: String,

    /// Human-readable display name
    pub display_name: String,

    /// Model description
    pub description: Option<String>,

    /// Model capabilities
    pub capabilities: LLMCapabilities,

    /// Model version
    pub version: Option<String>,

    /// Model pricing (tokens per dollar)
    pub pricing: Option<ModelPricing>,

    /// Whether the model is available
    pub available: bool,
}

/// Pricing information for a model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPricing {
    /// Input token cost (per 1K tokens)
    pub input_cost_per_1k: Option<f64>,

    /// Output token cost (per 1K tokens)
    pub output_cost_per_1k: Option<f64>,
}

// ============================================================================
// CORE PROVIDER TRAIT
// ============================================================================

/// Core trait for LLM providers
///
/// Implement this trait to add support for new LLM providers. The trait provides
/// a standardized interface for generating text, managing models, and handling
/// provider-specific functionality.
///
/// # Examples
///
/// ```rust,no_run
/// use turbomcp_client::llm::{LLMProvider, LLMRequest, LLMResponse, LLMResult, ModelInfo, LLMCapabilities};
/// use async_trait::async_trait;
///
/// #[derive(Debug)]
/// struct CustomProvider;
///
/// #[async_trait]
/// impl LLMProvider for CustomProvider {
///     fn name(&self) -> &str {
///         "custom"
///     }
///
///     async fn generate(&self, request: &LLMRequest) -> LLMResult<LLMResponse> {
///         // Implementation here
///         todo!()
///     }
///
///     async fn list_models(&self) -> LLMResult<Vec<ModelInfo>> {
///         // Return available models
///         todo!()
///     }
///
///     fn capabilities(&self) -> &LLMCapabilities {
///         // Return provider capabilities
///         todo!()
///     }
/// }
/// ```
#[async_trait]
pub trait LLMProvider: Send + Sync + std::fmt::Debug {
    /// Provider name (e.g., "openai", "anthropic")
    fn name(&self) -> &str;

    /// Provider version
    fn version(&self) -> &str {
        "1.0.0"
    }

    /// Generate a response for the given request
    ///
    /// This is the core method that handles text generation. Implementations
    /// should convert the request to the provider's format, make the API call,
    /// and return a standardized response.
    async fn generate(&self, request: &LLMRequest) -> LLMResult<LLMResponse>;

    /// List available models for this provider
    async fn list_models(&self) -> LLMResult<Vec<ModelInfo>>;

    /// Get provider capabilities
    fn capabilities(&self) -> &LLMCapabilities;

    /// Get information about a specific model
    async fn get_model_info(&self, model: &str) -> LLMResult<ModelInfo> {
        let models = self.list_models().await?;
        models
            .into_iter()
            .find(|m| m.name == model)
            .ok_or_else(|| LLMError::model_not_available(model))
    }

    /// Check if a model is supported
    async fn supports_model(&self, model: &str) -> bool {
        self.get_model_info(model).await.is_ok()
    }

    /// Estimate token count for text (optional override)
    fn estimate_tokens(&self, text: &str) -> usize {
        // Simple estimation: ~1 token per 4 characters
        text.len().div_ceil(4)
    }

    /// Validate a request before sending
    async fn validate_request(&self, request: &LLMRequest) -> LLMResult<()> {
        // Check if model is supported
        if !self.supports_model(&request.model).await {
            return Err(LLMError::model_not_available(&request.model));
        }

        // Validate messages
        if request.messages.is_empty() {
            return Err(LLMError::invalid_parameters(
                "At least one message is required",
            ));
        }

        // Check token limits if available
        let model_info = self.get_model_info(&request.model).await?;
        if let Some(max_tokens) = model_info.capabilities.max_context_tokens {
            let estimated_tokens: usize = request
                .messages
                .iter()
                .filter_map(|msg| msg.content.as_text())
                .map(|text| self.estimate_tokens(text))
                .sum();

            if estimated_tokens > max_tokens {
                return Err(LLMError::token_limit_exceeded(estimated_tokens, max_tokens));
            }
        }

        Ok(())
    }

    /// Health check for the provider
    async fn health_check(&self) -> LLMResult<()> {
        // Try to list models as a basic health check
        self.list_models().await?;
        Ok(())
    }

    /// Handle MCP CreateMessageRequest (adapts to LLM types)
    ///
    /// This method provides a bridge between MCP protocol types and the LLM system.
    /// It converts CreateMessageRequest to LLMRequest, calls generate(), and converts
    /// the response back to CreateMessageResult.
    async fn handle_create_message(
        &self,
        request: turbomcp_protocol::types::CreateMessageRequest,
    ) -> LLMResult<turbomcp_protocol::types::CreateMessageResult> {
        use turbomcp_protocol::types::{Content, CreateMessageResult, Role, TextContent};

        // Convert MCP messages to LLM messages
        let llm_messages: Vec<LLMMessage> = request
            .messages
            .iter()
            .map(|msg| {
                let role = match msg.role {
                    Role::User => MessageRole::User,
                    Role::Assistant => MessageRole::Assistant,
                };

                let content = match &msg.content {
                    Content::Text(text) => MessageContent::text(&text.text),
                    _ => MessageContent::text("Non-text content not yet supported"),
                };

                LLMMessage {
                    role,
                    content,
                    metadata: std::collections::HashMap::new(),
                    timestamp: chrono::Utc::now(),
                }
            })
            .collect();

        // Add system message if provided
        let mut all_messages = Vec::new();
        if let Some(system_prompt) = &request.system_prompt {
            all_messages.push(LLMMessage::system(system_prompt));
        }
        all_messages.extend(llm_messages);

        // Build generation parameters
        let params = GenerationParams {
            max_tokens: Some(request.max_tokens as i32),
            temperature: request.temperature.map(|t| t as f32),
            stop_sequences: request.stop_sequences.clone(),
            ..Default::default()
        };

        // Determine model to use
        let model = if let Some(prefs) = &request.model_preferences
            && let Some(hints) = &prefs.hints
            && let Some(model_hint) = hints.first()
        {
            model_hint.name.clone()
        } else {
            // Use first available model
            let models = self.list_models().await.unwrap_or_default();
            models
                .first()
                .map(|m| m.name.clone())
                .unwrap_or_else(|| "default".to_string())
        };

        // Create LLM request
        let llm_request = LLMRequest::new(model, all_messages).with_params(params);

        // Generate response
        let llm_response = self.generate(&llm_request).await?;

        // Convert back to MCP format
        let text = llm_response.text().unwrap_or("").to_string();
        let result = CreateMessageResult {
            role: Role::Assistant,
            content: Content::Text(TextContent {
                text,
                annotations: None,
                meta: None,
            }),
            model: Some(llm_response.model),
            stop_reason: llm_response.stop_reason,
        };

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_llm_error_creation() {
        let config_error = LLMError::configuration("Test error");
        assert!(config_error.to_string().contains("Configuration error"));

        let auth_error = LLMError::authentication("Invalid key");
        assert!(auth_error.to_string().contains("Authentication failed"));

        let token_error = LLMError::token_limit_exceeded(1000, 800);
        assert!(token_error.to_string().contains("1000 > 800"));
    }

    #[test]
    fn test_message_creation() {
        let user_msg = LLMMessage::user("Hello, world!");
        assert_eq!(user_msg.role, MessageRole::User);
        assert_eq!(user_msg.content.as_text(), Some("Hello, world!"));

        let assistant_msg = LLMMessage::assistant("Hi there!");
        assert_eq!(assistant_msg.role, MessageRole::Assistant);

        let system_msg = LLMMessage::system("You are a helpful assistant");
        assert_eq!(system_msg.role, MessageRole::System);
    }

    #[test]
    fn test_message_content() {
        let text_content = MessageContent::text("Hello");
        assert!(text_content.is_text());
        assert!(!text_content.is_image());
        assert_eq!(text_content.as_text(), Some("Hello"));

        let image_content = MessageContent::image("https://example.com/image.jpg", None);
        assert!(!image_content.is_text());
        assert!(image_content.is_image());
        assert_eq!(image_content.as_text(), None);
    }

    #[test]
    fn test_generation_params() {
        let params = GenerationParams {
            temperature: Some(0.7),
            max_tokens: Some(100),
            ..Default::default()
        };

        assert_eq!(params.temperature, Some(0.7));
        assert_eq!(params.max_tokens, Some(100));
        assert_eq!(params.top_p, None);
    }

    #[test]
    fn test_llm_request() {
        let messages = vec![
            LLMMessage::user("What's 2+2?"),
            LLMMessage::assistant("2+2 equals 4."),
        ];

        let request = LLMRequest::new("gpt-4", messages.clone())
            .with_streaming(true)
            .with_metadata("session_id".to_string(), json!("session123"));

        assert_eq!(request.model, "gpt-4");
        assert_eq!(request.messages.len(), 2);
        assert!(request.stream);
        assert_eq!(request.get_user_message(), Some("What's 2+2?"));
        assert_eq!(request.message_count(), 2);
    }

    #[test]
    fn test_token_usage() {
        let usage = TokenUsage::new(100, 50);
        assert_eq!(usage.prompt_tokens, 100);
        assert_eq!(usage.completion_tokens, 50);
        assert_eq!(usage.total_tokens, 150);

        let empty_usage = TokenUsage::empty();
        assert_eq!(empty_usage.total_tokens, 0);
    }

    #[test]
    fn test_llm_response() {
        let message = LLMMessage::assistant("The answer is 4.");
        let usage = TokenUsage::new(20, 10);

        let response = LLMResponse::new(message, "gpt-4", usage)
            .with_stop_reason("complete")
            .with_metadata("finish_reason".to_string(), json!("stop"));

        assert_eq!(response.model, "gpt-4");
        assert_eq!(response.text(), Some("The answer is 4."));
        assert_eq!(response.stop_reason, Some("complete".to_string()));
        assert!(response.is_complete());
        assert_eq!(response.usage.total_tokens, 30);
    }

    #[test]
    fn test_provider_config() {
        let config = LLMProviderConfig::new("test-key", "gpt-4")
            .with_base_url("https://custom.api.com")
            .with_timeout(60)
            .with_max_retries(5)
            .with_header("Custom-Header".to_string(), "value".to_string())
            .with_option("custom_option".to_string(), json!(true));

        assert_eq!(config.api_key, "test-key");
        assert_eq!(config.model, "gpt-4");
        assert_eq!(config.base_url, Some("https://custom.api.com".to_string()));
        assert_eq!(config.timeout_seconds, 60);
        assert_eq!(config.max_retries, 5);
        assert_eq!(
            config.headers.get("Custom-Header"),
            Some(&"value".to_string())
        );

        assert!(config.validate().is_ok());

        let invalid_config = LLMProviderConfig {
            api_key: "".to_string(),
            ..config
        };
        assert!(invalid_config.validate().is_err());
    }

    #[test]
    fn test_capabilities() {
        let mut capabilities = LLMCapabilities::default();
        assert!(!capabilities.streaming);
        assert!(!capabilities.vision);
        assert!(!capabilities.function_calling);
        assert_eq!(capabilities.content_types, vec!["text".to_string()]);

        capabilities.streaming = true;
        capabilities.vision = true;
        capabilities.max_context_tokens = Some(128000);

        assert!(capabilities.streaming);
        assert!(capabilities.vision);
        assert_eq!(capabilities.max_context_tokens, Some(128000));
    }

    #[test]
    fn test_model_info() {
        let capabilities = LLMCapabilities {
            streaming: true,
            vision: false,
            max_context_tokens: Some(128000),
            ..Default::default()
        };

        let model = ModelInfo {
            name: "gpt-4".to_string(),
            display_name: "GPT-4".to_string(),
            description: Some("Advanced language model".to_string()),
            capabilities,
            version: Some("2024-01".to_string()),
            pricing: None,
            available: true,
        };

        assert_eq!(model.name, "gpt-4");
        assert_eq!(model.display_name, "GPT-4");
        assert!(model.available);
        assert!(model.capabilities.streaming);
    }
}
