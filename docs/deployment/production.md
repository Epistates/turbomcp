# Production Deployment

Complete guide to deploying TurboMCP servers in production environments, including configuration management, graceful shutdown, error handling, scaling, and operational best practices.

## Production Checklist

### Pre-Deployment

- [ ] **Security audit completed**
  - [ ] Dependencies scanned for vulnerabilities
  - [ ] Authentication/authorization configured
  - [ ] Secrets management in place
  - [ ] TLS/SSL certificates configured
  - [ ] CORS policies reviewed

- [ ] **Performance testing completed**
  - [ ] Load testing at expected traffic levels
  - [ ] Stress testing at 2x expected traffic
  - [ ] Memory leak testing (24+ hour runs)
  - [ ] Database connection pooling verified
  - [ ] Cache hit rates optimized

- [ ] **Monitoring configured**
  - [ ] Health check endpoints implemented
  - [ ] Metrics collection configured
  - [ ] Log aggregation setup
  - [ ] Alerts configured for critical metrics
  - [ ] Dashboards created

- [ ] **Disaster recovery prepared**
  - [ ] Backup strategy implemented
  - [ ] Recovery procedures documented
  - [ ] Failover tested
  - [ ] Database migrations tested
  - [ ] Rollback procedures verified

- [ ] **Documentation complete**
  - [ ] Runbook created
  - [ ] API documentation published
  - [ ] Deployment guide written
  - [ ] Troubleshooting guide available
  - [ ] Architecture diagrams updated

## Configuration Management

### Environment Variables

Production-grade environment configuration:

```rust
use config::{Config, ConfigError, Environment, File};
use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct ServerConfig {
    pub server: ServerSettings,
    pub database: DatabaseSettings,
    pub redis: RedisSettings,
    pub security: SecuritySettings,
    pub observability: ObservabilitySettings,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServerSettings {
    pub host: String,
    pub port: u16,
    pub workers: usize,
    pub max_connections: usize,
    pub keep_alive: u64,
    pub request_timeout: u64,
    pub graceful_shutdown_timeout: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DatabaseSettings {
    pub url: String,
    pub max_connections: u32,
    pub min_connections: u32,
    pub connect_timeout: u64,
    pub idle_timeout: u64,
    pub max_lifetime: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct SecuritySettings {
    pub cors_allowed_origins: Vec<String>,
    pub rate_limit_requests: usize,
    pub rate_limit_window: u64,
    pub max_request_body_size: usize,
    pub require_https: bool,
}

impl ServerConfig {
    pub fn from_env() -> Result<Self, ConfigError> {
        let environment = std::env::var("APP_ENV").unwrap_or_else(|_| "development".into());

        let config = Config::builder()
            // Start with default config
            .add_source(File::with_name("config/default"))
            // Layer environment-specific config
            .add_source(File::with_name(&format!("config/{}", environment)).required(false))
            // Override with environment variables
            .add_source(Environment::with_prefix("APP").separator("__"))
            .build()?;

        config.try_deserialize()
    }
}
```

### Configuration Files

**config/default.toml:**

```toml
[server]
host = "0.0.0.0"
port = 8080
workers = 0  # Auto-detect
max_connections = 10000
keep_alive = 75
request_timeout = 30
graceful_shutdown_timeout = 30

[database]
max_connections = 50
min_connections = 10
connect_timeout = 30
idle_timeout = 600
max_lifetime = 1800

[security]
cors_allowed_origins = ["*"]
rate_limit_requests = 1000
rate_limit_window = 60
max_request_body_size = 1048576  # 1MB
require_https = false

[observability]
log_level = "info"
enable_metrics = true
enable_tracing = true
metrics_port = 9090
```

**config/production.toml:**

```toml
[server]
workers = 0  # Use all CPU cores
max_connections = 50000
graceful_shutdown_timeout = 60

[database]
max_connections = 100
min_connections = 20

[security]
cors_allowed_origins = ["https://yourdomain.com"]
rate_limit_requests = 100
max_request_body_size = 524288  # 512KB
require_https = true

[observability]
log_level = "warn"
enable_metrics = true
enable_tracing = false  # Reduce overhead
```

### Secrets Management

**Using environment variables (development):**

```bash
export DATABASE_URL="postgresql://user:pass@localhost/db"
export REDIS_URL="redis://localhost:6379"
export JWT_SECRET="your-secret-key"
```

