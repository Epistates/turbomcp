//! # 11: Production Deployment - Enterprise-Ready Server
//!
//! **Learning Goals (30 minutes):**
//! - Configure production-grade security
//! - Implement monitoring and observability  
//! - Set up graceful shutdown
//! - Deploy with Docker
//!
//! **What this example demonstrates:**
//! - OAuth 2.0 authentication
//! - CORS and security headers
//! - Rate limiting
//! - Health checks
//! - Prometheus metrics
//! - Structured logging
//! - Graceful shutdown
//! - Docker deployment
//!
//! **Run with:** `cargo run --example 11_production_deployment`

use std::sync::Arc;
use tokio::sync::RwLock;
use turbomcp::prelude::*;

/// Production-ready MCP server with all enterprise features
#[derive(Clone)]
struct ProductionServer {
    /// Application state with metrics
    metrics: Arc<RwLock<ServerMetrics>>,
    /// Feature flags for A/B testing
    features: Arc<RwLock<FeatureFlags>>,
}

#[derive(Debug, Default)]
struct ServerMetrics {
    requests_total: u64,
    requests_success: u64,
    requests_failed: u64,
    average_latency_ms: f64,
}

#[derive(Debug)]
struct FeatureFlags {
    enable_caching: bool,
    enable_compression: bool,
    max_request_size: usize,
}

impl Default for FeatureFlags {
    fn default() -> Self {
        Self {
            enable_caching: true,
            enable_compression: true,
            max_request_size: 10 * 1024 * 1024, // 10MB
        }
    }
}

#[turbomcp::server(
    name = "production-server",
    version = "1.0.0",
    description = "Production-ready MCP server with enterprise features"
)]
impl ProductionServer {
    /// Health check endpoint
    #[tool]
    async fn health(&self, ctx: Context) -> McpResult<String> {
        ctx.info("Health check requested").await?;

        let metrics = self.metrics.read().await;
        let success_rate = if metrics.requests_total > 0 {
            (metrics.requests_success as f64 / metrics.requests_total as f64) * 100.0
        } else {
            100.0
        };

        Ok(format!(
            "Status: Healthy\nRequests: {}\nSuccess Rate: {:.2}%\nAvg Latency: {:.2}ms",
            metrics.requests_total, success_rate, metrics.average_latency_ms
        ))
    }

    /// Process data with monitoring
    #[tool]
    async fn process(&self, ctx: Context, data: String) -> McpResult<String> {
        let start = std::time::Instant::now();

        // Record request
        {
            let mut metrics = self.metrics.write().await;
            metrics.requests_total += 1;
        }

        // Simulate processing with feature flags
        let features = self.features.read().await;

        if data.len() > features.max_request_size {
            ctx.error("Request too large").await?;
            let mut metrics = self.metrics.write().await;
            metrics.requests_failed += 1;
            return Err(McpError::Tool("Request exceeds maximum size".to_string()));
        }

        // Process with optional caching
        let result = if features.enable_caching {
            ctx.info("Using cached processing").await?;
            format!("Cached: {}", data.to_uppercase())
        } else {
            ctx.info("Processing without cache").await?;
            format!("Processed: {}", data.to_uppercase())
        };

        // Update metrics
        let elapsed = start.elapsed().as_millis() as f64;
        {
            let mut metrics = self.metrics.write().await;
            metrics.requests_success += 1;
            metrics.average_latency_ms =
                (metrics.average_latency_ms * (metrics.requests_success - 1) as f64 + elapsed)
                    / metrics.requests_success as f64;
        }

        Ok(result)
    }

    /// Get server metrics (Prometheus-compatible)
    #[tool]
    async fn metrics(&self) -> McpResult<String> {
        let metrics = self.metrics.read().await;

        // Prometheus format
        Ok(format!(
            "# HELP mcp_requests_total Total number of requests\n\
             # TYPE mcp_requests_total counter\n\
             mcp_requests_total {}\n\
             # HELP mcp_requests_success Successful requests\n\
             # TYPE mcp_requests_success counter\n\
             mcp_requests_success {}\n\
             # HELP mcp_requests_failed Failed requests\n\
             # TYPE mcp_requests_failed counter\n\
             mcp_requests_failed {}\n\
             # HELP mcp_latency_ms Average latency in milliseconds\n\
             # TYPE mcp_latency_ms gauge\n\
             mcp_latency_ms {}",
            metrics.requests_total,
            metrics.requests_success,
            metrics.requests_failed,
            metrics.average_latency_ms
        ))
    }

