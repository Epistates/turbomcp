//! Ping handler trait for bidirectional health monitoring

use async_trait::async_trait;
use serde_json::Value;
use turbomcp_protocol::RequestContext;
use turbomcp_protocol::types::{PingRequest, PingResult};

use crate::ServerResult;

/// Ping handler trait for bidirectional health monitoring
#[async_trait]
pub trait PingHandler: Send + Sync {
    /// Handle a ping request
    async fn handle(&self, request: PingRequest, ctx: RequestContext) -> ServerResult<PingResult>;

    /// Get current health status
    async fn get_health_status(&self, _ctx: RequestContext) -> ServerResult<Value> {
        Ok(serde_json::json!({
            "status": "healthy",
            "timestamp": chrono::Utc::now().to_rfc3339(),
        }))
    }

    /// Get connection metrics if available
    async fn get_connection_metrics(&self, _ctx: RequestContext) -> ServerResult<Option<Value>> {
        Ok(None) // Default: no metrics
    }

    /// Handle ping timeout
    async fn handle_timeout(&self, _request_id: &str, _ctx: RequestContext) -> ServerResult<()> {
        Ok(())
    }

    /// Check if ping should include detailed health information
    fn include_health_details(&self) -> bool {
        false
    }

    /// Get expected response time threshold in milliseconds
    fn response_threshold_ms(&self) -> u64 {
        5_000 // 5 seconds default
    }

    /// Process custom ping payload
    async fn process_ping_payload(
        &self,
        payload: Option<&Value>,
        _ctx: RequestContext,
    ) -> ServerResult<Option<Value>> {
        // Default: echo back the payload
        Ok(payload.cloned())
    }
}
