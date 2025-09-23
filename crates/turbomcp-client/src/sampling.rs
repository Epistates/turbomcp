//! Client-side sampling support for handling server-initiated requests
//!
//! This module provides production-grade LLM backend integration for processing
//! sampling requests from servers, enabling bidirectional LLM interactions in the MCP protocol.
//!
//! ## Features
//!
//! - **Multi-provider support**: OpenAI, Anthropic, and extensible architecture
//! - **MCP protocol compliance**: Full CreateMessageRequest → CreateMessageResult flow
//! - **Production-grade error handling**: Comprehensive error types and recovery
//! - **Conversation context**: Proper message history management
//! - **Configuration**: Flexible backend selection and parameter tuning
//! - **Async-first**: Send + Sync throughout with proper async patterns
//!
//! ## Example
//!
//! ```rust,no_run
//! use turbomcp_client::sampling::{LLMBackendConfig, LLMProvider, ProductionSamplingHandler};
//!
//! let config = LLMBackendConfig {
//!     provider: LLMProvider::OpenAI {
//!         api_key: std::env::var("OPENAI_API_KEY").unwrap(),
//!         base_url: None,
//!         organization: None,
//!     },
//!     default_model: Some("gpt-4".to_string()),
//!     timeout_seconds: 30,
//!     max_retries: 3,
//! };
//!
//! let handler = ProductionSamplingHandler::new(config)?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use async_trait::async_trait;
use reqwest;
use std::time::Duration;
use thiserror::Error;
use tracing::{debug, error, info, warn};
use turbomcp_protocol::types::{
    Content, CreateMessageRequest, CreateMessageResult, Role, SamplingMessage, TextContent,
};

// ============================================================================
// ERROR TYPES - PRODUCTION-GRADE ERROR HANDLING
// ============================================================================

/// Comprehensive error types for LLM backend operations
#[derive(Error, Debug)]
pub enum LLMBackendError {
    /// Configuration errors
    #[error("Configuration error: {message}")]
    Configuration { message: String },

    /// Authentication failures
    #[error("Authentication failed: {details}")]
    Authentication { details: String },

    /// Network and connectivity issues
    #[error("Network error: {source}")]
    Network {
        #[from]
        source: reqwest::Error,
    },

    /// API rate limiting
    #[error("Rate limited. Retry after: {retry_after_seconds}s")]
    RateLimit { retry_after_seconds: u64 },

    /// Model or parameter validation errors
    #[error("Invalid model parameters: {details}")]
    InvalidParameters { details: String },

    /// LLM provider-specific errors
    #[error("LLM provider error [{code}]: {message}")]
    ProviderError { code: i32, message: String },

    /// Timeout during LLM request
    #[error("Request timed out after {seconds}s")]
    Timeout { seconds: u64 },

    /// Content parsing or serialization errors
    #[error("Content processing error: {details}")]
    ContentProcessing { details: String },

    /// Generic errors with context
    #[error("LLM backend error: {message}")]
    Generic { message: String },
}

type LLMResult<T> = Result<T, LLMBackendError>;

// ============================================================================
// CONFIGURATION TYPES - FLEXIBLE BACKEND CONFIGURATION
// ============================================================================

/// Supported LLM providers
#[derive(Debug, Clone)]
pub enum LLMProvider {
    /// OpenAI (GPT models)
    OpenAI {
        api_key: String,
        base_url: Option<String>,
        organization: Option<String>,
    },
    /// Anthropic (Claude models)
    Anthropic {
        api_key: String,
        base_url: Option<String>,
    },
    // Future providers can be added here
    // Ollama { base_url: String },
    // AzureOpenAI { ... },
}

/// Comprehensive backend configuration
#[derive(Debug, Clone)]
pub struct LLMBackendConfig {
    /// The LLM provider to use
    pub provider: LLMProvider,
    /// Default model name (can be overridden per request)
    pub default_model: Option<String>,
    /// Request timeout in seconds
    pub timeout_seconds: u64,
    /// Maximum retry attempts for transient failures
    pub max_retries: u32,
}

impl LLMBackendConfig {
    /// Validate configuration parameters
    pub fn validate(&self) -> LLMResult<()> {
        // Validate timeout
        if self.timeout_seconds == 0 {
            return Err(LLMBackendError::Configuration {
                message: "Timeout must be greater than 0".to_string(),
            });
        }

        // Validate provider-specific settings
        match &self.provider {
            LLMProvider::OpenAI { api_key, .. } => {
                if api_key.trim().is_empty() {
                    return Err(LLMBackendError::Configuration {
                        message: "OpenAI API key cannot be empty".to_string(),
                    });
                }
            }
            LLMProvider::Anthropic { api_key, .. } => {
                if api_key.trim().is_empty() {
                    return Err(LLMBackendError::Configuration {
                        message: "Anthropic API key cannot be empty".to_string(),
                    });
                }
            }
        }

        Ok(())
    }
}

