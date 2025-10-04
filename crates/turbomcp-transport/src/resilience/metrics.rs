//! Metrics and observability for TurboTransport operations
//!
//! This module provides comprehensive metrics collection for monitoring:
//! - Retry attempts and success rates
//! - Circuit breaker state changes and trip counts
//! - Health check failures and recovery times
//! - Message deduplication statistics
//! - Operation latency measurements

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::RwLock;

use super::circuit_breaker::CircuitState;
use super::health::HealthStatus;

/// TurboTransport metrics collection
#[derive(Debug, Default)]
pub struct TurboTransportMetrics {
    /// Total retry attempts across all operations
    pub retry_attempts: AtomicU64,
    /// Number of successful retry operations
    pub successful_retries: AtomicU64,
    /// Number of times circuit breaker has tripped
    pub circuit_breaker_trips: AtomicU64,
    /// Number of health check failures
    pub health_check_failures: AtomicU64,
    /// Number of duplicate messages filtered
    pub duplicate_messages_filtered: AtomicU64,
    /// Average operation latency in microseconds
    pub avg_operation_latency_us: AtomicU64,
    /// Current circuit breaker state
    pub circuit_state: Arc<RwLock<CircuitState>>,
    /// Current health status
    pub health_status: Arc<RwLock<HealthStatus>>,
}

/// Snapshot of metrics at a point in time
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsSnapshot {
    /// Total retry attempts
    pub retry_attempts: u64,
    /// Successful retries
    pub successful_retries: u64,
    /// Circuit breaker trips
    pub circuit_breaker_trips: u64,
    /// Health check failures
    pub health_check_failures: u64,
    /// Duplicate messages filtered
    pub duplicate_messages_filtered: u64,
    /// Average operation latency (microseconds)
    pub avg_operation_latency_us: u64,
    /// Current circuit state
    pub circuit_state: CircuitState,
    /// Current health status
    pub health_status: HealthStatus,
    /// Retry success rate (0.0 - 1.0)
    pub retry_success_rate: f64,
}

/// Latency statistics tracker
#[derive(Debug)]
pub struct LatencyTracker {
    samples: Vec<u64>,
    max_samples: usize,
}

