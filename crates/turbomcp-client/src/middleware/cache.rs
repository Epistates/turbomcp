//! Response caching middleware for MCP client.
//!
//! Tower Layer that caches successful responses with configurable TTL
//! and LRU eviction. Supports method-based caching policies.
//!
//! ## Usage
//!
//! ```rust,ignore
//! use turbomcp_client::middleware::{CacheLayer, CacheConfig};
//! use tower::ServiceBuilder;
//!
//! let service = ServiceBuilder::new()
//!     .layer(CacheLayer::new(CacheConfig {
//!         max_entries: 1000,
//!         ttl: Duration::from_secs(300),
//!         ..Default::default()
//!     }))
//!     .service(inner_service);
//! ```

use super::request::{McpRequest, McpResponse};
use futures_util::future::BoxFuture;
use parking_lot::RwLock;
use serde_json::Value;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};
use tower_layer::Layer;
use tower_service::Service;
use turbomcp_protocol::McpError;

/// Cache configuration.
#[derive(Debug, Clone)]
pub struct CacheConfig {
    /// Maximum number of cached entries
    pub max_entries: usize,
    /// Time-to-live for cached entries
    pub ttl: Duration,
    /// Methods to cache (empty = cache all cacheable methods)
    pub cache_methods: Vec<String>,
    /// Methods to never cache
    pub exclude_methods: Vec<String>,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            max_entries: 1000,
            ttl: Duration::from_secs(300), // 5 minutes
            cache_methods: Vec::new(),
            exclude_methods: vec![
                // Mutations should not be cached
                "tools/call".to_string(),
                "sampling/createMessage".to_string(),
                // Notifications
                "notifications/".to_string(),
            ],
        }
    }
}

impl CacheConfig {
    /// Check if a method should be cached.
    fn should_cache(&self, method: &str) -> bool {
        // Check exclusions first
        for excluded in &self.exclude_methods {
            if method.starts_with(excluded) || method == excluded {
                return false;
            }
        }

        // If specific methods are configured, check membership
        if !self.cache_methods.is_empty() {
            return self.cache_methods.iter().any(|m| method.starts_with(m) || method == m);
        }

        // Default: cache read-like operations
        method.starts_with("resources/")
            || method.starts_with("prompts/")
            || method == "tools/list"
            || method == "resources/list"
            || method == "prompts/list"
    }
}

/// Cache entry with metadata.
#[derive(Debug, Clone)]
struct CacheEntry {
    data: Value,
    created: Instant,
    last_accessed: Instant,
    access_count: u64,
}

impl CacheEntry {
    fn new(data: Value) -> Self {
        let now = Instant::now();
        Self {
            data,
            created: now,
            last_accessed: now,
            access_count: 0,
        }
    }

    fn is_expired(&self, ttl: Duration) -> bool {
        self.created.elapsed() > ttl
    }

    fn access(&mut self) -> &Value {
        self.last_accessed = Instant::now();
        self.access_count += 1;
        &self.data
    }
}

/// Cache statistics.
#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    /// Cache hits
    pub hits: u64,
    /// Cache misses
    pub misses: u64,
    /// Entries evicted due to size limit
    pub evictions: u64,
    /// Entries expired
    pub expirations: u64,
    /// Current entry count
    pub current_entries: usize,
}

/// Thread-safe response cache.
#[derive(Debug)]
pub struct Cache {
    config: CacheConfig,
    entries: RwLock<HashMap<String, CacheEntry>>,
    hits: AtomicU64,
    misses: AtomicU64,
    evictions: AtomicU64,
    expirations: AtomicU64,
}

impl Cache {
    /// Create a new cache with the given configuration.
    #[must_use]
    pub fn new(config: CacheConfig) -> Self {
        Self {
            config,
            entries: RwLock::new(HashMap::new()),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            evictions: AtomicU64::new(0),
            expirations: AtomicU64::new(0),
        }
    }

    /// Generate a cache key from request.
    fn cache_key(req: &McpRequest) -> String {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();

        req.method().hash(&mut hasher);
        if let Some(params) = req.params() {
            params.to_string().hash(&mut hasher);
        }

        format!("{}:{:x}", req.method(), hasher.finish())
    }

    /// Check if method should be cached.
    pub fn should_cache(&self, method: &str) -> bool {
        self.config.should_cache(method)
    }

    /// Get a cached value.
    pub fn get(&self, key: &str) -> Option<Value> {
        let mut entries = self.entries.write();

        if let Some(entry) = entries.get_mut(key) {
            if entry.is_expired(self.config.ttl) {
                entries.remove(key);
                self.expirations.fetch_add(1, Ordering::Relaxed);
                self.misses.fetch_add(1, Ordering::Relaxed);
                return None;
            }

            self.hits.fetch_add(1, Ordering::Relaxed);
            return Some(entry.access().clone());
        }

        self.misses.fetch_add(1, Ordering::Relaxed);
        None
    }

