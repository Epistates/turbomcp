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
type BoxSamplingFuture<'a, T> =
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
    fn get_server_info(&self) -> BoxSamplingFuture<'_, ServerInfo>;
}

/// Interface for user interaction (human-in-the-loop)
pub trait UserInteractionHandler: Send + Sync + std::fmt::Debug {
    /// Present sampling request to user for approval
    fn approve_request(&self, request: &CreateMessageRequest) -> BoxSamplingFuture<'_, bool>;

    /// Present result to user for review
    fn approve_response(
        &self,
        request: &CreateMessageRequest,
        response: &CreateMessageResult,
    ) -> BoxSamplingFuture<'_, Option<CreateMessageResult>>;
}

/// Server information for model selection
#[derive(Debug, Clone)]
pub struct ServerInfo {
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

    /// Select best LLM client based on model preferences
    async fn select_llm_client(
        &self,
        _request: &CreateMessageRequest,
    ) -> Result<Arc<dyn LLMServerClient>, Box<dyn std::error::Error + Send + Sync>> {
        // This is where the intelligence goes - matching model preferences
        // to available LLM servers, exactly as the MCP spec describes

        if let Some(first_client) = self.llm_clients.first() {
            Ok(first_client.clone())
        } else {
            // FIXED: Return HandlerError::Configuration instead of string error
            // This ensures proper error code mapping (-32601)
            Err(Box::new(crate::handlers::HandlerError::Configuration {
                message: "No LLM servers configured".to_string(),
            }))
        }
    }
}

/// Default user handler that automatically approves (for development)
#[derive(Debug)]
pub struct AutoApprovingUserHandler;

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
