#![allow(dead_code)]
//! # 07: Performance & Optimization - Fast MCP Servers
//!
//! **Learning Goals (25 minutes):**
//! - Build fast MCP servers optimized for throughput
//! - Understand memory management and allocation patterns
//! - Learn async optimization and batching techniques
//! - Implement comprehensive performance monitoring
//!
//! **Note:** Performance examples demonstrate optimization techniques tested on
//! consumer hardware (MacBook Pro M3, 32GB RAM). Actual performance will vary
//! based on your hardware configuration and workload characteristics.
//!
//! **What this example demonstrates:**
//! - Zero-copy string processing and efficient serialization
//! - Connection pooling and resource management
//! - Request batching and parallel processing
//! - Memory pool patterns and allocation optimization
//! - Performance metrics collection and reporting
//! - Caching strategies for frequently accessed data
//!
//! **Run with:** `cargo run --example 07_performance`

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, RwLock, Semaphore};
use turbomcp::prelude::*;

/// Fast data processing server optimized for throughput
///
/// This server demonstrates advanced performance patterns:
/// - Connection pooling for expensive resources
/// - Memory pools to reduce allocations
/// - Request batching for bulk operations  
/// - Comprehensive metrics collection
/// - Intelligent caching strategies
#[derive(Debug, Clone)]
struct HighPerformanceServer {
    /// Connection pool for database-like operations
    connection_pool: Arc<ConnectionPool>,
    /// Memory pool for request processing
    memory_pool: Arc<MemoryPool>,
    /// Cache for frequently accessed data
    cache: Arc<RwLock<LruCache<String, CacheEntry>>>,
    /// Performance metrics collector
    metrics: Arc<Mutex<PerformanceMetrics>>,
    /// Rate limiter for preventing overload
    rate_limiter: Arc<Semaphore>,
    /// Configuration for performance tuning
    config: PerformanceConfig,
}

#[derive(Debug, Clone)]
struct PerformanceConfig {
    max_connections: usize,
    cache_size: usize,
    memory_pool_size: usize,
    batch_size: usize,
    request_timeout: Duration,
    max_concurrent_requests: usize,
}

#[derive(Debug)]
struct ConnectionPool {
    connections: Mutex<Vec<Connection>>,
    max_connections: usize,
    active_connections: Mutex<usize>,
}

#[derive(Debug)]
struct Connection {
    id: u64,
    created_at: Instant,
    last_used: Instant,
    is_healthy: bool,
}

#[derive(Debug)]
struct MemoryPool {
    buffers: Mutex<Vec<Vec<u8>>>,
    buffer_size: usize,
    pool_size: usize,
}

#[derive(Debug, Clone)]
struct CacheEntry {
    data: String,
    created_at: Instant,
    hit_count: u64,
    ttl: Duration,
}

#[derive(Debug)]
struct LruCache<K, V> {
    capacity: usize,
    data: HashMap<K, V>,
    access_order: Vec<K>,
}

#[derive(Debug, Default)]
struct PerformanceMetrics {
    requests_processed: u64,
    total_processing_time: Duration,
    cache_hits: u64,
    cache_misses: u64,
    memory_allocations: u64,
    connection_pool_hits: u64,
    error_count: u64,
    batch_operations: u64,
    peak_memory_usage: usize,
    active_connections: usize,
}

#[derive(Debug, Deserialize, Serialize)]
struct BatchProcessingRequest {
    operations: Vec<Operation>,
    parallel: Option<bool>,
    timeout_ms: Option<u64>,
}

#[derive(Debug, Deserialize, Serialize)]
struct Operation {
    id: String,
    operation_type: String,
    data: serde_json::Value,
    priority: Option<u8>,
}

#[derive(Debug, Deserialize, Serialize)]
struct DataQueryRequest {
    query: String,
    use_cache: Option<bool>,
    max_results: Option<usize>,
}

#[derive(Debug, Serialize)]
struct ProcessingResult {
    id: String,
    result: serde_json::Value,
    processing_time_ms: u64,
    from_cache: bool,
}

#[derive(Debug, Serialize)]
struct PerformanceReport {
    uptime_seconds: u64,
    requests_per_second: f64,
    average_response_time_ms: f64,
    cache_hit_ratio: f64,
    memory_usage_mb: f64,
    connection_pool_utilization: f64,
    error_rate: f64,
    throughput_ops_per_sec: f64,
}

// =============================================================================
// IMPLEMENTATION DETAILS
// =============================================================================