**Using HashiCorp Vault (production):**

```rust
use vaultrs::client::{VaultClient, VaultClientSettingsBuilder};

pub async fn load_secrets() -> Result<Secrets, Box<dyn std::error::Error>> {
    let vault_addr = std::env::var("VAULT_ADDR")?;
    let vault_token = std::env::var("VAULT_TOKEN")?;

    let client = VaultClient::new(
        VaultClientSettingsBuilder::default()
            .address(&vault_addr)
            .token(&vault_token)
            .build()?
    )?;

    let secrets: Secrets = vaultrs::kv2::read(&client, "mcp-server", "production").await?;

    Ok(secrets)
}
```

**Using AWS Secrets Manager:**

```rust
use aws_sdk_secretsmanager::Client;

pub async fn load_aws_secrets() -> Result<Secrets, Box<dyn std::error::Error>> {
    let config = aws_config::load_from_env().await;
    let client = Client::new(&config);

    let response = client
        .get_secret_value()
        .secret_id("mcp-server/production")
        .send()
        .await?;

    let secret_string = response.secret_string().unwrap();
    let secrets: Secrets = serde_json::from_str(secret_string)?;

    Ok(secrets)
}
```

## Graceful Shutdown

### Signal Handling

Implement graceful shutdown to avoid dropping in-flight requests:

```rust
use tokio::signal;
use std::sync::Arc;
use tokio::sync::Notify;

pub async fn shutdown_signal(notify: Arc<Notify>) {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("Failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            tracing::info!("Received Ctrl+C, initiating graceful shutdown");
        },
        _ = terminate => {
            tracing::info!("Received SIGTERM, initiating graceful shutdown");
        },
    }

    notify.notify_waiters();
}

pub async fn run_server(config: ServerConfig) -> Result<(), Box<dyn std::error::Error>> {
    let notify = Arc::new(Notify::new());
    let notify_clone = notify.clone();

    // Spawn shutdown signal handler
    tokio::spawn(async move {
        shutdown_signal(notify_clone).await;
    });

    let server = axum::Server::bind(&config.server.addr())
        .serve(app.into_make_service())
        .with_graceful_shutdown(async move {
            notify.notified().await;
        });

    tracing::info!("Server listening on {}", config.server.addr());

    server.await?;

    tracing::info!("Server shutdown complete");

    Ok(())
}
```

### Connection Draining

Drain active connections before shutdown:

```rust
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

pub struct ConnectionTracker {
    active: Arc<AtomicUsize>,
}

impl ConnectionTracker {
    pub fn new() -> Self {
        Self {
            active: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub fn increment(&self) {
        self.active.fetch_add(1, Ordering::SeqCst);
    }

    pub fn decrement(&self) {
        self.active.fetch_sub(1, Ordering::SeqCst);
    }

    pub fn count(&self) -> usize {
        self.active.load(Ordering::SeqCst)
    }

    pub async fn wait_for_zero(&self, timeout: Duration) -> bool {
        let start = std::time::Instant::now();

        while self.count() > 0 {
            if start.elapsed() >= timeout {
                tracing::warn!(
                    "Shutdown timeout reached with {} active connections",
                    self.count()
                );
                return false;
            }

            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        true
    }
}

// Usage in request handler
async fn handle_request(
    tracker: Arc<ConnectionTracker>,
    req: Request,
) -> Response {
    tracker.increment();

    let response = process_request(req).await;

    tracker.decrement();

    response
}
```

### Database Connection Cleanup

Properly close database connections:

```rust
pub async fn graceful_shutdown(
    db: Pool<Postgres>,
    redis: redis::Client,
    tracker: Arc<ConnectionTracker>,
) {
    tracing::info!("Starting graceful shutdown sequence");

    // 1. Stop accepting new connections
    tracing::info!("Stopped accepting new connections");

    // 2. Wait for active requests to complete
    let timeout = Duration::from_secs(30);
    if tracker.wait_for_zero(timeout).await {
        tracing::info!("All active connections drained");
    } else {
        tracing::warn!("Forced shutdown with active connections");
    }

    // 3. Close database connections
    tracing::info!("Closing database pool");
    db.close().await;

    // 4. Close Redis connections
    tracing::info!("Closing Redis connections");
    drop(redis);

    tracing::info!("Graceful shutdown complete");
}
```

