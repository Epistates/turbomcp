//! Client-side sampling support for handling server-initiated requests
//!
//! This module provides the handler for processing sampling requests from servers,
//! enabling bidirectional LLM interactions in the MCP protocol.

use async_trait::async_trait;
use turbomcp_protocol::types::{
    Content, CreateMessageRequest, CreateMessageResult, Role, TextContent,
};

/// Handler for server-initiated sampling requests
///
/// Implement this trait to handle sampling requests from MCP servers.
/// The handler receives a `CreateMessageRequest` and must return a response
/// with the generated content.
///
/// # Examples
///
/// ```rust,no_run
/// use turbomcp_client::sampling::{SamplingHandler, DefaultSamplingHandler};
/// use turbomcp_protocol::types::{CreateMessageRequest, CreateMessageResult, Role, Content, TextContent};
///
/// #[derive(Debug)]
/// struct MySamplingHandler;
///
/// #[async_trait::async_trait]
/// impl SamplingHandler for MySamplingHandler {
///     async fn handle_create_message(
///         &self,
///         request: CreateMessageRequest,
///     ) -> Result<CreateMessageResult, Box<dyn std::error::Error + Send + Sync>> {
///         // Process the request with your LLM
///         let response = CreateMessageResult {
///             role: Role::Assistant,
///             content: Content::Text(TextContent {
///                 text: "I can help with that!".to_string(),
///                 annotations: None,
///                 meta: None,
///             }),
///             model: Some("my-model".to_string()),
///             stop_reason: Some("complete".to_string()),
///         };
///         Ok(response)
///     }
/// }
/// ```
#[async_trait]
pub trait SamplingHandler: Send + Sync + std::fmt::Debug {
    /// Handle a sampling/createMessage request from the server
    async fn handle_create_message(
        &self,
        request: CreateMessageRequest,
    ) -> Result<CreateMessageResult, Box<dyn std::error::Error + Send + Sync>>;
}

/// Default sampling handler that returns a simple response
///
/// This handler can be used for testing or as a placeholder.
/// In production, you should implement your own handler that
/// integrates with your LLM backend.
#[derive(Debug, Clone)]
pub struct DefaultSamplingHandler;

#[async_trait]
impl SamplingHandler for DefaultSamplingHandler {
    async fn handle_create_message(
        &self,
        request: CreateMessageRequest,
    ) -> Result<CreateMessageResult, Box<dyn std::error::Error + Send + Sync>> {
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

        // Generate a simple response
        let response_text = format!(
            "I received your message: '{}'. This is a default response from TurboMCP client. \
             In production, this would be processed by your LLM backend.",
            user_message
        );

        Ok(CreateMessageResult {
            role: Role::Assistant,
            content: Content::Text(TextContent {
                text: response_text,
                annotations: None,
                meta: None,
            }),
            model: Some("turbomcp-default".to_string()),
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