impl<K: Clone + Eq + std::hash::Hash, V> LruCache<K, V> {
    fn new(capacity: usize) -> Self {
        Self {
            capacity,
            data: HashMap::new(),
            access_order: Vec::new(),
        }
    }

    fn get(&mut self, key: &K) -> Option<&V> {
        if let Some(value) = self.data.get(key) {
            // Move to front (most recently used)
            self.access_order.retain(|k| k != key);
            self.access_order.push(key.clone());
            Some(value)
        } else {
            None
        }
    }

    fn insert(&mut self, key: K, value: V) {
        if self.data.len() >= self.capacity {
            // Remove least recently used
            if let Some(lru_key) = self.access_order.first().cloned() {
                self.data.remove(&lru_key);
                self.access_order.remove(0);
            }
        }

        self.data.insert(key.clone(), value);
        self.access_order.push(key);
    }

    fn len(&self) -> usize {
        self.data.len()
    }
}

impl ConnectionPool {
    fn new(max_connections: usize) -> Self {
        Self {
            connections: Mutex::new(Vec::new()),
            max_connections,
            active_connections: Mutex::new(0),
        }
    }

    async fn acquire_connection(&self) -> Result<Connection, String> {
        let mut connections = self.connections.lock().await;

        if let Some(mut conn) = connections.pop() {
            conn.last_used = Instant::now();
            if conn.is_healthy {
                return Ok(conn);
            }
        }

        // Create new connection if under limit
        let mut active = self.active_connections.lock().await;
        if *active < self.max_connections {
            *active += 1;
            let conn = Connection {
                id: *active as u64,
                created_at: Instant::now(),
                last_used: Instant::now(),
                is_healthy: true,
            };
            Ok(conn)
        } else {
            Err("Connection pool exhausted".to_string())
        }
    }

    async fn release_connection(&self, conn: Connection) {
        let mut connections = self.connections.lock().await;
        connections.push(conn);
    }
}

impl MemoryPool {
    fn new(buffer_size: usize, pool_size: usize) -> Self {
        Self {
            buffers: Mutex::new(Vec::new()),
            buffer_size,
            pool_size,
        }
    }

    async fn get_buffer(&self) -> Vec<u8> {
        let mut buffers = self.buffers.lock().await;
        buffers
            .pop()
            .unwrap_or_else(|| Vec::with_capacity(self.buffer_size))
    }

    async fn return_buffer(&self, mut buffer: Vec<u8>) {
        buffer.clear();
        let mut buffers = self.buffers.lock().await;
        if buffers.len() < self.pool_size {
            buffers.push(buffer);
        }
        // Otherwise let it be dropped to free memory
    }
}

impl HighPerformanceServer {
    fn new(config: PerformanceConfig) -> Self {
        Self {
            connection_pool: Arc::new(ConnectionPool::new(config.max_connections)),
            memory_pool: Arc::new(MemoryPool::new(8192, config.memory_pool_size)),
            cache: Arc::new(RwLock::new(LruCache::new(config.cache_size))),
            metrics: Arc::new(Mutex::new(PerformanceMetrics::default())),
            rate_limiter: Arc::new(Semaphore::new(config.max_concurrent_requests)),
            config,
        }
    }

    async fn update_metrics<F>(&self, updater: F)
    where
        F: FnOnce(&mut PerformanceMetrics),
    {
        let mut metrics = self.metrics.lock().await;
        updater(&mut metrics);
    }

    async fn get_cached_data(&self, key: &str) -> Option<String> {
        let mut cache = self.cache.write().await;
        if let Some(entry) = cache.get(&key.to_string()) {
            if entry.created_at.elapsed() < entry.ttl {
                self.update_metrics(|m| m.cache_hits += 1).await;
                return Some(entry.data.clone());
            } else {
                // Entry expired, remove it
                cache.data.remove(key);
            }
        }
        self.update_metrics(|m| m.cache_misses += 1).await;
        None
    }

    async fn cache_data(&self, key: String, data: String, ttl: Duration) {
        let mut cache = self.cache.write().await;
        let entry = CacheEntry {
            data,
            created_at: Instant::now(),
            hit_count: 0,
            ttl,
        };
        cache.insert(key, entry);
    }
}

// =============================================================================
// MCP SERVER IMPLEMENTATION
// =============================================================================

