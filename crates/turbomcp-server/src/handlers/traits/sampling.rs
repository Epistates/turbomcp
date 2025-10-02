//! Sampling handler trait for processing sampling requests

use async_trait::async_trait;
use turbomcp_core::RequestContext;
use turbomcp_protocol::types::{CreateMessageRequest, CreateMessageResult, SamplingCapabilities};

use crate::ServerResult;

/// Sampling handler trait for processing sampling requests
#[async_trait]
pub trait SamplingHandler: Send + Sync {
    /// Handle a sampling request
    async fn handle(
        &self,
        request: CreateMessageRequest,
        ctx: RequestContext,
    ) -> ServerResult<CreateMessageResult>;

    /// Get supported sampling capabilities
    fn sampling_capabilities(&self) -> SamplingCapabilities {
        SamplingCapabilities
    }
}
