//! Metrics middleware for MCP client.
//!
//! Tower Layer that collects request/response metrics including:
//! - Request counts (total, success, error)
//! - Response latency (min, max, average, percentiles)
//! - Method-specific statistics
//! - Requests per second
//!
//! ## Usage
//!
//! ```rust,ignore
//! use turbomcp_client::middleware::{MetricsLayer, Metrics};
//! use tower::ServiceBuilder;
//! use std::sync::Arc;
//!
//! // Create shared metrics collector
//! let metrics = Arc::new(Metrics::new());
//!
//! // Add to service stack
//! let service = ServiceBuilder::new()
//!     .layer(MetricsLayer::new(Arc::clone(&metrics)))
//!     .service(inner_service);
//!
//! // Query metrics
//! let snapshot = metrics.snapshot();
//! println!("Total requests: {}", snapshot.total_requests);
//! ```

use super::request::{McpRequest, McpResponse};
use futures_util::future::BoxFuture;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};
use tower_layer::Layer;
use tower_service::Service;
use turbomcp_protocol::McpError;

/// Thread-safe metrics collector.
///
/// Uses atomic operations for counters and a read-write lock for
/// more complex statistics to minimize contention.
#[derive(Debug)]
pub struct Metrics {
    /// Total request count
    total_requests: AtomicU64,
    /// Successful response count
    successful_responses: AtomicU64,
    /// Error response count
    error_responses: AtomicU64,
    /// Response time tracking (protected by RwLock)
    response_times: RwLock<ResponseTimeStats>,
    /// Per-method metrics
    method_metrics: RwLock<HashMap<String, MethodMetrics>>,
    /// Collection start time
    start_time: Instant,
}

#[derive(Debug, Default)]
struct ResponseTimeStats {
    total_ms: u64,
    count: u64,
    min_ms: Option<u64>,
    max_ms: u64,
    /// Recent response times for percentile calculation (ring buffer)
    recent: Vec<u64>,
}

/// Per-method metrics.
#[derive(Debug, Clone, Default)]
pub struct MethodMetrics {
    /// Total calls to this method
    pub count: u64,
    /// Average duration in milliseconds
    pub avg_duration_ms: f64,
    /// Successful calls
    pub success_count: u64,
    /// Error calls
    pub error_count: u64,
}

/// Metrics snapshot for reporting.
#[derive(Debug, Clone)]
pub struct MetricsSnapshot {
    /// Total requests made
    pub total_requests: u64,
    /// Successful responses received
    pub successful_responses: u64,
    /// Error responses received
    pub error_responses: u64,
    /// Average response time in milliseconds
    pub avg_response_time_ms: f64,
    /// Minimum response time in milliseconds
    pub min_response_time_ms: Option<u64>,
    /// Maximum response time in milliseconds
    pub max_response_time_ms: u64,
    /// Requests per second since start
    pub requests_per_second: f64,
    /// Per-method statistics
    pub method_metrics: HashMap<String, MethodMetrics>,
    /// Duration since metrics collection started
    pub uptime: Duration,
}

impl Metrics {
    /// Create a new metrics collector.
    #[must_use]
    pub fn new() -> Self {
        Self {
            total_requests: AtomicU64::new(0),
            successful_responses: AtomicU64::new(0),
            error_responses: AtomicU64::new(0),
            response_times: RwLock::new(ResponseTimeStats::default()),
            method_metrics: RwLock::new(HashMap::new()),
            start_time: Instant::now(),
        }
    }