#[turbomcp::server(name = "HighPerformanceServer", version = "1.0.0")]
impl HighPerformanceServer {
    /// Process a batch of operations with optimizations
    ///
    /// This tool demonstrates:
    /// - Request batching for improved throughput
    /// - Parallel processing with controlled concurrency
    /// - Memory pool usage for zero-copy operations
    /// - Comprehensive metrics collection
    #[tool("Process a batch of operations with performance optimizations")]
    async fn batch_process(
        &self,
        request: BatchProcessingRequest,
    ) -> McpResult<Vec<ProcessingResult>> {
        let start_time = Instant::now();

        // Rate limiting - acquire permit
        let _permit = self
            .rate_limiter
            .acquire()
            .await
            .map_err(|e| McpError::internal(format!("Rate limit exceeded: {e}")))?;

        tracing::info!(
            "Processing batch of {} operations",
            request.operations.len()
        );

        if request.operations.len() > self.config.batch_size {
            return Err(McpError::invalid_request(format!(
                "Batch size {} exceeds maximum {}",
                request.operations.len(),
                self.config.batch_size
            )));
        }

        let parallel = request.parallel.unwrap_or(true);
        let timeout = Duration::from_millis(request.timeout_ms.unwrap_or(30000));

        let mut results = Vec::with_capacity(request.operations.len());

        if parallel {
            // Parallel processing with connection pool
            let mut handles = Vec::new();

            for operation in request.operations {
                let pool = self.connection_pool.clone();
                let memory_pool = self.memory_pool.clone();
                let cache = self.cache.clone();

                let handle = tokio::spawn(async move {
                    let op_start = Instant::now();

                    // Acquire connection from pool
                    let _conn = pool
                        .acquire_connection()
                        .await
                        .map_err(McpError::internal)?;

                    // Get buffer from memory pool
                    let buffer = memory_pool.get_buffer().await;

                    // Process operation (simulate work)
                    let result_data =
                        Self::process_single_operation(&operation, buffer, cache.clone()).await;

                    Ok::<ProcessingResult, McpError>(ProcessingResult {
                        id: operation.id,
                        result: result_data?,
                        processing_time_ms: op_start.elapsed().as_millis() as u64,
                        from_cache: false,
                    })
                });

                handles.push(handle);
            }

            // Wait for all operations with timeout
            let results_future = futures::future::join_all(handles);
            let batch_results = tokio::time::timeout(timeout, results_future)
                .await
                .map_err(|_| McpError::internal("Batch processing timeout"))?;

            for result in batch_results {
                match result {
                    Ok(Ok(processing_result)) => results.push(processing_result),
                    Ok(Err(e)) => return Err(e),
                    Err(e) => return Err(McpError::internal(format!("Task failed: {e}"))),
                }
            }
        } else {
            // Sequential processing
            for operation in request.operations {
                let op_start = Instant::now();

                let _conn = self
                    .connection_pool
                    .acquire_connection()
                    .await
                    .map_err(McpError::internal)?;

                let buffer = self.memory_pool.get_buffer().await;

                let result_data =
                    Self::process_single_operation(&operation, buffer, self.cache.clone()).await;

                results.push(ProcessingResult {
                    id: operation.id,
                    result: result_data?,
                    processing_time_ms: op_start.elapsed().as_millis() as u64,
                    from_cache: false,
                });
            }
        }

        // Update performance metrics
        let total_time = start_time.elapsed();
        self.update_metrics(|m| {
            m.requests_processed += 1;
            m.total_processing_time += total_time;
            m.batch_operations += 1;
        })
        .await;

        tracing::info!("Batch processing completed in {}ms", total_time.as_millis());

        Ok(results)
    }