// ============================================================================
// SAMPLING HANDLER TRAIT - PRODUCTION-READY INTERFACE
// ============================================================================

/// Handler for server-initiated sampling requests
///
/// Implement this trait to handle sampling requests from MCP servers.
/// The handler receives a `CreateMessageRequest` and must return a response
/// with the generated content.
///
/// # Examples
///
/// ```rust,no_run
/// use turbomcp_client::sampling::{SamplingHandler, ProductionSamplingHandler, LLMBackendConfig, LLMProvider};
/// use turbomcp_protocol::types::{CreateMessageRequest, CreateMessageResult};
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
/// let config = LLMBackendConfig {
///     provider: LLMProvider::OpenAI {
///         api_key: std::env::var("OPENAI_API_KEY")?,
///         base_url: None,
///         organization: None,
///     },
///     default_model: Some("gpt-4".to_string()),
///     timeout_seconds: 30,
///     max_retries: 3,
/// };
///
/// let handler = ProductionSamplingHandler::new(config)?;
/// // Use handler for MCP sampling requests
/// # Ok(()) }
/// ```
#[async_trait]
pub trait SamplingHandler: Send + Sync + std::fmt::Debug {
    /// Handle a sampling/createMessage request from the server
    async fn handle_create_message(
        &self,
        request: CreateMessageRequest,
    ) -> Result<CreateMessageResult, Box<dyn std::error::Error + Send + Sync>>;
}

// ============================================================================
// PRODUCTION SAMPLING HANDLER - WORLD-CLASS LLM INTEGRATION
// ============================================================================

/// Production-grade sampling handler with real LLM backend integration
///
/// This handler provides enterprise-ready LLM integration with:
/// - Multi-provider support (OpenAI, Anthropic)
/// - MCP protocol compliance
/// - Comprehensive error handling
/// - Conversation context management
/// - Retry logic and timeout handling
/// - Proper async patterns with Send + Sync
///
/// # Example Usage
///
/// ```rust,no_run
/// use turbomcp_client::sampling::{ProductionSamplingHandler, LLMBackendConfig, LLMProvider};
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
/// let config = LLMBackendConfig {
///     provider: LLMProvider::OpenAI {
///         api_key: std::env::var("OPENAI_API_KEY")?,
///         base_url: None,
///         organization: None,
///     },
///     default_model: Some("gpt-4".to_string()),
///     timeout_seconds: 30,
///     max_retries: 3,
/// };
///
/// let handler = ProductionSamplingHandler::new(config)?;
/// # Ok(()) }
/// ```
#[derive(Debug)]
pub struct ProductionSamplingHandler {
    config: LLMBackendConfig,
    http_client: reqwest::Client,
}

