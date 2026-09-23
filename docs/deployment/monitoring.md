# Monitoring & Observability

Monitor TurboMCP servers in production with comprehensive health checks, metrics, and alerting.

## Health Checks

TurboMCP does not add health endpoints of its own. With the `http` feature,
serve them from your own Axum routes next to the MCP routes:

```rust
use axum::{Json, Router, routing::get};
use serde_json::json;
use turbomcp::prelude::*;

#[derive(Clone)]
struct MyServer;

#[server(name = "my-server", version = "1.0.0")]
impl MyServer {
    /// Say hello
    #[tool]
    async fn hello(&self) -> String {
        "hello".to_string()
    }
}

/// Liveness: the process is up and serving requests.
async fn live() -> Json<serde_json::Value> {
    Json(json!({ "status": "alive" }))
}

/// Readiness: the dependencies the handlers need are reachable.
async fn ready() -> Json<serde_json::Value> {
    // Ping your database, cache, ... here and report what you find
    Json(json!({ "status": "ready" }))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let app = Router::new()
        .route("/health/live", get(live))
        .route("/health/ready", get(ready))
        .merge(MyServer.builder().into_axum_router());

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    axum::serve(listener, app).await?;
    Ok(())
}
```

**Test it:**
```bash
curl http://localhost:8080/health/live
# Returns: {"status":"alive"}

curl http://localhost:8080/health/ready
# Returns: {"status":"ready"}
```

## Kubernetes Integration

### Liveness & Readiness Probes

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: turbomcp-server
spec:
  replicas: 3
  template:
    spec:
      containers:
      - name: server
        image: turbomcp-server:latest
        ports:
        - containerPort: 8080

        # Liveness probe - restart if dead
        livenessProbe:
          httpGet:
            path: /health/live
            port: 8080
          initialDelaySeconds: 10
          periodSeconds: 10
          failureThreshold: 3

        # Readiness probe - exclude from traffic if not ready
        readinessProbe:
          httpGet:
            path: /health/ready
            port: 8080
          initialDelaySeconds: 5
          periodSeconds: 5
          failureThreshold: 2
```

## Metrics Collection

### Prometheus Integration

With the `telemetry` feature on `turbomcp` (or `turbomcp-telemetry` with its
`prometheus` feature), `TelemetryConfig::prometheus_port` starts a Prometheus
exporter on its own port. It binds `127.0.0.1` unless you set
`prometheus_bind_addr`; exposing it beyond loopback serves unauthenticated
metrics to anyone who can reach the port.

```rust
use std::net::{IpAddr, Ipv4Addr};
use turbomcp::prelude::*;

#[derive(Clone)]
struct MyServer;

