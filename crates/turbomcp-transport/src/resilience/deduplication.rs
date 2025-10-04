//! Message deduplication cache for preventing duplicate processing
//!
//! This module provides a time-based cache for tracking message IDs to prevent
//! duplicate message processing. Features include:
//! - TTL-based automatic cleanup of expired entries
//! - Size-based LRU eviction to prevent unbounded growth
//! - Efficient duplicate detection with O(1) lookup

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Message deduplication cache configuration
#[derive(Debug, Clone)]
pub struct DeduplicationConfig {
    /// Maximum number of entries in the cache
    pub max_size: usize,
    /// Time-to-live for cache entries
    pub ttl: Duration,
}

/// Message deduplication cache
#[derive(Debug)]
pub struct DeduplicationCache {
    /// Message ID cache with timestamps
    pub cache: HashMap<String, Instant>,
    /// Cache size limit
    pub max_size: usize,
    /// Cache entry TTL
    pub ttl: Duration,
}

impl Default for DeduplicationConfig {
    fn default() -> Self {
        Self {
            max_size: 1000,
            ttl: Duration::from_secs(300), // 5 minutes
        }
    }
}

impl DeduplicationConfig {
    /// Create a new deduplication configuration with sensible defaults
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a configuration optimized for high-throughput scenarios
    pub fn high_throughput() -> Self {
        Self {
            max_size: 10000,
            ttl: Duration::from_secs(60), // 1 minute
        }
    }

    /// Create a configuration optimized for low-memory usage
    pub fn low_memory() -> Self {
        Self {
            max_size: 100,
            ttl: Duration::from_secs(600), // 10 minutes
        }
    }
}

impl DeduplicationCache {
    /// Create a new deduplication cache
    pub fn new(max_size: usize, ttl: Duration) -> Self {
        Self {
            cache: HashMap::new(),
            max_size,
            ttl,
        }
    }

    /// Create a deduplication cache with default configuration
    pub fn with_defaults() -> Self {
        Self::new(1000, Duration::from_secs(300))
    }

    /// Create a deduplication cache from configuration
    pub fn from_config(config: DeduplicationConfig) -> Self {
        Self::new(config.max_size, config.ttl)
    }

    /// Check if message is duplicate and add to cache if not
    pub fn is_duplicate(&mut self, message_id: &str) -> bool {
        self.cleanup_expired();

        if self.cache.contains_key(message_id) {
            true
        } else {
            self.cache.insert(message_id.to_string(), Instant::now());
            self.maintain_size_limit();
            false
        }
    }

    /// Add message ID to cache (marking it as seen)
    pub fn mark_seen(&mut self, message_id: &str) {
        self.cache.insert(message_id.to_string(), Instant::now());
        self.maintain_size_limit();
    }

    /// Check if message ID exists in cache without modifying it
    pub fn contains(&self, message_id: &str) -> bool {
        if let Some(timestamp) = self.cache.get(message_id) {
            Instant::now().duration_since(*timestamp) < self.ttl
        } else {
            false
        }
    }

    /// Get current cache size
    pub fn size(&self) -> usize {
        self.cache.len()
    }

    /// Check if cache is empty
    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }

    /// Clear all entries from the cache
    pub fn clear(&mut self) {
        self.cache.clear();
    }

    /// Get cache statistics
    pub fn statistics(&self) -> DeduplicationStats {
        let now = Instant::now();
        let expired_count = self
            .cache
            .values()
            .filter(|timestamp| now.duration_since(**timestamp) >= self.ttl)
            .count();

        DeduplicationStats {
            total_entries: self.cache.len(),
            expired_entries: expired_count,
            active_entries: self.cache.len() - expired_count,
            max_size: self.max_size,
            ttl: self.ttl,
        }
    }

    /// Clean up expired entries
    fn cleanup_expired(&mut self) {
        let now = Instant::now();
        self.cache
            .retain(|_, timestamp| now.duration_since(*timestamp) < self.ttl);
    }

    /// Maintain cache size limit by removing oldest entries
    fn maintain_size_limit(&mut self) {
        if self.cache.len() > self.max_size {
            // Remove oldest entries
            let mut entries: Vec<_> = self.cache.iter().collect();
            entries.sort_by_key(|(_, timestamp)| *timestamp);

            let to_remove = self.cache.len() - self.max_size;
            let keys_to_remove: Vec<String> = entries
                .iter()
                .take(to_remove)
                .map(|(k, _)| (*k).clone())
                .collect();

            for key in keys_to_remove {
                self.cache.remove(&key);
            }
        }
    }
}