## Error Handling

### Production Error Types

Structured error handling with context:

```rust
use thiserror::Error;
use serde::Serialize;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Redis error: {0}")]
    Redis(#[from] redis::RedisError),

    #[error("Configuration error: {0}")]
    Config(#[from] config::ConfigError),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Unauthorized")]
    Unauthorized,

    #[error("Rate limit exceeded")]
    RateLimitExceeded,

    #[error("Internal server error")]
    Internal(#[from] anyhow::Error),
}

#[derive(Serialize)]
pub struct ErrorResponse {
    pub error: String,
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
    pub request_id: String,
}

impl AppError {
    pub fn to_response(&self, request_id: String) -> (StatusCode, Json<ErrorResponse>) {
        let (status, code, message) = match self {
            AppError::Database(e) => {
                tracing::error!("Database error: {:?}", e);
                (StatusCode::INTERNAL_SERVER_ERROR, "DATABASE_ERROR", "Database operation failed")
            }
            AppError::Redis(e) => {
                tracing::error!("Redis error: {:?}", e);
                (StatusCode::INTERNAL_SERVER_ERROR, "CACHE_ERROR", "Cache operation failed")
            }
            AppError::Validation(msg) => {
                (StatusCode::BAD_REQUEST, "VALIDATION_ERROR", msg.as_str())
            }
            AppError::NotFound(msg) => {
                (StatusCode::NOT_FOUND, "NOT_FOUND", msg.as_str())
            }
            AppError::Unauthorized => {
                (StatusCode::UNAUTHORIZED, "UNAUTHORIZED", "Authentication required")
            }
            AppError::RateLimitExceeded => {
                (StatusCode::TOO_MANY_REQUESTS, "RATE_LIMIT", "Rate limit exceeded")
            }
            _ => {
                tracing::error!("Internal error: {:?}", self);
                (StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR", "An internal error occurred")
            }
        };

        let response = ErrorResponse {
            error: self.to_string(),
            code: code.to_string(),
            message: message.to_string(),
            details: None,
            request_id,
        };

        (status, Json(response))
    }
}
```

### Error Logging

Structured error logging with context:

```rust
use tracing::{error, warn, info, instrument};

#[instrument(skip(db), fields(user_id = %user_id))]
pub async fn get_user(
    db: &Pool<Postgres>,
    user_id: i64,
) -> Result<User, AppError> {
    sqlx::query_as!(User, "SELECT * FROM users WHERE id = $1", user_id)
        .fetch_optional(db)
        .await
        .map_err(|e| {
            error!(
                error = %e,
                user_id = %user_id,
                "Failed to fetch user from database"
            );
            AppError::Database(e)
        })?
        .ok_or_else(|| {
            warn!(user_id = %user_id, "User not found");
            AppError::NotFound(format!("User {} not found", user_id))
        })
}
```

### Panic Recovery

Recover from panics without crashing:

```rust
use std::panic::{catch_unwind, AssertUnwindSafe};

pub async fn safe_execute<F, T>(
    operation: F,
    context: &str,
) -> Result<T, AppError>
where
    F: FnOnce() -> Result<T, AppError> + std::panic::UnwindSafe,
{
    match catch_unwind(AssertUnwindSafe(operation)) {
        Ok(result) => result,
        Err(panic_error) => {
            let panic_msg = if let Some(s) = panic_error.downcast_ref::<String>() {
                s.clone()
            } else if let Some(s) = panic_error.downcast_ref::<&str>() {
                s.to_string()
            } else {
                "Unknown panic".to_string()
            };

            error!(
                context = %context,
                panic = %panic_msg,
                "Recovered from panic"
            );

            Err(AppError::Internal(anyhow::anyhow!(
                "Operation panicked: {}",
                panic_msg
            )))
        }
    }
}
```

## Scaling Strategies

### Horizontal Scaling

Load balancing across multiple instances:

**Nginx configuration:**

