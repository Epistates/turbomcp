//! Example: #[ping] Attribute Macro Demonstration
//!
//! This example demonstrates the #[ping] attribute macro for marking methods
//! as ping handlers that enable bidirectional health checks and connection monitoring.
//!
//! The #[ping] macro generates handlers that respond to health check requests
//! and can provide detailed system status information.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use turbomcp::prelude::*;

#[derive(Clone)]
struct HealthMonitoringServer {
    start_time: Instant,
    ping_count: Arc<Mutex<u64>>,
    system_status: Arc<Mutex<SystemStatus>>,
}

#[derive(Clone, Debug)]
struct SystemStatus {
    cpu_usage: f64,
    memory_usage: f64,
    disk_usage: f64,
    active_connections: u32,
    errors_last_hour: u32,
    last_backup: Option<Instant>,
}

#[server(
    name = "health-monitoring-demo",
    version = "1.0.4",
    description = "Demonstrates #[ping] attribute macro functionality"
)]
impl HealthMonitoringServer {
    fn new() -> Self {
        Self {
            start_time: Instant::now(),
            ping_count: Arc::new(Mutex::new(0)),
            system_status: Arc::new(Mutex::new(SystemStatus {
                cpu_usage: 15.3,
                memory_usage: 42.1,
                disk_usage: 68.9,
                active_connections: 12,
                errors_last_hour: 0,
                last_backup: Some(Instant::now() - Duration::from_secs(3600)), // 1 hour ago
            })),
        }
    }

    /// Basic ping handler for simple health checks
    #[ping("Basic health check")]
    async fn health_check(&self) -> McpResult<String> {
        let mut count = self.ping_count.lock().unwrap();
        *count += 1;

        let uptime = self.start_time.elapsed();
        Ok(format!(
            "Server healthy - Uptime: {}s, Ping #{}",
            uptime.as_secs(),
            *count
        ))
    }

    /// Advanced ping handler with detailed system information
    #[ping("Detailed system health check")]
    async fn detailed_health_check(&self) -> McpResult<String> {
        let mut count = self.ping_count.lock().unwrap();
        *count += 1;

        let status = self.system_status.lock().unwrap();
        let uptime = self.start_time.elapsed();

        Ok(format!(
            "🟢 System Status: HEALTHY\n\
             ⏱️  Uptime: {}m {}s\n\
             🔄 Ping Count: {}\n\
             💻 CPU Usage: {:.1}%\n\
             🧠 Memory Usage: {:.1}%\n\
             💾 Disk Usage: {:.1}%\n\
             🔗 Active Connections: {}\n\
             ❌ Errors (Last Hour): {}\n\
             💾 Last Backup: {}",
            uptime.as_secs() / 60,
            uptime.as_secs() % 60,
            *count,
            status.cpu_usage,
            status.memory_usage,
            status.disk_usage,
            status.active_connections,
            status.errors_last_hour,
            status
                .last_backup
                .map(|b| format!("{}m ago", b.elapsed().as_secs() / 60))
                .unwrap_or_else(|| "Never".to_string())
        ))
    }

    /// Ping handler with context injection for logging
    #[ping("Health check with monitoring")]
    async fn monitored_health_check(&self, ctx: Context) -> McpResult<String> {
        ctx.info("Executing monitored health check").await?;

        // Scope mutex guards to avoid holding them across await
        let (ping_count, system_metrics) = {
            let mut count = self.ping_count.lock().unwrap();
            *count += 1;
            let current_count = *count;

            let status = self.system_status.lock().unwrap();
            let metrics = (
                status.cpu_usage,
                status.memory_usage,
                status.active_connections,
                status.disk_usage,
                status.errors_last_hour,
            );

            (current_count, metrics)
        };

        // Log important metrics
        ctx.info(&format!(
            "CPU: {:.1}%, Memory: {:.1}%, Connections: {}",
            system_metrics.0, system_metrics.1, system_metrics.2
        ))
        .await?;

        // Check for alerts using extracted metrics
        let mut alerts = Vec::new();
        if system_metrics.0 > 80.0 {
            // cpu_usage
            alerts.push("🔴 High CPU usage");
        }
        if system_metrics.1 > 90.0 {
            // memory_usage
            alerts.push("🔴 High memory usage");
        }
        if system_metrics.3 > 85.0 {
            // disk_usage
            alerts.push("🟡 High disk usage");
        }
        if system_metrics.4 > 10 {
            // errors_last_hour
            alerts.push("🔴 High error rate");
        }

        let health_status = if alerts.is_empty() {
            "🟢 HEALTHY".to_string()
        } else {
            format!("🟡 DEGRADED - {}", alerts.join(", "))
        };

        ctx.info(&format!(
            "Health check #{} completed: {}",
            ping_count, health_status
        ))
        .await?;

        Ok(format!(
            "Health Status: {}\nUptime: {}s\nPing: #{}",
            health_status,
            self.start_time.elapsed().as_secs(),
            ping_count
        ))
    }

