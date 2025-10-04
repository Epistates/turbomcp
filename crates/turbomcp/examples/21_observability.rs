//! Production-grade observability example with structured logging and security auditing
//!
//! This example demonstrates TurboMCP's comprehensive observability features:
//! - Structured JSON logging for production environments
//! - Security audit logging for compliance
//! - Performance monitoring with distributed tracing
//! - Metrics integration for monitoring
//!
//! Run with:
//! ```bash
//! cargo run --example 21_observability
//! ```

use tracing::{error, info, warn};
use turbomcp::ServerBuilder;
use turbomcp_server::{ObservabilityConfig, global_observability};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== TurboMCP Observability Example ===\n");
    println!("This example demonstrates comprehensive observability features.\n");

    // Initialize observability with full configuration
    let obs_config = ObservabilityConfig::new("observability-example")
        .with_service_version("1.0.0")
        .with_log_level("info,turbomcp=debug")
        .enable_security_auditing()
        .enable_performance_monitoring();

    let _guard = obs_config.init()?;

    info!("Starting observability example server");

    // Access global observability components
    let global_obs = global_observability();

    // Log that security auditing is enabled
    if let Some(security_logger) = global_obs.security_audit_logger().await {
        // Example authentication event
        security_logger.log_authentication("example_user", true, Some("Example authentication"));

        // Example authorization event
        security_logger.log_authorization("example_user", "/api/tools", "execute", true);

        // Example tool execution event
        security_logger.log_tool_execution("example_user", "secure_data_access", true, 150);

        // Example security violation
        security_logger.log_security_violation(
            "rate_limit_exceeded",
            "User exceeded API rate limit",
            "warning",
        );
    }

    // Log performance monitoring
    if let Some(perf_monitor) = global_obs.performance_monitor().await {
        // Create a performance span
        let span = perf_monitor.start_span("example_operation");

        // Simulate some work
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // Finish the span (logs the duration)
        let duration = span.finish();
        info!("Operation completed in {:?}", duration);
    }

    // Build server with observability
    let _server = ServerBuilder::new()
        .name("ObservabilityServer")
        .version("1.0.0")
        .build();

    println!("\nServer configured with observability features:");
    println!("- Structured JSON logging to stderr");
    println!("- Security audit logging enabled");
    println!("- Performance monitoring active");
    println!("- Distributed tracing configured");
    println!("\nObservability logs will appear in JSON format on stderr.");
    println!("\nDemonstrating various log levels:");

    // Demonstrate different logging levels
    info!(
        event = "server_startup",
        service = "observability-example",
        version = "1.0.0",
        "Server initialization complete"
    );

    warn!(
        event = "configuration_warning",
        setting = "max_connections",
        value = 10000,
        "High connection limit configured"
    );

    error!(
        event = "example_error",
        error_type = "demonstration",
        "This is an example error message (not a real error)"
    );

    // Log with structured fields
    info!(
        event = "metrics",
        requests_processed = 1234,
        average_latency_ms = 45,
        success_rate = 0.998,
        "Server metrics update"
    );

    // Log security-related events
    info!(
        event = "security_audit",
        action = "api_access",
        user = "test_user",
        resource = "/api/sensitive",
        result = "allowed",
        "Security audit trail"
    );

    println!("\nObservability example complete. Check stderr for structured JSON logs.");
    println!("In production, these logs would be sent to your observability platform.");
    println!("\nTo run a full server with observability, use:");
    println!("  server.run_stdio().await?;");

    Ok(())
}
