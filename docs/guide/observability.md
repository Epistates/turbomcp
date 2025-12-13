# Observability, Logging & Monitoring

Implement comprehensive logging, tracing, and monitoring for production MCP servers.

## Overview

TurboMCP provides first-class observability support:

- **Structured Logging** - JSON logs with correlation IDs
- **Distributed Tracing** - Trace requests across async boundaries
- **Metrics** - Built-in performance and health metrics
- **Health Checks** - Liveness and readiness probes
- **Error Tracking** - Automatic error categorization and reporting

## Structured Logging

### Basic Logging

Inject the `Logger` into your handlers:

```rust
#[tool]
async fn my_tool(logger: Logger) -> McpResult<String> {
    logger.info("Tool starting").await?;
    logger.warn("Cache miss for key").await?;
    logger.error("Database connection failed").await?;
    Ok("Done".to_string())
}
```

### Log Levels

TurboMCP supports standard log levels:

```rust
logger.debug("Detailed debugging info").await?;
logger.info("General information").await?;
logger.warn("Warning condition").await?;
logger.error("Error occurred").await?;
```

### Structured Fields

Add context to logs:

```rust
#[tool]
async fn handler(logger: Logger) -> McpResult<String> {
    logger.with_field("user_id", "123")
        .with_field("action", "create_resource")
        .info("User action logged")
        .await?;
    Ok("Done".to_string())
}
```

### Configuration

```rust
use turbomcp::logging::LogConfig;

let server = McpServer::new()
    .with_logging(LogConfig {
        level: LogLevel::Info,
        format: LogFormat::Json,  // or Text
        output: LogOutput::Stdout,
        include_timestamps: true,
        include_source: true,
    })
    .stdio()
    .run()
    .await?;
```

## Request Correlation

Track requests across your system:

```rust
#[tool]
async fn handler(info: RequestInfo, logger: Logger) -> McpResult<String> {
    // Every request gets a unique ID
    let request_id = &info.request_id;

    // And a correlation ID (same for retries)
    let correlation_id = &info.correlation_id;

    logger.with_field("request_id", request_id)
        .with_field("correlation_id", correlation_id)
        .info("Processing request")
        .await?;

    Ok("Done".to_string())
}
```

**Log output:**
```json
{
  "timestamp": "2025-12-10T10:30:45Z",
  "level": "INFO",
  "message": "Processing request",
  "request_id": "550e8400-e29b-41d4-a716-446655440000",
  "correlation_id": "550e8400-e29b-41d4-a716-446655440001"
}
```

## Distributed Tracing

### OpenTelemetry Integration

```toml
turbomcp = { version = "2.3", features = ["tracing"] }
opentelemetry = "0.21"
opentelemetry-jaeger = "0.20"
```

### Configuration

```rust
use turbomcp::tracing::TracingConfig;

let tracer = opentelemetry_jaeger::new_agent_pipeline()
    .install_simple()
    .unwrap();

let server = McpServer::new()
    .with_tracing(TracingConfig {
        enabled: true,
        sample_rate: 1.0,  // Sample all requests
    })
    .stdio()
    .run()
    .await?;
```

### Span Creation

Spans are automatically created for requests, but you can add custom spans:

```rust
#[tool]
async fn complex_operation(logger: Logger) -> McpResult<String> {
    // Automatic span for this tool
    // Traces show: tool_call → database_query → cache_write

    // Custom spans for sub-operations
    let _span = tracing::info_span!("fetch_data").entered();

    // ... operation code ...

    Ok("Done".to_string())
}
```

## Metrics

### Built-in Metrics

TurboMCP automatically tracks:

- **Request metrics**: Count, latency, errors
- **Handler metrics**: Per-tool success rate and latency
- **Transport metrics**: Connection count, messages/sec
- **System metrics**: Memory, CPU, goroutine count

### Accessing Metrics

```rust
let metrics = server.get_metrics().await?;

println!("Total requests: {}", metrics.request_count);
println!("Error rate: {:.2}%", metrics.error_rate);
println!("P99 latency: {:.1}ms", metrics.latency_p99);

// Per-tool metrics
for (tool_name, tool_metrics) in &metrics.by_tool {
    println!("{}: {} calls, {} errors",
        tool_name,
        tool_metrics.call_count,
        tool_metrics.error_count
    );
}
```

### Custom Metrics

```rust
#[tool]
async fn handler(metrics: Metrics) -> McpResult<String> {
    // Increment a counter
    metrics.increment("custom_counter", 1)?;

    // Record a value
    metrics.record("processing_time_ms", 150)?;

    // Set a gauge
    metrics.set_gauge("queue_size", 42)?;

    Ok("Done".to_string())
}
```

### Exporting Metrics

```rust
let server = McpServer::new()
    .with_metrics_export(MetricsExportConfig {
        enabled: true,
        interval: Duration::from_secs(60),
        format: MetricsFormat::Prometheus,  // Or JSON
        endpoint: Some("http://prometheus:9090".to_string()),
    })
    .stdio()
    .run()
    .await?;
```