impl ProductionSamplingHandler {
    /// Create a new production sampling handler
    pub fn new(config: LLMBackendConfig) -> LLMResult<Self> {
        // Validate configuration
        config.validate()?;

        // Create HTTP client with proper configuration
        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.timeout_seconds))
            .user_agent("turbomcp-client/1.0.12")
            .build()
            .map_err(|e| LLMBackendError::Configuration {
                message: format!("Failed to create HTTP client: {}", e),
            })?;

        info!(
            "Initialized ProductionSamplingHandler with provider: {:?}",
            std::mem::discriminant(&config.provider)
        );

        Ok(Self {
            config,
            http_client,
        })
    }

    /// Handle request with retry logic
    async fn handle_with_retries(
        &self,
        request: CreateMessageRequest,
    ) -> LLMResult<CreateMessageResult> {
        let mut last_error = None;

        for attempt in 0..=self.config.max_retries {
            if attempt > 0 {
                let backoff_duration = Duration::from_millis(1000 * (2_u64.pow(attempt - 1)));
                debug!(
                    "Retrying request after {}ms backoff",
                    backoff_duration.as_millis()
                );
                tokio::time::sleep(backoff_duration).await;
            }

            match self.handle_single_request(&request).await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    match &e {
                        LLMBackendError::Authentication { .. } => {
                            // Don't retry auth errors
                            return Err(e);
                        }
                        LLMBackendError::InvalidParameters { .. } => {
                            // Don't retry parameter validation errors
                            return Err(e);
                        }
                        _ => {
                            warn!("Request attempt {} failed: {}", attempt + 1, e);
                            last_error = Some(e);
                        }
                    }
                }
            }
        }

        Err(last_error.unwrap_or_else(|| LLMBackendError::Generic {
            message: "Max retries exceeded".to_string(),
        }))
    }

    /// Handle a single request without retries
    async fn handle_single_request(
        &self,
        request: &CreateMessageRequest,
    ) -> LLMResult<CreateMessageResult> {
        match &self.config.provider {
            LLMProvider::OpenAI {
                api_key,
                base_url,
                organization,
            } => {
                self.handle_openai_request(
                    request,
                    api_key,
                    base_url.as_deref(),
                    organization.as_deref(),
                )
                .await
            }
            LLMProvider::Anthropic { api_key, base_url } => {
                self.handle_anthropic_request(request, api_key, base_url.as_deref())
                    .await
            }
        }
    }

    /// Handle OpenAI API request
    async fn handle_openai_request(
        &self,
        request: &CreateMessageRequest,
        api_key: &str,
        base_url: Option<&str>,
        organization: Option<&str>,
    ) -> LLMResult<CreateMessageResult> {
        let url = format!(
            "{}/v1/chat/completions",
            base_url.unwrap_or("https://api.openai.com")
        );

        // Convert MCP messages to OpenAI format
        let messages = self.convert_messages_to_openai(&request.messages)?;

        // Determine model
        let model = self.determine_model(request);

        // Build OpenAI request
        let mut openai_request = serde_json::json!({
            "model": model,
            "messages": messages,
            "max_tokens": request.max_tokens
        });

        // Add optional parameters
        if let Some(temp) = request.temperature {
            openai_request["temperature"] = serde_json::Value::Number(
                serde_json::Number::from_f64(temp).unwrap_or_else(|| serde_json::Number::from(0)),
            );
        }

        if let Some(system_prompt) = &request.system_prompt {
            // Add system message to the beginning
            let mut msgs = openai_request["messages"].as_array().unwrap().clone();
            msgs.insert(
                0,
                serde_json::json!({
                    "role": "system",
                    "content": system_prompt
                }),
            );
            openai_request["messages"] = serde_json::Value::Array(msgs);
        }

        if let Some(stop_sequences) = &request.stop_sequences {
            openai_request["stop"] = serde_json::Value::Array(
                stop_sequences
                    .iter()
                    .map(|s| serde_json::Value::String(s.clone()))
                    .collect(),
            );
        }

        // Build HTTP request
        let mut req_builder = self
            .http_client
            .post(&url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&openai_request);

        if let Some(org) = organization {
            req_builder = req_builder.header("OpenAI-Organization", org);
        }

        // Send request
        let response = req_builder
            .send()
            .await
            .map_err(|source| LLMBackendError::Network { source })?;

        // Handle HTTP status codes
        if !response.status().is_success() {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());

            return Err(match status.as_u16() {
                401 => LLMBackendError::Authentication {
                    details: format!("Invalid API key: {}", error_text),
                },
                429 => {
                    // Extract retry-after header if available
                    let retry_after = 60; // Default
                    LLMBackendError::RateLimit {
                        retry_after_seconds: retry_after,
                    }
                }
                _ => LLMBackendError::ProviderError {
                    code: status.as_u16() as i32,
                    message: error_text,
                },
            });
        }

        // Parse response
        let response_json: serde_json::Value =
            response
                .json()
                .await
                .map_err(|e| LLMBackendError::ContentProcessing {
                    details: format!("Failed to parse OpenAI response: {}", e),
                })?;

        // Extract message content
        let choices = response_json["choices"].as_array().ok_or_else(|| {
            LLMBackendError::ContentProcessing {
                details: "No choices in OpenAI response".to_string(),
            }
        })?;

        let first_choice = choices
            .first()
            .ok_or_else(|| LLMBackendError::ContentProcessing {
                details: "Empty choices array in OpenAI response".to_string(),
            })?;

        let message = &first_choice["message"];
        let content = message["content"].as_str().unwrap_or("");
        let finish_reason = first_choice["finish_reason"].as_str().unwrap_or("unknown");
        let model_used = response_json["model"].as_str().unwrap_or(&model);

        Ok(CreateMessageResult {
            role: Role::Assistant,
            content: Content::Text(TextContent {
                text: content.to_string(),
                annotations: None,
                meta: None,
            }),
            model: Some(model_used.to_string()),
            stop_reason: Some(finish_reason.to_string()),
        })
    }

    /// Handle Anthropic API request
    async fn handle_anthropic_request(
        &self,
        request: &CreateMessageRequest,
        api_key: &str,
        base_url: Option<&str>,
    ) -> LLMResult<CreateMessageResult> {
        let url = format!(
            "{}/v1/messages",
            base_url.unwrap_or("https://api.anthropic.com")
        );

        // Convert MCP messages to Anthropic format
        let messages = self.convert_messages_to_anthropic(&request.messages)?;

        // Determine model
        let model = self.determine_model(request);

        // Build Anthropic request
        let mut anthropic_request = serde_json::json!({
            "model": model,
            "messages": messages,
            "max_tokens": request.max_tokens
        });

        // Add optional parameters
        if let Some(temp) = request.temperature {
            anthropic_request["temperature"] = serde_json::Value::Number(
                serde_json::Number::from_f64(temp).unwrap_or_else(|| serde_json::Number::from(0)),
            );
        }

        if let Some(system_prompt) = &request.system_prompt {
            anthropic_request["system"] = serde_json::Value::String(system_prompt.clone());
        }

        if let Some(stop_sequences) = &request.stop_sequences {
            anthropic_request["stop_sequences"] = serde_json::Value::Array(
                stop_sequences
                    .iter()
                    .map(|s| serde_json::Value::String(s.clone()))
                    .collect(),
            );
        }

        // Send request
        let response = self
            .http_client
            .post(&url)
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .json(&anthropic_request)
            .send()
            .await
            .map_err(|source| LLMBackendError::Network { source })?;

        // Handle HTTP status codes
        if !response.status().is_success() {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());

            return Err(match status.as_u16() {
                401 => LLMBackendError::Authentication {
                    details: format!("Invalid API key: {}", error_text),
                },
                429 => LLMBackendError::RateLimit {
                    retry_after_seconds: 60,
                },
                _ => LLMBackendError::ProviderError {
                    code: status.as_u16() as i32,
                    message: error_text,
                },
            });
        }

        // Parse response
        let response_json: serde_json::Value =
            response
                .json()
                .await
                .map_err(|e| LLMBackendError::ContentProcessing {
                    details: format!("Failed to parse Anthropic response: {}", e),
                })?;

        // Extract content
        let content_array = response_json["content"].as_array().ok_or_else(|| {
            LLMBackendError::ContentProcessing {
                details: "No content in Anthropic response".to_string(),
            }
        })?;

        let first_content =
            content_array
                .first()
                .ok_or_else(|| LLMBackendError::ContentProcessing {
                    details: "Empty content array in Anthropic response".to_string(),
                })?;

        let text_content = first_content["text"].as_str().unwrap_or("");
        let stop_reason = response_json["stop_reason"].as_str().unwrap_or("unknown");
        let model_used = response_json["model"].as_str().unwrap_or(&model);

        Ok(CreateMessageResult {
            role: Role::Assistant,
            content: Content::Text(TextContent {
                text: text_content.to_string(),
                annotations: None,
                meta: None,
            }),
            model: Some(model_used.to_string()),
            stop_reason: Some(stop_reason.to_string()),
        })
    }

    /// Convert MCP messages to OpenAI format
    fn convert_messages_to_openai(
        &self,
        messages: &[SamplingMessage],
    ) -> LLMResult<Vec<serde_json::Value>> {
        let mut converted = Vec::new();

        for msg in messages {
            let role = match msg.role {
                Role::User => "user",
                Role::Assistant => "assistant",
            };

            let content = match &msg.content {
                Content::Text(text) => text.text.clone(),
                _ => {
                    return Err(LLMBackendError::ContentProcessing {
                        details: "Non-text content not yet supported".to_string(),
                    });
                }
            };

            converted.push(serde_json::json!({
                "role": role,
                "content": content
            }));
        }

        Ok(converted)
    }

    /// Convert MCP messages to Anthropic format
    fn convert_messages_to_anthropic(
        &self,
        messages: &[SamplingMessage],
    ) -> LLMResult<Vec<serde_json::Value>> {
        let mut converted = Vec::new();

        for msg in messages {
            let role = match msg.role {
                Role::User => "user",
                Role::Assistant => "assistant",
            };

            let content = match &msg.content {
                Content::Text(text) => text.text.clone(),
                _ => {
                    return Err(LLMBackendError::ContentProcessing {
                        details: "Non-text content not yet supported".to_string(),
                    });
                }
            };

            converted.push(serde_json::json!({
                "role": role,
                "content": content
            }));
        }

        Ok(converted)
    }

    /// Determine which model to use for the request
    fn determine_model(&self, request: &CreateMessageRequest) -> String {
        // Check if model is specified in preferences
        if let Some(prefs) = &request.model_preferences
            && let Some(hints) = &prefs.hints
            && let Some(model_hint) = hints.first()
        {
            let model_name = &model_hint.name;
            if !model_name.is_empty() {
                return model_name.clone();
            }
        }

        // Use default model or provider-specific default
        self.config
            .default_model
            .clone()
            .unwrap_or_else(|| match &self.config.provider {
                LLMProvider::OpenAI { .. } => "gpt-3.5-turbo".to_string(),
                LLMProvider::Anthropic { .. } => "claude-3-haiku-20240307".to_string(),
            })
    }
}

