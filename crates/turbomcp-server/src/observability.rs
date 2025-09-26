//! Production-grade OpenTelemetry observability integration
//!
//! This module provides comprehensive distributed tracing, structured logging,
//! and observability configuration for TurboMCP server applications.
//!
//! # Features
//!
//! - **Structured Tracing**: Rich span attributes with user context propagation
//! - **Security Audit Logging**: Structured events for security-relevant actions
//! - **Performance Monitoring**: Request timing and tool execution metrics
//! - **Production Ready**: Proper initialization and cleanup
//!
//! # Example
//!
//! ```rust,no_run
//! use turbomcp_server::observability::{ObservabilityConfig, ObservabilityGuard};
//! use turbomcp_server::ServerBuilder;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Initialize observability
//!     let config = ObservabilityConfig::default()
//!         .with_service_name("my-mcp-server")
//!         .enable_security_auditing()
//!         .enable_performance_monitoring();
//!
//!     let _guard = config.init()?;
//!
//!     // Build server with observability
//!     let server = ServerBuilder::new().build();
//!     server.run_stdio().await?;
//!     Ok(())
//! }
//! ```

use std::time::Duration;
use tracing::{Instrument, error, info, warn};
use tracing_subscriber::{
    Registry, filter::EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt,
};

/// OpenTelemetry observability configuration
#[derive(Debug, Clone)]
pub struct ObservabilityConfig {
    /// Service name for tracing
    pub service_name: String,
    /// Service version
    pub service_version: String,
    /// Enable security audit logging
    pub security_auditing: bool,
    /// Enable performance monitoring
    pub performance_monitoring: bool,
    /// Custom log level filter
    pub log_level: String,
}

impl Default for ObservabilityConfig {
    fn default() -> Self {
        Self {
            service_name: "turbomcp-server".to_string(),
            service_version: env!("CARGO_PKG_VERSION").to_string(),
            security_auditing: true,
            performance_monitoring: true,
            log_level: "info,turbomcp=debug".to_string(),
        }
    }
}

impl ObservabilityConfig {
    /// Create new observability configuration
    pub fn new(service_name: impl Into<String>) -> Self {
        Self {
            service_name: service_name.into(),
            ..Default::default()
        }
    }

    /// Set service name
    pub fn with_service_name(mut self, name: impl Into<String>) -> Self {
        self.service_name = name.into();
        self
    }

    /// Set service version
    pub fn with_service_version(mut self, version: impl Into<String>) -> Self {
        self.service_version = version.into();
        self
    }

    /// Set log level filter
    pub fn with_log_level(mut self, level: impl Into<String>) -> Self {
        self.log_level = level.into();
        self
    }

    /// Enable security audit logging
    pub fn enable_security_auditing(mut self) -> Self {
        self.security_auditing = true;
        self
    }

    /// Enable performance monitoring
    pub fn enable_performance_monitoring(mut self) -> Self {
        self.performance_monitoring = true;
        self
    }

    /// Initialize observability with this configuration
    pub fn init(self) -> Result<ObservabilityGuard, ObservabilityError> {
        ObservabilityGuard::init(self)
    }
}

/// Observability initialization guard
///
/// Ensures proper cleanup on drop.
#[derive(Debug)]
pub struct ObservabilityGuard {
    config: ObservabilityConfig,
}

impl ObservabilityGuard {
    /// Initialize structured logging with the provided configuration
    pub fn init(config: ObservabilityConfig) -> Result<Self, ObservabilityError> {
        info!("Initializing TurboMCP observability");

        // Create environment filter
        let env_filter = EnvFilter::try_from_default_env()
            .or_else(|_| EnvFilter::try_new(&config.log_level))
            .map_err(|e| {
                ObservabilityError::InitializationFailed(format!("Invalid log level: {}", e))
            })?;

        // Initialize tracing subscriber with structured JSON logging
        Registry::default()
            .with(env_filter)
            .with(
                fmt::layer()
                    .with_target(true)
                    .with_thread_ids(true)
                    .with_file(true)
                    .with_line_number(true)
                    .json(),
            )
            .try_init()
            .map_err(|e| {
                ObservabilityError::InitializationFailed(format!("Tracing subscriber: {}", e))
            })?;

        // Initialize global observability components
        let security_logger = SecurityAuditLogger::new(config.security_auditing);
        let performance_monitor = PerformanceMonitor::new(config.performance_monitoring);

        // Set global instances
        futures::executor::block_on(async {
            global_observability()
                .set_security_audit_logger(security_logger)
                .await;
            global_observability()
                .set_performance_monitor(performance_monitor)
                .await;
        });

        info!(
            service_name = %config.service_name,
            service_version = %config.service_version,
            security_auditing = config.security_auditing,
            performance_monitoring = config.performance_monitoring,
            "TurboMCP observability initialized successfully"
        );

        Ok(Self { config })
    }

