//! Health checking and monitoring for transport connections
//!
//! This module provides comprehensive health checking capabilities:
//! - Configurable health check intervals and timeouts
//! - Consecutive success/failure thresholds
//! - Health status tracking with detailed information
//! - Integration with transport implementations

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, SystemTime};
use tokio::time::timeout;

use crate::core::{Transport, TransportResult};

/// Health check configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheckConfig {
    /// Health check interval
    pub interval: Duration,
    /// Health check timeout
    pub timeout: Duration,
    /// Number of consecutive failures before marking unhealthy
    pub failure_threshold: u32,
    /// Number of consecutive successes before marking healthy
    pub success_threshold: u32,
    /// Custom health check endpoint or command
    pub custom_check: Option<String>,
}

/// Health status
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HealthStatus {
    /// Transport is healthy
    Healthy,
    /// Transport is unhealthy
    Unhealthy,
    /// Health status is unknown
    Unknown,
    /// Health check is in progress
    Checking,
}

/// Transport health information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthInfo {
    /// Current health status
    pub status: HealthStatus,
    /// Last health check time
    pub last_check: SystemTime,
    /// Consecutive successful checks
    pub consecutive_successes: u32,
    /// Consecutive failed checks
    pub consecutive_failures: u32,
    /// Additional health details
    pub details: HashMap<String, serde_json::Value>,
}

/// Health checker implementation
#[derive(Debug)]
pub struct HealthChecker {
    /// Health check configuration
    config: HealthCheckConfig,
    /// Current health information
    health_info: HealthInfo,
    /// Last health check result
    last_check_result: Option<bool>,
}

impl Default for HealthStatus {
    fn default() -> Self {
        Self::Unknown
    }
}

impl Default for HealthCheckConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(30),
            timeout: Duration::from_secs(5),
            failure_threshold: 3,
            success_threshold: 2,
            custom_check: None,
        }
    }
}

impl Default for HealthInfo {
    fn default() -> Self {
        Self {
            status: HealthStatus::Unknown,
            last_check: SystemTime::now(),
            consecutive_successes: 0,
            consecutive_failures: 0,
            details: HashMap::new(),
        }
    }
}

impl HealthCheckConfig {
    /// Create a new health check configuration with sensible defaults
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a configuration optimized for network transports
    pub fn for_network() -> Self {
        Self {
            interval: Duration::from_secs(15),
            timeout: Duration::from_secs(3),
            failure_threshold: 2,
            success_threshold: 1,
            custom_check: None,
        }
    }

    /// Create a configuration optimized for local transports
    pub fn for_local() -> Self {
        Self {
            interval: Duration::from_secs(60),
            timeout: Duration::from_secs(1),
            failure_threshold: 5,
            success_threshold: 2,
            custom_check: None,
        }
    }
}

impl HealthChecker {
    /// Create a new health checker
    pub fn new(config: HealthCheckConfig) -> Self {
        Self {
            config,
            health_info: HealthInfo::default(),
            last_check_result: None,
        }
    }

    /// Create a health checker with default configuration
    pub fn with_defaults() -> Self { Self::new(HealthCheckConfig::default()) }

    /// Perform health check on the given transport
    pub async fn check_health(&mut self, transport: &dyn Transport) -> bool {
        self.health_info.status = HealthStatus::Checking;
        self.health_info.last_check = SystemTime::now();

        let check_result = timeout(self.config.timeout, self.perform_check(transport)).await;

        let success = match check_result {
            Ok(Ok(healthy)) => healthy,
            Ok(Err(_)) => false,
            Err(_) => false, // Timeout
        };

        self.update_health_status(success);
        success
    }

    /// Get current health information
    pub const fn health_info(&self) -> &HealthInfo {
        &self.health_info
    }

    /// Get the last check result
    pub const fn last_check_result(&self) -> Option<bool> {
        self.last_check_result
    }

    /// Check if the transport is currently healthy
    pub fn is_healthy(&self) -> bool {
        matches!(self.health_info.status, HealthStatus::Healthy)
    }

    /// Check if enough time has passed for the next health check
    pub fn should_check(&self) -> bool {
        match self.health_info.last_check.elapsed() {
            Ok(elapsed) => elapsed >= self.config.interval,
            Err(_) => true, // If we can't determine elapsed time, err on the side of checking
        }
    }

    /// Add custom health details
    pub fn add_health_detail(&mut self, key: String, value: serde_json::Value) {
        self.health_info.details.insert(key, value);
    }

    /// Clear all health details
    pub fn clear_health_details(&mut self) {
        self.health_info.details.clear();
    }

    /// Reset health checker to initial state
    pub fn reset(&mut self) {
        self.health_info = HealthInfo::default();
        self.last_check_result = None;
    }

    /// Perform actual health check
    async fn perform_check(&self, transport: &dyn Transport) -> TransportResult<bool> {
        // Basic health check - verify transport is connected
        Ok(transport.is_connected().await)
    }

