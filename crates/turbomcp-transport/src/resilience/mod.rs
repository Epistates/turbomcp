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
//! // Custom retry configuration - explicit and discoverable
//! let retry_config = RetryConfig {
//!     max_attempts: 5,
//!     base_delay: Duration::from_millis(200),
//!     max_delay: Duration::from_secs(30),
//!     backoff_multiplier: 2.0,
//!     ..Default::default()
//! };
//!
//! // Custom circuit breaker configuration
//! let circuit_config = CircuitBreakerConfig {
//!     failure_threshold: 3,
//!     timeout: Duration::from_secs(30),
//!     ..Default::default()
//! };
//!
//! // Custom health check configuration
//! let health_config = HealthCheckConfig {
//!     interval: Duration::from_secs(15),
//!     timeout: Duration::from_secs(5),
//!     ..Default::default()
//! };
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