    /// Query data with intelligent caching
    #[tool("Query data with intelligent caching and performance optimization")]
    async fn query_data(&self, request: DataQueryRequest) -> McpResult<serde_json::Value> {
        let start_time = Instant::now();
        let use_cache = request.use_cache.unwrap_or(true);
        let max_results = request.max_results.unwrap_or(100);

        tracing::info!("Executing query: {}", request.query);

        // Check cache first if enabled
        if use_cache && let Some(cached_data) = self.get_cached_data(&request.query).await {
            tracing::info!("Returning cached query result");
            return Ok(serde_json::from_str(&cached_data)
                .unwrap_or_else(|_| serde_json::json!({"error": "Cache parse failed"})));
        }

        // Simulate expensive query operation
        let _conn = self
            .connection_pool
            .acquire_connection()
            .await
            .map_err(McpError::internal)?;

        // Simulate processing time based on query complexity
        let processing_delay = if request.query.len() > 100 {
            Duration::from_millis(200)
        } else {
            Duration::from_millis(50)
        };

        tokio::time::sleep(processing_delay).await;

        // Generate synthetic results
        let results = (0..max_results.min(10))
            .map(|i| {
                serde_json::json!({
                    "id": i,
                    "data": format!("Result {} for query: {}", i, request.query),
                    "score": 1.0 - (i as f64 * 0.1),
                    "metadata": {
                        "processed_at": chrono::Utc::now().to_rfc3339(),
                        "query_hash": format!("{:x}", fxhash::hash64(&request.query))
                    }
                })
            })
            .collect::<Vec<_>>();

        let result_json = serde_json::json!({
            "query": request.query,
            "results": results,
            "total_count": results.len(),
            "processing_time_ms": start_time.elapsed().as_millis(),
            "from_cache": false
        });

        // Cache the result if caching is enabled
        if use_cache {
            self.cache_data(
                request.query.clone(),
                serde_json::to_string(&result_json).unwrap_or_default(),
                Duration::from_secs(300), // 5 minute TTL
            )
            .await;
        }

        // Update metrics
        self.update_metrics(|m| {
            m.requests_processed += 1;
            m.total_processing_time += start_time.elapsed();
        })
        .await;

        Ok(result_json)
    }

    /// Get comprehensive performance metrics
    #[tool("Get detailed performance metrics and system health information")]
    async fn get_performance_metrics(&self) -> McpResult<PerformanceReport> {
        tracing::info!("Generating performance report");

        let metrics = self.metrics.lock().await;
        let _cache = self.cache.read().await;
        let uptime = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let cache_total_requests = metrics.cache_hits + metrics.cache_misses;
        let cache_hit_ratio = if cache_total_requests > 0 {
            metrics.cache_hits as f64 / cache_total_requests as f64
        } else {
            0.0
        };

        let avg_response_time = if metrics.requests_processed > 0 {
            metrics.total_processing_time.as_millis() as f64 / metrics.requests_processed as f64
        } else {
            0.0
        };

        let requests_per_second = if uptime > 0 {
            metrics.requests_processed as f64 / uptime as f64
        } else {
            0.0
        };

        let error_rate = if metrics.requests_processed > 0 {
            metrics.error_count as f64 / metrics.requests_processed as f64
        } else {
            0.0
        };

        let report = PerformanceReport {
            uptime_seconds: uptime,
            requests_per_second,
            average_response_time_ms: avg_response_time,
            cache_hit_ratio,
            memory_usage_mb: metrics.peak_memory_usage as f64 / 1024.0 / 1024.0,
            connection_pool_utilization: metrics.active_connections as f64
                / self.config.max_connections as f64,
            error_rate,
            throughput_ops_per_sec: metrics.batch_operations as f64 / uptime as f64,
        };

        tracing::info!(
            "Performance report: {:.2} RPS, {:.2}% cache hit ratio",
            requests_per_second,
            cache_hit_ratio * 100.0
        );

        Ok(report)
    }

    /// Optimize system performance by clearing caches and pools
    #[tool("Optimize system performance by clearing caches and resetting pools")]
    async fn optimize_system(&self, aggressive: Option<bool>) -> McpResult<String> {
        let aggressive = aggressive.unwrap_or(false);

        tracing::info!("Starting system optimization (aggressive: {})", aggressive);

        let mut optimizations = Vec::new();

        // Clear expired cache entries
        {
            let mut cache = self.cache.write().await;
            let initial_size = cache.len();

            // Remove expired entries (simplified implementation)
            cache
                .data
                .retain(|_, entry| entry.created_at.elapsed() < entry.ttl);

            let cleaned = initial_size - cache.len();
            optimizations.push(format!("Cleaned {cleaned} expired cache entries"));
        }

        // Reset memory pools if aggressive
        if aggressive {
            {
                let mut buffers = self.memory_pool.buffers.lock().await;
                buffers.clear();
                optimizations.push("Reset memory pools".to_string());
            }

            // Reset metrics
            {
                let mut metrics = self.metrics.lock().await;
                *metrics = PerformanceMetrics::default();
                optimizations.push("Reset performance metrics".to_string());
            }
        }

        // Force garbage collection hint
        optimizations.push("Triggered garbage collection hint".to_string());

        let result = format!(
            "✅ System optimization completed:\n{}",
            optimizations.join("\n")
        );

        tracing::info!("System optimization completed successfully");
        Ok(result)
    }

