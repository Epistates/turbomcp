//! Message ID translation for proxy routing
//!
//! Manages bidirectional mapping between frontend and backend message IDs.
//! This is critical for request/response correlation when multiple frontend
//! clients send requests through the proxy to a single backend server.
//!
//! # Security Features
//!
//! - Bounded memory: Maximum 10,000 mappings to prevent unbounded growth
//! - Timeout-based eviction: Mappings expire after 5 minutes
//! - Lock-free concurrency: Uses DashMap for thread-safe, lock-free operations
//! - Race-free cleanup: Atomic removal operations prevent TOCTOU bugs

use dashmap::DashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use turbomcp_protocol::MessageId;

use crate::error::{ProxyError, ProxyResult};

/// Maximum number of concurrent ID mappings (prevents unbounded memory growth)
const MAX_MAPPINGS: usize = 10_000;

/// Mapping timeout - entries older than this are evicted (prevents memory leaks)
const MAPPING_TIMEOUT: Duration = Duration::from_secs(300); // 5 minutes

/// Mapping entry with creation timestamp for timeout-based eviction
#[derive(Debug, Clone)]
struct MappingEntry {
    /// The backend ID this frontend ID maps to
    backend_id: MessageId,
    /// When this mapping was created (for timeout-based eviction)
    created_at: Instant,
}

/// Thread-safe bidirectional MessageId translator
///
/// Handles ID translation between frontend clients and backend server:
/// - Frontend clients use their own ID schemes (strings, numbers)
/// - Backend server expects sequential or specific IDs
/// - Translator maintains bidirectional mapping for correlation
///
/// # Security
///
/// - Bounded: Maximum 10,000 concurrent mappings
/// - Timeout: Mappings expire after 5 minutes
/// - Lock-free: DashMap provides concurrent access without locks
/// - Race-free: Atomic operations prevent TOCTOU bugs
#[derive(Debug, Clone)]
pub struct IdTranslator {
    /// Frontend ID → MappingEntry (contains backend ID + timestamp)
    frontend_to_backend: Arc<DashMap<MessageId, MappingEntry>>,

    /// Backend ID → Frontend ID mapping (for reverse lookup)
    backend_to_frontend: Arc<DashMap<MessageId, MessageId>>,

    /// Counter for generating sequential backend IDs
    next_backend_id: Arc<AtomicU64>,

    /// Maximum number of concurrent mappings
    max_mappings: usize,

    /// Mapping timeout duration
    mapping_timeout: Duration,
}

impl IdTranslator {
    /// Create a new ID translator with default limits
    pub fn new() -> Self {
        Self::with_limits(MAX_MAPPINGS, MAPPING_TIMEOUT)
    }

    /// Create a new ID translator with custom limits
    ///
    /// # Arguments
    ///
    /// * `max_mappings` - Maximum concurrent mappings (prevents memory exhaustion)
    /// * `mapping_timeout` - How long before mappings expire (prevents leaks)
    pub fn with_limits(max_mappings: usize, mapping_timeout: Duration) -> Self {
        Self {
            frontend_to_backend: Arc::new(DashMap::new()),
            backend_to_frontend: Arc::new(DashMap::new()),
            next_backend_id: Arc::new(AtomicU64::new(1)),
            max_mappings,
            mapping_timeout,
        }
    }

    /// Allocate a backend ID for a frontend request
    ///
    /// # Arguments
    ///
    /// * `frontend_id` - The frontend message ID
    ///
    /// # Returns
    ///
    /// The allocated backend ID that should be used for the backend request
    ///
    /// # Errors
    ///
    /// Returns `ProxyError::RateLimitExceeded` if:
    /// - Too many concurrent mappings (server overloaded)
    /// - Cannot evict expired mappings to make room
    pub fn allocate(&self, frontend_id: MessageId) -> ProxyResult<MessageId> {
        // Evict expired entries first to make room
        self.evict_expired();

        // Check if we're at the limit
        if self.frontend_to_backend.len() >= self.max_mappings {
            return Err(ProxyError::rate_limit_exceeded(format!(
                "Too many pending requests ({}/{}), server overloaded",
                self.frontend_to_backend.len(),
                self.max_mappings
            )));
        }

        // Generate sequential backend ID
        let backend_id_num = self.next_backend_id.fetch_add(1, Ordering::SeqCst);
        let backend_id = MessageId::Number(backend_id_num as i64);

        // Create mapping entry with timestamp
        let entry = MappingEntry {
            backend_id: backend_id.clone(),
            created_at: Instant::now(),
        };

        // Store bidirectional mapping
        self.frontend_to_backend.insert(frontend_id.clone(), entry);
        self.backend_to_frontend
            .insert(backend_id.clone(), frontend_id);

        Ok(backend_id)
    }

