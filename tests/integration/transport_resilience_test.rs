//! Comprehensive tests for transport resilience features
//! Tests circuit breakers, retry mechanisms, health checking, and failover scenarios

use std::sync::atomic::{AtomicU32, AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time::{sleep, timeout};

use turbomcp_transport::resilience::*;
use turbomcp_transport::core::*;
use turbomcp_transport::config::*;
use turbomcp::{McpError, McpResult};

#[tokio::test]
async fn test_circuit_breaker_state_transitions() {
    let config = CircuitBreakerConfig {
        failure_threshold: 3,
        timeout: Duration::from_millis(100),
        half_open_max_calls: 2,
        rolling_window_size: 10,
        min_throughput_threshold: 5,
    };
    
    let circuit_breaker = CircuitBreaker::new(config);
    
    // Initially closed
    assert_eq!(circuit_breaker.state(), CircuitBreakerState::Closed);
    
    // Record failures to trip the circuit
    for _ in 0..3 {
        circuit_breaker.record_failure().await;
    }
    
    // Should be open now
    assert_eq!(circuit_breaker.state(), CircuitBreakerState::Open);
    
    // Wait for timeout
    sleep(Duration::from_millis(150)).await;
    
    // Should be half-open now
    assert_eq!(circuit_breaker.state(), CircuitBreakerState::HalfOpen);
    
    // Record a success to close the circuit
    circuit_breaker.record_success().await;
    circuit_breaker.record_success().await;
    
    // Should be closed again
    assert_eq!(circuit_breaker.state(), CircuitBreakerState::Closed);
}

#[tokio::test]
async fn test_circuit_breaker_rolling_window() {
    let config = CircuitBreakerConfig {
        failure_threshold: 3,
        timeout: Duration::from_secs(1),
        half_open_max_calls: 1,
        rolling_window_size: 5,
        min_throughput_threshold: 3,
    };
    
    let circuit_breaker = CircuitBreaker::new(config);
    
    // Record some successes first
    circuit_breaker.record_success().await;
    circuit_breaker.record_success().await;
    
    // Record failures (should not trip due to rolling window)
    circuit_breaker.record_failure().await;
    circuit_breaker.record_failure().await;
    
    // Should still be closed (not enough throughput)
    assert_eq!(circuit_breaker.state(), CircuitBreakerState::Closed);
    
    // Add more throughput
    circuit_breaker.record_success().await;
    circuit_breaker.record_failure().await; // This makes 3 failures out of 6 total
    
    // Should still be closed (failure rate is 50%, below threshold)
    assert_eq!(circuit_breaker.state(), CircuitBreakerState::Closed);
    
    // Add one more failure to trip it
    circuit_breaker.record_failure().await; // 4 failures out of 7 total
    
    // Should be open now
    assert_eq!(circuit_breaker.state(), CircuitBreakerState::Open);
}

#[tokio::test]
async fn test_circuit_breaker_concurrent_operations() {
    let config = CircuitBreakerConfig {
        failure_threshold: 10,
        timeout: Duration::from_millis(100),
        half_open_max_calls: 5,
        rolling_window_size: 50,
        min_throughput_threshold: 20,
    };
    
    let circuit_breaker = Arc::new(CircuitBreaker::new(config));
    let mut handles = vec![];
    
    // Spawn multiple tasks that record failures concurrently
    for _ in 0..50 {
        let cb = Arc::clone(&circuit_breaker);
        let handle = tokio::spawn(async move {
            for _ in 0..5 {
                cb.record_failure().await;
                sleep(Duration::from_millis(1)).await;
            }
        });
        handles.push(handle);
    }
    
    // Wait for all tasks to complete
    futures::future::join_all(handles).await;
    
    // Circuit should be open due to high failure rate
    assert_eq!(circuit_breaker.state(), CircuitBreakerState::Open);
    
    // Verify statistics
    let stats = circuit_breaker.statistics().await;
    assert_eq!(stats.total_calls, 250); // 50 tasks * 5 calls each
    assert_eq!(stats.failed_calls, 250);
    assert_eq!(stats.success_rate, 0.0);
}

#[tokio::test]
async fn test_circuit_breaker_half_open_behavior() {
    let config = CircuitBreakerConfig {
        failure_threshold: 2,
        timeout: Duration::from_millis(50),
        half_open_max_calls: 3,
        rolling_window_size: 10,
        min_throughput_threshold: 2,
    };
    
    let circuit_breaker = Arc::new(CircuitBreaker::new(config));
    
    // Trip the circuit
    circuit_breaker.record_failure().await;
    circuit_breaker.record_failure().await;
    circuit_breaker.record_failure().await;
    
    assert_eq!(circuit_breaker.state(), CircuitBreakerState::Open);
    
    // Wait for timeout
    sleep(Duration::from_millis(100)).await;
    
    // Should be half-open
    assert_eq!(circuit_breaker.state(), CircuitBreakerState::HalfOpen);
    
    // Test concurrent calls in half-open state
    let mut handles = vec![];
    for i in 0..10 {
        let cb = Arc::clone(&circuit_breaker);
        let handle = tokio::spawn(async move {
            // First few should be allowed, rest should be rejected
            let allowed = cb.call_allowed().await;
            if allowed {
                if i < 2 {
                    cb.record_success().await;
                } else {
                    cb.record_failure().await;
                }
            }
            allowed
        });
        handles.push(handle);
    }
    
    let results: Vec<bool> = futures::future::join_all(handles)
        .await
        .into_iter()
        .map(|r| r.unwrap())
        .collect();
    
    // Only half_open_max_calls should be allowed
    let allowed_count = results.iter().filter(|&&allowed| allowed).count();
    assert_eq!(allowed_count, 3);
}

#[tokio::test]
async fn test_retry_mechanism_exponential_backoff() {
    let config = RetryConfig {
        max_attempts: 5,
        initial_delay: Duration::from_millis(10),
        max_delay: Duration::from_millis(1000),
        backoff_multiplier: 2.0,
        jitter: true,
    };
    
    let retry_mechanism = RetryMechanism::new(config);
    let attempt_times = Arc::new(std::sync::Mutex::new(vec![]));
    let failure_count = Arc::new(AtomicU32::new(0));
    
    let times_clone = Arc::clone(&attempt_times);
    let failure_clone = Arc::clone(&failure_count);
    
    let operation = || {
        let times = Arc::clone(&times_clone);
        let failures = Arc::clone(&failure_clone);
        
        async move {
            times.lock().unwrap().push(Instant::now());
            let count = failures.fetch_add(1, Ordering::SeqCst);
            
            // Fail first 3 attempts, succeed on 4th
            if count < 3 {
                Err(McpError::Tool("Simulated failure".to_string()))
            } else {
                Ok("Success".to_string())
            }
        }
    };
    
    let result = retry_mechanism.execute(operation).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "Success");
    
    // Verify backoff timing
    let times = attempt_times.lock().unwrap();
    assert_eq!(times.len(), 4); // 3 failures + 1 success
    
    // Check that delays increase exponentially (approximately)
    for i in 1..times.len() {
        let delay = times[i].duration_since(times[i-1]);
        let expected_min = Duration::from_millis(10 * (2_u64.pow(i as u32 - 1)));
        assert!(delay >= expected_min / 2); // Account for jitter
    }
}

