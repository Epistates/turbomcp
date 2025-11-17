//! Production-grade metrics collection and monitoring system
//!
//! This module provides a comprehensive, lock-free metrics collection system designed
//! for high-performance production environments with zero-allocation hot paths.

use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Production-grade server metrics collector with lock-free atomic operations
#[derive(Debug)]
pub struct ServerMetrics {
    /// Total number of requests received since server start
    pub requests_total: AtomicU64,
    /// Number of requests that completed successfully
    pub requests_successful: AtomicU64,
    /// Number of requests that failed with errors
    pub requests_failed: AtomicU64,
    /// Number of requests currently being processed
    pub requests_in_flight: AtomicU64,

    /// Total number of errors across all categories
    pub errors_total: AtomicU64,
    /// Number of validation/schema errors
    pub errors_validation: AtomicU64,
    /// Number of authentication/authorization errors
    pub errors_auth: AtomicU64,
    /// Number of network-related errors
    pub errors_network: AtomicU64,
    /// Number of timeout errors
    pub errors_timeout: AtomicU64,

    /// Security and timeout-specific metrics for audit and DoS detection
    /// Number of tool executions that exceeded timeout
    pub tool_timeouts_total: AtomicU64,
    /// Number of tool executions cancelled cooperatively
    pub tool_cancellations_total: AtomicU64,
    /// Number of tool executions that completed within timeout
    pub tool_executions_successful: AtomicU64,
    /// Total time spent in timed-out operations (microseconds)
    pub timeout_wasted_time_us: AtomicU64,
    /// Number of active tool executions currently running
    pub tool_executions_active: AtomicU64,

    /// Sum of all response times in microseconds
    pub total_response_time_us: AtomicU64,
    /// Minimum response time observed (microseconds)
    pub min_response_time_us: AtomicU64,
    /// Maximum response time observed (microseconds)
    pub max_response_time_us: AtomicU64,

    /// Total number of tool calls initiated
    pub tool_calls_total: AtomicU64,
    /// Number of tool calls that completed successfully
    pub tool_calls_successful: AtomicU64,
    /// Number of tool calls that failed
    pub tool_calls_failed: AtomicU64,

    /// Number of currently active connections
    pub connections_active: AtomicU64,
    /// Total connections accepted since server start
    pub connections_total: AtomicU64,
    /// Number of connections rejected (rate limiting, etc.)
    pub connections_rejected: AtomicU64,

    /// Current memory usage in bytes
    pub memory_usage_bytes: AtomicU64,
    /// Current CPU usage as percentage × 100 (due to no AtomicF64)
    pub cpu_usage_percent_x100: AtomicU64,

    /// Custom application-specific metrics (rare updates, RwLock acceptable)
    pub custom: RwLock<HashMap<String, f64>>,

    /// Response time histogram for latency distribution analysis
    pub response_time_buckets: ResponseTimeHistogram,

    /// Server start time for uptime calculation
    pub start_time: Instant,
}

/// High-performance histogram for response time distribution
#[derive(Debug)]
pub struct ResponseTimeHistogram {
    /// Requests completed in under 1 millisecond
    pub bucket_1ms: AtomicU64,
    /// Requests completed in 1-5 milliseconds
    pub bucket_5ms: AtomicU64,
    /// Requests completed in 5-10 milliseconds
    pub bucket_10ms: AtomicU64,
    /// Requests completed in 10-25 milliseconds
    pub bucket_25ms: AtomicU64,
    /// Requests completed in 25-50 milliseconds
    pub bucket_50ms: AtomicU64,
    /// Requests completed in 50-100 milliseconds
    pub bucket_100ms: AtomicU64,
    /// Requests completed in 100-250 milliseconds
    pub bucket_250ms: AtomicU64,
    /// Requests completed in 250-500 milliseconds
    pub bucket_500ms: AtomicU64,
    /// Requests completed in 500ms-1 second
    pub bucket_1s: AtomicU64,
    /// Requests completed in 1-2.5 seconds
    pub bucket_2_5s: AtomicU64,
    /// Requests completed in 2.5-5 seconds
    pub bucket_5s: AtomicU64,
    /// Requests completed in 5-10 seconds
    pub bucket_10s: AtomicU64,
    /// Requests completed in over 10 seconds
    pub bucket_inf: AtomicU64,
}

impl Default for ResponseTimeHistogram {
    fn default() -> Self {
        Self::new()
    }
}