    /// Get the frontend ID for a backend response
    ///
    /// # Arguments
    ///
    /// * `backend_id` - The backend message ID from the response
    ///
    /// # Returns
    ///
    /// The corresponding frontend ID, or None if not found
    pub fn get_frontend_id(&self, backend_id: &MessageId) -> Option<MessageId> {
        self.backend_to_frontend
            .get(backend_id)
            .map(|entry| entry.value().clone())
    }

    /// Release a mapping after response is sent
    ///
    /// Cleans up the bidirectional mapping to prevent memory leaks.
    /// Uses atomic operations to prevent TOCTOU race conditions.
    ///
    /// # Arguments
    ///
    /// * `frontend_id` - The frontend message ID to release
    pub fn release(&self, frontend_id: &MessageId) {
        // Atomically remove frontend mapping and get backend ID
        if let Some((_, entry)) = self.frontend_to_backend.remove(frontend_id) {
            // Atomically remove backend mapping, but only if it still points to this frontend_id
            // This prevents race conditions where the mapping might have changed
            self.backend_to_frontend
                .remove_if(&entry.backend_id, |_k, v| v == frontend_id);
        }
    }

    /// Evict expired mappings based on timeout
    ///
    /// This is called automatically by `allocate()` to prevent unbounded growth.
    /// Can also be called manually or by a background task.
    fn evict_expired(&self) {
        let now = Instant::now();

        // Remove expired frontend mappings
        self.frontend_to_backend
            .retain(|_k, v| now.duration_since(v.created_at) < self.mapping_timeout);

        // Clean up orphaned backend mappings (where frontend mapping no longer exists)
        self.backend_to_frontend
            .retain(|_backend_id, frontend_id| self.frontend_to_backend.contains_key(frontend_id));
    }

    /// Spawn a background task to periodically evict expired mappings
    ///
    /// Returns a join handle that can be used to cancel the task.
    /// The task runs every 60 seconds and evicts expired entries.
    pub fn spawn_eviction_task(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            loop {
                interval.tick().await;
                self.evict_expired();
            }
        })
    }

    /// Get current mapping count (for monitoring/metrics)
    pub fn mapping_count(&self) -> usize {
        self.frontend_to_backend.len()
    }

    /// Clear all mappings (for shutdown/reset)
    pub fn clear(&self) {
        self.frontend_to_backend.clear();
        self.backend_to_frontend.clear();
    }
}

impl Default for IdTranslator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_allocate_and_lookup() {
        let translator = IdTranslator::new();

        // Allocate for frontend request
        let frontend_id = MessageId::String("frontend-123".to_string());
        let backend_id = translator.allocate(frontend_id.clone()).unwrap();

        // Verify backend ID is sequential number
        assert!(matches!(backend_id, MessageId::Number(1)));

        // Reverse lookup
        let found_frontend_id = translator.get_frontend_id(&backend_id);
        assert_eq!(found_frontend_id, Some(frontend_id.clone()));

        // Release mapping
        translator.release(&frontend_id);