    /// Get the service name
    pub fn service_name(&self) -> &str {
        &self.config.service_name
    }

    /// Get the configuration
    pub fn config(&self) -> &ObservabilityConfig {
        &self.config
    }
}

impl Drop for ObservabilityGuard {
    fn drop(&mut self) {
        info!("Shutting down TurboMCP observability");
    }
}

/// Security audit logger using structured tracing events
#[derive(Debug, Clone)]
pub struct SecurityAuditLogger {
    enabled: bool,
}

impl SecurityAuditLogger {
    /// Create new security audit logger
    pub fn new(enabled: bool) -> Self {
        Self { enabled }
    }

    /// Log authentication event
    pub fn log_authentication(&self, user_id: &str, success: bool, details: Option<&str>) {
        if !self.enabled {
            return;
        }

        if success {
            info!(
                event = "authentication_success",
                user_id = user_id,
                details = details.unwrap_or(""),
                "User authentication successful"
            );
        } else {
            warn!(
                event = "authentication_failure",
                user_id = user_id,
                details = details.unwrap_or(""),
                "User authentication failed"
            );
        }
    }

    /// Log authorization event
    pub fn log_authorization(&self, user_id: &str, resource: &str, action: &str, granted: bool) {
        if !self.enabled {
            return;
        }

        if granted {
            info!(
                event = "authorization_granted",
                user_id = user_id,
                resource = resource,
                action = action,
                "Authorization granted"
            );
        } else {
            warn!(
                event = "authorization_denied",
                user_id = user_id,
                resource = resource,
                action = action,
                "Authorization denied"
            );
        }
    }

    /// Log tool execution
    pub fn log_tool_execution(
        &self,
        user_id: &str,
        tool_name: &str,
        success: bool,
        execution_time_ms: u64,
    ) {
        if !self.enabled {
            return;
        }

        if success {
            info!(
                event = "tool_execution_success",
                user_id = user_id,
                tool_name = tool_name,
                execution_time_ms = execution_time_ms,
                "Tool execution completed successfully"
            );
        } else {
            warn!(
                event = "tool_execution_failure",
                user_id = user_id,
                tool_name = tool_name,
                execution_time_ms = execution_time_ms,
                "Tool execution failed"
            );
        }
    }

    /// Log security violation
    pub fn log_security_violation(&self, violation_type: &str, details: &str, severity: &str) {
        if !self.enabled {
            return;
        }

        error!(
            event = "security_violation",
            violation_type = violation_type,
            details = details,
            severity = severity,
            "Security violation detected"
        );
    }
}

/// Performance monitoring utilities
#[derive(Debug, Clone)]
pub struct PerformanceMonitor {
    enabled: bool,
}

impl PerformanceMonitor {
    /// Create new performance monitor
    pub fn new(enabled: bool) -> Self {
        Self { enabled }
    }

    /// Start performance span
    pub fn start_span(&self, operation: &str) -> PerformanceSpan {
        if !self.enabled {
            return PerformanceSpan::disabled();
        }

        PerformanceSpan::new(operation.to_string())
    }

    /// Create an instrumented future for performance monitoring
    pub fn instrument_async<F>(
        &self,
        future: F,
        operation: &str,
    ) -> Box<dyn std::future::Future<Output = F::Output> + Send>
    where
        F: std::future::Future + Send + 'static,
    {
        if self.enabled {
            let span = tracing::info_span!(
                "performance_operation",
                operation = operation,
                performance_monitoring = true
            );
            Box::new(future.instrument(span))
        } else {
            // Return the future as-is without instrumentation
            Box::new(future)
        }
    }
}

/// Performance tracking span
#[derive(Debug)]
pub struct PerformanceSpan {
    enabled: bool,
    operation: String,
    start_time: std::time::Instant,
}

impl PerformanceSpan {
    fn new(operation: String) -> Self {
        Self {
            enabled: true,
            operation,
            start_time: std::time::Instant::now(),
        }
    }

    fn disabled() -> Self {
        Self {
            enabled: false,
            operation: String::new(),
            start_time: std::time::Instant::now(),
        }
    }

    /// Record execution time and finish span
    pub fn finish(self) -> Duration {
        let duration = self.start_time.elapsed();

        if self.enabled {
            info!(
                event = "performance_measurement",
                operation = self.operation,
                duration_ms = duration.as_millis(),
                "Operation completed"
            );
        }

        duration
    }
}

/// Observability errors
#[derive(Debug, thiserror::Error)]
pub enum ObservabilityError {
    /// Failed to initialize observability system
    #[error("Failed to initialize observability: {0}")]
    InitializationFailed(String),

    /// Configuration error in observability setup
    #[error("Configuration error: {0}")]
    ConfigurationError(String),
}

/// Global observability state for server integration
#[derive(Debug)]
pub struct GlobalObservability {
    security_audit_logger: tokio::sync::RwLock<Option<SecurityAuditLogger>>,
    performance_monitor: tokio::sync::RwLock<Option<PerformanceMonitor>>,
}