/// Deduplication cache statistics
#[derive(Debug, Clone)]
pub struct DeduplicationStats {
    /// Total number of entries in cache
    pub total_entries: usize,
    /// Number of expired entries
    pub expired_entries: usize,
    /// Number of active (non-expired) entries
    pub active_entries: usize,
    /// Maximum cache size
    pub max_size: usize,
    /// Entry time-to-live
    pub ttl: Duration,
}

impl DeduplicationStats {
    /// Calculate cache utilization as a percentage
    pub fn utilization_percent(&self) -> f64 {
        if self.max_size == 0 {
            0.0
        } else {
            (self.active_entries as f64 / self.max_size as f64) * 100.0
        }
    }

    /// Calculate expired entry percentage
    pub fn expired_percent(&self) -> f64 {
        if self.total_entries == 0 {
            0.0
        } else {
            (self.expired_entries as f64 / self.total_entries as f64) * 100.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;

    #[test]
    fn test_deduplication_cache_basic() {
        let mut cache = DeduplicationCache::new(10, Duration::from_secs(1));

        // First occurrence should not be duplicate
        assert!(!cache.is_duplicate("msg1"));
        assert_eq!(cache.size(), 1);

        // Second occurrence should be duplicate
        assert!(cache.is_duplicate("msg1"));
        assert_eq!(cache.size(), 1); // Size shouldn't change
    }

    #[test]
    fn test_deduplication_cache_ttl() {
        let mut cache = DeduplicationCache::new(10, Duration::from_millis(100));

        assert!(!cache.is_duplicate("msg1"));
        assert!(cache.contains("msg1"));

        // Wait for TTL to expire
        sleep(Duration::from_millis(150));

        // Should no longer contain the message
        assert!(!cache.contains("msg1"));

        // Should not be duplicate after expiry
        assert!(!cache.is_duplicate("msg1"));
    }

    #[test]
    fn test_deduplication_cache_size_limit() {
        let mut cache = DeduplicationCache::new(3, Duration::from_secs(10));

        // Add messages up to limit
        assert!(!cache.is_duplicate("msg1"));
        assert!(!cache.is_duplicate("msg2"));
        assert!(!cache.is_duplicate("msg3"));
        assert_eq!(cache.size(), 3);

        // Adding another should evict the oldest
        assert!(!cache.is_duplicate("msg4"));
        assert_eq!(cache.size(), 3);

        // msg1 should have been evicted
        assert!(!cache.contains("msg1"));
        assert!(cache.contains("msg4"));
    }

    #[test]
    fn test_deduplication_cache_clear() {
        let mut cache = DeduplicationCache::new(10, Duration::from_secs(1));

        cache.mark_seen("msg1");
        cache.mark_seen("msg2");
        assert_eq!(cache.size(), 2);

        cache.clear();
        assert_eq!(cache.size(), 0);
        assert!(cache.is_empty());
    }

    #[test]
    fn test_deduplication_stats() {
        let cache = DeduplicationCache::new(100, Duration::from_secs(60));

        let stats = cache.statistics();
        assert_eq!(stats.max_size, 100);
        assert_eq!(stats.ttl, Duration::from_secs(60));
        assert_eq!(stats.utilization_percent(), 0.0);
    }

    #[test]
    fn test_deduplication_config_presets() {
        let high_throughput = DeduplicationConfig::high_throughput();
        assert_eq!(high_throughput.max_size, 10000);
        assert_eq!(high_throughput.ttl, Duration::from_secs(60));

        let low_memory = DeduplicationConfig::low_memory();
        assert_eq!(low_memory.max_size, 100);
        assert_eq!(low_memory.ttl, Duration::from_secs(600));
    }
}