        // Verify mapping is gone
        let not_found = translator.get_frontend_id(&backend_id);
        assert_eq!(not_found, None);
    }

    #[test]
    fn test_multiple_allocations() {
        let translator = IdTranslator::new();

        // Allocate multiple IDs
        let id1 = MessageId::String("req-1".to_string());
        let id2 = MessageId::String("req-2".to_string());
        let id3 = MessageId::Number(999);

        let backend1 = translator.allocate(id1.clone()).unwrap();
        let backend2 = translator.allocate(id2.clone()).unwrap();
        let backend3 = translator.allocate(id3.clone()).unwrap();

        // Verify sequential backend IDs
        assert_eq!(backend1, MessageId::Number(1));
        assert_eq!(backend2, MessageId::Number(2));
        assert_eq!(backend3, MessageId::Number(3));

        // Verify reverse lookups
        assert_eq!(translator.get_frontend_id(&backend1), Some(id1.clone()));
        assert_eq!(translator.get_frontend_id(&backend2), Some(id2.clone()));
        assert_eq!(translator.get_frontend_id(&backend3), Some(id3.clone()));

        // Verify count
        assert_eq!(translator.mapping_count(), 3);

        // Clear all
        translator.clear();
        assert_eq!(translator.mapping_count(), 0);
    }

    #[test]
    fn test_sequential_backend_ids() {
        let translator = IdTranslator::new();

        // Allocate 10 IDs and verify they're sequential
        for i in 1..=10 {
            let frontend_id = MessageId::String(format!("req-{}", i));
            let backend_id = translator.allocate(frontend_id).unwrap();
            assert_eq!(backend_id, MessageId::Number(i as i64));
        }
    }

    #[test]
    fn test_max_mappings_limit() {
        // Create translator with small limit for testing
        let translator = IdTranslator::with_limits(5, Duration::from_secs(300));

        // Allocate up to limit
        for i in 1..=5 {
            let frontend_id = MessageId::String(format!("req-{}", i));
            let result = translator.allocate(frontend_id);
            assert!(result.is_ok(), "Should allocate within limit");
        }

        // Next allocation should fail
        let frontend_id = MessageId::String("req-overflow".to_string());
        let result = translator.allocate(frontend_id);
        assert!(result.is_err(), "Should fail when exceeding limit");

        // Verify it's a rate limit error
        match result {
            Err(ProxyError::RateLimitExceeded { .. }) => {}
            _ => panic!("Expected RateLimitExceeded error"),
        }
    }

    #[test]
    fn test_timeout_eviction() {
        // Create translator with very short timeout for testing
        let translator = IdTranslator::with_limits(10, Duration::from_millis(100));

        // Allocate some mappings
        let id1 = MessageId::String("req-1".to_string());
        let id2 = MessageId::String("req-2".to_string());

        translator.allocate(id1.clone()).unwrap();
        translator.allocate(id2.clone()).unwrap();

        assert_eq!(translator.mapping_count(), 2);

        // Wait for timeout
        thread::sleep(Duration::from_millis(150));

        // Manually trigger eviction
        translator.evict_expired();

        // Mappings should be gone
        assert_eq!(translator.mapping_count(), 0);
    }

    #[test]
    fn test_release_race_condition() {
        // Test that release() doesn't have TOCTOU issues
        let translator = Arc::new(IdTranslator::new());

        let frontend_id = MessageId::String("concurrent-test".to_string());
        let backend_id = translator.allocate(frontend_id.clone()).unwrap();

        // Verify mapping exists
        assert!(translator.get_frontend_id(&backend_id).is_some());

        // Spawn multiple threads trying to release simultaneously
        let handles: Vec<_> = (0..10)
            .map(|_| {
                let t = Arc::clone(&translator);
                let fid = frontend_id.clone();
                thread::spawn(move || {
                    t.release(&fid);
                })
            })
            .collect();

        // Wait for all threads
        for handle in handles {
            handle.join().unwrap();
        }

        // Mapping should be gone (no panic, no inconsistent state)
        assert!(translator.get_frontend_id(&backend_id).is_none());
        assert_eq!(translator.mapping_count(), 0);
    }

    #[test]
    fn test_eviction_after_limit_makes_room() {
        // Create translator with short timeout and small limit
        let translator = IdTranslator::with_limits(3, Duration::from_millis(100));

        // Fill to capacity
        for i in 1..=3 {
            let frontend_id = MessageId::String(format!("req-{}", i));
            translator.allocate(frontend_id).unwrap();
        }

        // Next should fail
        let result = translator.allocate(MessageId::String("overflow".to_string()));
        assert!(result.is_err());

        // Wait for timeout
        thread::sleep(Duration::from_millis(150));

        // Now allocation should succeed because eviction happens automatically
        let result = translator.allocate(MessageId::String("after-timeout".to_string()));
        assert!(
            result.is_ok(),
            "Should succeed after expired entries are evicted"
        );
    }
}