## Health Checks

### Liveness & Readiness

```rust
let server = McpServer::new()
    .with_health_check(HealthCheckConfig {
        enabled: true,
        liveness_path: "/health/live",
        readiness_path: "/health/ready",
        detailed: true,
    })
    .http(8080)
    .run()
    .await?;
```

**Checking health:**

```bash
# Liveness (is server running?)
curl http://localhost:8080/health/live

# Readiness (is server ready for traffic?)
curl http://localhost:8080/health/ready
```

### Custom Health Checks

```rust
let server = McpServer::new()
    .with_custom_health_check(|ctx| async move {
        // Check database connectivity
        let db_ok = ctx.database().ping().await.is_ok();

        // Check cache connectivity
        let cache_ok = ctx.cache().ping().await.is_ok();

        Ok(HealthStatus {
            overall: if db_ok && cache_ok { Healthy } else { Unhealthy },
            components: vec![
                ("database", if db_ok { Healthy } else { Unhealthy }),
                ("cache", if cache_ok { Healthy } else { Unhealthy }),
            ],
        })
    })
    .http(8080)
    .run()
    .await?;
```

## Error Tracking & Reporting

### Automatic Error Categorization

```rust
#[tool]
async fn handler() -> McpResult<String> {
    // Errors are automatically categorized
    Err(McpError::InvalidInput("Bad parameter".into()))
    // Tracked as: error_type=invalid_input, handler=handler
}
```

### Error Context

```rust
#[tool]
async fn handler(logger: Logger) -> McpResult<String> {
    match some_operation().await {
        Ok(result) => Ok(result),
        Err(e) => {
            logger.with_field("error_type", "operation_failed")
                .with_field("error_message", e.to_string())
                .error("Operation failed")
                .await?;

            Err(McpError::InternalError(e.to_string()))
        }
    }
}
```

### Error Reporting Service

Integrate with error tracking services:

```rust
let server = McpServer::new()
    .with_error_reporting(ErrorReportingConfig {
        enabled: true,
        service: ErrorReportingService::Sentry {
            dsn: "https://...@sentry.io/...".to_string(),
            release: Some("1.0.0".to_string()),
        },
        breadcrumb_limit: 50,
        attach_logs: true,
    })
    .stdio()
    .run()
    .await?;
```

## Monitoring Dashboard

### Prometheus Integration

```rust
let server = McpServer::new()
    .http(8080)
    .with_prometheus_endpoint("/metrics")  // Expose metrics at /metrics
    .run()
    .await?;
```

Scrape with Prometheus:

```yaml
scrape_configs:
  - job_name: 'turbomcp'
    static_configs:
      - targets: ['localhost:8080']
    metrics_path: '/metrics'
```

### Grafana Dashboards

Pre-built dashboards available:
- Request rate and latency
- Error rate by handler
- Handler-specific performance
- System resource usage
- Transport-level metrics

## Logging Best Practices

### 1. Use Structured Logging

```rust
// ✅ Good
logger.with_field("user_id", user_id)
    .with_field("action", "delete_resource")
    .info("Resource deleted")
    .await?;

// ❌ Avoid
logger.info(format!("User {} deleted resource", user_id)).await?;
```

### 2. Include Context IDs

```rust
// ✅ Always include correlation IDs
logger.with_field("request_id", info.request_id)
    .with_field("correlation_id", info.correlation_id)
    .info("Request processed")
    .await?;
```

### 3. Don't Log Sensitive Data

```rust
// ❌ Never log passwords, tokens, or API keys
logger.info(format!("Token: {}", token)).await?;

// ✅ Log safely
logger.info("User authenticated").await?;
```

### 4. Use Appropriate Log Levels

```rust
logger.debug("Cache hit for key").await?;           // Debug details
logger.info("Request received").await?;              // Normal flow
logger.warn("Slow query detected: 500ms").await?;   // Warnings
logger.error("Database connection failed").await?;   // Errors
```

## Troubleshooting

### "Logs not appearing"

Check configuration:

```rust
let server = McpServer::new()
    .with_logging(LogConfig {
        level: LogLevel::Debug,  // Lower log level
        format: LogFormat::Json,
        output: LogOutput::Stdout,
        ..Default::default()
    })
    .run()
    .await?;
```

### High memory usage from logging

Reduce log verbosity or buffer size:

```rust
let server = McpServer::new()
    .with_logging(LogConfig {
        level: LogLevel::Info,  // Not Debug
        buffer_size: 1000,      // Reduce buffer
        ..Default::default()
    })
    .run()
    .await?;
```

## Performance Impact

Logging and tracing have minimal performance impact:

- Structured logging: <1ms per log line
- Tracing: <5% overhead for fully sampled requests
- Metrics: Negligible impact (<0.1%)

## Next Steps

- **[Advanced Patterns](advanced-patterns.md)** - Complex observability setups
- **[Deployment](../deployment/production.md)** - Production monitoring setup
- **[Examples](../examples/basic.md)** - Real-world observability examples