    /// Update health status based on check result
    fn update_health_status(&mut self, success: bool) {
        if success {
            self.health_info.consecutive_successes += 1;
            self.health_info.consecutive_failures = 0;

            if self.health_info.consecutive_successes >= self.config.success_threshold {
                self.health_info.status = HealthStatus::Healthy;
            } else {
                self.health_info.status = HealthStatus::Unknown;
            }
        } else {
            self.health_info.consecutive_failures += 1;
            self.health_info.consecutive_successes = 0;

            if self.health_info.consecutive_failures >= self.config.failure_threshold {
                self.health_info.status = HealthStatus::Unhealthy;
            } else {
                self.health_info.status = HealthStatus::Unknown;
            }
        }

        self.last_check_result = Some(success);
    }
}

/// Trait for health checkable components
#[async_trait]
pub trait HealthCheckable {
    /// Perform a health check
    async fn health_check(&self) -> TransportResult<HealthInfo>;

    /// Get current health status
    fn health_status(&self) -> HealthStatus;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{
        TransportCapabilities, TransportMessage, TransportMetrics, TransportState, TransportType,
    };
    use bytes::Bytes;
    use turbomcp_core::MessageId;
    use uuid::Uuid;

    #[derive(Debug)]
    struct MockTransport {
        connected: bool,
    }

    #[async_trait]
    impl Transport for MockTransport {
        async fn connect(&mut self) -> TransportResult<()> {
            self.connected = true;
            Ok(())
        }

        async fn disconnect(&mut self) -> TransportResult<()> {
            self.connected = false;
            Ok(())
        }

        async fn send(&mut self, _message: TransportMessage) -> TransportResult<()> {
            Ok(())
        }

        async fn receive(&mut self) -> TransportResult<Option<TransportMessage>> {
            Ok(Some(TransportMessage::new(
                MessageId::from(Uuid::new_v4()),
                Bytes::from("test"),
            )))
        }

        async fn state(&self) -> TransportState {
            if self.connected {
                TransportState::Connected
            } else {
                TransportState::Disconnected
            }
        }

        fn transport_type(&self) -> TransportType {
            TransportType::Stdio
        }

        fn capabilities(&self) -> &TransportCapabilities {
            static CAPS: std::sync::LazyLock<TransportCapabilities> =
                std::sync::LazyLock::new(TransportCapabilities::default);
            &CAPS
        }

        async fn metrics(&self) -> TransportMetrics {
            TransportMetrics::default()
        }

        fn endpoint(&self) -> Option<String> {
            Some("mock://test".to_string())
        }

        async fn configure(
            &mut self,
            _config: crate::core::TransportConfig,
        ) -> TransportResult<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_health_checker_healthy_transport() {
        let mut checker = HealthChecker::default();
        let transport = MockTransport { connected: true };

        let result = checker.check_health(&transport).await;
        assert!(result);
        assert_eq!(checker.health_info().status, HealthStatus::Unknown); // Needs multiple successes
        assert_eq!(checker.health_info().consecutive_successes, 1);
    }

    #[tokio::test]
    async fn test_health_checker_unhealthy_transport() {
        let mut checker = HealthChecker::default();
        let transport = MockTransport { connected: false };

        let result = checker.check_health(&transport).await;
        assert!(!result);
        assert_eq!(checker.health_info().consecutive_failures, 1);
    }

    #[tokio::test]
    async fn test_health_checker_threshold_behavior() {
        let config = HealthCheckConfig {
            success_threshold: 2,
            failure_threshold: 2,
            ..HealthCheckConfig::default()
        };
        let mut checker = HealthChecker::new(config);
        let transport = MockTransport { connected: true };

        // First success
        checker.check_health(&transport).await;
        assert_eq!(checker.health_info().status, HealthStatus::Unknown);

        // Second success - should become healthy
        checker.check_health(&transport).await;
        assert_eq!(checker.health_info().status, HealthStatus::Healthy);
        assert!(checker.is_healthy());
    }

    #[test]
    fn test_health_config_presets() {
        let network_config = HealthCheckConfig::for_network();
        assert_eq!(network_config.interval, Duration::from_secs(15));
        assert_eq!(network_config.failure_threshold, 2);

        let local_config = HealthCheckConfig::for_local();
        assert_eq!(local_config.interval, Duration::from_secs(60));
        assert_eq!(local_config.failure_threshold, 5);
    }

    #[test]
    fn test_health_details() {
        let mut checker = HealthChecker::default();

        checker.add_health_detail("latency".to_string(), serde_json::json!(150));
        checker.add_health_detail("endpoint".to_string(), serde_json::json!("localhost:8080"));

        let details = &checker.health_info().details;
        assert_eq!(details.len(), 2);
        assert_eq!(details["latency"], serde_json::json!(150));

        checker.clear_health_details();
        assert!(checker.health_info().details.is_empty());
    }
}