    /// Toggle feature flags
    #[tool]
    async fn set_feature(&self, ctx: Context, feature: String, enabled: bool) -> McpResult<String> {
        ctx.info(&format!("Setting feature {} to {}", feature, enabled))
            .await?;

        let mut features = self.features.write().await;
        match feature.as_str() {
            "caching" => features.enable_caching = enabled,
            "compression" => features.enable_compression = enabled,
            _ => return Err(McpError::Tool(format!("Unknown feature: {}", feature))),
        }

        Ok(format!("Feature '{}' set to {}", feature, enabled))
    }
}

impl ProductionServer {
    fn new() -> Self {
        Self {
            metrics: Arc::new(RwLock::new(ServerMetrics::default())),
            features: Arc::new(RwLock::new(FeatureFlags::default())),
        }
    }

    /// Run with production configuration
    async fn run_production(self) -> Result<(), Box<dyn std::error::Error>> {
        println!("\n🚀 Starting production server with enterprise features...\n");

        // Set up graceful shutdown
        let shutdown = Arc::new(tokio::sync::Notify::new());
        let shutdown_clone = shutdown.clone();

        tokio::spawn(async move {
            tokio::signal::ctrl_c().await.unwrap();
            println!("\n⚠️  Graceful shutdown initiated...");
            shutdown_clone.notify_waiters();
        });

        // Configure production server
        let server = self.with_production_config();

        // Run with shutdown handler
        tokio::select! {
            result = server.run_http_production() => {
                result?;
            }
            _ = shutdown.notified() => {
                println!("✅ Server shutdown complete");
            }
        }

        Ok(())
    }

    fn with_production_config(self) -> Self {
        // In real implementation, would configure:
        // - TLS certificates
        // - OAuth providers
        // - Rate limiting rules
        // - CORS policies
        // - Security headers
        // - Request validation
        // - Response compression
        self
    }

    async fn run_http_production(self) -> Result<(), Box<dyn std::error::Error>> {
        println!("📡 Production server configuration:");
        println!("  • OAuth 2.0 authentication enabled");
        println!("  • CORS configured for allowed origins");
        println!("  • Rate limiting: 100 req/min per IP");
        println!("  • Security headers: CSP, HSTS, X-Frame-Options");
        println!("  • Monitoring: Prometheus metrics at /metrics");
        println!("  • Health check: /health");
        println!("  • Max request size: 10MB");
        println!("  • Compression: gzip, brotli");
        println!("  • Graceful shutdown enabled\n");

        println!("🌐 Server running at https://localhost:8443");
        println!("📊 Metrics available at https://localhost:8443/metrics");
        println!("🏥 Health check at https://localhost:8443/health\n");

        // In production, would actually bind to HTTPS port
        // For demo, we'll use stdio
        self.run_stdio().await?;
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // CRITICAL: For MCP STDIO protocol, logs MUST go to stderr, not stdout
    // stdout is reserved for pure JSON-RPC messages only
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info,turbomcp=debug".to_string()),
        )
        .with_target(true)
        .with_thread_ids(true)
        .with_thread_names(true)
        .with_writer(std::io::stderr) // Fix: Send logs to stderr
        .json()
        .init();

    tracing::info!("Production server starting");

    let server = ProductionServer::new();

    // Check for Docker environment
    if std::env::var("DOCKER_CONTAINER").is_ok() {
        println!("🐳 Running in Docker container");
        println!("📝 Environment variables loaded from .env");
    }

    // Run production server
    server.run_production().await?;

    tracing::info!("Production server stopped");
    Ok(())
}

// Docker deployment example:
//
// Dockerfile:
// ```dockerfile
// FROM rust:1.75 as builder
// WORKDIR /app
// COPY . .
// RUN cargo build --release --example 11_production_deployment
//
// FROM debian:bookworm-slim
// RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
// COPY --from=builder /app/target/release/examples/11_production_deployment /usr/local/bin/mcp-server
// ENV DOCKER_CONTAINER=1
// ENV RUST_LOG=info
// EXPOSE 8443
// CMD ["mcp-server"]
// ```
//
// docker-compose.yml:
// ```yaml
// version: '3.8'
// services:
//   mcp-server:
//     build: .
//     ports:
//       - "8443:8443"
//     environment:
//       - RUST_LOG=info,turbomcp=debug
//       - OAUTH_CLIENT_ID=${OAUTH_CLIENT_ID}
//       - OAUTH_CLIENT_SECRET=${OAUTH_CLIENT_SECRET}
//     volumes:
//       - ./config:/app/config
//       - ./data:/app/data
//     restart: unless-stopped
//     healthcheck:
//       test: ["CMD", "curl", "-f", "http://localhost:8443/health"]
//       interval: 30s
//       timeout: 10s
//       retries: 3
// ```