#[async_trait]
impl SamplingHandler for ProductionSamplingHandler {
    async fn handle_create_message(
        &self,
        request: CreateMessageRequest,
    ) -> Result<CreateMessageResult, Box<dyn std::error::Error + Send + Sync>> {
        info!(
            "Processing CreateMessageRequest with {} messages",
            request.messages.len()
        );

        self.handle_with_retries(request)
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
    }
}

// ============================================================================
// LEGACY HANDLERS - MAINTAINED FOR BACKWARDS COMPATIBILITY
// ============================================================================

/// Default sampling handler - provides echo functionality for testing/development
///
/// **DEPRECATED**: Use `ProductionSamplingHandler` for real applications.
/// This handler is maintained for backwards compatibility and testing only.
#[derive(Debug, Clone)]
pub struct DefaultSamplingHandler;

#[async_trait]
impl SamplingHandler for DefaultSamplingHandler {
    async fn handle_create_message(
        &self,
        request: CreateMessageRequest,
    ) -> Result<CreateMessageResult, Box<dyn std::error::Error + Send + Sync>> {
        warn!(
            "Using DefaultSamplingHandler - this is for testing only. Use ProductionSamplingHandler for real applications."
        );

        // Extract the user's message
        let user_message = request
            .messages
            .iter()
            .find_map(|msg| {
                if msg.role == Role::User {
                    match &msg.content {
                        Content::Text(text) => Some(text.text.clone()),
                        _ => None,
                    }
                } else {
                    None
                }
            })
            .unwrap_or_else(|| "No user message provided".to_string());

        let response_text = format!(
            "Echo response: {}. [This is DefaultSamplingHandler - use ProductionSamplingHandler for real LLM integration]",
            user_message
        );

        Ok(CreateMessageResult {
            role: Role::Assistant,
            content: Content::Text(TextContent {
                text: response_text,
                annotations: None,
                meta: None,
            }),
            model: Some("turbomcp-echo".to_string()),
            stop_reason: Some("complete".to_string()),
        })
    }
}

