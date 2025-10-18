//! Example plugin implementations
//!
//! Provides plugin implementations for common use cases:
//! - MetricsPlugin: Request/response metrics collection
//! - RetryPlugin: Automatic retry with exponential backoff
//! - CachePlugin: Response caching with TTL

use crate::plugins::core::{
    ClientPlugin, PluginConfig, PluginContext, PluginResult, RequestContext, ResponseContext,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

// ============================================================================
// METRICS PLUGIN
// ============================================================================

/// Request/response metrics data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsData {
    /// Total number of requests
    pub total_requests: u64,

    /// Total number of successful responses
    pub successful_responses: u64,

    /// Total number of failed responses
    pub failed_responses: u64,

    /// Average response time in milliseconds
    pub avg_response_time_ms: f64,

    /// Minimum response time in milliseconds
    pub min_response_time_ms: u64,

    /// Maximum response time in milliseconds
    pub max_response_time_ms: u64,

    /// Requests per minute (last minute)
    pub requests_per_minute: f64,

    /// Method-specific metrics
    pub method_metrics: HashMap<String, MethodMetrics>,

    /// Start time for metrics collection
    pub start_time: DateTime<Utc>,

    /// Last reset time
    pub last_reset: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MethodMetrics {
    pub count: u64,
    pub avg_duration_ms: f64,
    pub success_count: u64,
    pub error_count: u64,
}

impl Default for MetricsData {
    fn default() -> Self {
        let now = Utc::now();
        Self {
            total_requests: 0,
            successful_responses: 0,
            failed_responses: 0,
            avg_response_time_ms: 0.0,
            min_response_time_ms: u64::MAX,
            max_response_time_ms: 0,
            requests_per_minute: 0.0,
            method_metrics: HashMap::new(),
            start_time: now,
            last_reset: now,
        }
    }
}

/// Plugin for collecting request/response metrics
#[derive(Debug)]
pub struct MetricsPlugin {
    /// Thread-safe metrics storage
    metrics: Arc<Mutex<MetricsData>>,

    /// Request start times for duration calculation
    request_times: Arc<Mutex<HashMap<String, Instant>>>,

    /// Recent request timestamps for rate calculation
    recent_requests: Arc<Mutex<Vec<Instant>>>,
}

impl MetricsPlugin {
    /// Create a new metrics plugin
    #[must_use]
    pub fn new(_config: PluginConfig) -> Self {
        Self {
            metrics: Arc::new(Mutex::new(MetricsData::default())),
            request_times: Arc::new(Mutex::new(HashMap::new())),
            recent_requests: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Get current metrics data
    #[must_use]
    pub fn get_metrics(&self) -> MetricsData {
        self.metrics.lock().unwrap().clone()
    }

    /// Reset all metrics
    pub fn reset_metrics(&self) {
        let mut metrics = self.metrics.lock().unwrap();
        let now = Utc::now();
        *metrics = MetricsData {
            start_time: metrics.start_time,
            last_reset: now,
            ..MetricsData::default()
        };
        self.request_times.lock().unwrap().clear();
        self.recent_requests.lock().unwrap().clear();
    }

    fn update_request_rate(&self) {
        let mut recent = self.recent_requests.lock().unwrap();
        let now = Instant::now();

        // Remove requests older than 1 minute
        recent.retain(|&timestamp| now.duration_since(timestamp).as_secs() < 60);

        // Add current request
        recent.push(now);

        // Update rate
        let mut metrics = self.metrics.lock().unwrap();
        metrics.requests_per_minute = recent.len() as f64;
    }

    fn update_method_metrics(&self, method: &str, duration: Duration, is_success: bool) {
        let mut metrics = self.metrics.lock().unwrap();
        let entry = metrics
            .method_metrics
            .entry(method.to_string())
            .or_insert(MethodMetrics {
                count: 0,
                avg_duration_ms: 0.0,
                success_count: 0,
                error_count: 0,
            });

        entry.count += 1;
        if is_success {
            entry.success_count += 1;
        } else {
            entry.error_count += 1;
        }

        // Update running average
        let duration_ms = duration.as_millis() as f64;
        entry.avg_duration_ms =
            (entry.avg_duration_ms * (entry.count - 1) as f64 + duration_ms) / entry.count as f64;
    }
}

#[async_trait]
impl ClientPlugin for MetricsPlugin {
    fn name(&self) -> &str {
        "metrics"
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    fn description(&self) -> Option<&str> {
        Some("Collects request/response metrics and performance data")
    }

    async fn initialize(&self, context: &PluginContext) -> PluginResult<()> {
        info!(
            "Metrics plugin initialized for client: {}",
            context.client_name
        );
        Ok(())
    }

    async fn before_request(&self, context: &mut RequestContext) -> PluginResult<()> {
        let request_id = context.request.id.to_string();

        // Record request start time
        self.request_times
            .lock()
            .unwrap()
            .insert(request_id.clone(), Instant::now());

        // Update metrics
        {
            let mut metrics = self.metrics.lock().unwrap();
            metrics.total_requests += 1;
        }

        self.update_request_rate();

        // Add metrics metadata
        context.add_metadata("metrics.request_id".to_string(), json!(request_id));
        context.add_metadata(
            "metrics.start_time".to_string(),
            json!(Utc::now().to_rfc3339()),
        );

        debug!(
            "Metrics: Recorded request start for method: {}",
            context.method()
        );
        Ok(())
    }

    async fn after_response(&self, context: &mut ResponseContext) -> PluginResult<()> {
        let request_id = context.request_context.request.id.to_string();

        // Calculate duration
        let duration =
            if let Some(start_time) = self.request_times.lock().unwrap().remove(&request_id) {
                start_time.elapsed()
            } else {
                context.duration
            };

        let is_success = context.is_success();
        let method = context.method().to_string();
        let duration_ms = duration.as_millis();

        // Update metrics
        {
            let mut metrics = self.metrics.lock().unwrap();

            if is_success {
                metrics.successful_responses += 1;
            } else {
                metrics.failed_responses += 1;
            }

            let duration_ms_u64 = duration_ms as u64;

            // Update min/max
            if duration_ms_u64 < metrics.min_response_time_ms {
                metrics.min_response_time_ms = duration_ms_u64;
            }
            if duration_ms_u64 > metrics.max_response_time_ms {
                metrics.max_response_time_ms = duration_ms_u64;
            }

            // Update running average
            let total_responses = metrics.successful_responses + metrics.failed_responses;
            metrics.avg_response_time_ms =
                (metrics.avg_response_time_ms * (total_responses - 1) as f64 + duration_ms as f64)
                    / total_responses as f64;
        }

        // Update method-specific metrics
        self.update_method_metrics(&method, duration, is_success);

        // Add metrics metadata
        context.add_metadata("metrics.duration_ms".to_string(), json!(duration_ms));
        context.add_metadata("metrics.success".to_string(), json!(is_success));

        debug!(
            "Metrics: Recorded response for method: {} ({}ms, success: {})",
            method, duration_ms, is_success
        );

        Ok(())
    }

    async fn handle_custom(
        &self,
        method: &str,
        params: Option<Value>,
    ) -> PluginResult<Option<Value>> {
        match method {
            "metrics.get_stats" => {
                let metrics = self.get_metrics();
                Ok(Some(serde_json::to_value(metrics).unwrap()))
            }
            "metrics.reset" => {
                self.reset_metrics();
                info!("Metrics reset");
                Ok(Some(json!({"status": "reset"})))
            }
            "metrics.get_method_stats" => {
                if let Some(params) = params {
                    if let Some(method_name) = params.get("method").and_then(|v| v.as_str()) {
                        let metrics = self.metrics.lock().unwrap();
                        if let Some(method_metrics) = metrics.method_metrics.get(method_name) {
                            Ok(Some(serde_json::to_value(method_metrics).unwrap()))
                        } else {
                            Ok(Some(json!({"error": "Method not found"})))
                        }
                    } else {
                        Ok(Some(json!({"error": "Method parameter required"})))
                    }
                } else {
                    Ok(Some(json!({"error": "Parameters required"})))
                }
            }
            _ => Ok(None),
        }
    }
}

// ============================================================================
// RETRY PLUGIN
// ============================================================================

/// Configuration for retry behavior
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryConfig {
    /// Maximum number of retry attempts
    pub max_retries: u32,

    /// Base delay between retries in milliseconds
    pub base_delay_ms: u64,

    /// Maximum delay between retries in milliseconds
    pub max_delay_ms: u64,

    /// Multiplier for exponential backoff
    pub backoff_multiplier: f64,

    /// Whether to retry on timeout errors
    pub retry_on_timeout: bool,

    /// Whether to retry on connection errors
    pub retry_on_connection_error: bool,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            base_delay_ms: 100,
            max_delay_ms: 5000,
            backoff_multiplier: 2.0,
            retry_on_timeout: true,
            retry_on_connection_error: true,
        }
    }
}

/// Plugin for automatic retry with exponential backoff
#[derive(Debug)]
pub struct RetryPlugin {
    config: RetryConfig,
    retry_stats: Arc<Mutex<HashMap<String, u32>>>,
}

impl RetryPlugin {
    /// Create a new retry plugin
    #[must_use]
    pub fn new(config: PluginConfig) -> Self {
        let retry_config = match config {
            PluginConfig::Retry(config) => config,
            _ => RetryConfig::default(),
        };

        Self {
            config: retry_config,
            retry_stats: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn should_retry(&self, error: &turbomcp_protocol::Error) -> bool {
        let error_string = error.to_string().to_lowercase();

        if self.config.retry_on_connection_error
            && (error_string.contains("transport") || error_string.contains("connection"))
        {
            return true;
        }

        if self.config.retry_on_timeout && error_string.contains("timeout") {
            return true;
        }

        false
    }

    fn calculate_delay(&self, attempt: u32) -> Duration {
        let delay_ms = (self.config.base_delay_ms as f64
            * self.config.backoff_multiplier.powi(attempt as i32)) as u64;
        Duration::from_millis(delay_ms.min(self.config.max_delay_ms))
    }

    fn get_retry_count(&self, request_id: &str) -> u32 {
        self.retry_stats
            .lock()
            .unwrap()
            .get(request_id)
            .copied()
            .unwrap_or(0)
    }

    fn increment_retry_count(&self, request_id: &str) {
        let mut stats = self.retry_stats.lock().unwrap();
        let count = stats.entry(request_id.to_string()).or_insert(0);
        *count += 1;
    }

    fn clear_retry_count(&self, request_id: &str) {
        self.retry_stats.lock().unwrap().remove(request_id);
    }
}

#[async_trait]
impl ClientPlugin for RetryPlugin {
    fn name(&self) -> &str {
        "retry"
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    fn description(&self) -> Option<&str> {
        Some("Automatic retry with exponential backoff for failed requests")
    }

    async fn initialize(&self, context: &PluginContext) -> PluginResult<()> {
        info!(
            "Retry plugin initialized for client: {} (max_retries: {}, base_delay: {}ms)",
            context.client_name, self.config.max_retries, self.config.base_delay_ms
        );
        Ok(())
    }

    async fn before_request(&self, context: &mut RequestContext) -> PluginResult<()> {
        let request_id = context.request.id.to_string();
        let retry_count = self.get_retry_count(&request_id);

        // Add retry metadata
        context.add_metadata("retry.attempt".to_string(), json!(retry_count + 1));
        context.add_metadata(
            "retry.max_attempts".to_string(),
            json!(self.config.max_retries + 1),
        );

        if retry_count > 0 {
            debug!(
                "Retry: Attempt {} for request {} (method: {})",
                retry_count + 1,
                request_id,
                context.method()
            );
        }

        Ok(())
    }

    async fn after_response(&self, context: &mut ResponseContext) -> PluginResult<()> {
        let request_id = context.request_context.request.id.to_string();

        if context.is_success() {
            // Clear retry count on success
            self.clear_retry_count(&request_id);
            debug!("Retry: Request {} succeeded", request_id);
        } else if let Some(error) = &context.error {
            let retry_count = self.get_retry_count(&request_id);

            if self.should_retry(error) && retry_count < self.config.max_retries {
                // Increment retry count and schedule retry
                self.increment_retry_count(&request_id);

                let delay = self.calculate_delay(retry_count);
                warn!(
                    "Retry: Request {} failed (attempt {}), will retry after {:?}",
                    request_id,
                    retry_count + 1,
                    delay
                );

                // Add retry metadata
                context.add_metadata("retry.will_retry".to_string(), json!(true));
                context.add_metadata("retry.delay_ms".to_string(), json!(delay.as_millis()));
                context.add_metadata("retry.next_attempt".to_string(), json!(retry_count + 2));

                // Schedule retry by modifying the response to indicate retry needed
                // The client can check for retry metadata and handle accordingly
                context.add_metadata("retry.should_retry".to_string(), json!(true));
                context.add_metadata(
                    "retry.recommended_action".to_string(),
                    json!("retry_request"),
                );
            } else {
                // Max retries reached or error not retryable
                self.clear_retry_count(&request_id);
                if retry_count >= self.config.max_retries {
                    warn!("Retry: Request {} exhausted all retry attempts", request_id);
                } else {
                    debug!("Retry: Error not retryable for request {}", request_id);
                }
                context.add_metadata("retry.will_retry".to_string(), json!(false));
                context.add_metadata(
                    "retry.reason".to_string(),
                    json!(if retry_count >= self.config.max_retries {
                        "max_retries_reached"
                    } else {
                        "error_not_retryable"
                    }),
                );
            }
        }

        Ok(())
    }

    async fn handle_custom(
        &self,
        method: &str,
        _params: Option<Value>,
    ) -> PluginResult<Option<Value>> {
        match method {
            "retry.get_config" => Ok(Some(serde_json::to_value(&self.config).unwrap())),
            "retry.get_stats" => {
                let stats = self.retry_stats.lock().unwrap();
                Ok(Some(json!({
                    "active_retries": stats.len(),
                    "retry_counts": stats.clone()
                })))
            }
            "retry.clear_stats" => {
                self.retry_stats.lock().unwrap().clear();
                info!("Retry stats cleared");
                Ok(Some(json!({"status": "cleared"})))
            }
            _ => Ok(None),
        }
    }
}

// ============================================================================
// CACHE PLUGIN
// ============================================================================

/// Configuration for caching behavior
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    /// Maximum number of cached entries
    pub max_entries: usize,

    /// Time-to-live for cached entries in seconds
    pub ttl_seconds: u64,

    /// Whether to cache successful responses
    pub cache_responses: bool,

    /// Whether to cache resource content
    pub cache_resources: bool,

    /// Whether to cache tool results
    pub cache_tools: bool,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            max_entries: 1000,
            ttl_seconds: 300, // 5 minutes
            cache_responses: true,
            cache_resources: true,
            cache_tools: true,
        }
    }
}

#[derive(Debug, Clone)]
struct CacheEntry {
    data: Value,
    timestamp: Instant,
    access_count: u64,
}

impl CacheEntry {
    fn new(data: Value) -> Self {
        Self {
            data,
            timestamp: Instant::now(),
            access_count: 0,
        }
    }

    fn is_expired(&self, ttl: Duration) -> bool {
        self.timestamp.elapsed() > ttl
    }

    fn access(&mut self) -> &Value {
        self.access_count += 1;
        &self.data
    }
}

/// Plugin for caching responses with TTL
#[derive(Debug)]
pub struct CachePlugin {
    config: CacheConfig,
    cache: Arc<Mutex<HashMap<String, CacheEntry>>>,
    stats: Arc<Mutex<CacheStats>>,
}

#[derive(Debug, Default)]
struct CacheStats {
    hits: u64,
    misses: u64,
    evictions: u64,
    total_entries: u64,
}

impl CachePlugin {
    /// Create a new cache plugin
    #[must_use]
    pub fn new(config: PluginConfig) -> Self {
        let cache_config = match config {
            PluginConfig::Cache(config) => config,
            _ => CacheConfig::default(),
        };

        Self {
            config: cache_config,
            cache: Arc::new(Mutex::new(HashMap::new())),
            stats: Arc::new(Mutex::new(CacheStats::default())),
        }
    }

    fn should_cache_method(&self, method: &str) -> bool {
        match method {
            m if m.starts_with("tools/") && self.config.cache_tools => true,
            m if m.starts_with("resources/") && self.config.cache_resources => true,
            _ if self.config.cache_responses => true,
            _ => false,
        }
    }

    fn generate_cache_key(&self, context: &RequestContext) -> String {
        // Simple cache key based on method and parameters
        let params_hash = if let Some(params) = &context.request.params {
            format!("{:x}", fxhash::hash64(params))
        } else {
            "no_params".to_string()
        };
        format!("{}:{}", context.method(), params_hash)
    }

    fn get_cached(&self, key: &str) -> Option<Value> {
        let mut cache = self.cache.lock().unwrap();
        let ttl = Duration::from_secs(self.config.ttl_seconds);

        if let Some(entry) = cache.get_mut(key) {
            if !entry.is_expired(ttl) {
                let mut stats = self.stats.lock().unwrap();
                stats.hits += 1;
                return Some(entry.access().clone());
            } else {
                // Remove expired entry
                cache.remove(key);
                let mut stats = self.stats.lock().unwrap();
                stats.evictions += 1;
            }
        }

        let mut stats = self.stats.lock().unwrap();
        stats.misses += 1;
        None
    }

    fn store_cached(&self, key: String, data: Value) {
        let mut cache = self.cache.lock().unwrap();

        // Evict oldest entries if cache is full
        if cache.len() >= self.config.max_entries {
            // Simple LRU: remove oldest entries
            let evict_keys: Vec<_> = {
                let mut entries: Vec<_> = cache
                    .iter()
                    .map(|(k, v)| (k.clone(), v.timestamp))
                    .collect();
                entries.sort_by_key(|(_, timestamp)| *timestamp);

                let evict_count = (cache.len() - self.config.max_entries + 1).min(cache.len() / 2);
                entries
                    .into_iter()
                    .take(evict_count)
                    .map(|(key, _)| key)
                    .collect()
            };

            let evict_count = evict_keys.len();
            for key in evict_keys {
                cache.remove(&key);
            }

            let mut stats = self.stats.lock().unwrap();
            stats.evictions += evict_count as u64;
        }

        cache.insert(key, CacheEntry::new(data));
        let mut stats = self.stats.lock().unwrap();
        stats.total_entries += 1;
    }

    fn cleanup_expired(&self) {
        let mut cache = self.cache.lock().unwrap();
        let ttl = Duration::from_secs(self.config.ttl_seconds);

        let expired_keys: Vec<_> = cache
            .iter()
            .filter(|(_, entry)| entry.is_expired(ttl))
            .map(|(key, _)| key.clone())
            .collect();

        let eviction_count = expired_keys.len();
        for key in expired_keys {
            cache.remove(&key);
        }

        if eviction_count > 0 {
            let mut stats = self.stats.lock().unwrap();
            stats.evictions += eviction_count as u64;
            debug!("Cache: Cleaned up {} expired entries", eviction_count);
        }
    }
}

#[async_trait]
impl ClientPlugin for CachePlugin {
    fn name(&self) -> &str {
        "cache"
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    fn description(&self) -> Option<&str> {
        Some("Response caching with TTL and LRU eviction")
    }

    async fn initialize(&self, context: &PluginContext) -> PluginResult<()> {
        info!(
            "Cache plugin initialized for client: {} (max_entries: {}, ttl: {}s)",
            context.client_name, self.config.max_entries, self.config.ttl_seconds
        );
        Ok(())
    }

    async fn before_request(&self, context: &mut RequestContext) -> PluginResult<()> {
        if !self.should_cache_method(context.method()) {
            return Ok(());
        }

        let cache_key = self.generate_cache_key(context);

        if let Some(cached_data) = self.get_cached(&cache_key) {
            debug!(
                "Cache: Hit for method {} (key: {})",
                context.method(),
                cache_key
            );
            context.add_metadata("cache.hit".to_string(), json!(true));
            context.add_metadata("cache.key".to_string(), json!(cache_key));
            context.add_metadata("cache.response_source".to_string(), json!("cache"));
            // Store cached response for retrieval after protocol call is skipped
            context.add_metadata("cache.response_data".to_string(), cached_data.clone());
            context.add_metadata("cache.should_skip_request".to_string(), json!(true));
        } else {
            debug!(
                "Cache: Miss for method {} (key: {})",
                context.method(),
                cache_key
            );
            context.add_metadata("cache.hit".to_string(), json!(false));
            context.add_metadata("cache.key".to_string(), json!(cache_key));
            context.add_metadata("cache.should_skip_request".to_string(), json!(false));
        }

        Ok(())
    }

    async fn after_response(&self, context: &mut ResponseContext) -> PluginResult<()> {
        // Handle cache hits - if we have cached response data, use it
        if let Some(cached_response_data) =
            context.request_context.get_metadata("cache.response_data")
        {
            context.response = Some(cached_response_data.clone());
            debug!(
                "Cache: Used cached response for method {}",
                context.method()
            );
            return Ok(());
        }

        if !self.should_cache_method(context.method()) || !context.is_success() {
            return Ok(());
        }

        if let Some(cache_key) = context
            .request_context
            .get_metadata("cache.key")
            .and_then(|v| v.as_str())
            && let Some(response_data) = &context.response
        {
            self.store_cached(cache_key.to_string(), response_data.clone());
            debug!(
                "Cache: Stored response for method {} (key: {})",
                context.method(),
                cache_key
            );
            context.add_metadata("cache.stored".to_string(), json!(true));
        }

        // Periodic cleanup of expired entries
        self.cleanup_expired();

        Ok(())
    }

    async fn handle_custom(
        &self,
        method: &str,
        params: Option<Value>,
    ) -> PluginResult<Option<Value>> {
        match method {
            "cache.get_stats" => {
                let stats = self.stats.lock().unwrap();
                let cache_size = self.cache.lock().unwrap().len();

                Ok(Some(json!({
                    "hits": stats.hits,
                    "misses": stats.misses,
                    "evictions": stats.evictions,
                    "total_entries": stats.total_entries,
                    "current_size": cache_size,
                    "hit_rate": if stats.hits + stats.misses > 0 {
                        stats.hits as f64 / (stats.hits + stats.misses) as f64
                    } else {
                        0.0
                    }
                })))
            }
            "cache.clear" => {
                let mut cache = self.cache.lock().unwrap();
                let cleared_count = cache.len();
                cache.clear();
                info!("Cache: Cleared {} entries", cleared_count);
                Ok(Some(json!({"cleared_entries": cleared_count})))
            }
            "cache.get_config" => Ok(Some(serde_json::to_value(&self.config).unwrap())),
            "cache.cleanup" => {
                self.cleanup_expired();
                let cache_size = self.cache.lock().unwrap().len();
                Ok(Some(json!({"remaining_entries": cache_size})))
            }
            "cache.get" => {
                if let Some(params) = params {
                    if let Some(key) = params.get("key").and_then(|v| v.as_str()) {
                        if let Some(data) = self.get_cached(key) {
                            Ok(Some(json!({"found": true, "data": data})))
                        } else {
                            Ok(Some(json!({"found": false})))
                        }
                    } else {
                        Ok(Some(json!({"error": "Key parameter required"})))
                    }
                } else {
                    Ok(Some(json!({"error": "Parameters required"})))
                }
            }
            _ => Ok(None),
        }
    }
}

// Helper function for hashing (using a fast hash function)
mod fxhash {
    use serde_json::Value;
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    pub fn hash64(value: &Value) -> u64 {
        let mut hasher = DefaultHasher::new();

        // Simple hash of JSON string representation
        let json_str = value.to_string();
        json_str.hash(&mut hasher);

        hasher.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_metrics_plugin_creation() {
        let plugin = MetricsPlugin::new(PluginConfig::Metrics);
        assert_eq!(plugin.name(), "metrics");

        let metrics = plugin.get_metrics();
        assert_eq!(metrics.total_requests, 0);
        assert_eq!(metrics.successful_responses, 0);
    }

    #[tokio::test]
    async fn test_retry_plugin_creation() {
        let config = RetryConfig {
            max_retries: 5,
            base_delay_ms: 200,
            max_delay_ms: 2000,
            backoff_multiplier: 1.5,
            retry_on_timeout: true,
            retry_on_connection_error: false,
        };

        let plugin = RetryPlugin::new(PluginConfig::Retry(config.clone()));
        assert_eq!(plugin.name(), "retry");
        assert_eq!(plugin.config.max_retries, 5);
        assert_eq!(plugin.config.base_delay_ms, 200);
    }

    #[tokio::test]
    async fn test_cache_plugin_creation() {
        let config = CacheConfig {
            max_entries: 500,
            ttl_seconds: 600,
            cache_responses: true,
            cache_resources: false,
            cache_tools: true,
        };

        let plugin = CachePlugin::new(PluginConfig::Cache(config.clone()));
        assert_eq!(plugin.name(), "cache");
        assert_eq!(plugin.config.max_entries, 500);
        assert_eq!(plugin.config.ttl_seconds, 600);
    }

    #[test]
    fn test_retry_delay_calculation() {
        let config = RetryConfig {
            max_retries: 3,
            base_delay_ms: 100,
            max_delay_ms: 1000,
            backoff_multiplier: 2.0,
            retry_on_timeout: true,
            retry_on_connection_error: true,
        };

        let plugin = RetryPlugin::new(PluginConfig::Retry(config));

        assert_eq!(plugin.calculate_delay(0), Duration::from_millis(100));
        assert_eq!(plugin.calculate_delay(1), Duration::from_millis(200));
        assert_eq!(plugin.calculate_delay(2), Duration::from_millis(400));
        assert_eq!(plugin.calculate_delay(3), Duration::from_millis(800));
        assert_eq!(plugin.calculate_delay(4), Duration::from_millis(1000)); // Capped at max
    }

    #[test]
    fn test_cache_entry_expiration() {
        let entry = CacheEntry::new(json!({"test": "data"}));
        assert!(!entry.is_expired(Duration::from_secs(1)));

        // Can't easily test actual expiration without sleeping or mocking time
    }
}
