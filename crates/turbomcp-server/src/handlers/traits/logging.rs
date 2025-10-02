//! Logging handler trait for processing logging requests

use async_trait::async_trait;
use turbomcp_core::RequestContext;
use turbomcp_protocol::LogLevel;
use turbomcp_protocol::types::{EmptyResult, LoggingCapabilities, SetLevelRequest};

use crate::ServerResult;

/// Logging handler trait for processing logging requests
#[async_trait]
pub trait LoggingHandler: Send + Sync {
    /// Handle a log level change request
    async fn handle(
        &self,
        request: SetLevelRequest,
        ctx: RequestContext,
    ) -> ServerResult<EmptyResult>;

    /// Get current log level
    fn current_level(&self) -> LogLevel;

    /// Get logging capabilities
    fn logging_capabilities(&self) -> LoggingCapabilities {
        LoggingCapabilities
    }
}