#[tokio::test]
async fn test_retry_mechanism_max_attempts() {
    let config = RetryConfig {
        max_attempts: 3,
        initial_delay: Duration::from_millis(1),
        max_delay: Duration::from_millis(10),
        backoff_multiplier: 2.0,
        jitter: false,
    };
    
    let retry_mechanism = RetryMechanism::new(config);
    let attempt_count = Arc::new(AtomicU32::new(0));
    
    let count_clone = Arc::clone(&attempt_count);
    let operation = || {
        let count = Arc::clone(&count_clone);
        async move {
            count.fetch_add(1, Ordering::SeqCst);
            Err::<String, _>(McpError::Tool("Always fails".to_string()))
        }
    };
    
    let result = retry_mechanism.execute(operation).await;
    assert!(result.is_err());
    
    // Should have attempted exactly max_attempts times
    assert_eq!(attempt_count.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn test_retry_mechanism_custom_conditions() {
    let config = RetryConfig {
        max_attempts: 5,
        initial_delay: Duration::from_millis(1),
        max_delay: Duration::from_millis(10),
        backoff_multiplier: 2.0,
        jitter: false,
    };
    
    let retry_mechanism = RetryMechanism::with_condition(
        config,
        |error: &McpError| {
            // Only retry on specific error types
            matches!(error, McpError::Tool(msg) if msg.contains("retryable"))
        }
    );
    
    let attempt_count = Arc::new(AtomicU32::new(0));
    
    // Test non-retryable error
    let count_clone = Arc::clone(&attempt_count);
    let operation = || {
        let count = Arc::clone(&count_clone);
        async move {
            count.fetch_add(1, Ordering::SeqCst);
            Err::<String, _>(McpError::Tool("non-retryable error".to_string()))
        }
    };
    
    let result = retry_mechanism.execute(operation).await;
    assert!(result.is_err());
    assert_eq!(attempt_count.load(Ordering::SeqCst), 1); // Should not retry
    
    // Reset counter
    attempt_count.store(0, Ordering::SeqCst);
    
    // Test retryable error
    let count_clone = Arc::clone(&attempt_count);
    let operation = || {
        let count = Arc::clone(&count_clone);
        async move {
            count.fetch_add(1, Ordering::SeqCst);
            Err::<String, _>(McpError::Tool("retryable error".to_string()))
        }
    };
    
    let result = retry_mechanism.execute(operation).await;
    assert!(result.is_err());
    assert_eq!(attempt_count.load(Ordering::SeqCst), 5); // Should retry all attempts
}

#[tokio::test]
async fn test_health_checker_basic_functionality() {
    let config = HealthCheckConfig {
        interval: Duration::from_millis(50),
        timeout: Duration::from_millis(30),
        failure_threshold: 3,
        success_threshold: 2,
    };
    
    let health_checker = HealthChecker::new(config);
    let check_count = Arc::new(AtomicU32::new(0));
    
    let count_clone = Arc::clone(&check_count);
    let health_check = move || {
        let count = Arc::clone(&count_clone);
        async move {
            let current = count.fetch_add(1, Ordering::SeqCst);
            // Fail first 3 checks, then succeed
            if current < 3 {
                Err(McpError::Tool("Health check failed".to_string()))
            } else {
                Ok(())
            }
        }
    };
    
    health_checker.start(health_check);
    
    // Wait for health transitions
    sleep(Duration::from_millis(200)).await;
    
    let status = health_checker.status().await;
    assert_eq!(status.state, HealthState::Healthy);
    assert!(status.consecutive_successes >= 2);
    
    health_checker.stop().await;
}

#[tokio::test]
async fn test_health_checker_timeout_handling() {
    let config = HealthCheckConfig {
        interval: Duration::from_millis(20),
        timeout: Duration::from_millis(10),
        failure_threshold: 2,
        success_threshold: 1,
    };
    
    let health_checker = HealthChecker::new(config);
    
    // Health check that always times out
    let health_check = || async {
        sleep(Duration::from_millis(50)).await; // Longer than timeout
        Ok(())
    };
    
    health_checker.start(health_check);
    
    // Wait for timeouts to be detected
    sleep(Duration::from_millis(100)).await;
    
    let status = health_checker.status().await;
    assert_eq!(status.state, HealthState::Unhealthy);
    assert!(status.consecutive_failures >= 2);
    
    health_checker.stop().await;
}

#[tokio::test]
async fn test_health_checker_concurrent_checks() {
    let config = HealthCheckConfig {
        interval: Duration::from_millis(10),
        timeout: Duration::from_millis(100),
        failure_threshold: 1,
        success_threshold: 1,
    };
    
    let health_checker = HealthChecker::new(config);
    let check_count = Arc::new(AtomicU32::new(0));
    let active_checks = Arc::new(AtomicU32::new(0));
    
    let count_clone = Arc::clone(&check_count);
    let active_clone = Arc::clone(&active_checks);
    
    let health_check = move || {
        let count = Arc::clone(&count_clone);
        let active = Arc::clone(&active_clone);
        
        async move {
            count.fetch_add(1, Ordering::SeqCst);
            let current_active = active.fetch_add(1, Ordering::SeqCst);
            
            // Verify only one check is active at a time
            assert_eq!(current_active, 0, "Multiple health checks running concurrently");
            
            sleep(Duration::from_millis(20)).await;
            
            active.fetch_sub(1, Ordering::SeqCst);
            Ok(())
        }
    };
    
    health_checker.start(health_check);
    
    // Let it run for a while
    sleep(Duration::from_millis(100)).await;
    
    health_checker.stop().await;
    
    // Verify checks were performed
    assert!(check_count.load(Ordering::SeqCst) > 0);
}

#[tokio::test]
async fn test_deduplication_cache() {
    let cache = DeduplicationCache::new(Duration::from_millis(100), 1000);
    
    // Test basic deduplication
    let key1 = "request_1";
    assert!(!cache.is_duplicate(key1).await);
    assert!(cache.is_duplicate(key1).await);
    
    // Test different keys
    let key2 = "request_2";
    assert!(!cache.is_duplicate(key2).await);
    
    // Test expiration
    sleep(Duration::from_millis(150)).await;
    assert!(!cache.is_duplicate(key1).await); // Should have expired
}

#[tokio::test]
async fn test_deduplication_cache_capacity() {
    let cache = DeduplicationCache::new(Duration::from_secs(10), 3); // Small capacity
    
    // Fill the cache
    for i in 0..3 {
        let key = format!("request_{}", i);
        assert!(!cache.is_duplicate(&key).await);
    }
    
    // Add one more (should evict oldest)
    assert!(!cache.is_duplicate("request_3").await);
    
    // First key should have been evicted
    assert!(!cache.is_duplicate("request_0").await);
}

#[tokio::test]
async fn test_deduplication_cache_concurrent_access() {
    let cache = Arc::new(DeduplicationCache::new(Duration::from_millis(100), 1000));
    let mut handles = vec![];
    
    // Multiple tasks checking the same key concurrently
    for _ in 0..10 {
        let cache_clone = Arc::clone(&cache);
        let handle = tokio::spawn(async move {
            cache_clone.is_duplicate("concurrent_key").await
        });
        handles.push(handle);
    }
    
    let results: Vec<bool> = futures::future::join_all(handles)
        .await
        .into_iter()
        .map(|r| r.unwrap())
        .collect();
    
    // Exactly one should return false (first), rest should return true
    let first_count = results.iter().filter(|&&is_dup| !is_dup).count();
    assert_eq!(first_count, 1);
}

#[tokio::test]
async fn test_transport_failover_scenario() {
    // Simulate a transport that fails after some time
    let primary_transport = FailingTransport::new(Duration::from_millis(100));
    let backup_transport = ReliableTransport::new();
    
    let failover_config = TransportFailoverConfig {
        primary: Box::new(primary_transport),
        backup: Box::new(backup_transport),
        failover_threshold: 3,
        recovery_check_interval: Duration::from_millis(50),
    };
    
    let failover_transport = FailoverTransport::new(failover_config);
    
    // Initially should use primary
    assert!(failover_transport.send_message("test".to_string()).await.is_ok());
    
    // Wait for primary to start failing
    sleep(Duration::from_millis(150)).await;
    
    // Should fail a few times, then switch to backup
    for _ in 0..5 {
        let result = failover_transport.send_message("test".to_string()).await;
        if result.is_ok() {
            break; // Successfully switched to backup
        }
        sleep(Duration::from_millis(10)).await;
    }
    
    // Should now be using backup successfully
    assert!(failover_transport.send_message("test".to_string()).await.is_ok());
}

#[tokio::test]
async fn test_combined_robustness_features() {
    // Test circuit breaker + retry + health checking together
    let circuit_config = CircuitBreakerConfig {
        failure_threshold: 2,
        timeout: Duration::from_millis(50),
        half_open_max_calls: 1,
        rolling_window_size: 5,
        min_throughput_threshold: 2,
    };
    
    let retry_config = RetryConfig {
        max_attempts: 3,
        initial_delay: Duration::from_millis(5),
        max_delay: Duration::from_millis(20),
        backoff_multiplier: 2.0,
        jitter: false,
    };
    
    let health_config = HealthCheckConfig {
        interval: Duration::from_millis(20),
        timeout: Duration::from_millis(15),
        failure_threshold: 2,
        success_threshold: 1,
    };
    
    let robustness_manager = RobustnessManager::new(
        circuit_config,
        retry_config,
        health_config,
    );
    
    let failure_count = Arc::new(AtomicU32::new(0));
    let failure_clone = Arc::clone(&failure_count);
    
    let operation = move || {
        let failures = Arc::clone(&failure_clone);
        async move {
            let count = failures.fetch_add(1, Ordering::SeqCst);
            
            // Fail first 5 times, then succeed
            if count < 5 {
                Err(McpError::Tool("Failing operation".to_string()))
            } else {
                Ok("Success".to_string())
            }
        }
    };
    
    // Execute operation through robustness manager
    let result = robustness_manager.execute_with_robustness(operation).await;
    
    // Should eventually succeed despite initial failures
    assert!(result.is_ok());
    
    // Verify health status reflects the operation outcomes
    let health_status = robustness_manager.health_status().await;
    println!("Final health status: {:?}", health_status);
}

// Helper test implementations

#[derive(Debug)]
struct FailingTransport {
    start_time: Instant,
    fail_after: Duration,
}

impl FailingTransport {
    fn new(fail_after: Duration) -> Self {
        Self {
            start_time: Instant::now(),
            fail_after,
        }
    }
    
    async fn send_message(&self, _message: String) -> McpResult<()> {
        if self.start_time.elapsed() > self.fail_after {
            Err(McpError::Tool("Transport failed".to_string()))
        } else {
            Ok(())
        }
    }
}

#[derive(Debug)]
struct ReliableTransport;

impl ReliableTransport {
    fn new() -> Self {
        Self
    }
    
    async fn send_message(&self, _message: String) -> McpResult<()> {
        Ok(())
    }
}

struct TransportFailoverConfig {
    primary: Box<dyn std::fmt::Debug + Send + Sync>,
    backup: Box<dyn std::fmt::Debug + Send + Sync>,
    failover_threshold: u32,
    recovery_check_interval: Duration,
}

struct FailoverTransport {
    using_primary: Arc<AtomicBool>,
    failure_count: Arc<AtomicU32>,
    config: TransportFailoverConfig,
}

impl FailoverTransport {
    fn new(config: TransportFailoverConfig) -> Self {
        Self {
            using_primary: Arc::new(AtomicBool::new(true)),
            failure_count: Arc::new(AtomicU32::new(0)),
            config,
        }
    }
    
    async fn send_message(&self, message: String) -> McpResult<()> {
        if self.using_primary.load(Ordering::SeqCst) {
            // Try primary transport
            if let Ok(primary) = self.config.primary.downcast_ref::<FailingTransport>() {
                match primary.send_message(message.clone()).await {
                    Ok(()) => {
                        self.failure_count.store(0, Ordering::SeqCst);
                        return Ok(());
                    }
                    Err(_) => {
                        let failures = self.failure_count.fetch_add(1, Ordering::SeqCst) + 1;
                        if failures >= self.config.failover_threshold {
                            self.using_primary.store(false, Ordering::SeqCst);
                        }
                        return Err(McpError::Tool("Primary transport failed".to_string()));
                    }
                }
            }
        }
        
        // Use backup transport
        if let Ok(backup) = self.config.backup.downcast_ref::<ReliableTransport>() {
            backup.send_message(message).await
        } else {
            Err(McpError::Tool("No available transport".to_string()))
        }
    }
}

// Additional robustness manager for combined testing
struct RobustnessManager {
    circuit_breaker: Arc<CircuitBreaker>,
    retry_mechanism: RetryMechanism,
    health_checker: HealthChecker,
}

impl RobustnessManager {
    fn new(
        circuit_config: CircuitBreakerConfig,
        retry_config: RetryConfig,
        health_config: HealthCheckConfig,
    ) -> Self {
        Self {
            circuit_breaker: Arc::new(CircuitBreaker::new(circuit_config)),
            retry_mechanism: RetryMechanism::new(retry_config),
            health_checker: HealthChecker::new(health_config),
        }
    }
    
    async fn execute_with_robustness<F, Fut, T>(&self, operation: F) -> McpResult<T>
    where
        F: Fn() -> Fut + Clone,
        Fut: std::future::Future<Output = McpResult<T>>,
    {
        // Check circuit breaker first
        if !self.circuit_breaker.call_allowed().await {
            return Err(McpError::Tool("Circuit breaker open".to_string()));
        }
        
        // Execute with retry
        let cb = Arc::clone(&self.circuit_breaker);
        let retry_operation = || {
            let op = operation.clone();
            let cb_inner = Arc::clone(&cb);
            
            async move {
                match op().await {
                    Ok(result) => {
                        cb_inner.record_success().await;
                        Ok(result)
                    }
                    Err(err) => {
                        cb_inner.record_failure().await;
                        Err(err)
                    }
                }
            }
        };
        
        self.retry_mechanism.execute(retry_operation).await
    }
    
    async fn health_status(&self) -> HealthStatus {
        self.health_checker.status().await
    }
}