```nginx
upstream mcp_servers {
    least_conn;  # Use least connections algorithm

    server mcp-server-1:8080 max_fails=3 fail_timeout=30s;
    server mcp-server-2:8080 max_fails=3 fail_timeout=30s;
    server mcp-server-3:8080 max_fails=3 fail_timeout=30s;

    keepalive 32;
}

server {
    listen 80;
    server_name api.example.com;

    location / {
        proxy_pass http://mcp_servers;
        proxy_http_version 1.1;

        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;

        proxy_connect_timeout 60s;
        proxy_send_timeout 60s;
        proxy_read_timeout 60s;

        # Connection reuse
        proxy_set_header Connection "";

        # Health check
        proxy_next_upstream error timeout invalid_header http_500 http_502 http_503;
    }

    location /health {
        access_log off;
        proxy_pass http://mcp_servers;
    }
}
```

**AWS Application Load Balancer (Terraform):**

```hcl
resource "aws_lb" "mcp" {
  name               = "mcp-alb"
  internal           = false
  load_balancer_type = "application"
  security_groups    = [aws_security_group.alb.id]
  subnets            = aws_subnet.public[*].id

  enable_deletion_protection = true
  enable_http2              = true
  enable_cross_zone_load_balancing = true

  tags = {
    Environment = "production"
  }
}

resource "aws_lb_target_group" "mcp" {
  name     = "mcp-tg"
  port     = 8080
  protocol = "HTTP"
  vpc_id   = aws_vpc.main.id

  health_check {
    enabled             = true
    healthy_threshold   = 2
    unhealthy_threshold = 3
    timeout             = 5
    interval            = 30
    path                = "/health"
    matcher             = "200"
  }

  deregistration_delay = 30

  stickiness {
    type            = "lb_cookie"
    cookie_duration = 3600
    enabled         = false
  }
}

resource "aws_lb_listener" "mcp" {
  load_balancer_arn = aws_lb.mcp.arn
  port              = "443"
  protocol          = "HTTPS"
  ssl_policy        = "ELBSecurityPolicy-TLS-1-2-2017-01"
  certificate_arn   = aws_acm_certificate.cert.arn

  default_action {
    type             = "forward"
    target_group_arn = aws_lb_target_group.mcp.arn
  }
}
```

### Auto-Scaling

Kubernetes Horizontal Pod Autoscaler:

```yaml
apiVersion: autoscaling/v2
kind: HorizontalPodAutoscaler
metadata:
  name: mcp-server-hpa
  namespace: production
spec:
  scaleTargetRef:
    apiVersion: apps/v1
    kind: Deployment
    name: mcp-server
  minReplicas: 3
  maxReplicas: 20
  metrics:
  - type: Resource
    resource:
      name: cpu
      target:
        type: Utilization
        averageUtilization: 70
  - type: Resource
    resource:
      name: memory
      target:
        type: Utilization
        averageUtilization: 80
  - type: Pods
    pods:
      metric:
        name: http_requests_per_second
      target:
        type: AverageValue
        averageValue: "1000"
  behavior:
    scaleDown:
      stabilizationWindowSeconds: 300
      policies:
      - type: Percent
        value: 50
        periodSeconds: 60
    scaleUp:
      stabilizationWindowSeconds: 0
      policies:
      - type: Percent
        value: 100
        periodSeconds: 30
      - type: Pods
        value: 4
        periodSeconds: 30
      selectPolicy: Max
```

### Database Scaling

**Read replicas:**

```rust
use sqlx::{Pool, Postgres};
use std::sync::Arc;

pub struct DatabasePools {
    pub primary: Arc<Pool<Postgres>>,
    pub replicas: Vec<Arc<Pool<Postgres>>>,
    current_replica: AtomicUsize,
}

impl DatabasePools {
    pub async fn new(config: &DatabaseConfig) -> Result<Self, sqlx::Error> {
        let primary = Pool::connect(&config.primary_url).await?;

        let mut replicas = Vec::new();
        for replica_url in &config.replica_urls {
            let pool = Pool::connect(replica_url).await?;
            replicas.push(Arc::new(pool));
        }

        Ok(Self {
            primary: Arc::new(primary),
            replicas,
            current_replica: AtomicUsize::new(0),
        })
    }

    pub fn get_primary(&self) -> &Pool<Postgres> {
        &self.primary
    }

    pub fn get_replica(&self) -> &Pool<Postgres> {
        if self.replicas.is_empty() {
            return &self.primary;
        }

        let idx = self.current_replica.fetch_add(1, Ordering::Relaxed) % self.replicas.len();
        &self.replicas[idx]
    }
}

// Usage
pub async fn get_user(pools: &DatabasePools, user_id: i64) -> Result<User, AppError> {
    // Use replica for reads
    sqlx::query_as!(User, "SELECT * FROM users WHERE id = $1", user_id)
        .fetch_one(pools.get_replica())
        .await
        .map_err(AppError::Database)
}

pub async fn create_user(pools: &DatabasePools, user: NewUser) -> Result<User, AppError> {
    // Use primary for writes
    sqlx::query_as!(
        User,
        "INSERT INTO users (name, email) VALUES ($1, $2) RETURNING *",
        user.name,
        user.email
    )
    .fetch_one(pools.get_primary())
    .await
    .map_err(AppError::Database)
}
```

