//! Tower Layer integration for gRPC transport
//!
//! This module provides Tower Layer/Service implementations for composable
//! middleware with the gRPC transport.

use std::task::{Context, Poll};
use std::time::Instant;
use tower::{Layer, Service};
use tracing::{Instrument, debug, info_span};

/// Tower Layer for MCP gRPC services
///
/// Provides request/response logging, timing, and metadata handling.
#[derive(Debug, Clone)]
pub struct McpGrpcLayer {
    /// Enable request/response logging
    logging: bool,
    /// Enable timing metrics
    timing: bool,
}

impl McpGrpcLayer {
    /// Create a new layer with default settings
    #[must_use]
    pub fn new() -> Self {
        Self {
            logging: true,
            timing: true,
        }
    }

    /// Enable or disable logging
    #[must_use]
    pub fn logging(mut self, enabled: bool) -> Self {
        self.logging = enabled;
        self
    }

    /// Enable or disable timing metrics
    #[must_use]
    pub fn timing(mut self, enabled: bool) -> Self {
        self.timing = enabled;
        self
    }
}

impl Default for McpGrpcLayer {
    fn default() -> Self {
        Self::new()
    }
}

impl<S> Layer<S> for McpGrpcLayer {
    type Service = McpGrpcService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        McpGrpcService {
            inner,
            logging: self.logging,
            timing: self.timing,
        }
    }
}

/// Tower Service wrapper for MCP gRPC
#[derive(Debug, Clone)]
pub struct McpGrpcService<S> {
    inner: S,
    logging: bool,
    timing: bool,
}

impl<S, ReqBody, ResBody> Service<http::Request<ReqBody>> for McpGrpcService<S>
where
    S: Service<http::Request<ReqBody>, Response = http::Response<ResBody>> + Clone + Send + 'static,
    S::Future: Send,
    ReqBody: Send + 'static,
    ResBody: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>,
    >;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: http::Request<ReqBody>) -> Self::Future {
        let mut inner = self.inner.clone();
        let logging = self.logging;
        let timing = self.timing;

        // Extract method for logging
        let method = request
            .uri()
            .path()
            .split('/')
            .next_back()
            .unwrap_or("unknown")
            .to_string();

        // Clone for the span (method is moved into the async block)
        let span_method = method.clone();

        Box::pin(
            async move {
                let start = if timing { Some(Instant::now()) } else { None };

                if logging {
                    debug!(method = %method, "gRPC request");
                }

                let response = inner.call(request).await;

                if let Some(start) = start {
                    let elapsed = start.elapsed();
                    debug!(
                        method = %method,
                        duration_ms = %elapsed.as_millis(),
                        "gRPC response"
                    );
                }

                response
            }
            .instrument(info_span!("grpc_request", method = %span_method)),
        )
    }
}

/// Interceptor for adding metadata to gRPC requests
#[derive(Debug, Clone)]
pub struct MetadataInterceptor {
    /// Custom metadata to add to requests
    metadata: Vec<(String, String)>,
}

impl MetadataInterceptor {
    /// Create a new metadata interceptor
    #[must_use]
    pub fn new() -> Self {
        Self {
            metadata: Vec::new(),
        }
    }

    /// Add a metadata key-value pair
    #[must_use]
    pub fn add(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.push((key.into(), value.into()));
        self
    }

    /// Create a tonic interceptor function
    pub fn into_interceptor(
        self,
    ) -> impl Fn(tonic::Request<()>) -> Result<tonic::Request<()>, tonic::Status> + Clone {
        move |mut req: tonic::Request<()>| {
            for (key, value) in &self.metadata {
                if let Ok(key) = tonic::metadata::MetadataKey::from_bytes(key.as_bytes())
                    && let Ok(value) = value.parse()
                {
                    req.metadata_mut().insert(key, value);
                }
            }
            Ok(req)
        }
    }
}

impl Default for MetadataInterceptor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_layer_builder() {
        let layer = McpGrpcLayer::new()
            .logging(true)
            .timing(true);

        assert!(layer.logging);
        assert!(layer.timing);
    }

    #[test]
    fn test_metadata_interceptor() {
        let interceptor = MetadataInterceptor::new()
            .add("x-request-id", "test-123")
            .add("x-client-id", "client-456");

        assert_eq!(interceptor.metadata.len(), 2);
    }
}
