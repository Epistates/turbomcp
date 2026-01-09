//! Tracing middleware for MCP client.
//!
//! Tower Layer that adds distributed tracing spans for all MCP requests.
//! Integrates with the `tracing` ecosystem for observability.
//!
//! ## Usage
//!
//! ```rust,ignore
//! use turbomcp_client::middleware::TracingLayer;
//! use tower::ServiceBuilder;
//!
//! let service = ServiceBuilder::new()
//!     .layer(TracingLayer::new())
//!     .service(inner_service);
//! ```

use super::request::{McpRequest, McpResponse};
use futures_util::future::BoxFuture;
use std::task::{Context, Poll};
use tower_layer::Layer;
use tower_service::Service;
use tracing::{Instrument, Span, field, info_span};
use turbomcp_protocol::McpError;

/// Tower Layer that adds tracing spans.
#[derive(Debug, Clone, Copy, Default)]
pub struct TracingLayer;

impl TracingLayer {
    /// Create a new tracing layer.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl<S> Layer<S> for TracingLayer {
    type Service = TracingService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        TracingService { inner }
    }
}

/// Tower Service that adds tracing spans.
#[derive(Debug, Clone)]
pub struct TracingService<S> {
    inner: S,
}

impl<S> TracingService<S> {
    /// Get a reference to the inner service.
    pub fn inner(&self) -> &S {
        &self.inner
    }

    /// Get a mutable reference to the inner service.
    pub fn inner_mut(&mut self) -> &mut S {
        &mut self.inner
    }
}

impl<S> Service<McpRequest> for TracingService<S>
where
    S: Service<McpRequest, Response = McpResponse> + Clone + Send + 'static,
    S::Future: Send,
    S::Error: Into<McpError>,
{
    type Response = McpResponse;
    type Error = McpError;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx).map_err(Into::into)
    }

    fn call(&mut self, req: McpRequest) -> Self::Future {
        let method = req.method().to_string();
        let request_id = req.id().to_string();

        // Create span with fields that will be filled in later
        let span = info_span!(
            "mcp.request",
            method = %method,
            request_id = %request_id,
            success = field::Empty,
            duration_ms = field::Empty,
        );

        let mut inner = self.inner.clone();
        std::mem::swap(&mut self.inner, &mut inner);

        Box::pin(
            async move {
                let result = inner.call(req).await.map_err(Into::into);

                // Record outcome in span
                match &result {
                    Ok(response) => {
                        Span::current().record("success", response.is_success());
                        Span::current().record("duration_ms", response.duration.as_millis() as u64);
                    }
                    Err(_) => {
                        Span::current().record("success", false);
                    }
                }

                result
            }
            .instrument(span),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::time::Duration;
    use turbomcp_protocol::MessageId;
    use turbomcp_protocol::jsonrpc::{JsonRpcRequest, JsonRpcVersion};

    fn test_request() -> McpRequest {
        McpRequest::new(JsonRpcRequest {
            jsonrpc: JsonRpcVersion,
            id: MessageId::from("test-1"),
            method: "test/method".to_string(),
            params: None,
        })
    }

    #[tokio::test]
    async fn test_tracing_layer() {
        use tower::ServiceExt;

        let mock_service = tower::service_fn(|_req: McpRequest| async {
            Ok::<_, McpError>(McpResponse::success(
                json!({"result": "ok"}),
                Duration::from_millis(10),
            ))
        });

        let mut service = TracingLayer::new().layer(mock_service);

        let response = service
            .ready()
            .await
            .unwrap()
            .call(test_request())
            .await
            .unwrap();

        assert!(response.is_success());
    }

    #[test]
    fn test_tracing_layer_creation() {
        let _layer = TracingLayer::new();
        // TracingLayer is a zero-sized type with no configuration
    }
}