/// Mock LLM handler for testing
///
/// This handler simulates an LLM by providing canned responses
/// based on the input. Useful for testing and examples.
#[derive(Debug, Clone)]
pub struct MockLLMHandler {
    model_name: String,
}

impl MockLLMHandler {
    /// Create a new mock LLM handler
    pub fn new(model_name: impl Into<String>) -> Self {
        Self {
            model_name: model_name.into(),
        }
    }
}

#[async_trait]
impl SamplingHandler for MockLLMHandler {
    async fn handle_create_message(
        &self,
        request: CreateMessageRequest,
    ) -> Result<CreateMessageResult, Box<dyn std::error::Error + Send + Sync>> {
        // Extract the question
        let question = request
            .messages
            .iter()
            .find_map(|msg| {
                if msg.role == Role::User {
                    match &msg.content {
                        Content::Text(text) => Some(text.text.clone()),
                        _ => None,
                    }
                } else {
                    None
                }
            })
            .unwrap_or_default();

        // Provide mock responses based on patterns
        let response_text = if question.to_lowercase().contains("capital") {
            "The capital of France is Paris.".to_string()
        } else if question.to_lowercase().contains("2+2")
            || question.to_lowercase().contains("2 + 2")
        {
            "2 + 2 equals 4.".to_string()
        } else if question.to_lowercase().contains("hello") {
            "Hello! How can I assist you today?".to_string()
        } else if question.to_lowercase().contains("weather") {
            "I don't have access to real-time weather data, but I can help you understand weather patterns!".to_string()
        } else {
            format!(
                "I understand you're asking about: {}. Let me help you with that.",
                question
            )
        };

        Ok(CreateMessageResult {
            role: Role::Assistant,
            content: Content::Text(TextContent {
                text: response_text,
                annotations: None,
                meta: None,
            }),
            model: Some(self.model_name.clone()),
            stop_reason: Some("complete".to_string()),
        })
    }
}
