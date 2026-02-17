//! Transport metrics types.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

/// A serializable snapshot of a transport's performance metrics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TransportMetrics {
    /// Total number of bytes sent.
    pub bytes_sent: u64,

    /// Total number of bytes received.
    pub bytes_received: u64,

    /// Total number of messages sent.
    pub messages_sent: u64,

    /// Total number of messages received.
    pub messages_received: u64,

    /// Total number of connection attempts.
    pub connections: u64,

    /// Total number of failed connection attempts.
    pub failed_connections: u64,

    /// The average latency of operations, in milliseconds.
    pub average_latency_ms: f64,

    /// The current number of active connections.
    pub active_connections: u64,

    /// The compression ratio (uncompressed size / compressed size), if applicable.
    pub compression_ratio: Option<f64>,

    /// A map for custom, transport-specific metrics.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, serde_json::Value>,
}

/// A lock-free, atomic structure for high-performance metrics updates.
#[derive(Debug)]
pub struct AtomicMetrics {
    /// Total bytes sent (atomic counter).
    pub bytes_sent: AtomicU64,

    /// Total bytes received (atomic counter).
    pub bytes_received: AtomicU64,

    /// Total messages sent (atomic counter).
    pub messages_sent: AtomicU64,

    /// Total messages received (atomic counter).
    pub messages_received: AtomicU64,

    /// Total connection attempts (atomic counter).
    pub connections: AtomicU64,

    /// Failed connection attempts (atomic counter).
    pub failed_connections: AtomicU64,

    /// Current active connections (atomic counter).
    pub active_connections: AtomicU64,

    /// The average latency, stored as an exponential moving average in microseconds.
    avg_latency_us: AtomicU64,

    /// Total bytes before compression.
    uncompressed_bytes: AtomicU64,

    /// Total bytes after compression.
    compressed_bytes: AtomicU64,
}

impl Default for AtomicMetrics {
    fn default() -> Self {
        Self {
            bytes_sent: AtomicU64::new(0),
            bytes_received: AtomicU64::new(0),
            messages_sent: AtomicU64::new(0),
            messages_received: AtomicU64::new(0),
            connections: AtomicU64::new(0),
            failed_connections: AtomicU64::new(0),
            active_connections: AtomicU64::new(0),
            avg_latency_us: AtomicU64::new(0),
            uncompressed_bytes: AtomicU64::new(0),
            compressed_bytes: AtomicU64::new(0),
        }
    }
}

impl AtomicMetrics {
    /// Creates a new `AtomicMetrics` instance with all counters initialized to zero.
    pub fn new() -> Self {
        Self::default()
    }

    /// Updates the average latency using an exponential moving average (EMA).
    pub fn update_latency_us(&self, latency_us: u64) {
        let current = self.avg_latency_us.load(Ordering::Relaxed);
        let new_avg = if current == 0 {
            latency_us
        } else {
            // EMA with alpha = 0.1: new_avg = old_avg * 0.9 + new_value * 0.1
            // Use saturating operations to prevent overflow on sustained multi-second latencies
            current.saturating_mul(9).saturating_add(latency_us) / 10
        };
        self.avg_latency_us.store(new_avg, Ordering::Relaxed);
    }

    /// Records compression statistics to track the compression ratio.
    pub fn record_compression(&self, uncompressed_size: u64, compressed_size: u64) {
        self.uncompressed_bytes
            .fetch_add(uncompressed_size, Ordering::Relaxed);
        self.compressed_bytes
            .fetch_add(compressed_size, Ordering::Relaxed);
    }

    /// Creates a serializable `TransportMetrics` snapshot from the current atomic values.
    pub fn snapshot(&self) -> TransportMetrics {
        let avg_latency_us = self.avg_latency_us.load(Ordering::Relaxed);
        let uncompressed = self.uncompressed_bytes.load(Ordering::Relaxed);
        let compressed = self.compressed_bytes.load(Ordering::Relaxed);

        let compression_ratio = if compressed > 0 && uncompressed > 0 {
            Some(uncompressed as f64 / compressed as f64)
        } else {
            None
        };

        TransportMetrics {
            bytes_sent: self.bytes_sent.load(Ordering::Relaxed),
            bytes_received: self.bytes_received.load(Ordering::Relaxed),
            messages_sent: self.messages_sent.load(Ordering::Relaxed),
            messages_received: self.messages_received.load(Ordering::Relaxed),
            connections: self.connections.load(Ordering::Relaxed),
            failed_connections: self.failed_connections.load(Ordering::Relaxed),
            active_connections: self.active_connections.load(Ordering::Relaxed),
            average_latency_ms: (avg_latency_us as f64) / 1000.0,
            compression_ratio,
            metadata: HashMap::new(),
        }
    }

    /// Resets all atomic metric counters to zero.
    pub fn reset(&self) {
        self.bytes_sent.store(0, Ordering::Relaxed);
        self.bytes_received.store(0, Ordering::Relaxed);
        self.messages_sent.store(0, Ordering::Relaxed);
        self.messages_received.store(0, Ordering::Relaxed);
        self.connections.store(0, Ordering::Relaxed);
        self.failed_connections.store(0, Ordering::Relaxed);
        self.active_connections.store(0, Ordering::Relaxed);
        self.avg_latency_us.store(0, Ordering::Relaxed);
        self.uncompressed_bytes.store(0, Ordering::Relaxed);
        self.compressed_bytes.store(0, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_atomic_metrics_default() {
        let metrics = AtomicMetrics::new();
        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.bytes_sent, 0);
        assert_eq!(snapshot.messages_sent, 0);
    }

    #[test]
    fn test_atomic_metrics_update() {
        let metrics = AtomicMetrics::new();
        metrics.bytes_sent.fetch_add(100, Ordering::Relaxed);
        metrics.messages_sent.fetch_add(1, Ordering::Relaxed);

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.bytes_sent, 100);
        assert_eq!(snapshot.messages_sent, 1);
    }

    #[test]
    fn test_atomic_metrics_reset() {
        let metrics = AtomicMetrics::new();
        metrics.bytes_sent.fetch_add(100, Ordering::Relaxed);
        metrics.reset();

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.bytes_sent, 0);
    }

    #[test]
    fn test_ema_overflow_protection() {
        let metrics = AtomicMetrics::new();

        // Test with extremely large latency values that could cause overflow
        // without saturating operations
        let large_latency = u64::MAX / 5;

        // First update - should not panic
        metrics.update_latency_us(large_latency);
        let snapshot1 = metrics.snapshot();
        assert_eq!(snapshot1.average_latency_ms, large_latency as f64 / 1000.0);

        // Second update with large value - EMA calculation should saturate instead of overflow
        metrics.update_latency_us(large_latency);
        let snapshot2 = metrics.snapshot();

        // Verify the result is reasonable and didn't overflow
        assert!(snapshot2.average_latency_ms > 0.0);
        assert!(snapshot2.average_latency_ms.is_finite());

        // Multiple sustained high-latency updates should not overflow
        for _ in 0..100 {
            metrics.update_latency_us(large_latency);
        }
        let snapshot3 = metrics.snapshot();
        assert!(snapshot3.average_latency_ms > 0.0);
        assert!(snapshot3.average_latency_ms.is_finite());
    }
}
