//! Tower Service implementation for telemetry

use super::TelemetryLayerConfig;
use crate::attributes::{McpSpanContext, record_completion};
use crate::span_attributes::MCP_ERROR_CODE;
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

    #[allow(clippy::too_many_lines)]
    fn call(&mut self, req: serde_json::Value) -> Self::Future {
        let start = Instant::now();
        let config = Arc::clone(&self.config);

        // Extract method from request
        let method = req
            .get("method")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown")
            .to_string();

        // Extract request ID (skip when redacted to avoid per-request cardinality)
        let request_id = if config.redact_request_id {
            None
        } else {
            req.get("id").map(|id| match id {
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::String(s) => s.clone(),
                _ => "unknown".to_string(),
            })
        };

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

        let resource_uri = if method == "resources/read" && !config.redact_resource_uri {
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

                // Record completion. A JSON-RPC error response is still `Ok`
                // at the service level, so look inside it for the error.
                let (success, error_code, error_msg) = match &result {
                    Ok(response) => match response.get("error") {
                        Some(error) => (
                            false,
                            error.get("code").and_then(serde_json::Value::as_i64),
                            error
                                .get("message")
                                .and_then(|m| m.as_str())
                                .map(String::from),
                        ),
                        None => (true, None, None),
                    },
                    Err(e) => (false, None, Some(e.to_string())),
                };

                // Log completion
                if config.record_timing {
                    let current_span = Span::current();
                    let error_msg = error_msg
                        .as_deref()
                        .map(|err| truncate_error_message(err, config.error_message_max_len))
                        .filter(|err| !err.is_empty());
                    record_completion(&current_span, duration, success, error_msg.as_deref());
                    if let Some(code) = error_code {
                        current_span.record(MCP_ERROR_CODE, code);
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
                    record_completion(&Span::current(), duration, success, None);

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

/// Bounded copy of a JSON-RPC error message for span recording. JSON-RPC
/// `error.message` can be arbitrary user-controlled or backend-leaked text;
/// the layer truncates it before exporting so OTel collectors don't ingest
/// kilobyte stack traces or echoed user input verbatim.
fn truncate_error_message(msg: &str, max_len: usize) -> std::borrow::Cow<'_, str> {
    if max_len == 0 {
        return std::borrow::Cow::Borrowed("");
    }
    if msg.len() <= max_len {
        return std::borrow::Cow::Borrowed(msg);
    }
    // Truncate at a UTF-8 char boundary, then append a marker.
    let mut end = max_len;
    while end > 0 && !msg.is_char_boundary(end) {
        end -= 1;
    }
    let mut out = String::with_capacity(end + 14);
    out.push_str(&msg[..end]);
    out.push_str("…[truncated]");
    std::borrow::Cow::Owned(out)
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

    #[test]
    fn test_truncate_error_message_short() {
        let msg = "boom";
        let out = truncate_error_message(msg, 512);
        assert_eq!(out, "boom");
    }

    #[test]
    fn test_truncate_error_message_long() {
        let msg = "x".repeat(1024);
        let out = truncate_error_message(&msg, 16);
        assert!(out.starts_with("xxxxxxxxxxxxxxxx"));
        assert!(out.ends_with("…[truncated]"));
    }

    #[test]
    fn test_truncate_error_message_zero_len() {
        let out = truncate_error_message("anything", 0);
        assert_eq!(out, "");
    }

    #[test]
    fn test_truncate_error_message_utf8_boundary() {
        // 'é' is two bytes; max_len 5 lands inside the 4th char's bytes
        let msg = "héllo world";
        let out = truncate_error_message(msg, 5);
        assert!(out.starts_with("héll"));
    }
}
