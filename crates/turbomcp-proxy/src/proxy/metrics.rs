//! Proxy metrics collection
//!
//! Lock-free atomic metrics for tracking proxy performance and health.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};

/// Lock-free atomic metrics for proxy performance tracking
///
/// Uses atomic operations for thread-safe, lock-free metric collection.
/// Suitable for high-throughput scenarios where lock contention would be problematic.
#[derive(Debug)]
pub struct AtomicMetrics {
    /// Total number of requests successfully forwarded to backend
    pub requests_forwarded: AtomicU64,

    /// Total number of failed requests
    pub requests_failed: AtomicU64,

    /// Total bytes sent to backend
    pub bytes_sent: AtomicU64,

    /// Total bytes received from backend
    pub bytes_received: AtomicU64,

    /// Current number of active sessions/connections
    pub active_sessions: AtomicU64,

    /// Exponential moving average of latency in microseconds
    avg_latency_us: AtomicU64,
}

impl AtomicMetrics {
    /// Create a new metrics collector with all counters at zero
    pub fn new() -> Self {
        Self {
            requests_forwarded: AtomicU64::new(0),
            requests_failed: AtomicU64::new(0),
            bytes_sent: AtomicU64::new(0),
            bytes_received: AtomicU64::new(0),
            active_sessions: AtomicU64::new(0),
            avg_latency_us: AtomicU64::new(0),
        }
    }

    /// Update the exponential moving average latency
    ///
    /// Uses a 90% weight for the current average and 10% for the new sample.
    /// This provides smoothing while still being responsive to changes.
    ///
    /// # Arguments
    ///
    /// * `latency_us` - New latency sample in microseconds
    pub fn update_latency_us(&self, latency_us: u64) {
        let current = self.avg_latency_us.load(Ordering::Relaxed);
        let new_avg = if current == 0 {
            // First sample - use it directly
            latency_us
        } else {
            // Exponential moving average: EMA = (current * 9 + new) / 10
            (current.saturating_mul(9).saturating_add(latency_us)) / 10
        };
        self.avg_latency_us.store(new_avg, Ordering::Relaxed);
    }