impl TurboTransportMetrics {
    /// Create a new metrics instance
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a retry attempt
    pub fn record_retry_attempt(&self) {
        self.retry_attempts.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a successful retry
    pub fn record_successful_retry(&self) {
        self.successful_retries.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a circuit breaker trip
    pub fn record_circuit_breaker_trip(&self) {
        self.circuit_breaker_trips.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a health check failure
    pub fn record_health_check_failure(&self) {
        self.health_check_failures.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a duplicate message being filtered
    pub fn record_duplicate_filtered(&self) {
        self.duplicate_messages_filtered
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Update average operation latency
    pub fn update_latency(&self, latency_us: u64) {
        // Simple exponential moving average
        let current = self.avg_operation_latency_us.load(Ordering::Relaxed);
        let new_avg = if current == 0 {
            latency_us
        } else {
            // EMA with alpha = 0.1
            (current * 9 + latency_us) / 10
        };
        self.avg_operation_latency_us
            .store(new_avg, Ordering::Relaxed);
    }

    /// Update circuit breaker state
    pub async fn update_circuit_state(&self, state: CircuitState) {
        let mut current_state = self.circuit_state.write().await;
        *current_state = state;
    }

    /// Update health status
    pub async fn update_health_status(&self, status: HealthStatus) {
        let mut current_status = self.health_status.write().await;
        *current_status = status;
    }

    /// Get current metrics snapshot
    pub async fn snapshot(&self) -> MetricsSnapshot {
        let retry_attempts = self.retry_attempts.load(Ordering::Relaxed);
        let successful_retries = self.successful_retries.load(Ordering::Relaxed);

        let retry_success_rate = if retry_attempts > 0 {
            successful_retries as f64 / retry_attempts as f64
        } else {
            0.0
        };

        MetricsSnapshot {
            retry_attempts,
            successful_retries,
            circuit_breaker_trips: self.circuit_breaker_trips.load(Ordering::Relaxed),
            health_check_failures: self.health_check_failures.load(Ordering::Relaxed),
            duplicate_messages_filtered: self.duplicate_messages_filtered.load(Ordering::Relaxed),
            avg_operation_latency_us: self.avg_operation_latency_us.load(Ordering::Relaxed),
            circuit_state: self.circuit_state.read().await.clone(),
            health_status: self.health_status.read().await.clone(),
            retry_success_rate,
        }
    }

    /// Reset all metrics to zero
    pub async fn reset(&self) {
        self.retry_attempts.store(0, Ordering::Relaxed);
        self.successful_retries.store(0, Ordering::Relaxed);
        self.circuit_breaker_trips.store(0, Ordering::Relaxed);
        self.health_check_failures.store(0, Ordering::Relaxed);
        self.duplicate_messages_filtered.store(0, Ordering::Relaxed);
        self.avg_operation_latency_us.store(0, Ordering::Relaxed);

        let mut circuit_state = self.circuit_state.write().await;
        *circuit_state = CircuitState::Closed;

        let mut health_status = self.health_status.write().await;
        *health_status = HealthStatus::Unknown;
    }

    /// Get retry success rate
    pub fn retry_success_rate(&self) -> f64 {
        let attempts = self.retry_attempts.load(Ordering::Relaxed);
        let successes = self.successful_retries.load(Ordering::Relaxed);

        if attempts > 0 {
            successes as f64 / attempts as f64
        } else {
            0.0
        }
    }

    /// Check if metrics indicate the transport is performing well
    pub async fn is_performing_well(&self) -> bool {
        let snapshot = self.snapshot().await;

        // Consider performing well if:
        // - Health status is healthy
        // - Circuit is closed
        // - Retry success rate is above 80% (if there have been retries)
        matches!(snapshot.health_status, HealthStatus::Healthy)
            && matches!(snapshot.circuit_state, CircuitState::Closed)
            && (snapshot.retry_attempts == 0 || snapshot.retry_success_rate >= 0.8)
    }
}

impl LatencyTracker {
    /// Create a new latency tracker
    pub fn new(max_samples: usize) -> Self {
        Self {
            samples: Vec::with_capacity(max_samples),
            max_samples,
        }
    }

    /// Add a latency sample
    pub fn add_sample(&mut self, latency_us: u64) {
        self.samples.push(latency_us);
        if self.samples.len() > self.max_samples {
            self.samples.remove(0);
        }
    }

    /// Get average latency
    pub fn average(&self) -> f64 {
        if self.samples.is_empty() {
            0.0
        } else {
            let sum: u64 = self.samples.iter().sum();
            sum as f64 / self.samples.len() as f64
        }
    }

    /// Get median latency
    pub fn median(&self) -> f64 {
        if self.samples.is_empty() {
            return 0.0;
        }

        let mut sorted = self.samples.clone();
        sorted.sort_unstable();

        let len = sorted.len();
        if len % 2 == 0 {
            (sorted[len / 2 - 1] + sorted[len / 2]) as f64 / 2.0
        } else {
            sorted[len / 2] as f64
        }
    }

    /// Get 95th percentile latency
    pub fn p95(&self) -> f64 {
        if self.samples.is_empty() {
            return 0.0;
        }

        let mut sorted = self.samples.clone();
        sorted.sort_unstable();

        let index = ((sorted.len() as f64) * 0.95) as usize;
        sorted[index.min(sorted.len() - 1)] as f64
    }

    /// Get minimum latency
    pub fn min(&self) -> u64 {
        self.samples.iter().copied().min().unwrap_or(0)
    }

    /// Get maximum latency
    pub fn max(&self) -> u64 {
        self.samples.iter().copied().max().unwrap_or(0)
    }

    /// Get number of samples
    pub fn sample_count(&self) -> usize {
        self.samples.len()
    }

    /// Clear all samples
    pub fn clear(&mut self) {
        self.samples.clear();
    }
}

impl MetricsSnapshot {
    /// Check if the snapshot indicates good performance
    pub fn is_healthy(&self) -> bool {
        matches!(self.health_status, HealthStatus::Healthy)
            && matches!(self.circuit_state, CircuitState::Closed)
            && self.retry_success_rate >= 0.8
    }

    /// Get a summary string of key metrics
    pub fn summary(&self) -> String {
        format!(
            "Retries: {}/{} ({:.1}%), CB trips: {}, Health failures: {}, Avg latency: {}μs",
            self.successful_retries,
            self.retry_attempts,
            self.retry_success_rate * 100.0,
            self.circuit_breaker_trips,
            self.health_check_failures,
            self.avg_operation_latency_us
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_metrics_recording() {
        let metrics = TurboTransportMetrics::new();

        metrics.record_retry_attempt();
        metrics.record_retry_attempt();
        metrics.record_successful_retry();

        let snapshot = metrics.snapshot().await;
        assert_eq!(snapshot.retry_attempts, 2);
        assert_eq!(snapshot.successful_retries, 1);
        assert_eq!(snapshot.retry_success_rate, 0.5);
    }

    #[tokio::test]
    async fn test_latency_tracking() {
        let metrics = TurboTransportMetrics::new();

        metrics.update_latency(100);
        metrics.update_latency(200);

        let snapshot = metrics.snapshot().await;
        // Should be around 110 due to exponential moving average
        assert!(snapshot.avg_operation_latency_us > 100);
        assert!(snapshot.avg_operation_latency_us < 200);
    }

    #[tokio::test]
    async fn test_state_updates() {
        let metrics = TurboTransportMetrics::new();

        metrics.update_circuit_state(CircuitState::Open).await;
        metrics.update_health_status(HealthStatus::Healthy).await;

        let snapshot = metrics.snapshot().await;
        assert_eq!(snapshot.circuit_state, CircuitState::Open);
        assert_eq!(snapshot.health_status, HealthStatus::Healthy);
    }

    #[test]
    fn test_latency_tracker() {
        let mut tracker = LatencyTracker::new(5);

        tracker.add_sample(100);
        tracker.add_sample(200);
        tracker.add_sample(300);

        assert_eq!(tracker.average(), 200.0);
        assert_eq!(tracker.median(), 200.0);
        assert_eq!(tracker.min(), 100);
        assert_eq!(tracker.max(), 300);
        assert_eq!(tracker.sample_count(), 3);
    }

    #[test]
    fn test_latency_tracker_max_samples() {
        let mut tracker = LatencyTracker::new(2);

        tracker.add_sample(100);
        tracker.add_sample(200);
        tracker.add_sample(300); // Should evict 100

        assert_eq!(tracker.sample_count(), 2);
        assert_eq!(tracker.min(), 200);
        assert_eq!(tracker.max(), 300);
    }

    #[tokio::test]
    async fn test_performance_assessment() {
        let metrics = TurboTransportMetrics::new();

        // Set good performance indicators
        metrics.update_health_status(HealthStatus::Healthy).await;
        metrics.update_circuit_state(CircuitState::Closed).await;

        assert!(metrics.is_performing_well().await);

        // Set poor performance indicators
        metrics.update_circuit_state(CircuitState::Open).await;

        assert!(!metrics.is_performing_well().await);
    }
}