impl ResponseTimeHistogram {
    /// Production-grade histogram creation with proper bucket initialization
    pub fn new() -> Self {
        Self {
            bucket_1ms: AtomicU64::new(0),
            bucket_5ms: AtomicU64::new(0),
            bucket_10ms: AtomicU64::new(0),
            bucket_25ms: AtomicU64::new(0),
            bucket_50ms: AtomicU64::new(0),
            bucket_100ms: AtomicU64::new(0),
            bucket_250ms: AtomicU64::new(0),
            bucket_500ms: AtomicU64::new(0),
            bucket_1s: AtomicU64::new(0),
            bucket_2_5s: AtomicU64::new(0),
            bucket_5s: AtomicU64::new(0),
            bucket_10s: AtomicU64::new(0),
            bucket_inf: AtomicU64::new(0),
        }
    }

    /// Record response time with proper bucket assignment
    #[inline]
    pub fn record(&self, duration_us: u64) {
        let duration_ms = duration_us / 1000;

        if duration_ms < 1 {
            self.bucket_1ms.fetch_add(1, Ordering::Relaxed);
        } else if duration_ms < 5 {
            self.bucket_5ms.fetch_add(1, Ordering::Relaxed);
        } else if duration_ms < 10 {
            self.bucket_10ms.fetch_add(1, Ordering::Relaxed);
        } else if duration_ms < 25 {
            self.bucket_25ms.fetch_add(1, Ordering::Relaxed);
        } else if duration_ms < 50 {
            self.bucket_50ms.fetch_add(1, Ordering::Relaxed);
        } else if duration_ms < 100 {
            self.bucket_100ms.fetch_add(1, Ordering::Relaxed);
        } else if duration_ms < 250 {
            self.bucket_250ms.fetch_add(1, Ordering::Relaxed);
        } else if duration_ms < 500 {
            self.bucket_500ms.fetch_add(1, Ordering::Relaxed);
        } else if duration_ms < 1000 {
            self.bucket_1s.fetch_add(1, Ordering::Relaxed);
        } else if duration_ms < 2500 {
            self.bucket_2_5s.fetch_add(1, Ordering::Relaxed);
        } else if duration_ms < 5000 {
            self.bucket_5s.fetch_add(1, Ordering::Relaxed);
        } else if duration_ms < 10000 {
            self.bucket_10s.fetch_add(1, Ordering::Relaxed);
        } else {
            self.bucket_inf.fetch_add(1, Ordering::Relaxed);
        }
    }
}

impl ServerMetrics {
    /// Create proven metrics collector with comprehensive initialization
    pub fn new() -> Self {
        Self {
            requests_total: AtomicU64::new(0),
            requests_successful: AtomicU64::new(0),
            requests_failed: AtomicU64::new(0),
            requests_in_flight: AtomicU64::new(0),

            errors_total: AtomicU64::new(0),
            errors_validation: AtomicU64::new(0),
            errors_auth: AtomicU64::new(0),
            errors_network: AtomicU64::new(0),
            errors_timeout: AtomicU64::new(0),

            // Initialize timeout-specific metrics
            tool_timeouts_total: AtomicU64::new(0),
            tool_cancellations_total: AtomicU64::new(0),
            tool_executions_successful: AtomicU64::new(0),
            timeout_wasted_time_us: AtomicU64::new(0),
            tool_executions_active: AtomicU64::new(0),

            total_response_time_us: AtomicU64::new(0),
            min_response_time_us: AtomicU64::new(u64::MAX),
            max_response_time_us: AtomicU64::new(0),

            tool_calls_total: AtomicU64::new(0),
            tool_calls_successful: AtomicU64::new(0),
            tool_calls_failed: AtomicU64::new(0),

            connections_active: AtomicU64::new(0),
            connections_total: AtomicU64::new(0),
            connections_rejected: AtomicU64::new(0),

            memory_usage_bytes: AtomicU64::new(0),
            cpu_usage_percent_x100: AtomicU64::new(0),

            custom: RwLock::new(HashMap::new()),
            response_time_buckets: ResponseTimeHistogram::new(),
            start_time: Instant::now(),
        }
    }

    /// Record request start with zero-allocation tracking
    #[inline]
    pub fn record_request_start(&self) {
        self.requests_total.fetch_add(1, Ordering::Relaxed);
        self.requests_in_flight.fetch_add(1, Ordering::Relaxed);
    }

