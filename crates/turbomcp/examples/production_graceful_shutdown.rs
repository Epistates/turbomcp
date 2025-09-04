//! # Graceful Shutdown Example
//!
//! This example demonstrates graceful shutdown patterns for TurboMCP servers
//! in production environments, including:
//!
//! - Signal-based shutdown (SIGTERM, SIGINT)
//! - Coordinated multi-service shutdown
//! - Container orchestration (Docker, Kubernetes)
//! - Health check coordination with load balancers
//! - Maintenance mode and planned downtime
//!
//! ## Usage
//!
//! ```bash
//! cargo run --example graceful_shutdown
//! ```
//!
//! ## Testing Shutdown
//!
//! - Ctrl+C (SIGINT) - Graceful shutdown with cleanup
//! - Kill -TERM <pid> (SIGTERM) - Production graceful shutdown
//! - Container stop signals - Orchestrated termination

use std::time::Duration;
use tokio::time::sleep;
use turbomcp::prelude::*;

/// Production server with business logic
#[derive(Clone)]
struct ProductionServer {
    service_name: String,
    version: String,
    started_at: std::time::SystemTime,
}

#[server]
impl ProductionServer {
    #[tool("Get service health status")]
    async fn health_check(&self) -> McpResult<serde_json::Value> {
        let uptime = self.started_at.elapsed().map(|d| d.as_secs()).unwrap_or(0);

        Ok(serde_json::json!({
            "service": self.service_name,
            "version": self.version,
            "status": "healthy",
            "uptime_seconds": uptime,
            "timestamp": chrono::Utc::now().to_rfc3339()
        }))
    }

    #[tool("Process critical business operation")]
    async fn process_operation(&self, operation_id: String) -> McpResult<String> {
        // Simulate processing time
        sleep(Duration::from_millis(100)).await;

        Ok(format!(
            "Operation {} processed successfully by {} v{}",
            operation_id, self.service_name, self.version
        ))
    }

    #[tool("Get service metrics")]
    async fn get_metrics(&self) -> McpResult<serde_json::Value> {
        Ok(serde_json::json!({
            "service": self.service_name,
            "active_connections": 42,
            "requests_per_second": 1337,
            "memory_usage_mb": 256,
            "cpu_usage_percent": 15.5
        }))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging for production
    tracing_subscriber::fmt()
        .with_target(false)
        .with_thread_ids(true)
        .with_line_number(true)
        .init();

    println!("🚀 Starting TurboMCP Production Server with Graceful Shutdown");
    println!("   Press Ctrl+C or send SIGTERM for graceful shutdown");

    // Create production server instance
    let server = ProductionServer {
        service_name: "turbomcp-production-api".to_string(),
        version: "1.0.0".to_string(),
        started_at: std::time::SystemTime::now(),
    };

    // Get server and shutdown handle with proper ownership
    let (server, shutdown_handle) = server.into_server_with_shutdown()?;

    // Clone shutdown handle for different shutdown sources
    let signal_shutdown = shutdown_handle.clone();
    let health_shutdown = shutdown_handle.clone();
    let maintenance_shutdown = shutdown_handle.clone();

    println!("✅ Server configured with graceful shutdown capabilities");

    // Pattern 1: Signal-based shutdown (SIGINT, SIGTERM)
    tokio::spawn(async move {
        println!("📡 Installing signal handlers...");

        // Handle Ctrl+C (SIGINT)
        let sigint_shutdown = signal_shutdown.clone();
        tokio::spawn(async move {
            if let Err(e) = tokio::signal::ctrl_c().await {
                eprintln!("⚠️  Failed to install Ctrl+C handler: {}", e);
                return;
            }
            println!("🛑 SIGINT received - initiating graceful shutdown...");
            sigint_shutdown.shutdown().await;
        });

        // Handle SIGTERM (production/container shutdown)
        #[cfg(unix)]
        {
            use tokio::signal::unix::{SignalKind, signal};
            let sigterm_shutdown = signal_shutdown.clone();
            tokio::spawn(async move {
                match signal(SignalKind::terminate()) {
                    Ok(mut sigterm) => {
                        sigterm.recv().await;
                        println!("🔄 SIGTERM received - initiating production shutdown...");
                        sigterm_shutdown.shutdown().await;
                    }
                    Err(e) => eprintln!("⚠️  Failed to install SIGTERM handler: {}", e),
                }
            });
        }

        println!("✅ Signal handlers installed successfully");
    });

    // Pattern 2: Health check coordination
    tokio::spawn(async move {
        println!("🏥 Starting health check coordination...");
        let mut interval = tokio::time::interval(Duration::from_secs(30));

        loop {
            interval.tick().await;

            // Check if shutdown has been initiated
            if health_shutdown.is_shutting_down().await {
                println!("🔄 Health checks stopped - server shutting down");
                break;
            }

            // Notify load balancer of health status
            println!("💓 Health check: Server healthy and accepting requests");
        }
    });

    // Pattern 3: Planned maintenance window
    tokio::spawn(async move {
        println!("🔧 Maintenance scheduler active...");

        // Simulate planned maintenance after 2 minutes
        sleep(Duration::from_secs(120)).await;

        if !maintenance_shutdown.is_shutting_down().await {
            println!("🔧 Planned maintenance window - initiating graceful shutdown...");
            maintenance_shutdown.shutdown().await;
        }
    });

    // Pattern 4: Monitoring and alerting integration
    let monitoring_shutdown = shutdown_handle.clone();
    tokio::spawn(async move {
        println!("📊 Monitoring integration active...");
        let mut interval = tokio::time::interval(Duration::from_secs(10));

        loop {
            interval.tick().await;

            if monitoring_shutdown.is_shutting_down().await {
                println!("📊 Sending shutdown alerts to monitoring systems...");
                // Send alerts to monitoring systems (PagerDuty, Slack, etc.)
                break;
            }

            // Monitor system resources, send metrics
            println!("📈 Metrics: CPU: 15%, Memory: 256MB, Connections: 42");
        }
    });

    println!("🌐 Starting server on STDIO transport...");
    println!("🎯 Ready to accept MCP connections");
    println!("   Available tools: health_check, process_operation, get_metrics");

    // Start the server (this will run until shutdown is triggered)
    match server.run_stdio().await {
        Ok(_) => println!("✅ Server shutdown completed successfully"),
        Err(e) => {
            eprintln!("❌ Server error: {}", e);
            std::process::exit(1);
        }
    }

    println!("🏁 Graceful shutdown complete - all services terminated cleanly");
    Ok(())
}