#[server(name = "my-server", version = "1.0.0")]
impl MyServer {
    /// Say hello
    #[tool]
    async fn hello(&self) -> String {
        "hello".to_string()
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _telemetry = TelemetryConfig::builder()
        .service_name("my-server")
        .service_version(env!("CARGO_PKG_VERSION"))
        .prometheus_port(9090)
        // Scraped from another host (e.g. a Prometheus pod): bind all interfaces
        .prometheus_bind_addr(IpAddr::V4(Ipv4Addr::UNSPECIFIED))
        .build()
        .init()?;

    // Register the metric descriptions
    turbomcp::telemetry::metrics::McpMetrics::init();

    MyServer.run_http("0.0.0.0:8080").await?;
    Ok(())
}
```

**Prometheus configuration:**

```yaml
global:
  scrape_interval: 15s
  evaluation_interval: 15s

scrape_configs:
  - job_name: 'turbomcp'
    static_configs:
      - targets: ['localhost:9090']
    metrics_path: '/metrics'
    scrape_interval: 5s
```

### Available Metrics

The server does not record these on its own: they are emitted by the helpers in
`turbomcp::telemetry::metrics` (`McpMetrics`, `RequestTimer`, `record_request`),
which your handlers or middleware call. The names and labels are:

```
# REQUEST METRICS
mcp_requests_total{method="tools/call",status="success"}
mcp_request_duration_seconds{method="tools/call"}
mcp_request_size_bytes{method="tools/call"}
mcp_response_size_bytes{method="tools/call"}

# HANDLER METRICS
mcp_tool_calls_total{tool="get_weather",status="success"}
mcp_tool_duration_seconds{tool="get_weather"}
mcp_resource_reads_total{uri_pattern="file://{path}",status="success"}
mcp_prompt_gets_total{prompt="code_review",status="success"}
mcp_errors_total{kind="internal",method="tools/call"}
mcp_rate_limited_total{tenant="acme"}

# CONNECTION METRICS
mcp_active_connections{transport="http"}
mcp_connections_total{transport="http"}
mcp_connection_duration_seconds{transport="http"}
```

### Custom Metrics

Record from a handler with `McpMetrics`, or with any `metrics` macro, which
reaches the same exporter:

```rust
use std::time::Instant;
use turbomcp::prelude::*;
use turbomcp::telemetry::metrics::McpMetrics;

#[derive(Clone)]
struct Weather;

#[server]
impl Weather {
    /// Current weather for a city
    #[tool]
    async fn get_weather(&self, city: String) -> McpResult<String> {
        let start = Instant::now();
        let result = if city.is_empty() {
            Err(McpError::invalid_params("city must not be empty"))
        } else {
            Ok(format!("Sunny in {city}"))
        };
        McpMetrics::tool_call("get_weather", result.is_ok(), start.elapsed().as_secs_f64());
        result
    }
}
```

## Grafana Dashboards

### Import Pre-built Dashboards

1. Open Grafana: http://localhost:3000
2. Go to Dashboards → Import
3. Search for "TurboMCP" in dashboard library
4. Import and configure data source

### Create Custom Dashboard

```json
{
  "dashboard": {
    "title": "TurboMCP Monitoring",
    "panels": [
      {
        "title": "Request Rate",
        "targets": [
          {
            "expr": "rate(mcp_requests_total[5m])"
          }
        ]
      },
      {
        "title": "Error Rate",
        "targets": [
          {
            "expr": "rate(mcp_requests_total{status=\"error\"}[5m])"
          }
        ]
      },
      {
        "title": "P99 Latency",
        "targets": [
          {
            "expr": "mcp_request_duration_seconds{quantile=\"0.99\"}"
          }
        ]
      }
    ]
  }
}
```

## Alerting

### Prometheus Alert Rules

Create `alerts.yml`:

```yaml
groups:
  - name: turbomcp
    rules:
      # Alert on high error rate
      - alert: HighErrorRate
        expr: |
          (
            sum(rate(mcp_requests_total{status="error"}[5m]))
            /
            sum(rate(mcp_requests_total[5m]))
          ) > 0.05
        for: 5m
        annotations:
          summary: "High error rate detected (>5%)"

      # Alert on slow responses
      - alert: SlowResponses
        expr: |
          mcp_request_duration_seconds{quantile="0.95"} > 1.0
        for: 10m
        annotations:
          summary: "Slow responses detected (>1s)"

      # Alert on service down
      - alert: ServiceDown
        expr: up{job="turbomcp"} == 0
        for: 2m
        annotations:
          summary: "TurboMCP service is down"