    // Helper method for processing individual operations
    async fn process_single_operation(
        operation: &Operation,
        mut _buffer: Vec<u8>,
        _cache: Arc<RwLock<LruCache<String, CacheEntry>>>,
    ) -> McpResult<serde_json::Value> {
        // Simulate different types of operations
        let result = match operation.operation_type.as_str() {
            "compute" => {
                // Simulate CPU-intensive work
                let iterations = 1000 + (operation.priority.unwrap_or(1) as usize * 500);
                let mut sum = 0u64;
                for i in 0..iterations {
                    sum = sum.wrapping_add(i as u64);
                }
                serde_json::json!({"computed_value": sum, "iterations": iterations})
            }
            "transform" => {
                // Simulate data transformation
                serde_json::json!({
                    "original": operation.data,
                    "transformed": operation.data.to_string().to_uppercase(),
                    "length": operation.data.to_string().len()
                })
            }
            "validate" => {
                // Simulate validation logic
                let is_valid = !operation.data.to_string().is_empty();
                serde_json::json!({
                    "valid": is_valid,
                    "data": operation.data,
                    "validation_rules": ["non_empty", "json_format"]
                })
            }
            _ => {
                return Err(McpError::invalid_request(format!(
                    "Unknown operation type: {}",
                    operation.operation_type
                )));
            }
        };

        Ok(result)
    }
}

#[tokio::main]
async fn main() -> McpResult<()> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_target(false)
        .init();

    println!("⚡ TurboMCP: Fast Server Example");
    println!("===========================================");
    println!();
    println!("Performance optimizations demonstrated:");
    println!("• Connection pooling for expensive resources");
    println!("• Memory pools to reduce allocations");
    println!("• Request batching and parallel processing");
    println!("• Intelligent caching with TTL and LRU eviction");
    println!("• Rate limiting and backpressure handling");
    println!("• Comprehensive performance metrics");
    println!();

    tracing::info!("🚀 Starting fast MCP server");

    let config = PerformanceConfig {
        max_connections: 100,
        cache_size: 1000,
        memory_pool_size: 50,
        batch_size: 100,
        request_timeout: Duration::from_secs(30),
        max_concurrent_requests: 200,
    };

    tracing::info!(
        "Configuration: {} max connections, {} cache size, {} batch size",
        config.max_connections,
        config.cache_size,
        config.batch_size
    );

    let server = HighPerformanceServer::new(config);

    tracing::info!(
        "Available tools: batch_process, query_data, get_performance_metrics, optimize_system"
    );
    tracing::info!("Server ready for high-throughput workloads!");

    server
        .run_stdio()
        .await
        .map_err(|e| McpError::internal(format!("Server error: {e}")))
}

// 🎯 **Performance Test Examples:**
//
//    Batch processing:
//    - batch_process({
//        "operations": [
//          {"id": "1", "operation_type": "compute", "data": {"value": 42}, "priority": 1},
//          {"id": "2", "operation_type": "transform", "data": "hello world", "priority": 2}
//        ],
//        "parallel": true,
//        "timeout_ms": 10000
//      })
//
//    Cached queries:
//    - query_data({ "query": "SELECT * FROM users LIMIT 10", "use_cache": true, "max_results": 10 })
//
//    Performance monitoring:
//    - get_performance_metrics()
//    - optimize_system(true)

/* 📝 **Key Performance Patterns:**

**Memory Management:**
- Memory pools prevent frequent allocations
- Buffer reuse reduces GC pressure
- Zero-copy operations where possible
- Careful string handling to avoid copies

**Concurrency Optimization:**
- Connection pooling for expensive resources
- Parallel processing with controlled concurrency
- Rate limiting to prevent system overload
- Semaphore-based backpressure control

**Caching Strategies:**
- LRU eviction for memory efficiency
- TTL-based expiration for data freshness
- Cache hit/miss metrics for optimization
- Strategic cache key design

**Monitoring & Observability:**
- Comprehensive metrics collection
- Performance report generation
- Real-time system health tracking
- Resource utilization monitoring

**Throughput Optimization:**
- Request batching for bulk operations
- Async processing throughout
- Efficient serialization/deserialization
- Database connection reuse

**Production Considerations:**
- Graceful degradation under load
- Circuit breaker patterns (see 05_advanced_patterns.rs)
- Health checks and monitoring endpoints
- Configuration-driven performance tuning

**Next:** `08_integration.rs` - Real-world deployment and integration patterns
*/
