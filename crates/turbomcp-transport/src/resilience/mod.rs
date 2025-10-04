//! Transport resilience features for fault tolerance and reliability
//!
//! This module provides comprehensive resilience features for MCP transports including:
//! - **Retry mechanisms** with exponential backoff and jitter
//! - **Circuit breaker** pattern for fault tolerance and fast failure
//! - **Health checking** and monitoring for proactive failure detection
//! - **Message deduplication** to prevent duplicate processing
//! - **Metrics collection** for observability and monitoring
//!
//! ## Architecture
//!
//! The resilience module is organized into focused components:
//!
//! ```text
//! resilience/
//! ├── retry.rs            # Retry logic with exponential backoff
//! ├── circuit_breaker.rs  # Circuit breaker pattern implementation
//! ├── health.rs           # Health checking and monitoring
//! ├── metrics.rs          # Comprehensive metrics collection
//! ├── deduplication.rs    # Message deduplication cache
//! └── transport.rs        # Main TurboTransport wrapper
//! ```
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use turbomcp_transport::resilience::{TurboTransport, RetryConfig, CircuitBreakerConfig};
//! use turbomcp_transport::stdio::StdioTransport;
//! use turbomcp_transport::Transport;  // Needed for connect() method
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Create TurboTransport wrapper
//! let base_transport = StdioTransport::new();
//! let mut turbo = TurboTransport::with_defaults(Box::new(base_transport));
//!
//! // Start health monitoring
//! turbo.start_health_monitoring().await;
//!
//! // Connect with automatic retry
//! turbo.connect().await?;
//!
//! // Get metrics
//! let metrics = turbo.get_metrics_snapshot().await;
//! println!("Retry attempts: {}", metrics.retry_attempts);
//! # Ok(())
//! # }
//! ```
//!
//! ## Customization
//!
//! ```rust,no_run
//! use turbomcp_transport::resilience::*;
//! use turbomcp_transport::stdio::StdioTransport;
//! use std::time::Duration;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Custom retry configuration
//! let mut retry_config = RetryConfig::for_network();
//! retry_config.max_attempts = 5;
//! retry_config.base_delay = Duration::from_millis(200);
//!
//! // Custom circuit breaker configuration
//! let circuit_config = CircuitBreakerConfig::for_network();
//!
//! // Custom health check configuration
//! let health_config = HealthCheckConfig::for_network();
//!
//! let base_transport = StdioTransport::new();
//! let mut turbo = TurboTransport::new(
//!     Box::new(base_transport),
//!     retry_config,
//!     circuit_config,
//!     health_config,
//! );
//! # Ok(())
//! # }
//! ```

pub mod circuit_breaker;
pub mod deduplication;
pub mod health;
pub mod metrics;
pub mod retry;
pub mod transport;

// Re-export all main types for convenience
pub use circuit_breaker::{
    CircuitBreaker, CircuitBreakerConfig, CircuitBreakerStats, CircuitState, OperationResult,
};
pub use deduplication::{DeduplicationCache, DeduplicationConfig, DeduplicationStats};
pub use health::{HealthCheckConfig, HealthCheckable, HealthChecker, HealthInfo, HealthStatus};
pub use metrics::{LatencyTracker, MetricsSnapshot, TurboTransportMetrics};
pub use retry::{RetryCondition, RetryConfig};
pub use transport::TurboTransport;

/// Presets for common resilience configurations
pub mod presets {
    use super::*;
    use std::time::Duration;

    /// High-reliability preset for critical systems
    pub fn high_reliability() -> (RetryConfig, CircuitBreakerConfig, HealthCheckConfig) {
        (
            RetryConfig {
                max_attempts: 5,
                base_delay: Duration::from_millis(100),
                max_delay: Duration::from_secs(30),
                backoff_multiplier: 1.5,
                jitter_factor: 0.1,
                retry_on_connection_error: true,
                retry_on_timeout: true,
                custom_retry_conditions: Vec::new(),
            },
            CircuitBreakerConfig {
                failure_threshold: 3,
                success_threshold: 2,
                timeout: Duration::from_secs(30),
                rolling_window_size: 50,
                minimum_requests: 5,
            },
            HealthCheckConfig {
                interval: Duration::from_secs(10),
                timeout: Duration::from_secs(3),
                failure_threshold: 2,
                success_threshold: 1,
                custom_check: None,
            },
        )
    }

    /// Performance-optimized preset for high-throughput scenarios
    pub fn high_performance() -> (RetryConfig, CircuitBreakerConfig, HealthCheckConfig) {
        (
            RetryConfig {
                max_attempts: 2,
                base_delay: Duration::from_millis(50),
                max_delay: Duration::from_secs(5),
                backoff_multiplier: 2.0,
                jitter_factor: 0.05,
                retry_on_connection_error: true,
                retry_on_timeout: false, // Don't retry timeouts in high-perf scenarios
                custom_retry_conditions: Vec::new(),
            },
            CircuitBreakerConfig {
                failure_threshold: 10,
                success_threshold: 5,
                timeout: Duration::from_secs(60),
                rolling_window_size: 100,
                minimum_requests: 20,
            },
            HealthCheckConfig {
                interval: Duration::from_secs(30),
                timeout: Duration::from_secs(1),
                failure_threshold: 5,
                success_threshold: 2,
                custom_check: None,
            },
        )
    }

    /// Resource-constrained preset for embedded or low-memory environments
    pub fn resource_constrained() -> (RetryConfig, CircuitBreakerConfig, HealthCheckConfig) {
        (
            RetryConfig {
                max_attempts: 3,
                base_delay: Duration::from_millis(200),
                max_delay: Duration::from_secs(10),
                backoff_multiplier: 2.0,
                jitter_factor: 0.1,
                retry_on_connection_error: true,
                retry_on_timeout: true,
                custom_retry_conditions: Vec::new(),
            },
            CircuitBreakerConfig {
                failure_threshold: 5,
                success_threshold: 3,
                timeout: Duration::from_secs(120),
                rolling_window_size: 20,
                minimum_requests: 5,
            },
            HealthCheckConfig {
                interval: Duration::from_secs(60),
                timeout: Duration::from_secs(5),
                failure_threshold: 3,
                success_threshold: 2,
                custom_check: None,
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_presets_compilation() {
        // Test that all presets compile and can be used
        let (retry, circuit, health) = presets::high_reliability();
        assert!(retry.max_attempts > 0);
        assert!(circuit.failure_threshold > 0);
        assert!(health.failure_threshold > 0);

        let (retry, circuit, health) = presets::high_performance();
        assert!(retry.max_attempts > 0);
        assert!(circuit.failure_threshold > 0);
        assert!(health.failure_threshold > 0);

        let (retry, circuit, health) = presets::resource_constrained();
        assert!(retry.max_attempts > 0);
        assert!(circuit.failure_threshold > 0);
        assert!(health.failure_threshold > 0);
    }
}
