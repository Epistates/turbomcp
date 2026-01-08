//! Tower Service implementation for telemetry

use super::TelemetryLayerConfig;
use crate::attributes::McpSpanContext;
use crate::span_attributes::*;
use futures_util::future::BoxFuture;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Instant;
use tower_service::Service;
use tracing::{Instrument, Span, info};

/// Tower Service that instruments requests with telemetry
#[derive(Debug, Clone)]
pub struct TelemetryService<S> {
    inner: S,
    config: Arc<TelemetryLayerConfig>,
}

impl<S> TelemetryService<S> {
    /// Create a new telemetry service wrapping the inner service
    pub fn new(inner: S, config: Arc<TelemetryLayerConfig>) -> Self {
        Self { inner, config }
    }

    /// Get a reference to the inner service
    #[must_use]
    pub fn inner(&self) -> &S {
        &self.inner
    }

    /// Get a mutable reference to the inner service
    pub fn inner_mut(&mut self) -> &mut S {
        &mut self.inner
    }

    /// Get the configuration
    #[must_use]
    pub fn config(&self) -> &TelemetryLayerConfig {
        &self.config
    }
}

/// Future type for telemetry service responses
pub type TelemetryServiceFuture<F> = BoxFuture<'static, <F as Future>::Output>;

// Implementation for JSON-RPC requests (serde_json::Value)
impl<S> Service<serde_json::Value> for TelemetryService<S>
where
    S: Service<serde_json::Value, Response = serde_json::Value> + Clone + Send + 'static,
    S::Future: Send,
    S::Error: std::fmt::Display + Send,
{
    type Response = serde_json::Value;
    type Error = S::Error;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: serde_json::Value) -> Self::Future {
        let start = Instant::now();
        let config = Arc::clone(&self.config);

        // Extract method from request
        let method = req
            .get("method")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown")
            .to_string();

        // Extract request ID
        let request_id = req.get("id").map(|id| match id {
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::String(s) => s.clone(),
            _ => "unknown".to_string(),
        });

        // Check if we should instrument this method
        if !config.should_instrument(&method) {
            let inner = self.inner.clone();
            let mut inner = std::mem::replace(&mut self.inner, inner);
            return Box::pin(async move { inner.call(req).await });
        }

        // Extract additional context from request
        let tool_name = if method == "tools/call" {
            req.get("params")
                .and_then(|p| p.get("name"))
                .and_then(|n| n.as_str())
                .map(String::from)
        } else {
            None
        };

        let resource_uri = if method == "resources/read" {
            req.get("params")
                .and_then(|p| p.get("uri"))
                .and_then(|u| u.as_str())
                .map(String::from)
        } else {
            None
        };

        let prompt_name = if method == "prompts/get" {
            req.get("params")
                .and_then(|p| p.get("name"))
                .and_then(|n| n.as_str())
                .map(String::from)
        } else {
            None
        };

        // Build span context
        let mut span_ctx = McpSpanContext::new()
            .method(&method)
            .server(&config.service_name, &config.service_version);

        if let Some(ref id) = request_id {
            span_ctx = span_ctx.request_id(id);
        }
        if let Some(ref name) = tool_name {
            span_ctx = span_ctx.tool_name(name);
        }
        if let Some(ref uri) = resource_uri {
            span_ctx = span_ctx.resource_uri(uri);
        }
        if let Some(ref name) = prompt_name {
            span_ctx = span_ctx.prompt_name(name);
        }

        let span = span_ctx.into_span();

        // Calculate request size if configured
        let request_size = if config.record_sizes {
            Some(req.to_string().len())
        } else {
            None
        };

        let inner = self.inner.clone();
        let mut inner = std::mem::replace(&mut self.inner, inner);

        Box::pin(
            async move {
                let result = inner.call(req).await;
                let duration = start.elapsed();

                // Record completion
                let (success, error_msg) = match &result {
                    Ok(response) => {
                        // Check if response indicates error
                        let is_error = response.get("error").is_some();
                        if is_error {
                            let error_message = response
                                .get("error")
                                .and_then(|e| e.get("message"))
                                .and_then(|m| m.as_str())
                                .map(String::from);
                            (false, error_message)
                        } else {
                            (true, None)
                        }
                    }
                    Err(e) => (false, Some(e.to_string())),
                };

                // Log completion
                if config.record_timing {
                    let current_span = Span::current();
                    current_span.record(MCP_DURATION_MS, duration.as_millis() as i64);
                    current_span.record(MCP_STATUS, if success { "success" } else { "error" });

                    if let Some(ref err) = error_msg {
                        current_span.record(MCP_ERROR_MESSAGE, err.as_str());
                    }

                    info!(
                        method = %method,
                        duration_ms = duration.as_millis(),
                        success = success,
                        request_size = request_size,
                        "MCP request completed"
                    );
                }

                result
            }
            .instrument(span),
        )
    }
}

// Implementation for HTTP requests
impl<S, B> Service<http::Request<B>> for TelemetryService<S>
where
    S: Service<http::Request<B>> + Clone + Send + 'static,
    S::Response: Send,
    S::Future: Send,
    S::Error: std::fmt::Display + Send,
    B: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: http::Request<B>) -> Self::Future {
        let start = Instant::now();
        let config = Arc::clone(&self.config);

        // Extract method from path
        let path = req.uri().path();
        let method = path.strip_prefix('/').unwrap_or(path).to_string();

        // Check if we should instrument
        if !config.should_instrument(&method) {
            let inner = self.inner.clone();
            let mut inner = std::mem::replace(&mut self.inner, inner);
            return Box::pin(async move { inner.call(req).await });
        }

        // Build span
        let span = McpSpanContext::new()
            .method(&method)
            .transport("http")
            .server(&config.service_name, &config.service_version)
            .into_span();

        let inner = self.inner.clone();
        let mut inner = std::mem::replace(&mut self.inner, inner);

        Box::pin(
            async move {
                let result = inner.call(req).await;
                let duration = start.elapsed();

                let success = result.is_ok();

                if config.record_timing {
                    let current_span = Span::current();
                    current_span.record(MCP_DURATION_MS, duration.as_millis() as i64);
                    current_span.record(MCP_STATUS, if success { "success" } else { "error" });

                    info!(
                        method = %method,
                        duration_ms = duration.as_millis(),
                        success = success,
                        "HTTP request completed"
                    );
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

    #[test]
    fn test_service_creation() {
        // Create a mock service
        let config = Arc::new(TelemetryLayerConfig::default());

        // Just verify the config is accessible
        assert!(config.record_timing);
        assert!(config.record_sizes);
    }
}