## Rate Limiting

### Token Bucket Implementation

```rust
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

pub struct RateLimiter {
    capacity: usize,
    tokens: Mutex<usize>,
    refill_rate: usize,
    last_refill: Mutex<Instant>,
}

impl RateLimiter {
    pub fn new(capacity: usize, refill_per_second: usize) -> Self {
        Self {
            capacity,
            tokens: Mutex::new(capacity),
            refill_rate: refill_per_second,
            last_refill: Mutex::new(Instant::now()),
        }
    }

    pub async fn check(&self) -> bool {
        let mut tokens = self.tokens.lock().await;
        let mut last_refill = self.last_refill.lock().await;

        // Refill tokens based on time elapsed
        let now = Instant::now();
        let elapsed = now.duration_since(*last_refill);
        let refill_amount = (elapsed.as_secs_f64() * self.refill_rate as f64) as usize;

        if refill_amount > 0 {
            *tokens = (*tokens + refill_amount).min(self.capacity);
            *last_refill = now;
        }

        if *tokens > 0 {
            *tokens -= 1;
            true
        } else {
            false
        }
    }
}

// Middleware
pub async fn rate_limit_middleware(
    State(limiter): State<Arc<RateLimiter>>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    if !limiter.check().await {
        return Err(StatusCode::TOO_MANY_REQUESTS);
    }

    Ok(next.run(request).await)
}
```

### Redis-Based Rate Limiting

```rust
use redis::AsyncCommands;

pub async fn check_rate_limit(
    redis: &redis::Client,
    key: &str,
    max_requests: usize,
    window_secs: usize,
) -> Result<bool, redis::RedisError> {
    let mut conn = redis.get_multiplexed_async_connection().await?;

    let count: usize = conn.incr(key, 1).await?;

    if count == 1 {
        conn.expire(key, window_secs).await?;
    }

    Ok(count <= max_requests)
}

// Usage in handler
pub async fn rate_limited_handler(
    State(redis): State<redis::Client>,
    request: Request,
) -> Result<Response, AppError> {
    let client_ip = request
        .headers()
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown");

    let key = format!("rate_limit:{}", client_ip);

    if !check_rate_limit(&redis, &key, 100, 60).await? {
        return Err(AppError::RateLimitExceeded);
    }

    // Process request
    Ok(Response::new("Success"))
}
```

## Circuit Breaker

Prevent cascading failures:

```rust
use std::sync::Arc;
use tokio::sync::RwLock;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq)]
pub enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}

pub struct CircuitBreaker {
    state: Arc<RwLock<CircuitState>>,
    failure_count: Arc<RwLock<usize>>,
    last_failure_time: Arc<RwLock<Option<Instant>>>,
    threshold: usize,
    timeout: Duration,
}

impl CircuitBreaker {
    pub fn new(threshold: usize, timeout: Duration) -> Self {
        Self {
            state: Arc::new(RwLock::new(CircuitState::Closed)),
            failure_count: Arc::new(RwLock::new(0)),
            last_failure_time: Arc::new(RwLock::new(None)),
            threshold,
            timeout,
        }
    }

    pub async fn call<F, T, E>(&self, operation: F) -> Result<T, E>
    where
        F: FnOnce() -> Result<T, E>,
    {
        // Check if we should attempt the operation
        let state = self.state.read().await;
        match *state {
            CircuitState::Open => {
                let last_failure = self.last_failure_time.read().await;
                if let Some(time) = *last_failure {
                    if time.elapsed() > self.timeout {
                        drop(state);
                        drop(last_failure);
                        // Try half-open
                        *self.state.write().await = CircuitState::HalfOpen;
                    } else {
                        return Err(/* Circuit open error */);
                    }
                }
            }
            _ => {}
        }
        drop(state);

        // Execute operation
        match operation() {
            Ok(result) => {
                self.on_success().await;
                Ok(result)
            }
            Err(e) => {
                self.on_failure().await;
                Err(e)
            }
        }
    }

    async fn on_success(&self) {
        *self.failure_count.write().await = 0;
        *self.state.write().await = CircuitState::Closed;
    }

    async fn on_failure(&self) {
        let mut count = self.failure_count.write().await;
        *count += 1;

        if *count >= self.threshold {
            *self.state.write().await = CircuitState::Open;
            *self.last_failure_time.write().await = Some(Instant::now());
        }
    }
}
```

