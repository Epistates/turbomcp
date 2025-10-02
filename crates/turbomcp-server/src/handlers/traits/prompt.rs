//! Prompt handler trait for processing prompt requests

use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use turbomcp_core::RequestContext;
use turbomcp_protocol::types::{GetPromptRequest, GetPromptResult, Prompt};

use crate::ServerResult;

/// Prompt handler trait for processing prompt requests
#[async_trait]
pub trait PromptHandler: Send + Sync {
    /// Handle a prompt request
    async fn handle(
        &self,
        request: GetPromptRequest,
        ctx: RequestContext,
    ) -> ServerResult<GetPromptResult>;

    /// Get the prompt definition
    fn prompt_definition(&self) -> Prompt;

    /// Validate prompt arguments (optional, default implementation allows all)
    fn validate_arguments(&self, _args: &HashMap<String, Value>) -> ServerResult<()> {
        Ok(())
    }
}
