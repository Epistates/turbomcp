//! Server-initiated sampling support for TurboMCP
//!
//! This module provides helper functions for tools to make sampling requests
//! to clients, enabling server-initiated LLM interactions.

use crate::{McpError, ServerResult};
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
        let capabilities = self.server_to_client().ok_or_else(|| {
            McpError::internal("No server capabilities available for sampling requests")
                .with_operation("sampling")
        })?;

        // Fully typed - no serialization needed!
        capabilities
            .create_message(request, self.clone())
            .await
            .map_err(|e| {
                McpError::internal(format!("Sampling request failed: {}", e))
                    .with_operation("sampling")
            })
    }

    async fn elicit(&self, request: ElicitRequest) -> ServerResult<ElicitResult> {
        let capabilities = self.server_to_client().ok_or_else(|| {
            McpError::internal("No server capabilities available for elicitation requests")
                .with_operation("elicitation")
        })?;

        // Fully typed - no serialization needed!
        capabilities
            .elicit(request, self.clone())
            .await
            .map_err(|e| {
                McpError::internal(format!("Elicitation request failed: {}", e))
                    .with_operation("elicitation")
            })
    }

    async fn list_roots(&self) -> ServerResult<ListRootsResult> {
        let capabilities = self.server_to_client().ok_or_else(|| {
            McpError::internal("No server capabilities available for roots listing")
                .with_operation("roots")
        })?;

        // Fully typed - no serialization needed!
        capabilities
            .list_roots(self.clone())
            .await
            .map_err(|e| {
                McpError::internal(format!("Roots listing failed: {}", e)).with_operation("roots")
            })
    }
}
