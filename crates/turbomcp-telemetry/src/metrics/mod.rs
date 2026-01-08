//! Prometheus metrics for MCP servers
//!
//! Provides pre-defined metrics following OpenTelemetry semantic conventions
//! for MCP operations.
//!
//! # Example
//!
//! ```rust,ignore
//! use turbomcp_telemetry::metrics::{McpMetrics, record_request};
//!
//! // Record a successful tool call
//! record_request("tools/call", "success", 15.5);
//!
//! // Record tool-specific metrics
//! McpMetrics::tool_call("calculator", true, 10.0);
//! ```

use metrics::{counter, describe_counter, describe_gauge, describe_histogram, gauge, histogram};
use std::sync::Once;

static INIT: Once = Once::new();

/// Initialize metric descriptions
///
/// This should be called once at startup to register metric descriptions.
pub fn init_metrics() {
    INIT.call_once(|| {
        // Request metrics
        describe_counter!(
            "mcp_requests_total",
            "Total number of MCP requests processed"
        );
        describe_histogram!(
            "mcp_request_duration_seconds",
            "Duration of MCP request processing in seconds"
        );
        describe_histogram!("mcp_request_size_bytes", "Size of MCP requests in bytes");
        describe_histogram!("mcp_response_size_bytes", "Size of MCP responses in bytes");

        // Connection metrics
        describe_gauge!("mcp_active_connections", "Number of active MCP connections");
        describe_counter!(
            "mcp_connections_total",
            "Total number of MCP connections established"
        );
        describe_histogram!(
            "mcp_connection_duration_seconds",
            "Duration of MCP connections in seconds"
        );

        // Tool metrics
        describe_counter!("mcp_tool_calls_total", "Total number of tool calls");
        describe_histogram!(
            "mcp_tool_duration_seconds",
            "Duration of tool execution in seconds"
        );

        // Resource metrics
        describe_counter!(
            "mcp_resource_reads_total",
            "Total number of resource read operations"
        );

        // Prompt metrics
        describe_counter!(
            "mcp_prompt_gets_total",
            "Total number of prompt get operations"
        );

        // Error metrics
        describe_counter!("mcp_errors_total", "Total number of MCP errors");

        // Rate limiting metrics
        describe_counter!(
            "mcp_rate_limited_total",
            "Total number of rate-limited requests"
        );
    });
}

/// Record a request
pub fn record_request(method: &str, status: &str, duration_seconds: f64) {
    counter!("mcp_requests_total", "method" => method.to_string(), "status" => status.to_string())
        .increment(1);
    histogram!(
        "mcp_request_duration_seconds",
        "method" => method.to_string()
    )
    .record(duration_seconds);
}

/// Record request size
pub fn record_request_size(method: &str, size_bytes: usize) {
    histogram!(
        "mcp_request_size_bytes",
        "method" => method.to_string()
    )
    .record(size_bytes as f64);
}

/// Record response size
pub fn record_response_size(method: &str, size_bytes: usize) {
    histogram!(
        "mcp_response_size_bytes",
        "method" => method.to_string()
    )
    .record(size_bytes as f64);
}

/// MCP-specific metrics recorder
pub struct McpMetrics;

impl McpMetrics {
    /// Initialize metrics (call once at startup)
    pub fn init() {
        init_metrics();
    }

    /// Record a tool call
    pub fn tool_call(tool_name: &str, success: bool, duration_seconds: f64) {
        let status = if success { "success" } else { "error" };
        counter!(
            "mcp_tool_calls_total",
            "tool" => tool_name.to_string(),
            "status" => status.to_string()
        )
        .increment(1);
        histogram!(
            "mcp_tool_duration_seconds",
            "tool" => tool_name.to_string()
        )
        .record(duration_seconds);
    }

    /// Record a resource read
    pub fn resource_read(uri_pattern: &str, success: bool) {
        let status = if success { "success" } else { "error" };
        counter!(
            "mcp_resource_reads_total",
            "uri_pattern" => uri_pattern.to_string(),
            "status" => status.to_string()
        )
        .increment(1);
    }

    /// Record a prompt get
    pub fn prompt_get(prompt_name: &str, success: bool) {
        let status = if success { "success" } else { "error" };
        counter!(
            "mcp_prompt_gets_total",
            "prompt" => prompt_name.to_string(),
            "status" => status.to_string()
        )
        .increment(1);
    }

    /// Record an error
    pub fn error(kind: &str, method: &str) {
        counter!(
            "mcp_errors_total",
            "kind" => kind.to_string(),
            "method" => method.to_string()
        )
        .increment(1);
    }

    /// Record a rate-limited request
    pub fn rate_limited(tenant: Option<&str>) {
        let tenant_label = tenant.unwrap_or("default");
        counter!(
            "mcp_rate_limited_total",
            "tenant" => tenant_label.to_string()
        )
        .increment(1);
    }

    /// Update active connection count
    pub fn set_active_connections(transport: &str, count: i64) {
        gauge!(
            "mcp_active_connections",
            "transport" => transport.to_string()
        )
        .set(count as f64);
    }

    /// Record a new connection
    pub fn connection_established(transport: &str) {
        counter!(
            "mcp_connections_total",
            "transport" => transport.to_string()
        )
        .increment(1);
    }

    /// Record connection duration
    pub fn connection_closed(transport: &str, duration_seconds: f64) {
        histogram!(
            "mcp_connection_duration_seconds",
            "transport" => transport.to_string()
        )
        .record(duration_seconds);
    }
}

/// Helper to measure and record request duration
pub struct RequestTimer {
    method: String,
    start: std::time::Instant,
}

impl RequestTimer {
    /// Start timing a request
    #[must_use]
    pub fn start(method: impl Into<String>) -> Self {
        Self {
            method: method.into(),
            start: std::time::Instant::now(),
        }
    }

    /// Complete the timer and record metrics
    pub fn complete(self, success: bool) {
        let duration = self.start.elapsed().as_secs_f64();
        let status = if success { "success" } else { "error" };
        record_request(&self.method, status, duration);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_metrics() {
        // Should not panic even when called multiple times
        init_metrics();
        init_metrics();
    }

    #[test]
    fn test_request_timer() {
        let timer = RequestTimer::start("tools/call");
        std::thread::sleep(std::time::Duration::from_millis(1));
        timer.complete(true);
        // Metrics are recorded (we can't easily verify without a recorder)
    }

    #[test]
    fn test_mcp_metrics() {
        McpMetrics::init();
        McpMetrics::tool_call("calculator", true, 0.015);
        McpMetrics::resource_read("file:///*", true);
        McpMetrics::prompt_get("greeting", false);
        McpMetrics::error("validation", "tools/call");
        McpMetrics::rate_limited(Some("tenant-123"));
        McpMetrics::connection_established("websocket");
        McpMetrics::set_active_connections("http", 5);
        McpMetrics::connection_closed("websocket", 60.0);
    }
}
