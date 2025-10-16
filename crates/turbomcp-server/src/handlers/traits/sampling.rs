//! Sampling handler trait for processing sampling requests

use async_trait::async_trait;
use turbomcp_protocol::RequestContext;
use turbomcp_protocol::types::{CreateMessageRequest, CreateMessageResult, SamplingCapabilities};

use crate::ServerResult;

/// Sampling handler trait for processing sampling requests
#[async_trait]
pub trait SamplingHandler: Send + Sync {
    /// Handle a sampling request
    ///
    /// # Arguments
    ///
    /// * `request_id` - The JSON-RPC request ID for response correlation
    /// * `request` - The sampling request parameters
    /// * `ctx` - The request context
    async fn handle(
        &self,
        request_id: String,
        request: CreateMessageRequest,
        ctx: RequestContext,
    ) -> ServerResult<CreateMessageResult>;

    /// Get supported sampling capabilities
    fn sampling_capabilities(&self) -> SamplingCapabilities {
        SamplingCapabilities
    }
}