    /// Ping handler with custom parameters (metadata-based)
    #[ping("Database connectivity check")]
    async fn database_ping(&self, timeout_seconds: Option<u32>) -> McpResult<String> {
        let timeout = timeout_seconds.unwrap_or(5);

        // Simulate database connectivity check
        tokio::time::sleep(Duration::from_millis(100)).await;

        let mut count = self.ping_count.lock().unwrap();
        *count += 1;

        // Mock database connectivity results
        let db_status = if *count % 7 == 0 {
            "🔴 Connection timeout"
        } else if *count % 5 == 0 {
            "🟡 Slow response"
        } else {
            "🟢 Connected"
        };

        Ok(format!(
            "Database Status: {}\nTimeout: {}s\nPing: #{}",
            db_status, timeout, *count
        ))
    }

    /// Ping handler that checks external dependencies
    #[ping("External services check")]
    async fn external_services_check(&self) -> McpResult<String> {
        let mut count = self.ping_count.lock().unwrap();
        *count += 1;

        // Simulate checking multiple external services
        let services = vec![
            ("auth-service", *count % 3 != 0),
            ("payment-gateway", *count % 4 != 0),
            ("notification-service", *count % 6 != 0),
            ("analytics-api", *count % 8 != 0),
        ];

        let mut status_lines = Vec::new();
        let mut all_healthy = true;

        for (service, is_healthy) in &services {
            let status = if *is_healthy {
                "🟢 UP"
            } else {
                all_healthy = false;
                "🔴 DOWN"
            };
            status_lines.push(format!("  {} {}", status, service));
        }

        let overall_status = if all_healthy {
            "🟢 ALL SERVICES HEALTHY"
        } else {
            "🔴 SOME SERVICES DOWN"
        };

        Ok(format!(
            "{}\nPing: #{}\n\nService Status:\n{}",
            overall_status,
            *count,
            status_lines.join("\n")
        ))
    }

    /// Tool to simulate system load for testing ping responses
    #[tool("Simulate system load")]
    async fn simulate_load(&self, cpu_load: f64, memory_load: f64) -> McpResult<String> {
        let mut status = self.system_status.lock().unwrap();
        status.cpu_usage = cpu_load.clamp(0.0, 100.0);
        status.memory_usage = memory_load.clamp(0.0, 100.0);

        Ok(format!(
            "System load simulated: CPU {:.1}%, Memory {:.1}%",
            status.cpu_usage, status.memory_usage
        ))
    }

    /// Tool to get ping statistics
    #[tool("Get ping statistics")]
    async fn get_ping_stats(&self) -> McpResult<String> {
        let count = self.ping_count.lock().unwrap();
        let uptime = self.start_time.elapsed();
        let pings_per_minute = if uptime.as_secs() > 0 {
            (*count as f64) / (uptime.as_secs() as f64 / 60.0)
        } else {
            0.0
        };

        Ok(format!(
            "📊 Ping Statistics:\n\
             Total Pings: {}\n\
             Uptime: {}m {}s\n\
             Avg Pings/min: {:.2}",
            *count,
            uptime.as_secs() / 60,
            uptime.as_secs() % 60,
            pings_per_minute
        ))
    }