    /// Store a value in the cache.
    pub fn put(&self, key: String, value: Value) {
        let mut entries = self.entries.write();

        // Evict if at capacity
        if entries.len() >= self.config.max_entries {
            self.evict_lru(&mut entries);
        }

        entries.insert(key, CacheEntry::new(value));
    }

    /// Evict least recently used entries.
    fn evict_lru(&self, entries: &mut HashMap<String, CacheEntry>) {
        // Find the oldest entries
        let mut to_evict: Vec<_> = entries
            .iter()
            .map(|(k, v)| (k.clone(), v.last_accessed))
            .collect();

        to_evict.sort_by_key(|(_, accessed)| *accessed);

        // Evict 10% of entries or at least 1
        let evict_count = (entries.len() / 10).max(1);
        for (key, _) in to_evict.into_iter().take(evict_count) {
            entries.remove(&key);
            self.evictions.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Get cache statistics.
    #[must_use]
    pub fn stats(&self) -> CacheStats {
        CacheStats {
            hits: self.hits.load(Ordering::Relaxed),
            misses: self.misses.load(Ordering::Relaxed),
            evictions: self.evictions.load(Ordering::Relaxed),
            expirations: self.expirations.load(Ordering::Relaxed),
            current_entries: self.entries.read().len(),
        }
    }

    /// Clear all cached entries.
    pub fn clear(&self) {
        self.entries.write().clear();
    }

    /// Remove expired entries.
    pub fn cleanup(&self) {
        let mut entries = self.entries.write();
        let ttl = self.config.ttl;

        let expired: Vec<_> = entries
            .iter()
            .filter(|(_, e)| e.is_expired(ttl))
            .map(|(k, _)| k.clone())
            .collect();

        for key in expired {
            entries.remove(&key);
            self.expirations.fetch_add(1, Ordering::Relaxed);
        }
    }
}

impl Default for Cache {
    fn default() -> Self {
        Self::new(CacheConfig::default())
    }
}

/// Tower Layer that adds response caching.
#[derive(Debug, Clone)]
pub struct CacheLayer {
    cache: Arc<Cache>,
}

impl CacheLayer {
    /// Create a new cache layer with the given configuration.
    #[must_use]
    pub fn new(config: CacheConfig) -> Self {
        Self {
            cache: Arc::new(Cache::new(config)),
        }
    }

    /// Create a new cache layer with a shared cache.
    #[must_use]
    pub fn with_cache(cache: Arc<Cache>) -> Self {
        Self { cache }
    }

    /// Get a reference to the cache.
    #[must_use]
    pub fn cache(&self) -> &Arc<Cache> {
        &self.cache
    }
}

impl Default for CacheLayer {
    fn default() -> Self {
        Self::new(CacheConfig::default())
    }
}

impl<S> Layer<S> for CacheLayer {
    type Service = CacheService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        CacheService {
            inner,
            cache: Arc::clone(&self.cache),
        }
    }
}

/// Tower Service that caches responses.
#[derive(Debug, Clone)]
pub struct CacheService<S> {
    inner: S,
    cache: Arc<Cache>,
}

impl<S> CacheService<S> {
    /// Get a reference to the inner service.
    pub fn inner(&self) -> &S {
        &self.inner
    }

    /// Get a mutable reference to the inner service.
    pub fn inner_mut(&mut self) -> &mut S {
        &mut self.inner
    }

