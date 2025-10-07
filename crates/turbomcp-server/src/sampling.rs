//! Server-initiated sampling support for TurboMCP
//!
//! This module provides helper functions for tools to make sampling requests
//! to clients, enabling server-initiated LLM interactions.

use crate::{ServerError, ServerResult};
use turbomcp_protocol::RequestContext;
use turbomcp_protocol::types::{
    CreateMessageRequest, CreateMessageResult, ElicitRequest, ElicitResult, ListRootsResult,
};

/// Extension trait for RequestContext to provide sampling capabilities
///
/// Note: We use `async-trait` to address the async fn in trait warning
#[async_trait::async_trait]
pub trait SamplingExt {
    /// Send a sampling/createMessage request to the client
    async fn create_message(
        &self,
        request: CreateMessageRequest,
    ) -> ServerResult<CreateMessageResult>;

    /// Send an elicitation request to the client for user input
    async fn elicit(&self, request: ElicitRequest) -> ServerResult<ElicitResult>;

    /// List client's root capabilities
    async fn list_roots(&self) -> ServerResult<ListRootsResult>;
}

#[async_trait::async_trait]
impl SamplingExt for RequestContext {
    async fn create_message(
        &self,
        request: CreateMessageRequest,
    ) -> ServerResult<CreateMessageResult> {
        let capabilities = self
            .server_to_client()
            .ok_or_else(|| ServerError::Handler {
                message: "No server capabilities available for sampling requests".to_string(),
                context: Some("sampling".to_string()),
            })?;

        // Fully typed - no serialization needed!
        capabilities
            .create_message(request, self.clone())
            .await
            .map_err(|e| ServerError::Handler {
                message: format!("Sampling request failed: {}", e),
                context: Some("sampling".to_string()),
            })
    }

    async fn elicit(&self, request: ElicitRequest) -> ServerResult<ElicitResult> {
        let capabilities = self
            .server_to_client()
            .ok_or_else(|| ServerError::Handler {
                message: "No server capabilities available for elicitation requests".to_string(),
                context: Some("elicitation".to_string()),
            })?;

        // Fully typed - no serialization needed!
        capabilities
            .elicit(request, self.clone())
            .await
            .map_err(|e| ServerError::Handler {
                message: format!("Elicitation request failed: {}", e),
                context: Some("elicitation".to_string()),
            })
    }

    async fn list_roots(&self) -> ServerResult<ListRootsResult> {
        let capabilities = self
            .server_to_client()
            .ok_or_else(|| ServerError::Handler {
                message: "No server capabilities available for roots listing".to_string(),
                context: Some("roots".to_string()),
            })?;

        // Fully typed - no serialization needed!
        capabilities
            .list_roots(self.clone())
            .await
            .map_err(|e| ServerError::Handler {
                message: format!("Roots listing failed: {}", e),
                context: Some("roots".to_string()),
            })
    }
}