    /// Show ping usage patterns
    #[tool("Show ping patterns")]
    async fn show_ping_patterns(&self) -> McpResult<String> {
        Ok(r#"
🎯 #[ping] Macro Usage Patterns:

💓 Basic Ping Handler:
  #[ping("Description")]
  async fn ping_handler(&self) -> McpResult<String>

🔗 With Context Injection:
  #[ping("Description")]
  async fn ping_handler(&self, ctx: Context) -> McpResult<String>

⚙️ With Parameters (from metadata):
  #[ping("Description")]
  async fn ping_handler(&self, timeout: Option<u32>) -> McpResult<String>

✅ Key Benefits:
• Automatic ping request/response handling
• Support for bidirectional health monitoring
• Context injection for logging and analytics
• Parameter extraction from ping metadata
• Type-safe return value handling
• Integration with MCP ping protocol
• Error handling and propagation
• Metadata generation for testing

🏗️ Generated Functions:
• ping_handler_metadata() - Returns (name, description, type) tuple
• Internal bridge function for protocol integration
• Parameter extraction from ping request metadata
• Result conversion to MCP ping format

💡 Best Practices:
• Include meaningful health status information
• Use structured status reporting (emoji + text)
• Monitor and log ping frequency
• Check external dependencies in ping handlers
• Use Context for ping analytics and alerting
• Return actionable health information
• Consider timeout parameters for external checks
        "#
        .trim()
        .to_string())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🎯 TurboMCP #[ping] Attribute Macro Demo");
    println!("=======================================");
    println!();
    println!("This example demonstrates the #[ping] attribute macro");
    println!("for creating bidirectional health check handlers.");
    println!();

    let server = HealthMonitoringServer::new();

    // Test that the macro generates metadata functions
    let (name, desc, handler_type) = HealthMonitoringServer::health_check_metadata();
    println!("✅ Ping metadata generated:");
    println!("   Name: {}", name);
    println!("   Description: {}", desc);
    println!("   Type: {}", handler_type);
    println!();

    // Test all ping handlers
    let handlers = [
        HealthMonitoringServer::health_check_metadata(),
        HealthMonitoringServer::detailed_health_check_metadata(),
        HealthMonitoringServer::monitored_health_check_metadata(),
        HealthMonitoringServer::database_ping_metadata(),
        HealthMonitoringServer::external_services_check_metadata(),
    ];

    println!("📋 All ping handlers:");
    for (name, desc, handler_type) in handlers {
        println!("   • {}: {} ({})", name, desc, handler_type);
    }
    println!();

    // Demonstrate ping functionality
    println!("💓 Testing ping handlers:");

    // Basic health check
    let basic_result = server.health_check().await?;
    println!("   Basic health: {}", basic_result);

    // Detailed health check
    let detailed_result = server.detailed_health_check().await?;
    println!(
        "   Detailed health: {}",
        detailed_result.lines().next().unwrap_or("")
    );

    // Monitored health check with context
    // Create a proper context for testing
    let request_ctx = RequestContext::new();
    let handler_meta = HandlerMetadata {
        name: "ping_demo".to_string(),
        handler_type: "ping".to_string(),
        description: Some("Ping demo".to_string()),
    };
    let ctx = Context::new(request_ctx, handler_meta);
    let monitored_result = server.monitored_health_check(ctx).await?;
    println!(
        "   Monitored health: {}",
        monitored_result.lines().next().unwrap_or("")
    );

    // Database ping with timeout
    let db_result = server.database_ping(Some(10)).await?;
    println!(
        "   Database ping: {}",
        db_result.lines().next().unwrap_or("")
    );

    // External services check
    let ext_result = server.external_services_check().await?;
    println!(
        "   External services: {}",
        ext_result.lines().next().unwrap_or("")
    );

    println!();

    // Show ping statistics
    let stats = server.get_ping_stats().await?;
    println!("📊 Current statistics:");
    for line in stats.lines() {
        println!("   {}", line);
    }

    println!();
    println!("✅ All #[ping] macros compiled and executed successfully!");
    println!();
    println!("The macro generates:");
    println!("• Metadata functions for testing");
    println!("• Parameter extraction from ping requests");
    println!("• Context injection support");
    println!("• Type-safe return value handling");
    println!("• Integration with MCP ping protocol");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_ping_handlers() {
        let server = HealthMonitoringServer::new();
        // Create a proper context for testing
        let request_ctx = RequestContext::new();
        let handler_meta = HandlerMetadata {
            name: "test_ping".to_string(),
            handler_type: "ping".to_string(),
            description: Some("Test ping handler".to_string()),
        };
        let ctx = Context::new(request_ctx, handler_meta);

        // Test basic health check
        let basic_result = server.health_check().await.unwrap();
        assert!(basic_result.contains("Server healthy"));
        assert!(basic_result.contains("Ping #1"));

        // Test detailed health check
        let detailed_result = server.detailed_health_check().await.unwrap();
        assert!(detailed_result.contains("System Status: HEALTHY"));
        assert!(detailed_result.contains("CPU Usage"));

        // Test monitored health check
        let monitored_result = server.monitored_health_check(ctx).await.unwrap();
        assert!(detailed_result.contains("Health Status"));
        assert!(monitored_result.contains("Ping: #"));

        // Test database ping with parameter
        let db_result = server.database_ping(Some(15)).await.unwrap();
        assert!(db_result.contains("Database Status"));
        assert!(db_result.contains("Timeout: 15s"));

        // Test database ping without parameter (default)
        let db_default_result = server.database_ping(None).await.unwrap();
        assert!(db_default_result.contains("Timeout: 5s")); // Default value

        // Test external services check
        let ext_result = server.external_services_check().await.unwrap();
        assert!(ext_result.contains("SERVICES"));
        assert!(ext_result.contains("auth-service"));
    }

    #[test]
    fn test_ping_metadata() {
        // Verify metadata functions exist and return correct data
        let (name, desc, handler_type) = HealthMonitoringServer::health_check_metadata();
        assert_eq!(name, "health_check");
        assert_eq!(desc, "Basic health check");
        assert_eq!(handler_type, "ping");

        let (name2, desc2, handler_type2) =
            HealthMonitoringServer::detailed_health_check_metadata();
        assert_eq!(name2, "detailed_health_check");
        assert_eq!(desc2, "Detailed system health check");
        assert_eq!(handler_type2, "ping");

        let (name3, desc3, handler_type3) =
            HealthMonitoringServer::monitored_health_check_metadata();
        assert_eq!(name3, "monitored_health_check");
        assert_eq!(desc3, "Health check with monitoring");
        assert_eq!(handler_type3, "ping");

        let (name4, desc4, handler_type4) = HealthMonitoringServer::database_ping_metadata();
        assert_eq!(name4, "database_ping");
        assert_eq!(desc4, "Database connectivity check");
        assert_eq!(handler_type4, "ping");

        let (name5, desc5, handler_type5) =
            HealthMonitoringServer::external_services_check_metadata();
        assert_eq!(name5, "external_services_check");
        assert_eq!(desc5, "External services check");
        assert_eq!(handler_type5, "ping");
    }

    #[tokio::test]
    async fn test_ping_statistics() {
        let server = HealthMonitoringServer::new();

        // Execute several pings
        server.health_check().await.unwrap();
        server.detailed_health_check().await.unwrap();
        server.external_services_check().await.unwrap();

        // Check statistics
        let stats = server.get_ping_stats().await.unwrap();
        assert!(stats.contains("Total Pings: 3"));
        assert!(stats.contains("Uptime:"));
        assert!(stats.contains("Avg Pings/min:"));
    }

    #[tokio::test]
    async fn test_system_simulation() {
        let server = HealthMonitoringServer::new();

        // Simulate high load
        let sim_result = server.simulate_load(85.0, 95.0).await.unwrap();
        assert!(sim_result.contains("CPU 85.0%"));
        assert!(sim_result.contains("Memory 95.0%"));

        // Check that health check reflects the load
        // Create context for health check test
        let request_ctx2 = RequestContext::new();
        let handler_meta2 = HandlerMetadata {
            name: "health_check".to_string(),
            handler_type: "ping".to_string(),
            description: Some("Health check test".to_string()),
        };
        let ctx2 = Context::new(request_ctx2, handler_meta2);
        let health_result = server.monitored_health_check(ctx2).await.unwrap();
        assert!(health_result.contains("DEGRADED") || health_result.contains("High"));
    }
}
