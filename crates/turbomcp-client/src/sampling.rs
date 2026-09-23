//! MCP-Compliant Client-Side Sampling Support
//!
//! This module provides the correct MCP architecture for handling sampling requests.
//! The client's role is to:
//! 1. Receive sampling/createMessage requests from servers
//! 2. Present them to users for approval (human-in-the-loop)
//! 3. Delegate to external LLM services (which can be MCP servers themselves)
//! 4. Return standardized results
//!
//! ## MCP Compliance
//!
//! Unlike embedding LLM APIs directly (anti-pattern), this implementation:
//! - Delegates to external services
//! - Maintains protocol boundaries
//! - Enables composition and flexibility
//! - Provides maximum developer experience through simplicity

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use turbomcp_protocol::types::{CreateMessageRequest, CreateMessageResult};

/// Boxed future type alias for sampling operations
pub type BoxSamplingFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, Box<dyn std::error::Error + Send + Sync>>> + Send + 'a>>;

/// MCP-compliant sampling handler trait
///
/// The client receives sampling requests and delegates to configured LLM services.
/// This maintains separation of concerns per MCP specification.
pub trait SamplingHandler: Send + Sync + std::fmt::Debug {
    /// Handle a sampling/createMessage request from a server
    ///
    /// This method should:
    /// 1. Present the request to the user for approval
    /// 2. Delegate to an external LLM service (could be another MCP server)
    /// 3. Present the result to the user for review
    /// 4. Return the approved result
    ///
    /// # Arguments
    ///
    /// * `request_id` - The JSON-RPC request ID from the server for proper response correlation
    /// * `request` - The sampling request parameters
    fn handle_create_message(
        &self,
        request_id: String,
        request: CreateMessageRequest,
    ) -> BoxSamplingFuture<'_, CreateMessageResult>;
}

/// Default implementation that delegates to external MCP servers
///
/// This is the "batteries included" approach - it connects to LLM MCP servers
/// but maintains protocol compliance.
#[derive(Debug)]
pub struct DelegatingSamplingHandler {
    /// Client instances for LLM MCP servers
    llm_clients: Vec<Arc<dyn LLMServerClient>>,
    /// User interaction handler
    user_handler: Arc<dyn UserInteractionHandler>,
}

/// Interface for connecting to LLM MCP servers
pub trait LLMServerClient: Send + Sync + std::fmt::Debug {
    /// Forward a sampling request to an LLM MCP server
    fn create_message(
        &self,
        request: CreateMessageRequest,
    ) -> BoxSamplingFuture<'_, CreateMessageResult>;

    /// Get server capabilities/model info
    fn get_server_info(&self) -> BoxSamplingFuture<'_, LlmServerInfo>;
}

/// Interface for user interaction (human-in-the-loop)
pub trait UserInteractionHandler: Send + Sync + std::fmt::Debug {
    /// Present sampling request to user for approval
    fn approve_request(&self, request: &CreateMessageRequest) -> BoxSamplingFuture<'_, bool>;

    /// Present result to user for review
    ///
    /// Return `Some(result)` to send an edited result in place of the one the
    /// LLM produced, or `None` to send it unmodified. Neither rejects it: to
    /// refuse, return an error — [`HandlerError::UserCancelled`] answers the
    /// server with the spec's `-1`.
    ///
    /// [`HandlerError::UserCancelled`]: crate::handlers::HandlerError::UserCancelled
    fn approve_response(
        &self,
        request: &CreateMessageRequest,
        response: &CreateMessageResult,
    ) -> BoxSamplingFuture<'_, Option<CreateMessageResult>>;
}

/// LLM-server descriptor used by sampling handlers for model selection.
#[derive(Debug, Clone)]
pub struct LlmServerInfo {
    pub name: String,
    pub models: Vec<String>,
    pub capabilities: Vec<String>,
}

impl SamplingHandler for DelegatingSamplingHandler {
    fn handle_create_message(
        &self,
        _request_id: String,
        request: CreateMessageRequest,
    ) -> BoxSamplingFuture<'_, CreateMessageResult> {
        Box::pin(async move {
            // 1. Human-in-the-loop: Get user approval
            if !self.user_handler.approve_request(&request).await? {
                // FIXED: Return HandlerError::UserCancelled (code -1) instead of string error
                // This ensures the error code is preserved when sent back to the server
                return Err(Box::new(crate::handlers::HandlerError::UserCancelled)
                    as Box<dyn std::error::Error + Send + Sync>);
            }

            // 2. Select appropriate LLM server based on model preferences
            let selected_client = self.select_llm_client(&request).await?;

            // 3. Delegate to external LLM MCP server
            let result = selected_client.create_message(request.clone()).await?;

            // 4. Present result for user review
            let approved_result = self
                .user_handler
                .approve_response(&request, &result)
                .await?;

            Ok(approved_result.unwrap_or(result))
        })
    }
}

impl DelegatingSamplingHandler {
    /// Create new handler with LLM server clients
    pub fn new(
        llm_clients: Vec<Arc<dyn LLMServerClient>>,
        user_handler: Arc<dyn UserInteractionHandler>,
    ) -> Self {
        Self {
            llm_clients,
            user_handler,
        }
    }