      # Alert on high memory usage
      - alert: HighMemoryUsage
        expr: |
          process_resident_memory_bytes{job="turbomcp"}
          > 500 * 1024 * 1024
        for: 5m
        annotations:
          summary: "High memory usage (>500MB)"
```

### Alertmanager Configuration

```yaml
global:
  resolve_timeout: 5m

route:
  receiver: 'default'
  group_by: ['alertname']
  group_wait: 30s
  group_interval: 5m
  repeat_interval: 12h

receivers:
  - name: 'default'
    slack_configs:
      - api_url: 'https://hooks.slack.com/services/...'
        channel: '#alerts'
        title: 'TurboMCP Alert'
        text: '{{ .GroupLabels.alertname }}'
```

## Distributed Tracing

### OpenTelemetry (OTLP)

`turbomcp-telemetry` exports traces over OTLP (HTTP/protobuf), which Jaeger,
Tempo, Honeycomb, and the OpenTelemetry Collector all accept. With the
`telemetry` feature:

```rust
use turbomcp::prelude::*;

fn init_tracing() -> Result<TelemetryGuard, Box<dyn std::error::Error>> {
    let guard = TelemetryConfig::builder()
        .service_name("my-server")
        // The URL is used as-is, so include /v1/traces
        .otlp_endpoint("http://localhost:4318/v1/traces")
        .sampling_ratio(1.0) // Sample all traces
        .build()
        .init()?;
    Ok(guard)
}
```

Keep the returned guard alive for the life of the process; dropping it flushes
and shuts down the exporters.

**View traces in Jaeger at http://localhost:16686**

To give every MCP request over HTTP its own span (method, tool name, status,
duration), continuing the caller's W3C trace context, wrap the server's Axum
router in `TelemetryLayer`:

```rust
use turbomcp::prelude::*;
use turbomcp::telemetry::tower::{TelemetryLayer, TelemetryLayerConfig};

#[derive(Clone)]
struct MyServer;

#[server(name = "my-server", version = "1.0.0")]
impl MyServer {
    /// Say hello
    #[tool]
    async fn hello(&self) -> String {
        "hello".to_string()
    }
}

fn app() -> axum::Router {
    let telemetry = TelemetryLayerConfig::new()
        .service_name("my-server")
        .exclude_method("ping");

    MyServer
        .builder()
        .into_axum_router()
        .layer(TelemetryLayer::new(telemetry))
}
```

### Trace Sampling

For high-traffic production:

```rust
use turbomcp::prelude::*;

let config = TelemetryConfig::builder()
    .service_name("my-server")
    .otlp_endpoint("http://otel-collector:4318/v1/traces")
    .sampling_ratio(0.1) // Sample 10% of requests
    .build();
```

## Structured Logging

### Log Aggregation with ELK

Configure Filebeat to send logs to Elasticsearch:

```yaml
# filebeat.yml
filebeat.inputs:
  - type: log
    enabled: true
    paths:
      - /var/log/turbomcp/*.log
    fields:
      service: turbomcp

output.elasticsearch:
  hosts: ["elasticsearch:9200"]

processors:
  - add_kubernetes_metadata:
  - add_docker_metadata:
```

Query logs in Kibana:

```
service:turbomcp AND level:error
service:turbomcp AND request_id:550e8400*
```

## Docker Monitoring

### Monitor Container Metrics

```bash
# Check container resource usage
docker stats turbomcp-server

# Container name: CPU usage, memory, network I/O
turbomcp-server    5.2%    256MiB / 1GiB    2.1MB / 1.5MB
```

### Cadvisor for Kubernetes

```yaml
apiVersion: v1
kind: Pod
metadata:
  name: cadvisor
spec:
  containers:
  - name: cadvisor
    image: gcr.io/cadvisor/cadvisor:latest
    volumeMounts:
    - name: rootfs
      mountPath: /rootfs
      readOnly: true
    - name: var-run
      mountPath: /var/run
    - name: sys
      mountPath: /sys
      readOnly: true
  volumes:
  - name: rootfs
    hostPath:
      path: /
  - name: var-run
    hostPath:
      path: /var/run
  - name: sys
    hostPath:
      path: /sys
```

## Performance Monitoring

### Monitor Handler Performance

`RequestTimer` records `mcp_requests_total` and `mcp_request_duration_seconds`
for whatever label you give it:

```rust
use turbomcp::prelude::*;
use turbomcp::telemetry::metrics::RequestTimer;

#[derive(Clone)]
struct Reports;

#[server]
impl Reports {
    /// Build the monthly report
    #[tool]
    async fn expensive_operation(&self) -> McpResult<String> {
        let timer = RequestTimer::start("tools/call:expensive_operation");
        let result: McpResult<String> = Ok("report".to_string()); // Do work
        timer.complete(result.is_ok());
        result
    }
}
```

Time database queries the same way, with a label per query.

## Best Practices

### 1. Set Appropriate Alert Thresholds

```yaml
# ✅ Good - based on baseline performance
- alert: SlowResponse
  expr: |
    mcp_request_duration_seconds{quantile="0.95"} > 0.5  # 500ms threshold
  for: 10m

# ❌ Avoid - too sensitive
- alert: SlowResponse
  expr: mcp_request_duration_seconds{quantile="0.5"} > 0.1
```

### 2. Monitor Key Metrics

Focus on:
- Request rate (requests/sec)
- Error rate (% of failed requests)
- Latency (p50, p95, p99)
- Handler-specific metrics
- Resource usage (CPU, memory)
- Dependencies (database, cache)

### 3. Implement Graceful Degradation

When an optional dependency (a cache, say) is down, fall back rather than fail
the tool call, and record that you did so the dashboards show it.

## Troubleshooting

### "Metrics not appearing in Prometheus"

1. Check the exporter answers on its port (`curl http://localhost:9090/metrics`), and that it is bound to an address Prometheus can reach (`prometheus_bind_addr`)
2. Verify Prometheus scrape config:
   ```bash
   curl http://prometheus:9090/config
   ```
3. Check server logs for errors
4. Verify port is open: `netstat -tlnp | grep 8080`

### "High memory usage"

1. Check for memory leaks: `valgrind --leak-check=full ./server`
2. Reduce buffer sizes in configuration
3. Monitor connection count: `mcp_active_connections` (if you record it)
4. Implement connection limits: `builder().with_connection_limit(n)`

### "Missing traces in Jaeger"

1. Verify Jaeger (or the collector) is running and accepts OTLP over HTTP on port 4318: `docker ps | grep jaeger`
2. Check `sampling_ratio` isn't 0
3. Check `otlp_endpoint` includes the `/v1/traces` path
4. Keep the `TelemetryGuard` alive; dropping it early stops export
5. Check application logs for trace errors

## Example: Complete Monitoring Stack

Docker Compose with full monitoring:

```yaml
version: '3'
services:
  turbomcp:
    image: turbomcp-server:latest
    ports:
      - "8080:8080"
    environment:
      RUST_LOG: info
      JAEGER_AGENT_HOST: jaeger
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:8080/health/live"]
      interval: 10s
      timeout: 5s
      retries: 3

  prometheus:
    image: prom/prometheus:latest
    ports:
      - "9090:9090"
    volumes:
      - ./prometheus.yml:/etc/prometheus/prometheus.yml
    command:
      - '--config.file=/etc/prometheus/prometheus.yml'

  grafana:
    image: grafana/grafana:latest
    ports:
      - "3000:3000"
    environment:
      GF_SECURITY_ADMIN_PASSWORD: admin

  jaeger:
    image: jaegertracing/all-in-one:latest
    ports:
      - "16686:16686"
      - "6831:6831/udp"

  elasticsearch:
    image: docker.elastic.co/elasticsearch/elasticsearch:8.0.0
    environment:
      - discovery.type=single-node
    ports:
      - "9200:9200"

  kibana:
    image: docker.elastic.co/kibana/kibana:8.0.0
    ports:
      - "5601:5601"
    depends_on:
      - elasticsearch
```

**Start the stack:**
```bash
docker-compose up -d

# Access dashboards
# Grafana: http://localhost:3000
# Prometheus: http://localhost:9090
# Jaeger: http://localhost:16686
# Kibana: http://localhost:5601
```

## Next Steps

- **[Production Setup](production.md)** - Production configuration and scaling
- **[Docker Deployment](docker.md)** - Container orchestration
- **[Observability Guide](../guide/observability.md)** - Logging and tracing details
- **[Architecture](../architecture/system-design.md)** - System design patterns