    /// Record successful request completion with timing
    #[inline]
    pub fn record_request_success(&self, duration: Duration) {
        self.requests_successful.fetch_add(1, Ordering::Relaxed);
        self.requests_in_flight.fetch_sub(1, Ordering::Relaxed);

        let duration_us = duration.as_micros() as u64;
        self.total_response_time_us
            .fetch_add(duration_us, Ordering::Relaxed);
        self.response_time_buckets.record(duration_us);

        // Update min/max with compare-and-swap
        self.update_min_response_time(duration_us);
        self.update_max_response_time(duration_us);
    }

    /// Record failed request with error categorization
    #[inline]
    pub fn record_request_failure(&self, error_type: &str, duration: Duration) {
        self.requests_failed.fetch_add(1, Ordering::Relaxed);
        self.requests_in_flight.fetch_sub(1, Ordering::Relaxed);
        self.errors_total.fetch_add(1, Ordering::Relaxed);

        // Categorize errors for comprehensive tracking
        match error_type {
            "validation" => self.errors_validation.fetch_add(1, Ordering::Relaxed),
            "auth" => self.errors_auth.fetch_add(1, Ordering::Relaxed),
            "network" => self.errors_network.fetch_add(1, Ordering::Relaxed),
            "timeout" => self.errors_timeout.fetch_add(1, Ordering::Relaxed),
            _ => 0, // Unknown error types don't increment specific counters
        };

        let duration_us = duration.as_micros() as u64;
        self.response_time_buckets.record(duration_us);
    }