    /// Increment the requests forwarded counter
    pub fn inc_requests_forwarded(&self) {
        self.requests_forwarded.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment the requests failed counter
    pub fn inc_requests_failed(&self) {
        self.requests_failed.fetch_add(1, Ordering::Relaxed);
    }

    /// Add bytes sent to the counter
    pub fn add_bytes_sent(&self, bytes: u64) {
        self.bytes_sent.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Add bytes received to the counter
    pub fn add_bytes_received(&self, bytes: u64) {
        self.bytes_received.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Increment active sessions counter
    pub fn inc_active_sessions(&self) {
        self.active_sessions.fetch_add(1, Ordering::Relaxed);
    }

    /// Decrement active sessions counter
    pub fn dec_active_sessions(&self) {
        self.active_sessions.fetch_sub(1, Ordering::Relaxed);
    }

    /// Take a snapshot of current metrics
    ///
    /// Creates a consistent snapshot of all metrics at a point in time.
    /// Note: This is not a fully atomic snapshot - metrics may change
    /// between individual reads, but each metric is read atomically.
    pub fn snapshot(&self) -> ProxyMetrics {
        ProxyMetrics {
            requests_forwarded: self.requests_forwarded.load(Ordering::Relaxed),
            requests_failed: self.requests_failed.load(Ordering::Relaxed),
            bytes_sent: self.bytes_sent.load(Ordering::Relaxed),
            bytes_received: self.bytes_received.load(Ordering::Relaxed),
            active_sessions: self.active_sessions.load(Ordering::Relaxed),
            average_latency_ms: self.avg_latency_us.load(Ordering::Relaxed) as f64 / 1000.0,
        }
    }

    /// Reset all metrics to zero
    ///
    /// Useful for testing or resetting counters without recreating the struct.
    pub fn reset(&self) {
        self.requests_forwarded.store(0, Ordering::Relaxed);
        self.requests_failed.store(0, Ordering::Relaxed);
        self.bytes_sent.store(0, Ordering::Relaxed);
        self.bytes_received.store(0, Ordering::Relaxed);
        self.active_sessions.store(0, Ordering::Relaxed);
        self.avg_latency_us.store(0, Ordering::Relaxed);
    }
}

impl Default for AtomicMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Snapshot of proxy metrics at a point in time
///
/// Serializable for JSON API responses and monitoring systems.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProxyMetrics {
    /// Total requests successfully forwarded
    pub requests_forwarded: u64,

    /// Total requests that failed
    pub requests_failed: u64,

    /// Total bytes sent to backend
    pub bytes_sent: u64,

    /// Total bytes received from backend
    pub bytes_received: u64,

    /// Currently active sessions
    pub active_sessions: u64,

    /// Average latency in milliseconds
    pub average_latency_ms: f64,
}

impl ProxyMetrics {
    /// Calculate success rate as a percentage
    ///
    /// Returns `None` if no requests have been made yet.
    pub fn success_rate(&self) -> Option<f64> {
        let total = self.requests_forwarded + self.requests_failed;
        if total == 0 {
            None
        } else {
            Some((self.requests_forwarded as f64 / total as f64) * 100.0)
        }
    }

    /// Calculate total requests (successful + failed)
    pub fn total_requests(&self) -> u64 {
        self.requests_forwarded + self.requests_failed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_atomic_metrics_creation() {
        let metrics = AtomicMetrics::new();
        let snapshot = metrics.snapshot();

        assert_eq!(snapshot.requests_forwarded, 0);
        assert_eq!(snapshot.requests_failed, 0);
        assert_eq!(snapshot.bytes_sent, 0);
        assert_eq!(snapshot.bytes_received, 0);
        assert_eq!(snapshot.active_sessions, 0);
        assert_eq!(snapshot.average_latency_ms, 0.0);
    }

    #[test]
    fn test_increment_operations() {
        let metrics = AtomicMetrics::new();

        metrics.inc_requests_forwarded();
        metrics.inc_requests_forwarded();
        metrics.inc_requests_failed();

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.requests_forwarded, 2);
        assert_eq!(snapshot.requests_failed, 1);
    }

    #[test]
    fn test_bytes_tracking() {
        let metrics = AtomicMetrics::new();

        metrics.add_bytes_sent(1024);
        metrics.add_bytes_sent(2048);
        metrics.add_bytes_received(512);

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.bytes_sent, 3072);
        assert_eq!(snapshot.bytes_received, 512);
    }

    #[test]
    fn test_active_sessions() {
        let metrics = AtomicMetrics::new();

        metrics.inc_active_sessions();
        metrics.inc_active_sessions();
        metrics.inc_active_sessions();
        assert_eq!(metrics.snapshot().active_sessions, 3);

        metrics.dec_active_sessions();
        assert_eq!(metrics.snapshot().active_sessions, 2);
    }

    #[test]
    fn test_latency_tracking() {
        let metrics = AtomicMetrics::new();

        // First sample
        metrics.update_latency_us(1000);
        assert_eq!(metrics.snapshot().average_latency_ms, 1.0);

        // Second sample - should be weighted average
        metrics.update_latency_us(2000);
        // (1000 * 9 + 2000) / 10 = 1100 microseconds = 1.1 ms
        assert_eq!(metrics.snapshot().average_latency_ms, 1.1);
    }

    #[test]
    fn test_reset() {
        let metrics = AtomicMetrics::new();

        metrics.inc_requests_forwarded();
        metrics.inc_requests_failed();
        metrics.add_bytes_sent(1024);
        metrics.update_latency_us(1000);

        metrics.reset();

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.requests_forwarded, 0);
        assert_eq!(snapshot.requests_failed, 0);
        assert_eq!(snapshot.bytes_sent, 0);
        assert_eq!(snapshot.average_latency_ms, 0.0);
    }

    #[test]
    fn test_proxy_metrics_success_rate() {
        let metrics = ProxyMetrics {
            requests_forwarded: 90,
            requests_failed: 10,
            bytes_sent: 0,
            bytes_received: 0,
            active_sessions: 0,
            average_latency_ms: 0.0,
        };

        assert_eq!(metrics.success_rate(), Some(90.0));
        assert_eq!(metrics.total_requests(), 100);
    }

    #[test]
    fn test_proxy_metrics_success_rate_no_requests() {
        let metrics = ProxyMetrics {
            requests_forwarded: 0,
            requests_failed: 0,
            bytes_sent: 0,
            bytes_received: 0,
            active_sessions: 0,
            average_latency_ms: 0.0,
        };

        assert_eq!(metrics.success_rate(), None);
        assert_eq!(metrics.total_requests(), 0);
    }

    #[test]
    fn test_serialization() {
        let metrics = ProxyMetrics {
            requests_forwarded: 100,
            requests_failed: 5,
            bytes_sent: 1024,
            bytes_received: 2048,
            active_sessions: 3,
            average_latency_ms: 15.5,
        };

        let json = serde_json::to_string(&metrics).unwrap();
        let deserialized: ProxyMetrics = serde_json::from_str(&json).unwrap();

        assert_eq!(metrics, deserialized);
    }
}
