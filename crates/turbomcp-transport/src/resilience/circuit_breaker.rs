//! Circuit breaker pattern implementation for fault tolerance
//!
//! This module provides a circuit breaker implementation that:
//! - Monitors operation success/failure rates
//! - Trips open to fail fast when error thresholds are exceeded
//! - Gradually recovers through a half-open state
//! - Maintains rolling windows of operation statistics
//! - Provides detailed metrics for monitoring

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Circuit breaker configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircuitBreakerConfig {
    /// Failure threshold to open circuit
    pub failure_threshold: u32,
    /// Success threshold to close circuit
    pub success_threshold: u32,
    /// Timeout in open state before trying half-open
    pub timeout: Duration,
    /// Rolling window size for failure counting
    pub rolling_window_size: usize,
    /// Minimum request threshold before opening circuit
    pub minimum_requests: u32,
}

/// Circuit breaker states
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CircuitState {
    /// Circuit is closed (normal operation)
    Closed,
    /// Circuit is open (failing fast)
    Open,
    /// Circuit is half-open (testing if service recovered)
    HalfOpen,
}

/// Operation result for circuit breaker tracking
#[derive(Debug, Clone)]
pub struct OperationResult {
    /// Operation timestamp
    pub timestamp: Instant,
    /// Whether operation was successful
    pub success: bool,
    /// Operation duration
    pub duration: Duration,
}

/// Circuit breaker statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircuitBreakerStats {
    /// Current circuit state
    pub state: CircuitState,
    /// Current failure count
    pub failure_count: u32,
    /// Current success count (in half-open)
    pub success_count: u32,
    /// Current failure rate (0.0 - 1.0)
    pub failure_rate: f64,
    /// Average operation duration
    pub avg_operation_duration: Duration,
    /// Time spent in current state
    pub time_in_current_state: Duration,
}

/// Circuit breaker implementation
#[derive(Debug)]
pub struct CircuitBreaker {
    /// Circuit breaker configuration
    config: CircuitBreakerConfig,
    /// Current circuit state
    state: CircuitState,
    /// Failure count in current window
    failure_count: u32,
    /// Success count in half-open state
    success_count: u32,
    /// Last state change time
    last_state_change: Instant,
    /// Rolling window of recent operations
    rolling_window: VecDeque<OperationResult>,
}

impl Default for CircuitState {
    fn default() -> Self {
        Self::Closed
    }
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            success_threshold: 3,
            timeout: Duration::from_secs(60),
            rolling_window_size: 100,
            minimum_requests: 10,
        }
    }
}

impl CircuitBreakerConfig {
    /// Create a new circuit breaker configuration with sensible defaults
    pub fn new() -> Self {
        Self::default()
    }
}