## Health Checks

Comprehensive health check implementation:

```rust
use axum::{Router, Json};
use serde::Serialize;

#[derive(Serialize)]
pub struct HealthStatus {
    pub status: String,
    pub version: String,
    pub uptime: u64,
    pub checks: HealthChecks,
}

#[derive(Serialize)]
pub struct HealthChecks {
    pub database: CheckResult,
    pub redis: CheckResult,
    pub disk_space: CheckResult,
    pub memory: CheckResult,
}

#[derive(Serialize)]
pub struct CheckResult {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

pub async fn health_check(
    State(state): State<Arc<AppState>>,
) -> Json<HealthStatus> {
    let start_time = state.start_time;
    let uptime = start_time.elapsed().as_secs();

    let checks = HealthChecks {
        database: check_database(&state.db).await,
        redis: check_redis(&state.redis).await,
        disk_space: check_disk_space().await,
        memory: check_memory().await,
    };

    let status = if checks.database.status == "healthy"
        && checks.redis.status == "healthy"
        && checks.disk_space.status == "healthy"
        && checks.memory.status == "healthy"
    {
        "healthy"
    } else {
        "degraded"
    };

    Json(HealthStatus {
        status: status.to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        uptime,
        checks,
    })
}

async fn check_database(db: &Pool<Postgres>) -> CheckResult {
    match sqlx::query("SELECT 1").execute(db).await {
        Ok(_) => CheckResult {
            status: "healthy".to_string(),
            message: None,
            details: None,
        },
        Err(e) => CheckResult {
            status: "unhealthy".to_string(),
            message: Some(format!("Database error: {}", e)),
            details: None,
        },
    }
}
```

## Zero-Downtime Deployments

### Blue-Green Deployment

```bash
#!/bin/bash
# blue-green-deploy.sh

set -e

BLUE_ENV="mcp-server-blue"
GREEN_ENV="mcp-server-green"
LB_TARGET_GROUP="mcp-tg"

# Determine current active environment
CURRENT=$(aws elbv2 describe-target-groups \
  --names $LB_TARGET_GROUP \
  --query 'TargetGroups[0].Tags[?Key==`active-env`].Value' \
  --output text)

if [ "$CURRENT" == "blue" ]; then
    INACTIVE="green"
    INACTIVE_ENV=$GREEN_ENV
else
    INACTIVE="blue"
    INACTIVE_ENV=$BLUE_ENV
fi

echo "Current active: $CURRENT"
echo "Deploying to: $INACTIVE"

# Deploy new version to inactive environment
echo "Deploying new version..."
docker-compose -f docker-compose.$INACTIVE.yml up -d

# Wait for health checks
echo "Waiting for health checks..."
sleep 30

# Check health
HEALTH=$(curl -f http://$INACTIVE_ENV:8080/health || echo "failed")

if [ "$HEALTH" == "failed" ]; then
    echo "Health check failed. Rolling back..."
    docker-compose -f docker-compose.$INACTIVE.yml down
    exit 1
fi

# Switch load balancer traffic
echo "Switching traffic to $INACTIVE..."
aws elbv2 modify-target-group \
  --target-group-arn $LB_TARGET_GROUP \
  --tags Key=active-env,Value=$INACTIVE

# Drain old environment
echo "Draining $CURRENT environment..."
sleep 60

# Stop old environment
echo "Stopping $CURRENT environment..."
docker-compose -f docker-compose.$CURRENT.yml down

echo "Deployment complete!"
```

## See Also

- [Docker Deployment](./docker.md) - Container configuration
- [Monitoring](./monitoring.md) - Observability and metrics
- [Architecture Guide](../guide/architecture.md) - System design
- [12-Factor App](https://12factor.net/) - Best practices
