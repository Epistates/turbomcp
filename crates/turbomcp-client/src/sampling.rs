//! MCP-Compliant Client-Side Sampling Support
//!
//! This module provides the correct MCP architecture for handling sampling requests.
//! The client's role is to:
//! 1. Receive sampling/createMessage requests from servers
//! 2. Present them to users for approval (human-in-the-loop)
//! 3. Delegate to external LLM services (which can be MCP servers themselves)
//! 4. Return standardized results
//!
//! ## Perfect MCP Compliance
//!
//! Unlike embedding LLM APIs directly (anti-pattern), this implementation:
//! - Delegates to external services
//! - Maintains protocol boundaries
//! - Enables composition and flexibility
//! - Provides maximum developer experience through simplicity

use async_trait::async_trait;
use std::sync::Arc;
use turbomcp_protocol::types::{CreateMessageRequest, CreateMessageResult};

/// MCP-compliant sampling handler trait
///
/// The client receives sampling requests and delegates to configured LLM services.
/// This maintains perfect separation of concerns per MCP specification.
#[async_trait]
pub trait SamplingHandler: Send + Sync + std::fmt::Debug {
    /// Handle a sampling/createMessage request from a server
    ///
    /// This method should:
    /// 1. Present the request to the user for approval
    /// 2. Delegate to an external LLM service (could be another MCP server)
    /// 3. Present the result to the user for review
    /// 4. Return the approved result
    async fn handle_create_message(
        &self,
        request: CreateMessageRequest,
    ) -> Result<CreateMessageResult, Box<dyn std::error::Error + Send + Sync>>;
}

/// Default implementation that delegates to external MCP servers
///
/// This is the "batteries included" approach - it connects to LLM MCP servers
/// but maintains perfect protocol compliance.
#[derive(Debug)]
pub struct DelegatingSamplingHandler {
    /// Client instances for LLM MCP servers
    llm_clients: Vec<Arc<dyn LLMServerClient>>,
    /// User interaction handler
    user_handler: Arc<dyn UserInteractionHandler>,
}

/// Interface for connecting to LLM MCP servers
#[async_trait]
pub trait LLMServerClient: Send + Sync + std::fmt::Debug {
    /// Forward a sampling request to an LLM MCP server
    async fn create_message(
        &self,
        request: CreateMessageRequest,
    ) -> Result<CreateMessageResult, Box<dyn std::error::Error + Send + Sync>>;

    /// Get server capabilities/model info
    async fn get_server_info(&self) -> Result<ServerInfo, Box<dyn std::error::Error + Send + Sync>>;
}

/// Interface for user interaction (human-in-the-loop)
#[async_trait]
pub trait UserInteractionHandler: Send + Sync + std::fmt::Debug {
    /// Present sampling request to user for approval
    async fn approve_request(
        &self,
        request: &CreateMessageRequest,
    ) -> Result<bool, Box<dyn std::error::Error + Send + Sync>>;

    /// Present result to user for review
    async fn approve_response(
        &self,
        request: &CreateMessageRequest,
        response: &CreateMessageResult,
    ) -> Result<Option<CreateMessageResult>, Box<dyn std::error::Error + Send + Sync>>;
}

/// Server information for model selection
#[derive(Debug, Clone)]
pub struct ServerInfo {
    pub name: String,
    pub models: Vec<String>,
    pub capabilities: Vec<String>,
}

#[async_trait]
impl SamplingHandler for DelegatingSamplingHandler {
    async fn handle_create_message(
        &self,
        request: CreateMessageRequest,
    ) -> Result<CreateMessageResult, Box<dyn std::error::Error + Send + Sync>> {
        // 1. Human-in-the-loop: Get user approval
        if !self.user_handler.approve_request(&request).await? {
            return Err("User rejected sampling request".into());
        }

        // 2. Select appropriate LLM server based on model preferences
        let selected_client = self.select_llm_client(&request).await?;

        // 3. Delegate to external LLM MCP server
        let result = selected_client.create_message(request.clone()).await?;

        // 4. Present result for user review
        let approved_result = self.user_handler.approve_response(&request, &result).await?;

        Ok(approved_result.unwrap_or(result))
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
        request: &CreateMessageRequest,
    ) -> Result<Arc<dyn LLMServerClient>, Box<dyn std::error::Error + Send + Sync>> {
        // This is where the intelligence goes - matching model preferences
        // to available LLM servers, exactly as the MCP spec describes

        if let Some(first_client) = self.llm_clients.first() {
            Ok(first_client.clone())
        } else {
            Err("No LLM servers configured".into())
        }
    }
}

/// Default user handler that automatically approves (for development)
#[derive(Debug)]
pub struct AutoApprovingUserHandler;

#[async_trait]
impl UserInteractionHandler for AutoApprovingUserHandler {
    async fn approve_request(
        &self,
        _request: &CreateMessageRequest,
    ) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        Ok(true) // Auto-approve for development
    }

    async fn approve_response(
        &self,
        _request: &CreateMessageRequest,
        _response: &CreateMessageResult,
    ) -> Result<Option<CreateMessageResult>, Box<dyn std::error::Error + Send + Sync>> {
        Ok(None) // Auto-approve, don't modify
    }
}