impl CircuitBreaker {
    /// Create a new circuit breaker
    pub fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            config,
            state: CircuitState::Closed,
            failure_count: 0,
            success_count: 0,
            last_state_change: Instant::now(),
            rolling_window: VecDeque::new(),
        }
    }

    /// Create a circuit breaker with default configuration
    pub fn with_defaults() -> Self {
        Self::new(CircuitBreakerConfig::default())
    }

    /// Check if operation should be allowed
    pub fn should_allow_operation(&mut self) -> bool {
        self.update_state();

        match self.state {
            CircuitState::Closed => true,
            CircuitState::Open => false,
            CircuitState::HalfOpen => true,
        }
    }

    /// Record operation result
    pub fn record_result(&mut self, success: bool, duration: Duration) {
        let result = OperationResult {
            timestamp: Instant::now(),
            success,
            duration,
        };

        self.rolling_window.push_back(result);

        // Maintain rolling window size
        while self.rolling_window.len() > self.config.rolling_window_size {
            self.rolling_window.pop_front();
        }

        match self.state {
            CircuitState::Closed => {
                if success {
                    self.failure_count = 0;
                } else {
                    self.failure_count += 1;
                    if self.should_trip() {
                        self.trip_circuit();
                    }
                }
            }
            CircuitState::HalfOpen => {
                if success {
                    self.success_count += 1;
                    if self.success_count >= self.config.success_threshold {
                        self.close_circuit();
                    }
                } else {
                    self.trip_circuit();
                }
            }
            CircuitState::Open => {
                // No action needed in open state
            }
        }
    }

    /// Get current circuit state
    pub fn state(&self) -> CircuitState {
        self.state.clone()
    }

    /// Get circuit breaker statistics
    pub fn statistics(&self) -> CircuitBreakerStats {
        let failure_rate = if self.rolling_window.is_empty() {
            0.0
        } else {
            let failures = self.rolling_window.iter().filter(|r| !r.success).count();
            failures as f64 / self.rolling_window.len() as f64
        };

        let avg_duration = if self.rolling_window.is_empty() {
            Duration::ZERO
        } else {
            let total: Duration = self.rolling_window.iter().map(|r| r.duration).sum();
            total / self.rolling_window.len() as u32
        };

        CircuitBreakerStats {
            state: self.state.clone(),
            failure_count: self.failure_count,
            success_count: self.success_count,
            failure_rate,
            avg_operation_duration: avg_duration,
            time_in_current_state: self.last_state_change.elapsed(),
        }
    }

    /// Reset the circuit breaker to closed state
    pub fn reset(&mut self) {
        self.state = CircuitState::Closed;
        self.failure_count = 0;
        self.success_count = 0;
        self.last_state_change = Instant::now();
        self.rolling_window.clear();
    }

    /// Check if circuit should trip
    fn should_trip(&self) -> bool {
        let total_requests = self.rolling_window.len() as u32;

        if total_requests < self.config.minimum_requests {
            return false;
        }

        self.failure_count >= self.config.failure_threshold
    }

    /// Trip the circuit breaker
    fn trip_circuit(&mut self) {
        self.state = CircuitState::Open;
        self.last_state_change = Instant::now();
        self.failure_count = 0;
        self.success_count = 0;
    }

    /// Close the circuit breaker
    fn close_circuit(&mut self) {
        self.state = CircuitState::Closed;
        self.last_state_change = Instant::now();
        self.failure_count = 0;
        self.success_count = 0;
    }

    /// Update circuit state based on time
    fn update_state(&mut self) {
        if self.state == CircuitState::Open
            && self.last_state_change.elapsed() >= self.config.timeout
        {
            self.state = CircuitState::HalfOpen;
            self.last_state_change = Instant::now();
            self.success_count = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_circuit_breaker_default_state() {
        let mut breaker = CircuitBreaker::with_defaults();
        assert_eq!(breaker.state(), CircuitState::Closed);
        assert!(breaker.should_allow_operation());
    }

    #[test]
    fn test_circuit_breaker_trip_on_failures() {
        let config = CircuitBreakerConfig {
            failure_threshold: 2,
            minimum_requests: 2,
            ..CircuitBreakerConfig::default()
        };
        let mut breaker = CircuitBreaker::new(config);

        // Record failures
        breaker.record_result(false, Duration::from_millis(100));
        breaker.record_result(false, Duration::from_millis(100));

        // Should trip to open
        assert_eq!(breaker.state(), CircuitState::Open);
        assert!(!breaker.should_allow_operation());
    }

    #[test]
    fn test_circuit_breaker_half_open_recovery() {
        let config = CircuitBreakerConfig {
            failure_threshold: 1,
            success_threshold: 2,
            timeout: Duration::from_millis(100),
            minimum_requests: 1,
            ..CircuitBreakerConfig::default()
        };
        let mut breaker = CircuitBreaker::new(config);

        // Trip circuit
        breaker.record_result(false, Duration::from_millis(100));
        assert_eq!(breaker.state(), CircuitState::Open);

        // Wait for timeout and check state
        std::thread::sleep(Duration::from_millis(150));
        assert!(breaker.should_allow_operation()); // Should transition to half-open

        // Record successes to close circuit
        breaker.record_result(true, Duration::from_millis(50));
        breaker.record_result(true, Duration::from_millis(50));

        assert_eq!(breaker.state(), CircuitState::Closed);
    }

    #[test]
    fn test_circuit_breaker_statistics() {
        let mut breaker = CircuitBreaker::with_defaults();

        breaker.record_result(true, Duration::from_millis(100));
        breaker.record_result(false, Duration::from_millis(200));

        let stats = breaker.statistics();
        assert_eq!(stats.state, CircuitState::Closed);
        assert_eq!(stats.failure_rate, 0.5);
        assert_eq!(stats.avg_operation_duration, Duration::from_millis(150));
    }
}
