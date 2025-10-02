//! Elicitation handler trait for server-initiated user input requests

use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use turbomcp_core::RequestContext;
use turbomcp_protocol::types::{ElicitRequest, ElicitResult};

use crate::ServerResult;

/// Elicitation handler trait for server-initiated user input requests
#[async_trait]
pub trait ElicitationHandler: Send + Sync {
    /// Handle an elicitation request (server-initiated user input)
    async fn handle(
        &self,
        request: ElicitRequest,
        ctx: RequestContext,
    ) -> ServerResult<ElicitResult>;

    /// Validate elicitation schema (optional, default implementation allows all)
    fn validate_schema(&self, _schema: &Value) -> ServerResult<()> {
        Ok(())
    }

    /// Get default timeout for user response in milliseconds
    fn default_timeout_ms(&self) -> u64 {
        60_000 // 1 minute default
    }

    /// Check if elicitation is cancellable
    fn is_cancellable(&self) -> bool {
        true
    }

    /// Handle elicitation cancellation
    async fn handle_cancellation(
        &self,
        _request_id: &str,
        _ctx: RequestContext,
    ) -> ServerResult<()> {
        Ok(())
    }

    /// Process user response for validation
    async fn process_response(
        &self,
        _response: &HashMap<String, Value>,
        _schema: &Value,
        _ctx: RequestContext,
    ) -> ServerResult<HashMap<String, Value>> {
        // Default implementation returns response as-is
        Ok(_response.clone())
    }
}