    /// Record a request being sent.
    pub fn record_request(&self) {
        self.total_requests.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a response received.
    pub fn record_response(&self, method: &str, duration: Duration, is_success: bool) {
        let duration_ms = duration.as_millis() as u64;

        // Update success/error counters
        if is_success {
            self.successful_responses.fetch_add(1, Ordering::Relaxed);
        } else {
            self.error_responses.fetch_add(1, Ordering::Relaxed);
        }

        // Update response time stats
        {
            let mut stats = self.response_times.write();
            stats.total_ms += duration_ms;
            stats.count += 1;
            stats.max_ms = stats.max_ms.max(duration_ms);
            stats.min_ms = Some(stats.min_ms.map_or(duration_ms, |min| min.min(duration_ms)));

            // Keep last 1000 response times for percentile calculation
            if stats.recent.len() >= 1000 {
                stats.recent.remove(0);
            }
            stats.recent.push(duration_ms);
        }

        // Update method-specific metrics
        {
            let mut methods = self.method_metrics.write();
            let entry = methods.entry(method.to_string()).or_default();
            entry.count += 1;
            if is_success {
                entry.success_count += 1;
            } else {
                entry.error_count += 1;
            }
            // Running average
            entry.avg_duration_ms = (entry.avg_duration_ms * (entry.count - 1) as f64
                + duration_ms as f64)
                / entry.count as f64;
        }
    }

    /// Get a snapshot of current metrics.
    #[must_use]
    pub fn snapshot(&self) -> MetricsSnapshot {
        let total = self.total_requests.load(Ordering::Relaxed);
        let successful = self.successful_responses.load(Ordering::Relaxed);
        let errors = self.error_responses.load(Ordering::Relaxed);
        let uptime = self.start_time.elapsed();

        let (avg_ms, min_ms, max_ms) = {
            let stats = self.response_times.read();
            let avg = if stats.count > 0 {
                stats.total_ms as f64 / stats.count as f64
            } else {
                0.0
            };
            (avg, stats.min_ms, stats.max_ms)
        };

        let method_metrics = self.method_metrics.read().clone();

        MetricsSnapshot {
            total_requests: total,
            successful_responses: successful,
            error_responses: errors,
            avg_response_time_ms: avg_ms,
            min_response_time_ms: min_ms,
            max_response_time_ms: max_ms,
            requests_per_second: if uptime.as_secs() > 0 {
                total as f64 / uptime.as_secs_f64()
            } else {
                total as f64
            },
            method_metrics,
            uptime,
        }
    }

    /// Reset all metrics.
    pub fn reset(&self) {
        self.total_requests.store(0, Ordering::Relaxed);
        self.successful_responses.store(0, Ordering::Relaxed);
        self.error_responses.store(0, Ordering::Relaxed);
        *self.response_times.write() = ResponseTimeStats::default();
        self.method_metrics.write().clear();
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Tower Layer that adds metrics collection.
#[derive(Debug, Clone)]
pub struct MetricsLayer {
    metrics: Arc<Metrics>,
}

impl MetricsLayer {
    /// Create a new metrics layer with a shared metrics collector.
    #[must_use]
    pub fn new(metrics: Arc<Metrics>) -> Self {
        Self { metrics }
    }

    /// Create a new metrics layer with a new internal collector.
    ///
    /// Note: If you need to query metrics, use `new()` with a shared `Arc<Metrics>`.
    #[must_use]
    pub fn with_internal_metrics() -> Self {
        Self {
            metrics: Arc::new(Metrics::new()),
        }
    }

    /// Get a reference to the metrics collector.
    #[must_use]
    pub fn metrics(&self) -> &Arc<Metrics> {
        &self.metrics
    }
}

impl<S> Layer<S> for MetricsLayer {
    type Service = MetricsService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        MetricsService {
            inner,
            metrics: Arc::clone(&self.metrics),
        }
    }
}

/// Tower Service that collects metrics.
#[derive(Debug, Clone)]
pub struct MetricsService<S> {
    inner: S,
    metrics: Arc<Metrics>,
}

impl<S> MetricsService<S> {
    /// Get a reference to the inner service.
    pub fn inner(&self) -> &S {
        &self.inner
    }

    /// Get a mutable reference to the inner service.
    pub fn inner_mut(&mut self) -> &mut S {
        &mut self.inner
    }

    /// Get a reference to the metrics collector.
    pub fn metrics(&self) -> &Arc<Metrics> {
        &self.metrics
    }
}

impl<S> Service<McpRequest> for MetricsService<S>
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
        let metrics = Arc::clone(&self.metrics);
        let start = Instant::now();

        // Clone inner service for the async block
        let mut inner = self.inner.clone();
        std::mem::swap(&mut self.inner, &mut inner);

        // Record request
        metrics.record_request();

        Box::pin(async move {
            let result = inner.call(req).await.map_err(Into::into);
            let duration = start.elapsed();

            match &result {
                Ok(response) => {
                    metrics.record_response(&method, duration, response.is_success());
                }
                Err(_) => {
                    metrics.record_response(&method, duration, false);
                }
            }

            result
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use turbomcp_protocol::MessageId;
    use turbomcp_protocol::jsonrpc::{JsonRpcRequest, JsonRpcVersion};

    #[test]
    fn test_metrics_creation() {
        let metrics = Metrics::new();
        let snapshot = metrics.snapshot();

        assert_eq!(snapshot.total_requests, 0);
        assert_eq!(snapshot.successful_responses, 0);
        assert_eq!(snapshot.error_responses, 0);
    }

    #[test]
    fn test_metrics_recording() {
        let metrics = Metrics::new();

        metrics.record_request();
        metrics.record_request();
        metrics.record_response("test/method", Duration::from_millis(100), true);
        metrics.record_response("test/method", Duration::from_millis(200), false);

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.total_requests, 2);
        assert_eq!(snapshot.successful_responses, 1);
        assert_eq!(snapshot.error_responses, 1);
        assert_eq!(snapshot.min_response_time_ms, Some(100));
        assert_eq!(snapshot.max_response_time_ms, 200);
    }

    #[test]
    fn test_method_metrics() {
        let metrics = Metrics::new();

        metrics.record_response("tools/call", Duration::from_millis(50), true);
        metrics.record_response("tools/call", Duration::from_millis(100), true);
        metrics.record_response("resources/read", Duration::from_millis(75), false);

        let snapshot = metrics.snapshot();

        let tool_metrics = snapshot.method_metrics.get("tools/call").unwrap();
        assert_eq!(tool_metrics.count, 2);
        assert_eq!(tool_metrics.success_count, 2);
        assert_eq!(tool_metrics.error_count, 0);
        assert_eq!(tool_metrics.avg_duration_ms, 75.0);

        let resource_metrics = snapshot.method_metrics.get("resources/read").unwrap();
        assert_eq!(resource_metrics.count, 1);
        assert_eq!(resource_metrics.success_count, 0);
        assert_eq!(resource_metrics.error_count, 1);
    }

    #[test]
    fn test_metrics_reset() {
        let metrics = Metrics::new();

        metrics.record_request();
        metrics.record_response("test", Duration::from_millis(100), true);

        metrics.reset();

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.total_requests, 0);
        assert!(snapshot.method_metrics.is_empty());
    }

    #[test]
    fn test_metrics_layer_creation() {
        let metrics = Arc::new(Metrics::new());
        let layer = MetricsLayer::new(Arc::clone(&metrics));

        assert!(Arc::ptr_eq(&metrics, layer.metrics()));
    }

    #[tokio::test]
    async fn test_metrics_service() {
        use tower::ServiceExt;

        let metrics = Arc::new(Metrics::new());

        let mock_service = tower::service_fn(|_req: McpRequest| async {
            Ok::<_, McpError>(McpResponse::success(
                json!({"result": "ok"}),
                Duration::from_millis(10),
            ))
        });

        let mut service = MetricsLayer::new(Arc::clone(&metrics)).layer(mock_service);

        let request = McpRequest::new(JsonRpcRequest {
            jsonrpc: JsonRpcVersion,
            id: MessageId::from("test-1"),
            method: "test/method".to_string(),
            params: None,
        });

        let _ = service.ready().await.unwrap().call(request).await.unwrap();

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.total_requests, 1);
        assert_eq!(snapshot.successful_responses, 1);
    }
}
