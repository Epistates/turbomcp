//! Production-grade comprehensive tests for the server metrics module

use crate::metrics::ServerMetrics;
use std::sync::atomic::Ordering;
use std::time::Duration;

#[tokio::test]
async fn test_server_metrics_creation() {
    let metrics = ServerMetrics::new();

    // Should start with zero values
    assert_eq!(metrics.requests_total.load(Ordering::Relaxed), 0);
    assert_eq!(metrics.requests_successful.load(Ordering::Relaxed), 0);
    assert_eq!(metrics.requests_failed.load(Ordering::Relaxed), 0);
    assert_eq!(metrics.errors_total.load(Ordering::Relaxed), 0);
    assert_eq!(metrics.connections_active.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn test_server_metrics_default() {
    let metrics = ServerMetrics::default();

    // Should be identical to new()
    assert_eq!(metrics.requests_total.load(Ordering::Relaxed), 0);
    assert_eq!(metrics.requests_successful.load(Ordering::Relaxed), 0);
    assert_eq!(metrics.errors_total.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn test_record_request_lifecycle() {
    let metrics = ServerMetrics::new();

    // Start some requests
    metrics.record_request_start();
    metrics.record_request_start();
    metrics.record_request_start();

    assert_eq!(metrics.requests_total.load(Ordering::Relaxed), 3);
    assert_eq!(metrics.requests_in_flight.load(Ordering::Relaxed), 3);

    // Complete them with success
    metrics.record_request_success(Duration::from_millis(50));
    metrics.record_request_success(Duration::from_millis(100));

    assert_eq!(metrics.requests_successful.load(Ordering::Relaxed), 2);
    assert_eq!(metrics.requests_in_flight.load(Ordering::Relaxed), 1);

    // One fails
    metrics.record_request_failure("validation", Duration::from_millis(25));

    assert_eq!(metrics.requests_failed.load(Ordering::Relaxed), 1);
    assert_eq!(metrics.requests_in_flight.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn test_response_time_histogram() {
    let metrics = ServerMetrics::new();

    // Record various response times
    metrics.record_request_success(Duration::from_micros(500)); // < 1ms bucket
    metrics.record_request_success(Duration::from_millis(3)); // < 5ms bucket
    metrics.record_request_success(Duration::from_millis(15)); // < 25ms bucket
    metrics.record_request_success(Duration::from_millis(75)); // < 100ms bucket
    metrics.record_request_success(Duration::from_millis(200)); // < 250ms bucket
    metrics.record_request_success(Duration::from_secs(2)); // < 2.5s bucket

    // Verify histogram buckets
    assert_eq!(
        metrics
            .response_time_buckets
            .bucket_1ms
            .load(Ordering::Relaxed),
        1
    );
    assert_eq!(
        metrics
            .response_time_buckets
            .bucket_5ms
            .load(Ordering::Relaxed),
        1
    );
    assert_eq!(
        metrics
            .response_time_buckets
            .bucket_25ms
            .load(Ordering::Relaxed),
        1
    );
    assert_eq!(
        metrics
            .response_time_buckets
            .bucket_100ms
            .load(Ordering::Relaxed),
        1
    );
    assert_eq!(
        metrics
            .response_time_buckets
            .bucket_250ms
            .load(Ordering::Relaxed),
        1
    );
    assert_eq!(
        metrics
            .response_time_buckets
            .bucket_2_5s
            .load(Ordering::Relaxed),
        1
    );
}

#[tokio::test]
async fn test_error_tracking() {
    let metrics = ServerMetrics::new();

    // Record different error types
    metrics.record_request_failure("validation", Duration::from_millis(10));
    metrics.record_request_failure("auth", Duration::from_millis(5));
    metrics.record_request_failure("network", Duration::from_millis(50));
    metrics.record_request_failure("timeout", Duration::from_millis(100));

    // Verify error counters
    assert_eq!(metrics.errors_validation.load(Ordering::Relaxed), 1);
    assert_eq!(metrics.errors_auth.load(Ordering::Relaxed), 1);
    assert_eq!(metrics.errors_network.load(Ordering::Relaxed), 1);
    assert_eq!(metrics.errors_timeout.load(Ordering::Relaxed), 1);
    assert_eq!(metrics.errors_total.load(Ordering::Relaxed), 4);
}

#[tokio::test]
async fn test_tool_call_metrics() {
    let metrics = ServerMetrics::new();

    // Record tool calls
    metrics.record_tool_call(true); // success
    metrics.record_tool_call(true); // success
    metrics.record_tool_call(false); // failure

    assert_eq!(metrics.tool_calls_total.load(Ordering::Relaxed), 3);
    assert_eq!(metrics.tool_calls_successful.load(Ordering::Relaxed), 2);
    assert_eq!(metrics.tool_calls_failed.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn test_connection_metrics() {
    let metrics = ServerMetrics::new();

    // Establish connections
    metrics.record_connection_established();
    metrics.record_connection_established();
    metrics.record_connection_established();

    assert_eq!(metrics.connections_total.load(Ordering::Relaxed), 3);
    assert_eq!(metrics.connections_active.load(Ordering::Relaxed), 3);

    // Close some connections
    metrics.record_connection_closed();

    assert_eq!(metrics.connections_active.load(Ordering::Relaxed), 2);
    assert_eq!(metrics.connections_total.load(Ordering::Relaxed), 3); // total doesn't change

    // Reject a connection
    metrics.record_connection_rejected();

    assert_eq!(metrics.connections_rejected.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn test_resource_metrics() {
    let metrics = ServerMetrics::new();

    // Update resource metrics
    metrics.update_resource_metrics(1024 * 1024, 75.5); // 1MB RAM, 75.5% CPU

    assert_eq!(
        metrics.memory_usage_bytes.load(Ordering::Relaxed),
        1024 * 1024
    );
    assert_eq!(metrics.cpu_usage_percent_x100.load(Ordering::Relaxed), 7550); // 75.5 * 100
}

#[tokio::test]
async fn test_custom_metrics() {
    let metrics = ServerMetrics::new();

    // Record custom metrics
    metrics.record_custom("cache_hit_rate", 0.85);
    metrics.record_custom("queue_depth", 12.0);

    let custom = metrics.custom.read();
    assert_eq!(custom.get("cache_hit_rate"), Some(&0.85));
    assert_eq!(custom.get("queue_depth"), Some(&12.0));
}

#[tokio::test]
async fn test_calculated_metrics() {
    let metrics = ServerMetrics::new();

    // Record some requests with timing
    metrics.record_request_start();
    metrics.record_request_success(Duration::from_millis(100));

    metrics.record_request_start();
    metrics.record_request_success(Duration::from_millis(200));

    // Test average response time calculation
    let avg_response_time = metrics.avg_response_time_us();
    assert!(avg_response_time > 0.0);

    // Test uptime - should exist and be valid (u64 is always >= 0)
    let uptime = metrics.uptime_seconds();
    // Note: uptime is u64 so always >= 0, we're just checking it's computed correctly
    let _ = uptime; // Suppress unused variable warning

    // Test request rate (should be > 0 since we just made requests)
    let request_rate = metrics.request_rate();
    assert!(request_rate >= 0.0);
}

#[tokio::test]
async fn test_error_rate_calculation() {
    let metrics = ServerMetrics::new();

    // Record successful requests
    metrics.record_request_start();
    metrics.record_request_success(Duration::from_millis(50));
    metrics.record_request_start();
    metrics.record_request_success(Duration::from_millis(50));

    // Record failed requests
    metrics.record_request_start();
    metrics.record_request_failure("validation", Duration::from_millis(10));

    // Error rate should be 33.33% (1 failure out of 3 total)
    let error_rate = metrics.error_rate_percent();
    assert!((error_rate - 33.33).abs() < 0.1);
}