    /// Get a reference to the cache.
    pub fn cache(&self) -> &Arc<Cache> {
        &self.cache
    }
}

impl<S> Service<McpRequest> for CacheService<S>
where
    S: Service<McpRequest, Response = McpResponse> + Clone + Send + 'static,
    S::Future: Send,
    S::Error: Into<McpError>,
{
    type Response = McpResponse;
    type Error = McpError;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx).map_err(Into::into)
    }

    fn call(&mut self, req: McpRequest) -> Self::Future {
        let method = req.method().to_string();
        let cache = Arc::clone(&self.cache);

        // Check if this method should be cached
        if !cache.should_cache(&method) {
            let mut inner = self.inner.clone();
            std::mem::swap(&mut self.inner, &mut inner);
            return Box::pin(async move { inner.call(req).await.map_err(Into::into) });
        }

        let cache_key = Cache::cache_key(&req);

        // Check cache
        if let Some(cached_value) = cache.get(&cache_key) {
            return Box::pin(async move {
                Ok(McpResponse {
                    result: Some(cached_value),
                    error: None,
                    metadata: {
                        let mut m = HashMap::new();
                        m.insert("cache.hit".to_string(), serde_json::json!(true));
                        m
                    },
                    duration: Duration::ZERO,
                })
            });
        }

        // Cache miss - call inner service
        let mut inner = self.inner.clone();
        std::mem::swap(&mut self.inner, &mut inner);

        Box::pin(async move {
            let start = Instant::now();
            let result = inner.call(req).await.map_err(Into::into)?;

            // Cache successful responses
            if result.is_success()
                && let Some(ref data) = result.result
            {
                cache.put(cache_key, data.clone());
            }

            let mut response = result;
            response.insert_metadata("cache.hit", serde_json::json!(false));
            response.duration = start.elapsed();

            Ok(response)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use turbomcp_protocol::MessageId;
    use turbomcp_protocol::jsonrpc::{JsonRpcRequest, JsonRpcVersion};

    fn test_request(method: &str) -> McpRequest {
        McpRequest::new(JsonRpcRequest {
            jsonrpc: JsonRpcVersion,
            id: MessageId::from("test-1"),
            method: method.to_string(),
            params: Some(json!({"key": "value"})),
        })
    }

    #[test]
    fn test_cache_config_defaults() {
        let config = CacheConfig::default();

        // Should cache read operations
        assert!(config.should_cache("resources/list"));
        assert!(config.should_cache("resources/read"));
        assert!(config.should_cache("prompts/list"));
        assert!(config.should_cache("tools/list"));

        // Should not cache mutations
        assert!(!config.should_cache("tools/call"));
        assert!(!config.should_cache("sampling/createMessage"));
    }

    #[test]
    fn test_cache_put_get() {
        let cache = Cache::default();

        let key = "test:123".to_string();
        let value = json!({"result": "test"});

        cache.put(key.clone(), value.clone());

        let retrieved = cache.get(&key);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap(), value);
    }

    #[test]
    fn test_cache_miss() {
        let cache = Cache::default();

        let retrieved = cache.get("nonexistent");
        assert!(retrieved.is_none());

        let stats = cache.stats();
        assert_eq!(stats.misses, 1);
        assert_eq!(stats.hits, 0);
    }

    #[test]
    fn test_cache_expiration() {
        let config = CacheConfig {
            ttl: Duration::from_millis(1),
            ..Default::default()
        };
        let cache = Cache::new(config);

        let key = "test:456".to_string();
        cache.put(key.clone(), json!({"data": "test"}));

        // Wait for expiration
        std::thread::sleep(Duration::from_millis(5));

        let retrieved = cache.get(&key);
        assert!(retrieved.is_none());

        let stats = cache.stats();
        assert_eq!(stats.expirations, 1);
    }

    #[test]
    fn test_cache_eviction() {
        let config = CacheConfig {
            max_entries: 2,
            ttl: Duration::from_secs(300),
            ..Default::default()
        };
        let cache = Cache::new(config);

        cache.put("key1".to_string(), json!(1));
        cache.put("key2".to_string(), json!(2));
        cache.put("key3".to_string(), json!(3)); // Should trigger eviction

        let stats = cache.stats();
        assert!(stats.evictions > 0);
        assert!(stats.current_entries <= 2);
    }

    #[test]
    fn test_cache_key_generation() {
        let req1 = test_request("resources/read");
        let req2 = test_request("resources/read");
        let req3 = test_request("resources/list");

        // Same method + params should have same key
        assert_eq!(Cache::cache_key(&req1), Cache::cache_key(&req2));

        // Different method should have different key
        assert_ne!(Cache::cache_key(&req1), Cache::cache_key(&req3));
    }

    #[tokio::test]
    async fn test_cache_service() {
        use tower::ServiceExt;

        let cache = Arc::new(Cache::default());
        let call_count = Arc::new(AtomicU64::new(0));
        let call_count_clone = Arc::clone(&call_count);

        let mock_service = tower::service_fn(move |_req: McpRequest| {
            let count = Arc::clone(&call_count_clone);
            async move {
                count.fetch_add(1, Ordering::Relaxed);
                Ok::<_, McpError>(McpResponse::success(
                    json!({"result": "data"}),
                    Duration::from_millis(10),
                ))
            }
        });

        let mut service = CacheLayer::with_cache(Arc::clone(&cache)).layer(mock_service);

        let request = test_request("resources/list");

        // First call - cache miss
        let response = service.ready().await.unwrap().call(request.clone()).await.unwrap();
        assert!(response.is_success());
        assert_eq!(call_count.load(Ordering::Relaxed), 1);

        // Second call - cache hit
        let mut service = CacheLayer::with_cache(Arc::clone(&cache)).layer(tower::service_fn(
            |_req: McpRequest| async {
                panic!("Inner service should not be called on cache hit");
                #[allow(unreachable_code)]
                Ok::<_, McpError>(McpResponse::success(json!({}), Duration::ZERO))
            },
        ));

        let response = service.ready().await.unwrap().call(request).await.unwrap();
        assert!(response.is_success());
        assert_eq!(response.get_metadata("cache.hit"), Some(&json!(true)));
    }
}