    /// Select the LLM client to delegate to, honouring `modelPreferences.hints`.
    ///
    /// sampling.mdx: hints are substrings matched against model names,
    /// evaluated in order, the first match winning. The first client serving
    /// a model that matches the earliest matching hint is chosen; with no
    /// hints, or none that match, the first client is. Hints are advisory, so
    /// a client whose server info cannot be fetched is skipped, not fatal.
    async fn select_llm_client(
        &self,
        request: &CreateMessageRequest,
    ) -> Result<Arc<dyn LLMServerClient>, Box<dyn std::error::Error + Send + Sync>> {
        let Some(first_client) = self.llm_clients.first() else {
            // HandlerError::Configuration maps to -32601 on the wire.
            return Err(Box::new(crate::handlers::HandlerError::Configuration {
                message: "No LLM servers configured".to_string(),
            }));
        };

        let hints: Vec<&str> = request
            .model_preferences
            .iter()
            .flat_map(|prefs| prefs.hints.iter().flatten())
            .filter_map(|hint| hint.name.as_deref())
            .collect();
        if hints.is_empty() || self.llm_clients.len() == 1 {
            return Ok(first_client.clone());
        }

        let mut served = Vec::with_capacity(self.llm_clients.len());
        for client in &self.llm_clients {
            match client.get_server_info().await {
                Ok(info) => served.push((client, info.models)),
                Err(e) => tracing::debug!("Skipping LLM server for hint matching: {e}"),
            }
        }

        let chosen = hints.iter().find_map(|hint| {
            served
                .iter()
                .find(|(_, models)| models.iter().any(|model| model.contains(hint)))
                .map(|(client, _)| Arc::clone(client))
        });
        Ok(chosen.unwrap_or_else(|| first_client.clone()))
    }
}

/// **Development-only** user handler that auto-approves every sampling
/// request and every response without prompting.
///
/// MCP specifies that sampling MUST have human-in-the-loop approval
/// (`schema.ts:2316-2333` security note). This implementation defeats
/// that — use it for tests, demos, and local CLI tools, never in a
/// deployed agent that processes untrusted prompts. Logs a warning at
/// construction time so the choice shows up in operator-visible output.
#[derive(Debug)]
pub struct AutoApprovingUserHandler;

impl AutoApprovingUserHandler {
    /// Construct an auto-approving handler. Emits a `tracing::warn!`
    /// to make the unsafe-by-default behavior auditable in deployed logs.
    #[must_use]
    pub fn new() -> Self {
        tracing::warn!(
            "AutoApprovingUserHandler constructed; sampling requests will be \
             approved without human review. Do not use in production agents."
        );
        Self
    }
}

impl Default for AutoApprovingUserHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl UserInteractionHandler for AutoApprovingUserHandler {
    fn approve_request(&self, _request: &CreateMessageRequest) -> BoxSamplingFuture<'_, bool> {
        Box::pin(async move {
            Ok(true) // Auto-approve for development
        })
    }

    fn approve_response(
        &self,
        _request: &CreateMessageRequest,
        _response: &CreateMessageResult,
    ) -> BoxSamplingFuture<'_, Option<CreateMessageResult>> {
        Box::pin(async move {
            Ok(None) // Auto-approve, don't modify
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use turbomcp_protocol::types::{ModelHint, ModelPreferences};

    /// An LLM server that serves one model and answers with its name.
    #[derive(Debug)]
    struct Serving(&'static str);

    impl LLMServerClient for Serving {
        fn create_message(
            &self,
            _request: CreateMessageRequest,
        ) -> BoxSamplingFuture<'_, CreateMessageResult> {
            Box::pin(async move {
                Ok(serde_json::from_value(serde_json::json!({
                    "role": "assistant",
                    "content": { "type": "text", "text": "hi" },
                    "model": self.0
                }))?)
            })
        }

        fn get_server_info(&self) -> BoxSamplingFuture<'_, LlmServerInfo> {
            Box::pin(async move {
                Ok(LlmServerInfo {
                    name: self.0.to_string(),
                    models: vec![self.0.to_string()],
                    capabilities: Vec::new(),
                })
            })
        }
    }

    fn hinted(hints: &[&str]) -> CreateMessageRequest {
        CreateMessageRequest {
            max_tokens: 16,
            model_preferences: Some(ModelPreferences {
                hints: Some(
                    hints
                        .iter()
                        .map(|name| ModelHint {
                            name: Some((*name).to_string()),
                        })
                        .collect(),
                ),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    async fn model_chosen_for(request: CreateMessageRequest) -> String {
        let handler = DelegatingSamplingHandler::new(
            vec![
                Arc::new(Serving("gpt-4o")),
                Arc::new(Serving("claude-3-5-sonnet")),
            ],
            Arc::new(AutoApprovingUserHandler),
        );
        handler
            .handle_create_message("1".to_string(), request)
            .await
            .expect("sampling succeeds")
            .model
    }

    /// sampling.mdx: hints are substrings of model names, tried in order.
    /// The first configured server used to be chosen whatever the hints said.
    #[tokio::test]
    async fn model_hints_choose_the_server() {
        assert_eq!(
            model_chosen_for(hinted(&["sonnet"])).await,
            "claude-3-5-sonnet"
        );
        assert_eq!(
            model_chosen_for(hinted(&["gemini", "gpt"])).await,
            "gpt-4o",
            "an unmatched hint falls through to the next"
        );
        assert_eq!(
            model_chosen_for(hinted(&["gemini"])).await,
            "gpt-4o",
            "no match falls back to the first server"
        );
        assert_eq!(
            model_chosen_for(CreateMessageRequest::default()).await,
            "gpt-4o"
        );
    }
}
