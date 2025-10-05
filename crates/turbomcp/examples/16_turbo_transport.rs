//! # 16: TurboTransport - Circuit Breakers & Resilience
//!
//! **Learning Goals:**
//! - Configure circuit breakers for fault tolerance
//! - Implement retry logic with exponential backoff
//! - Set up health checking and monitoring
//! - Handle transient failures gracefully
//!
//! **What this example demonstrates:**
//! - TurboTransport wrapper with resilience features
//! - Circuit breaker pattern to prevent cascade failures
//! - Automatic retry with configurable policies
//! - Health check monitoring
//! - Message deduplication
//!
//! **Run with:** `cargo run --example 16_turbo_transport`

use std::time::Duration;
use turbomcp_client::ClientBuilder;
use turbomcp_transport::resilience::{
    CircuitBreakerConfig, HealthCheckConfig, RetryConfig, TurboTransport,
};
use turbomcp_transport::stdio::StdioTransport;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging to stderr for MCP STDIO compatibility
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_writer(std::io::stderr)
        .init();

    tracing::info!("🛡️  TurboTransport Demo - Circuit Breakers & Resilience");

    // ============================================================================
    // CONFIGURE RETRY POLICY
    // ============================================================================
    let retry_config = RetryConfig {
        max_attempts: 5,                        // Retry up to 5 times
        base_delay: Duration::from_millis(100), // Start with 100ms
        max_delay: Duration::from_secs(30),     // Cap at 30s
        backoff_multiplier: 2.0,                // Exponential backoff
        jitter_factor: 0.1,                     // 10% jitter to prevent thundering herd
        retry_on_connection_error: true,        // Retry connection failures
        retry_on_timeout: true,                 // Retry timeouts
        custom_retry_conditions: Vec::new(),
    };

    // ============================================================================
    // CONFIGURE CIRCUIT BREAKER
    // ============================================================================
    let circuit_breaker_config = CircuitBreakerConfig {
        failure_threshold: 5,             // Open after 5 failures
        success_threshold: 2,             // Close after 2 successes
        timeout: Duration::from_secs(60), // Try again after 60s
        rolling_window_size: 100,         // Track last 100 requests
        minimum_requests: 10,             // Need 10 requests before opening
    };

    // ============================================================================
    // CONFIGURE HEALTH CHECKING
    // ============================================================================
    let health_check_config = HealthCheckConfig {
        interval: Duration::from_secs(30), // Check every 30s
        timeout: Duration::from_secs(5),   // Health check timeout
        failure_threshold: 3,              // Mark unhealthy after 3 failures
        success_threshold: 1,              // Mark healthy after 1 success
        custom_check: None,                // Use default ping-based check
    };

    tracing::info!(
        "🔧 Retry: {} attempts, exponential backoff",
        retry_config.max_attempts
    );
    tracing::info!(
        "🔌 Circuit Breaker: {} failure threshold",
        circuit_breaker_config.failure_threshold
    );
    tracing::info!("❤️  Health Check: every {:?}", health_check_config.interval);

    // ============================================================================
    // METHOD 1: Manual TurboTransport Construction
    // ============================================================================
    tracing::info!("\n📦 Method 1: Manual TurboTransport construction");

    let base_transport = StdioTransport::new();
    let _turbo_transport = TurboTransport::new(
        Box::new(base_transport),
        retry_config.clone(),
        circuit_breaker_config.clone(),
        health_check_config.clone(),
    );

    tracing::info!("✅ Created TurboTransport with full resilience features");

    // ============================================================================
    // METHOD 2: ClientBuilder with Robustness (Recommended)
    // ============================================================================
    tracing::info!("\n📦 Method 2: ClientBuilder with resilience (RECOMMENDED)");

    let _client = ClientBuilder::new()
        .with_tools(true)
        .with_prompts(true)
        .with_resources(true)
        .enable_resilience()
        .with_retry_config(retry_config)
        .with_circuit_breaker_config(circuit_breaker_config)
        .with_health_check_config(health_check_config)
        .build_resilient(StdioTransport::new())
        .await?;

    tracing::info!("✅ Client created with resilient transport");

    // ============================================================================
    // METHOD 3: Explicit Configuration with Defaults
    // ============================================================================
    tracing::info!("\n📦 Method 3: Using explicit configuration with sensible defaults");

    // Network scenario - customize critical settings, use defaults for others
    let _network_client = ClientBuilder::new()
        .with_retry_config(RetryConfig {
            max_attempts: 5,
            base_delay: Duration::from_millis(200),
            ..Default::default()
        })
        .with_circuit_breaker_config(CircuitBreakerConfig {
            failure_threshold: 3,
            timeout: Duration::from_secs(30),
            ..Default::default()
        })
        .with_health_check_config(HealthCheckConfig {
            interval: Duration::from_secs(15),
            timeout: Duration::from_secs(5),
            ..Default::default()
        })
        .build_resilient(StdioTransport::new())
        .await?;

    tracing::info!("✅ Network config: balanced retry + circuit breaker + health checks");

    // Local scenario - faster checks, less overhead
    let _local_client = ClientBuilder::new()
        .with_health_check_config(HealthCheckConfig {
            interval: Duration::from_secs(60),
            timeout: Duration::from_secs(10),
            failure_threshold: 5,
            ..Default::default()
        })
        .build_resilient(StdioTransport::new())
        .await?;

    tracing::info!("✅ Local config: optimized for low-latency local connections");

    tracing::info!("\n🎯 TurboTransport Features Demonstrated:");
    tracing::info!("  ✓ Automatic retry with exponential backoff");
    tracing::info!("  ✓ Circuit breaker pattern for fast failure");
    tracing::info!("  ✓ Periodic health checking");
    tracing::info!("  ✓ Message deduplication");
    tracing::info!("  ✓ Explicit, composable configuration");

    Ok(())
}

/* 📝 **Key Concepts:**

**Architecture:**
```text
Client → TurboTransport → BaseTransport (STDIO/TCP/etc)
         ├─ Retry Logic (exponential backoff)
         ├─ Circuit Breaker (fail fast when unhealthy)
         ├─ Health Checking (periodic ping)
         └─ Deduplication (prevent duplicate messages)
```

**Circuit Breaker States:**
1. **Closed** - Normal operation, requests pass through
2. **Open** - Too many failures, reject requests immediately
3. **Half-Open** - Testing if service recovered

**Retry Strategy:**
- Exponential backoff: 100ms → 200ms → 400ms → 800ms → ...
- Jitter prevents thundering herd (all clients retrying at once)
- Configurable max attempts and delay caps

**Health Checking:**
- Periodic ping to verify connection
- Marks transport healthy/unhealthy
- Configurable failure/success thresholds

**When to Use Each Preset:**

| Preset | Use Case | Characteristics |
|--------|----------|----------------|
| **High Reliability** | Critical systems, financial | Aggressive retry, tight health checks |
| **High Performance** | Low-latency APIs, real-time | Fast fail, optimized throughput |
| **Resource Constrained** | Edge devices, embedded | Minimal overhead, low memory |

**Production Best Practices:**
1. Start with a preset, tune as needed
2. Monitor circuit breaker state changes
3. Log retry attempts for debugging
4. Set timeouts appropriate for your workload
5. Use health checks for long-lived connections

**Next Example:** `17_client_plugins.rs` - Plugin system for cross-cutting concerns
*/
