# Monitoring & Observability

Monitor TurboMCP servers in production with comprehensive health checks, metrics, and alerting.

## Health Checks

### Liveness Probes

Detect if the server is running:

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

**Test it:**
```bash
curl http://localhost:8080/health/live
# Returns: {"status":"alive"}
```

### Readiness Probes

Verify the server is ready for traffic:

```bash
curl http://localhost:8080/health/ready
# Returns: {"status":"ready","dependencies":{"database":"up","cache":"up"}}
```

### Custom Health Checks

```rust
let server = McpServer::new()
    .with_custom_health_check(|ctx| async move {
        let db_ok = ctx.database().ping().await.is_ok();
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

Enable metrics export:

```rust
let server = McpServer::new()
    .http(8080)
    .with_prometheus_endpoint("/metrics")
    .run()
    .await?;
```

**Prometheus configuration:**

```yaml
global:
  scrape_interval: 15s
  evaluation_interval: 15s

scrape_configs:
  - job_name: 'turbomcp'
    static_configs:
      - targets: ['localhost:8080']
    metrics_path: '/metrics'
    scrape_interval: 5s
```

### Available Metrics

TurboMCP exposes Prometheus metrics automatically:

```
# REQUEST METRICS
turbomcp_requests_total{handler="hello",status="success"} 150
turbomcp_requests_total{handler="hello",status="error"} 5
turbomcp_request_duration_seconds{handler="hello",quantile="0.95"} 0.025
turbomcp_request_duration_seconds{handler="hello",quantile="0.99"} 0.050

# HANDLER METRICS
turbomcp_handler_calls_total{name="get_weather"} 1000
turbomcp_handler_errors_total{name="get_weather"} 10
turbomcp_handler_duration_seconds{name="get_weather",quantile="0.5"} 0.015

# SYSTEM METRICS
turbomcp_connections_active 42
turbomcp_messages_processed_total 10000
turbomcp_cache_hits_total 8500
turbomcp_cache_misses_total 1500
```

### Custom Metrics

```rust
#[tool]
async fn my_tool(metrics: Metrics) -> McpResult<String> {
    // Increment counter
    metrics.increment("custom_operations", 1)?;

    // Record distribution
    metrics.record("operation_time_ms", 45)?;

    // Set gauge
    metrics.set_gauge("queue_depth", 12)?;

    Ok("Done".to_string())
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
            "expr": "rate(turbomcp_requests_total[5m])"
          }
        ]
      },
      {
        "title": "Error Rate",
        "targets": [
          {
            "expr": "rate(turbomcp_requests_total{status=\"error\"}[5m])"
          }
        ]
      },
      {
        "title": "P99 Latency",
        "targets": [
          {
            "expr": "turbomcp_request_duration_seconds{quantile=\"0.99\"}"
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
            sum(rate(turbomcp_requests_total{status="error"}[5m]))
            /
            sum(rate(turbomcp_requests_total[5m]))
          ) > 0.05
        for: 5m
        annotations:
          summary: "High error rate detected (>5%)"

      # Alert on slow responses
      - alert: SlowResponses
        expr: |
          turbomcp_request_duration_seconds{quantile="0.95"} > 1.0
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

### Jaeger Integration

Enable tracing:

```rust
use turbomcp::tracing::TracingConfig;
use opentelemetry_jaeger;

let tracer = opentelemetry_jaeger::new_agent_pipeline()
    .install_simple()
    .unwrap();

let server = McpServer::new()
    .with_tracing(TracingConfig {
        enabled: true,
        sample_rate: 1.0,  // Sample all traces
    })
    .http(8080)
    .run()
    .await?;
```

**View traces at http://localhost:16686**

### Trace Sampling

For high-traffic production:

```rust
.with_tracing(TracingConfig {
    enabled: true,
    sample_rate: 0.1,  // Sample 10% of requests
})
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

```rust
#[tool]
async fn expensive_operation(
    metrics: Metrics,
) -> McpResult<String> {
    let start = std::time::Instant::now();

    // Do work
    let result = compute().await?;

    let duration_ms = start.elapsed().as_millis() as f64;
    metrics.record("expensive_operation_ms", duration_ms)?;

    Ok(result)
}
```

### Database Query Performance

```rust
#[tool]
async fn query_users(
    database: Database,
    metrics: Metrics,
) -> McpResult<Vec<User>> {
    let start = std::time::Instant::now();

    let users = database.query("SELECT * FROM users")
        .await?;

    metrics.record("db_query_ms", start.elapsed().as_millis() as f64)?;

    Ok(users)
}
```

## Best Practices

### 1. Set Appropriate Alert Thresholds

```yaml
# ✅ Good - based on baseline performance
- alert: SlowResponse
  expr: |
    histogram_quantile(0.95,
      rate(turbomcp_request_duration_seconds_bucket[5m])
    ) > 0.5  # 500ms threshold

# ❌ Avoid - too sensitive
- alert: SlowResponse
  expr: turbomcp_request_duration_seconds > 0.1
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

```rust
// Degrade cache performance but stay operational
#[tool]
async fn get_data(
    cache: Cache,
    database: Database,
) -> McpResult<Data> {
    if cache.is_healthy().await {
        return cache.get("data").await;
    }

    // Cache down - fetch from DB directly
    database.query().await
}
```

## Troubleshooting

### "Metrics not appearing in Prometheus"

1. Check `/metrics` endpoint exists
2. Verify Prometheus scrape config:
   ```bash
   curl http://prometheus:9090/config
   ```
3. Check server logs for errors
4. Verify port is open: `netstat -tlnp | grep 8080`

### "High memory usage"

1. Check for memory leaks: `valgrind --leak-check=full ./server`
2. Reduce buffer sizes in configuration
3. Monitor connection count: `turbomcp_connections_active`
4. Implement connection limits

### "Missing traces in Jaeger"

1. Verify Jaeger agent is running: `docker ps | grep jaeger`
2. Check trace sampling rate isn't 0
3. Verify JAEGER_AGENT_HOST environment variable
4. Check application logs for trace errors

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