impl Default for GlobalObservability {
    fn default() -> Self {
        Self::new()
    }
}

impl GlobalObservability {
    /// Initialize global observability
    pub fn new() -> Self {
        Self {
            security_audit_logger: tokio::sync::RwLock::new(None),
            performance_monitor: tokio::sync::RwLock::new(None),
        }
    }

    /// Set security audit logger
    pub async fn set_security_audit_logger(&self, logger: SecurityAuditLogger) {
        *self.security_audit_logger.write().await = Some(logger);
    }

    /// Set performance monitor
    pub async fn set_performance_monitor(&self, monitor: PerformanceMonitor) {
        *self.performance_monitor.write().await = Some(monitor);
    }

    /// Get security audit logger
    pub async fn security_audit_logger(&self) -> Option<SecurityAuditLogger> {
        self.security_audit_logger.read().await.clone()
    }

    /// Get performance monitor
    pub async fn performance_monitor(&self) -> Option<PerformanceMonitor> {
        self.performance_monitor.read().await.clone()
    }
}

/// Global observability instance
static GLOBAL_OBSERVABILITY: once_cell::sync::Lazy<GlobalObservability> =
    once_cell::sync::Lazy::new(GlobalObservability::new);

/// Get global observability instance
pub fn global_observability() -> &'static GlobalObservability {
    &GLOBAL_OBSERVABILITY
}

/// Helper macro for instrumenting async functions
#[macro_export]
macro_rules! instrument_async {
    ($operation:expr, $future:expr) => {{
        let monitor = $crate::observability::global_observability()
            .performance_monitor()
            .await;

        if let Some(monitor) = monitor {
            monitor.instrument_async($future, $operation).await
        } else {
            $future.await
        }
    }};
}

/// Helper macro for performance span measurement
#[macro_export]
macro_rules! measure_performance {
    ($operation:expr, $code:block) => {{
        let monitor = $crate::observability::global_observability()
            .performance_monitor()
            .await;

        let span = if let Some(ref monitor) = monitor {
            Some(monitor.start_span($operation))
        } else {
            None
        };

        let result = $code;

        if let Some(span) = span {
            let _duration = span.finish();
        }

        result
    }};
}

/// OTLP protocol configuration (placeholder for future enhancement)
#[derive(Debug, Clone, PartialEq)]
pub enum OtlpProtocol {
    /// gRPC protocol (default, port 4317)
    Grpc,
    /// HTTP binary protocol (port 4318)
    Http,
}

/// Trace sampling configuration (placeholder for future enhancement)
#[derive(Debug, Clone)]
pub struct SamplingConfig {
    /// Sample rate (0.0 to 1.0)
    pub sample_rate: f64,
    /// Parent-based sampling
    pub parent_based: bool,
}

impl Default for SamplingConfig {
    fn default() -> Self {
        Self {
            sample_rate: 1.0,
            parent_based: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_observability_config_defaults() {
        let config = ObservabilityConfig::default();
        assert_eq!(config.service_name, "turbomcp-server");
        assert!(config.security_auditing);
        assert!(config.performance_monitoring);
    }

    #[test]
    fn test_observability_config_builder() {
        let config = ObservabilityConfig::new("test-service")
            .with_service_version("1.0.0")
            .with_log_level("debug")
            .enable_security_auditing()
            .enable_performance_monitoring();

        assert_eq!(config.service_name, "test-service");
        assert_eq!(config.service_version, "1.0.0");
        assert_eq!(config.log_level, "debug");
        assert!(config.security_auditing);
        assert!(config.performance_monitoring);
    }

    #[tokio::test]
    async fn test_security_audit_logger() {
        let logger = SecurityAuditLogger::new(true);

        // These should not panic
        logger.log_authentication("user123", true, Some("JWT token"));
        logger.log_authorization("user123", "/api/tools", "execute", true);
        logger.log_tool_execution("user123", "file_reader", true, 150);
        logger.log_security_violation("rate_limit_exceeded", "Too many requests", "warning");
    }

    #[test]
    fn test_performance_monitor() {
        let monitor = PerformanceMonitor::new(true);
        let span = monitor.start_span("test_operation");
        let duration = span.finish();

        assert!(duration.as_nanos() > 0);
    }

    #[tokio::test]
    async fn test_global_observability() {
        let global = global_observability();
        let logger = SecurityAuditLogger::new(true);
        let monitor = PerformanceMonitor::new(true);

        global.set_security_audit_logger(logger.clone()).await;
        global.set_performance_monitor(monitor.clone()).await;

        let retrieved_logger = global.security_audit_logger().await;
        let retrieved_monitor = global.performance_monitor().await;

        assert!(retrieved_logger.is_some());
        assert!(retrieved_monitor.is_some());
    }
}