    /// Record tool call metrics with comprehensive tracking
    #[inline]
    pub fn record_tool_call(&self, success: bool) {
        self.tool_calls_total.fetch_add(1, Ordering::Relaxed);
        if success {
            self.tool_calls_successful.fetch_add(1, Ordering::Relaxed);
        } else {
            self.tool_calls_failed.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Update connection metrics with proper lifecycle tracking  
    #[inline]
    pub fn record_connection_established(&self) {
        self.connections_total.fetch_add(1, Ordering::Relaxed);
        self.connections_active.fetch_add(1, Ordering::Relaxed);
    }

    /// Record when a connection is closed/terminated
    #[inline]
    pub fn record_connection_closed(&self) {
        self.connections_active.fetch_sub(1, Ordering::Relaxed);
    }

    /// Record when a connection is rejected due to rate limiting or other policies
    #[inline]
    pub fn record_connection_rejected(&self) {
        self.connections_rejected.fetch_add(1, Ordering::Relaxed);
    }

    /// Update resource metrics (called periodically by monitoring task)
    pub fn update_resource_metrics(&self, memory_bytes: u64, cpu_percent: f64) {
        self.memory_usage_bytes
            .store(memory_bytes, Ordering::Relaxed);
        // Store CPU percentage as fixed-point (multiply by 100 to preserve 2 decimal places)
        self.cpu_usage_percent_x100
            .store((cpu_percent * 100.0) as u64, Ordering::Relaxed);
    }

    /// Record custom metric (infrequent operation, lock acceptable)
    pub fn record_custom(&self, name: &str, value: f64) {
        let mut custom = self.custom.write();
        custom.insert(name.to_string(), value);
    }

    /// Calculate uptime in seconds
    pub fn uptime_seconds(&self) -> u64 {
        self.start_time.elapsed().as_secs()
    }

    /// Calculate average response time in microseconds
    pub fn avg_response_time_us(&self) -> f64 {
        let total_time = self.total_response_time_us.load(Ordering::Relaxed);
        let successful_requests = self.requests_successful.load(Ordering::Relaxed);

        if successful_requests > 0 {
            total_time as f64 / successful_requests as f64
        } else {
            0.0
        }
    }

    /// Calculate request rate (requests per second)
    pub fn request_rate(&self) -> f64 {
        let total_requests = self.requests_total.load(Ordering::Relaxed);
        let uptime = self.uptime_seconds();

        if uptime > 0 {
            total_requests as f64 / uptime as f64
        } else {
            0.0
        }
    }

    /// Calculate error rate as percentage
    pub fn error_rate_percent(&self) -> f64 {
        let total_requests = self.requests_total.load(Ordering::Relaxed);
        let failed_requests = self.requests_failed.load(Ordering::Relaxed);

        if total_requests > 0 {
            (failed_requests as f64 / total_requests as f64) * 100.0
        } else {
            0.0
        }
    }

    /// Lock-free atomic update of minimum response time
    fn update_min_response_time(&self, new_value: u64) {
        loop {
            let current = self.min_response_time_us.load(Ordering::Relaxed);
            if new_value >= current {
                break; // New value is not smaller
            }
            match self.min_response_time_us.compare_exchange_weak(
                current,
                new_value,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,     // Successfully updated
                Err(_) => continue, // Retry with new current value
            }
        }
    }

    /// Lock-free atomic update of maximum response time
    fn update_max_response_time(&self, new_value: u64) {
        loop {
            let current = self.max_response_time_us.load(Ordering::Relaxed);
            if new_value <= current {
                break; // New value is not larger
            }
            match self.max_response_time_us.compare_exchange_weak(
                current,
                new_value,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,     // Successfully updated
                Err(_) => continue, // Retry with new current value
            }
        }
    }
}

impl Default for ServerMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Metrics collector trait for extensible metric collection systems
pub trait MetricsCollector: Send + Sync {
    /// Collect metrics into a HashMap for export to monitoring systems
    fn collect(&self) -> HashMap<String, f64>;
}

/// Production-grade comprehensive metrics collector implementation
#[derive(Debug)]
pub struct ComprehensiveMetricsCollector {
    /// Server metrics reference
    metrics: Arc<ServerMetrics>,
}

impl ComprehensiveMetricsCollector {
    /// Create a new comprehensive metrics collector
    #[must_use]
    pub const fn new(metrics: Arc<ServerMetrics>) -> Self {
        Self { metrics }
    }
}

impl MetricsCollector for ComprehensiveMetricsCollector {
    fn collect(&self) -> HashMap<String, f64> {
        let mut metrics = HashMap::new();

        // Request metrics with comprehensive tracking
        metrics.insert(
            "requests_total".to_string(),
            self.metrics.requests_total.load(Ordering::Relaxed) as f64,
        );
        metrics.insert(
            "requests_successful".to_string(),
            self.metrics.requests_successful.load(Ordering::Relaxed) as f64,
        );
        metrics.insert(
            "requests_failed".to_string(),
            self.metrics.requests_failed.load(Ordering::Relaxed) as f64,
        );
        metrics.insert(
            "requests_in_flight".to_string(),
            self.metrics.requests_in_flight.load(Ordering::Relaxed) as f64,
        );
        metrics.insert(
            "requests_rate_per_second".to_string(),
            self.metrics.request_rate(),
        );

        // Error metrics with detailed breakdown
        metrics.insert(
            "errors_total".to_string(),
            self.metrics.errors_total.load(Ordering::Relaxed) as f64,
        );
        metrics.insert(
            "errors_validation".to_string(),
            self.metrics.errors_validation.load(Ordering::Relaxed) as f64,
        );
        metrics.insert(
            "errors_auth".to_string(),
            self.metrics.errors_auth.load(Ordering::Relaxed) as f64,
        );
        metrics.insert(
            "errors_network".to_string(),
            self.metrics.errors_network.load(Ordering::Relaxed) as f64,
        );
        metrics.insert(
            "errors_timeout".to_string(),
            self.metrics.errors_timeout.load(Ordering::Relaxed) as f64,
        );

        // Security-focused timeout metrics for audit and DoS detection
        metrics.insert(
            "tool_timeouts_total".to_string(),
            self.metrics.tool_timeouts_total.load(Ordering::Relaxed) as f64,
        );
        metrics.insert(
            "tool_cancellations_total".to_string(),
            self.metrics
                .tool_cancellations_total
                .load(Ordering::Relaxed) as f64,
        );
        metrics.insert(
            "tool_executions_successful".to_string(),
            self.metrics
                .tool_executions_successful
                .load(Ordering::Relaxed) as f64,
        );
        metrics.insert(
            "timeout_wasted_time_us".to_string(),
            self.metrics.timeout_wasted_time_us.load(Ordering::Relaxed) as f64,
        );
        metrics.insert(
            "tool_executions_active".to_string(),
            self.metrics.tool_executions_active.load(Ordering::Relaxed) as f64,
        );

        metrics.insert(
            "error_rate_percent".to_string(),
            self.metrics.error_rate_percent(),
        );

        // Performance metrics with high-resolution timing
        metrics.insert(
            "response_time_avg_us".to_string(),
            self.metrics.avg_response_time_us(),
        );
        let min_time = self.metrics.min_response_time_us.load(Ordering::Relaxed);
        metrics.insert(
            "response_time_min_us".to_string(),
            if min_time == u64::MAX {
                0.0
            } else {
                min_time as f64
            },
        );
        metrics.insert(
            "response_time_max_us".to_string(),
            self.metrics.max_response_time_us.load(Ordering::Relaxed) as f64,
        );

        // Tool-specific metrics
        metrics.insert(
            "tool_calls_total".to_string(),
            self.metrics.tool_calls_total.load(Ordering::Relaxed) as f64,
        );
        metrics.insert(
            "tool_calls_successful".to_string(),
            self.metrics.tool_calls_successful.load(Ordering::Relaxed) as f64,
        );
        metrics.insert(
            "tool_calls_failed".to_string(),
            self.metrics.tool_calls_failed.load(Ordering::Relaxed) as f64,
        );

        // Connection metrics
        metrics.insert(
            "connections_active".to_string(),
            self.metrics.connections_active.load(Ordering::Relaxed) as f64,
        );
        metrics.insert(
            "connections_total".to_string(),
            self.metrics.connections_total.load(Ordering::Relaxed) as f64,
        );
        metrics.insert(
            "connections_rejected".to_string(),
            self.metrics.connections_rejected.load(Ordering::Relaxed) as f64,
        );

        // Resource metrics
        metrics.insert(
            "memory_usage_bytes".to_string(),
            self.metrics.memory_usage_bytes.load(Ordering::Relaxed) as f64,
        );
        metrics.insert(
            "cpu_usage_percent".to_string(),
            self.metrics.cpu_usage_percent_x100.load(Ordering::Relaxed) as f64 / 100.0,
        );

        // Server uptime
        metrics.insert(
            "uptime_seconds".to_string(),
            self.metrics.uptime_seconds() as f64,
        );

        // Response time histogram buckets (Prometheus-compatible)
        let buckets = &self.metrics.response_time_buckets;
        metrics.insert(
            "response_time_bucket_1ms".to_string(),
            buckets.bucket_1ms.load(Ordering::Relaxed) as f64,
        );
        metrics.insert(
            "response_time_bucket_5ms".to_string(),
            buckets.bucket_5ms.load(Ordering::Relaxed) as f64,
        );
        metrics.insert(
            "response_time_bucket_10ms".to_string(),
            buckets.bucket_10ms.load(Ordering::Relaxed) as f64,
        );
        metrics.insert(
            "response_time_bucket_25ms".to_string(),
            buckets.bucket_25ms.load(Ordering::Relaxed) as f64,
        );
        metrics.insert(
            "response_time_bucket_50ms".to_string(),
            buckets.bucket_50ms.load(Ordering::Relaxed) as f64,
        );
        metrics.insert(
            "response_time_bucket_100ms".to_string(),
            buckets.bucket_100ms.load(Ordering::Relaxed) as f64,
        );
        metrics.insert(
            "response_time_bucket_250ms".to_string(),
            buckets.bucket_250ms.load(Ordering::Relaxed) as f64,
        );
        metrics.insert(
            "response_time_bucket_500ms".to_string(),
            buckets.bucket_500ms.load(Ordering::Relaxed) as f64,
        );
        metrics.insert(
            "response_time_bucket_1s".to_string(),
            buckets.bucket_1s.load(Ordering::Relaxed) as f64,
        );
        metrics.insert(
            "response_time_bucket_2_5s".to_string(),
            buckets.bucket_2_5s.load(Ordering::Relaxed) as f64,
        );
        metrics.insert(
            "response_time_bucket_5s".to_string(),
            buckets.bucket_5s.load(Ordering::Relaxed) as f64,
        );
        metrics.insert(
            "response_time_bucket_10s".to_string(),
            buckets.bucket_10s.load(Ordering::Relaxed) as f64,
        );
        metrics.insert(
            "response_time_bucket_inf".to_string(),
            buckets.bucket_inf.load(Ordering::Relaxed) as f64,
        );

        // Custom metrics (infrequent read lock acceptable)
        if let Some(custom_metrics) = self.metrics.custom.try_read() {
            for (key, value) in custom_metrics.iter() {
                metrics.insert(format!("custom_{key}"), *value);
            }
        }

        metrics
    }
}

// Multi-tenancy metrics (opt-in feature)
#[cfg(feature = "multi-tenancy")]
pub mod multi_tenant;
#[cfg(feature = "multi-tenancy")]
pub use multi_tenant::{MultiTenantMetrics, TenantMetrics};

// Comprehensive tests in separate file (tokio/axum pattern)
#[cfg(test)]
mod tests;
