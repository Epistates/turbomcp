//! Comprehensive error propagation and fault injection tests
//! Tests error handling across module boundaries, fault tolerance, and system recovery

use std::sync::atomic::{AtomicU32, AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use std::collections::HashMap;
use tokio::sync::{Mutex, RwLock, oneshot, mpsc};
use tokio::time::{sleep, timeout};
use futures::stream::{FuturesUnordered, StreamExt};

use turbomcp::*;
use turbomcp_server::*;
use turbomcp_transport::*;
use turbomcp_protocol::*;

// Error injection framework
#[derive(Debug, Clone)]
pub struct FaultConfig {
    pub error_rate: f64,           // 0.0 to 1.0
    pub error_types: Vec<ErrorType>,
    pub delay_range: (Duration, Duration),
    pub burst_mode: bool,
    pub recovery_time: Duration,
}

#[derive(Debug, Clone)]
pub enum ErrorType {
    NetworkTimeout,
    ConnectionRefused,
    InvalidResponse,
    MemoryExhaustion,
    DiskFull,
    PermissionDenied,
    ServiceUnavailable,
    CorruptedData,
    PartialFailure,
    CascadingFailure,
}

pub struct FaultInjector {
    config: FaultConfig,
    error_count: AtomicU32,
    injection_active: AtomicBool,
    last_injection: Arc<Mutex<Option<Instant>>>,
}

impl FaultInjector {
    pub fn new(config: FaultConfig) -> Self {
        Self {
            config,
            error_count: AtomicU32::new(0),
            injection_active: AtomicBool::new(true),
            last_injection: Arc::new(Mutex::new(None)),
        }
    }
    
    pub async fn should_inject_error(&self) -> bool {
        if !self.injection_active.load(Ordering::SeqCst) {
            return false;
        }
        
        // Check if we're in recovery period
        if let Some(last_injection) = *self.last_injection.lock().await {
            if last_injection.elapsed() < self.config.recovery_time {
                return false;
            }
        }
        
        // Determine if we should inject based on error rate
        let should_inject = rand::random::<f64>() < self.config.error_rate;
        
        if should_inject {
            self.error_count.fetch_add(1, Ordering::SeqCst);
            *self.last_injection.lock().await = Some(Instant::now());
        }
        
        should_inject
    }
    
    pub async fn inject_error(&self) -> McpError {
        let error_type = &self.config.error_types[
            rand::random::<usize>() % self.config.error_types.len()
        ];
        
        // Add random delay if configured
        let delay = rand::random::<f64>() * 
            (self.config.delay_range.1.as_nanos() - self.config.delay_range.0.as_nanos()) as f64 +
            self.config.delay_range.0.as_nanos() as f64;
        
        sleep(Duration::from_nanos(delay as u64)).await;
        
        match error_type {
            ErrorType::NetworkTimeout => McpError::Transport("Network timeout".to_string()),
            ErrorType::ConnectionRefused => McpError::Transport("Connection refused".to_string()),
            ErrorType::InvalidResponse => McpError::InvalidResponse("Invalid response format".to_string()),
            ErrorType::MemoryExhaustion => McpError::Tool("Out of memory".to_string()),
            ErrorType::DiskFull => McpError::Tool("Disk full".to_string()),
            ErrorType::PermissionDenied => McpError::Tool("Permission denied".to_string()),
            ErrorType::ServiceUnavailable => McpError::Tool("Service unavailable".to_string()),
            ErrorType::CorruptedData => McpError::InvalidParams("Corrupted data detected".to_string()),
            ErrorType::PartialFailure => McpError::Tool("Partial failure occurred".to_string()),
            ErrorType::CascadingFailure => McpError::Tool("Cascading failure detected".to_string()),
        }
    }
    
    pub fn disable_injection(&self) {
        self.injection_active.store(false, Ordering::SeqCst);
    }
    
    pub fn enable_injection(&self) {
        self.injection_active.store(true, Ordering::SeqCst);
    }
    
    pub fn get_error_count(&self) -> u32 {
        self.error_count.load(Ordering::SeqCst)
    }
}

// Error propagation through transport layer
#[tokio::test]
async fn test_transport_error_propagation() {
    let fault_config = FaultConfig {
        error_rate: 0.3, // 30% error rate
        error_types: vec![
            ErrorType::NetworkTimeout,
            ErrorType::ConnectionRefused,
            ErrorType::InvalidResponse,
        ],
        delay_range: (Duration::from_millis(10), Duration::from_millis(100)),
        burst_mode: false,
        recovery_time: Duration::from_millis(50),
    };
    
    let fault_injector = Arc::new(FaultInjector::new(fault_config));
    let transport = FaultyTransport::new(Arc::clone(&fault_injector));
    let server = TestServer::new(Box::new(transport));
    
    const TOTAL_REQUESTS: usize = 100;
    let mut success_count = 0;
    let mut transport_errors = 0;
    let mut other_errors = 0;
    
    for i in 0..TOTAL_REQUESTS {
        let request = create_test_request(i);
        let result = server.handle_request(request).await;
        
        match result {
            Ok(_) => success_count += 1,
            Err(McpError::Transport(_)) => transport_errors += 1,
            Err(_) => other_errors += 1,
        }
    }
    
    println!("Transport Error Propagation Results:");
    println!("Success: {}/{}", success_count, TOTAL_REQUESTS);
    println!("Transport errors: {}", transport_errors);
    println!("Other errors: {}", other_errors);
    println!("Injected errors: {}", fault_injector.get_error_count());
    
    // Verify error propagation
    assert!(transport_errors > 0, "Expected some transport errors");
    assert!(success_count > 0, "Expected some successful requests");
    assert_eq!(success_count + transport_errors + other_errors, TOTAL_REQUESTS);
}

// Error propagation through middleware stack
#[tokio::test]
async fn test_middleware_error_propagation() {
    let fault_config = FaultConfig {
        error_rate: 0.2,
        error_types: vec![ErrorType::PermissionDenied, ErrorType::ServiceUnavailable],
        delay_range: (Duration::from_millis(5), Duration::from_millis(25)),
        burst_mode: false,
        recovery_time: Duration::from_millis(30),
    };
    
    let fault_injector = Arc::new(FaultInjector::new(fault_config));
    
    let mut middleware_stack = MiddlewareStack::new();
    middleware_stack.add_middleware(Box::new(FaultyMiddleware::new(Arc::clone(&fault_injector))), 1);
    middleware_stack.add_middleware(Box::new(ErrorRecoveryMiddleware::new()), 2);
    middleware_stack.add_middleware(Box::new(LoggingMiddleware::new()), 3);
    
    const TOTAL_REQUESTS: usize = 50;
    let mut middleware_errors = 0;
    let mut recovered_requests = 0;
    let mut successful_requests = 0;
    
    for i in 0..TOTAL_REQUESTS {
        let request = create_test_request(i);
        let result = middleware_stack.process_request(request).await;
        
        match result {
            Ok(response) => {
                if response.has_warning("recovered_from_error") {
                    recovered_requests += 1;
                } else {
                    successful_requests += 1;
                }
            }
            Err(_) => middleware_errors += 1,
        }
    }
    
    println!("Middleware Error Propagation Results:");
    println!("Successful: {}", successful_requests);
    println!("Recovered: {}", recovered_requests);
    println!("Failed: {}", middleware_errors);
    println!("Injected errors: {}", fault_injector.get_error_count());
    
    // Verify error handling and recovery
    assert!(fault_injector.get_error_count() > 0, "Expected fault injection to occur");
    assert!(recovered_requests > 0, "Expected some error recovery");
}

// Cascading failure simulation
#[tokio::test]
async fn test_cascading_failure_prevention() {
    let primary_fault_config = FaultConfig {
        error_rate: 0.8, // High error rate for primary service
        error_types: vec![ErrorType::ServiceUnavailable],
        delay_range: (Duration::from_millis(100), Duration::from_millis(200)),
        burst_mode: true,
        recovery_time: Duration::from_millis(500),
    };
    
    let secondary_fault_config = FaultConfig {
        error_rate: 0.1, // Low error rate for backup service
        error_types: vec![ErrorType::ServiceUnavailable],
        delay_range: (Duration::from_millis(10), Duration::from_millis(50)),
        burst_mode: false,
        recovery_time: Duration::from_millis(100),
    };
    
    let primary_injector = Arc::new(FaultInjector::new(primary_fault_config));
    let secondary_injector = Arc::new(FaultInjector::new(secondary_fault_config));
    
    let primary_service = FaultyService::new("primary", Arc::clone(&primary_injector));
    let secondary_service = FaultyService::new("secondary", Arc::clone(&secondary_injector));
    
    let circuit_breaker = CircuitBreaker::new(CircuitBreakerConfig {
        failure_threshold: 3,
        timeout: Duration::from_millis(100),
        half_open_max_calls: 2,
        rolling_window_size: 10,
        min_throughput_threshold: 5,
    });
    
    let failover_system = FailoverSystem::new(
        primary_service,
        secondary_service,
        circuit_breaker,
    );
    
    const TOTAL_REQUESTS: usize = 100;
    let mut primary_successes = 0;
    let mut secondary_successes = 0;
    let mut total_failures = 0;
    
    for i in 0..TOTAL_REQUESTS {
        let request = create_test_request(i);
        let result = failover_system.handle_request(request).await;
        
        match result {
            Ok(response) => {
                if response.service_used() == "primary" {
                    primary_successes += 1;
                } else {
                    secondary_successes += 1;
                }
            }
            Err(_) => total_failures += 1,
        }
        
        // Small delay between requests
        sleep(Duration::from_millis(10)).await;
    }
    
    println!("Cascading Failure Prevention Results:");
    println!("Primary successes: {}", primary_successes);
    println!("Secondary successes: {}", secondary_successes);
    println!("Total failures: {}", total_failures);
    println!("Primary injected errors: {}", primary_injector.get_error_count());
    println!("Secondary injected errors: {}", secondary_injector.get_error_count());
    
    // Verify cascading failure prevention
    assert!(secondary_successes > 0, "Expected failover to secondary service");
    assert!(total_failures < TOTAL_REQUESTS / 2, "Too many total failures - cascading failure not prevented");
    assert!(primary_successes + secondary_successes > TOTAL_REQUESTS / 2, "System should maintain availability");
}

// Error recovery and retry logic
#[tokio::test]
async fn test_error_recovery_strategies() {
    let fault_config = FaultConfig {
        error_rate: 0.6, // High error rate to test recovery
        error_types: vec![
            ErrorType::NetworkTimeout,
            ErrorType::ServiceUnavailable,
            ErrorType::PartialFailure,
        ],
        delay_range: (Duration::from_millis(20), Duration::from_millis(80)),
        burst_mode: false,
        recovery_time: Duration::from_millis(100),
    };
    
    let fault_injector = Arc::new(FaultInjector::new(fault_config));
    let faulty_service = FaultyService::new("test_service", Arc::clone(&fault_injector));
    
    // Test different recovery strategies
    let strategies = vec![
        RecoveryStrategy::SimpleRetry { max_attempts: 3 },
        RecoveryStrategy::ExponentialBackoff { 
            max_attempts: 3, 
            base_delay: Duration::from_millis(10) 
        },
        RecoveryStrategy::CircuitBreaker { 
            failure_threshold: 2,
            timeout: Duration::from_millis(50),
        },
        RecoveryStrategy::Hedging { 
            hedge_delay: Duration::from_millis(30),
            max_hedges: 2,
        },
    ];
    
    for strategy in strategies {
        let recovery_service = RecoveryService::new(
            Box::new(faulty_service.clone()),
            strategy.clone(),
        );
        
        const REQUESTS_PER_STRATEGY: usize = 30;
        let mut successes = 0;
        let mut total_attempts = 0;
        
        let start_time = Instant::now();
        
        for i in 0..REQUESTS_PER_STRATEGY {
            let request = create_test_request(i);
            let result = recovery_service.handle_request_with_recovery(request).await;
            
            total_attempts += recovery_service.get_attempt_count();
            
            if result.is_ok() {
                successes += 1;
            }
        }
        
        let total_time = start_time.elapsed();
        
        println!("Recovery Strategy: {:?}", strategy);
        println!("  Successes: {}/{}", successes, REQUESTS_PER_STRATEGY);
        println!("  Total attempts: {}", total_attempts);
        println!("  Average attempts per request: {:.2}", total_attempts as f64 / REQUESTS_PER_STRATEGY as f64);
        println!("  Total time: {:?}", total_time);
        println!("  Success rate: {:.2}%", successes as f64 / REQUESTS_PER_STRATEGY as f64 * 100.0);
        println!();
        
        // Each strategy should improve success rate compared to no recovery
        assert!(successes > REQUESTS_PER_STRATEGY / 3, "Recovery strategy should improve success rate");
    }
}

// Memory leak detection during error conditions
#[tokio::test]
async fn test_memory_leaks_under_error_conditions() {
    let fault_config = FaultConfig {
        error_rate: 0.5,
        error_types: vec![
            ErrorType::MemoryExhaustion,
            ErrorType::CorruptedData,
            ErrorType::PartialFailure,
        ],
        delay_range: (Duration::from_millis(1), Duration::from_millis(10)),
        burst_mode: false,
        recovery_time: Duration::from_millis(50),
    };
    
    let fault_injector = Arc::new(FaultInjector::new(fault_config));
    let memory_tracker = Arc::new(MemoryTracker::new());
    let service = MemoryTrackingService::new(
        Arc::clone(&fault_injector),
        Arc::clone(&memory_tracker),
    );
    
    let baseline_memory = memory_tracker.get_memory_usage();
    
    const TOTAL_REQUESTS: usize = 1000;
    let mut successful_requests = 0;
    let mut failed_requests = 0;
    
    for i in 0..TOTAL_REQUESTS {
        let request = create_large_test_request(i);
        let result = service.handle_request(request).await;
        
        match result {
            Ok(_) => successful_requests += 1,
            Err(_) => failed_requests += 1,
        }
        
        // Periodically check for memory growth
        if i % 100 == 0 {
            let current_memory = memory_tracker.get_memory_usage();
            let growth = current_memory.saturating_sub(baseline_memory);
            
            // Memory growth should be bounded even with errors
            assert!(growth < 50 * 1024 * 1024, "Excessive memory growth detected: {} bytes", growth);
        }
    }
    
    // Force garbage collection
    service.cleanup().await;
    
    let final_memory = memory_tracker.get_memory_usage();
    let memory_growth = final_memory.saturating_sub(baseline_memory);
    
    println!("Memory Leak Test Results:");
    println!("Successful requests: {}", successful_requests);
    println!("Failed requests: {}", failed_requests);
    println!("Baseline memory: {} bytes", baseline_memory);
    println!("Final memory: {} bytes", final_memory);
    println!("Memory growth: {} bytes", memory_growth);
    println!("Peak memory: {} bytes", memory_tracker.get_peak_memory());
    
    // Verify no significant memory leaks
    assert!(memory_growth < 10 * 1024 * 1024, "Memory leak detected: {} bytes growth", memory_growth);
    assert!(successful_requests > 0, "Expected some successful requests");
    assert!(failed_requests > 0, "Expected some failed requests due to fault injection");
}

// Distributed system partition tolerance
#[tokio::test]
async fn test_partition_tolerance() {
    let nodes = vec![
        create_distributed_node("node1", 0.1), // 10% error rate
        create_distributed_node("node2", 0.2), // 20% error rate  
        create_distributed_node("node3", 0.15), // 15% error rate
    ];
    
    let consensus_system = ConsensusSystem::new(nodes);
    
    // Simulate network partitions
    let partition_scenarios = vec![
        vec![0, 1], // Isolate node 2
        vec![1, 2], // Isolate node 0
        vec![0], // Isolate nodes 1 and 2
        vec![0, 1, 2], // No partition
    ];
    
    for (scenario_idx, available_nodes) in partition_scenarios.iter().enumerate() {
        println!("Testing partition scenario {}: nodes {:?} available", scenario_idx, available_nodes);
        
        consensus_system.simulate_partition(available_nodes.clone()).await;
        
        const REQUESTS_PER_SCENARIO: usize = 20;
        let mut consensus_successes = 0;
        let mut consensus_failures = 0;
        
        for i in 0..REQUESTS_PER_SCENARIO {
            let proposal = format!("proposal_{}_{}", scenario_idx, i);
            let result = consensus_system.propose_value(proposal).await;
            
            match result {
                Ok(_) => consensus_successes += 1,
                Err(_) => consensus_failures += 1,
            }
        }
        
        println!("  Consensus successes: {}/{}", consensus_successes, REQUESTS_PER_SCENARIO);
        
        // Verify partition tolerance
        if available_nodes.len() >= 2 {
            // With majority available, should achieve consensus
            assert!(consensus_successes > REQUESTS_PER_SCENARIO / 2, 
                "Consensus should succeed with majority available");
        } else {
            // With minority available, consensus may fail
            println!("  Minority partition - consensus failures expected");
        }
    }
}

// Chaos engineering test
#[tokio::test]
async fn test_chaos_engineering_scenario() {
    let chaos_config = ChaosConfig {
        network_failures: 0.1,
        memory_pressure: 0.05,
        cpu_spikes: 0.08,
        disk_errors: 0.03,
        service_crashes: 0.02,
        clock_skew: 0.01,
    };
    
    let chaos_monkey = ChaosMonkey::new(chaos_config);
    let system = ChaosTestSystem::new(Arc::new(chaos_monkey));
    
    // Run chaos test for extended period
    const TEST_DURATION: Duration = Duration::from_secs(30);
    const REQUEST_INTERVAL: Duration = Duration::from_millis(100);
    
    let start_time = Instant::now();
    let mut total_requests = 0;
    let mut successful_requests = 0;
    let mut error_types = HashMap::new();
    
    while start_time.elapsed() < TEST_DURATION {
        let request = create_test_request(total_requests);
        let result = system.handle_request(request).await;
        
        total_requests += 1;
        
        match result {
            Ok(_) => successful_requests += 1,
            Err(error) => {
                let error_type = classify_error(&error);
                *error_types.entry(error_type).or_insert(0) += 1;
            }
        }
        
        sleep(REQUEST_INTERVAL).await;
    }
    
    let uptime_percentage = (successful_requests as f64 / total_requests as f64) * 100.0;
    
    println!("Chaos Engineering Test Results:");
    println!("Total requests: {}", total_requests);
    println!("Successful requests: {}", successful_requests);
    println!("Uptime: {:.2}%", uptime_percentage);
    println!("Error distribution:");
    for (error_type, count) in error_types {
        println!("  {}: {}", error_type, count);
    }
    
    // System should maintain reasonable availability under chaos
    assert!(uptime_percentage > 80.0, "System availability too low under chaos: {:.2}%", uptime_percentage);
    assert!(total_requests > 200, "Test should have generated sufficient load");
}

// Helper implementations for testing

// ... (Due to length constraints, I'll provide the key helper implementations)

struct FaultyTransport {
    fault_injector: Arc<FaultInjector>,
}

impl FaultyTransport {
    fn new(fault_injector: Arc<FaultInjector>) -> Self {
        Self { fault_injector }
    }
    
    async fn send_message(&self, message: String) -> McpResult<String> {
        if self.fault_injector.should_inject_error().await {
            Err(self.fault_injector.inject_error().await)
        } else {
            // Simulate successful transport
            sleep(Duration::from_millis(10)).await;
            Ok(format!("response_to_{}", message))
        }
    }
}

#[derive(Debug, Clone)]
enum RecoveryStrategy {
    SimpleRetry { max_attempts: usize },
    ExponentialBackoff { max_attempts: usize, base_delay: Duration },
    CircuitBreaker { failure_threshold: u32, timeout: Duration },
    Hedging { hedge_delay: Duration, max_hedges: usize },
}

struct RecoveryService {
    inner_service: Box<dyn Service>,
    strategy: RecoveryStrategy,
    attempt_count: AtomicU32,
}

impl RecoveryService {
    fn new(inner_service: Box<dyn Service>, strategy: RecoveryStrategy) -> Self {
        Self {
            inner_service,
            strategy,
            attempt_count: AtomicU32::new(0),
        }
    }
    
    async fn handle_request_with_recovery(&self, request: TestRequest) -> McpResult<TestResponse> {
        match &self.strategy {
            RecoveryStrategy::SimpleRetry { max_attempts } => {
                for attempt in 0..*max_attempts {
                    self.attempt_count.fetch_add(1, Ordering::SeqCst);
                    
                    match self.inner_service.handle_request(request.clone()).await {
                        Ok(response) => return Ok(response),
                        Err(error) => {
                            if attempt == max_attempts - 1 {
                                return Err(error);
                            }
                            sleep(Duration::from_millis(10)).await;
                        }
                    }
                }
                unreachable!()
            }
            
            RecoveryStrategy::ExponentialBackoff { max_attempts, base_delay } => {
                for attempt in 0..*max_attempts {
                    self.attempt_count.fetch_add(1, Ordering::SeqCst);
                    
                    match self.inner_service.handle_request(request.clone()).await {
                        Ok(response) => return Ok(response),
                        Err(error) => {
                            if attempt == max_attempts - 1 {
                                return Err(error);
                            }
                            
                            let delay = *base_delay * 2_u32.pow(attempt as u32);
                            sleep(delay).await;
                        }
                    }
                }
                unreachable!()
            }
            
            _ => {
                // Simplified implementation for other strategies
                self.attempt_count.fetch_add(1, Ordering::SeqCst);
                self.inner_service.handle_request(request).await
            }
        }
    }
    
    fn get_attempt_count(&self) -> u32 {
        self.attempt_count.swap(0, Ordering::SeqCst)
    }
}

// Additional helper traits and types would be implemented here...

trait Service: Send + Sync {
    async fn handle_request(&self, request: TestRequest) -> McpResult<TestResponse>;
}

#[derive(Debug, Clone)]
struct TestRequest {
    id: usize,
    data: String,
}

#[derive(Debug)]
struct TestResponse {
    id: usize,
    result: String,
    service_name: Option<String>,
    warnings: Vec<String>,
}

impl TestResponse {
    fn service_used(&self) -> &str {
        self.service_name.as_deref().unwrap_or("unknown")
    }
    
    fn has_warning(&self, warning: &str) -> bool {
        self.warnings.iter().any(|w| w.contains(warning))
    }
}

fn create_test_request(id: usize) -> TestRequest {
    TestRequest {
        id,
        data: format!("test_data_{}", id),
    }
}

fn create_large_test_request(id: usize) -> TestRequest {
    TestRequest {
        id,
        data: "x".repeat(10000), // 10KB of data
    }
}

fn classify_error(error: &McpError) -> String {
    match error {
        McpError::Transport(_) => "transport".to_string(),
        McpError::Tool(_) => "tool".to_string(),
        McpError::InvalidParams(_) => "validation".to_string(),
        McpError::InvalidResponse(_) => "response".to_string(),
        McpError::MethodNotFound(_) => "method".to_string(),
        _ => "other".to_string(),
    }
}

// Mock implementations for complex components
struct MemoryTracker {
    current_usage: AtomicU64,
    peak_usage: AtomicU64,
}

impl MemoryTracker {
    fn new() -> Self {
        Self {
            current_usage: AtomicU64::new(1024 * 1024), // Start with 1MB baseline
            peak_usage: AtomicU64::new(1024 * 1024),
        }
    }
    
    fn get_memory_usage(&self) -> u64 {
        self.current_usage.load(Ordering::SeqCst)
    }
    
    fn get_peak_memory(&self) -> u64 {
        self.peak_usage.load(Ordering::SeqCst)
    }
}

struct ChaosConfig {
    network_failures: f64,
    memory_pressure: f64,
    cpu_spikes: f64,
    disk_errors: f64,
    service_crashes: f64,
    clock_skew: f64,
}

struct ChaosMonkey {
    config: ChaosConfig,
}

impl ChaosMonkey {
    fn new(config: ChaosConfig) -> Self {
        Self { config }
    }
    
    async fn should_cause_chaos(&self, chaos_type: &str) -> bool {
        let rate = match chaos_type {
            "network" => self.config.network_failures,
            "memory" => self.config.memory_pressure,
            "cpu" => self.config.cpu_spikes,
            "disk" => self.config.disk_errors,
            "crash" => self.config.service_crashes,
            "clock" => self.config.clock_skew,
            _ => 0.0,
        };
        
        rand::random::<f64>() < rate
    }
}

// More complex implementations would continue...

#[tokio::test]
async fn test_error_boundary_isolation() {
    // Test that errors in one component don't affect others
    let components = vec![
        create_error_prone_component("component_1", 0.3),
        create_error_prone_component("component_2", 0.2),
        create_error_prone_component("component_3", 0.1),
    ];
    
    let isolated_system = IsolatedSystem::new(components);
    
    const TOTAL_REQUESTS: usize = 100;
    let mut component_results = HashMap::new();
    
    for i in 0..TOTAL_REQUESTS {
        let component_id = i % 3;
        let request = create_test_request(i);
        
        let result = isolated_system.send_to_component(component_id, request).await;
        
        let entry = component_results.entry(component_id).or_insert((0, 0));
        if result.is_ok() {
            entry.0 += 1; // Success count
        } else {
            entry.1 += 1; // Error count
        }
    }
    
    println!("Error Boundary Isolation Results:");
    for (component_id, (successes, errors)) in component_results {
        println!("Component {}: {} successes, {} errors", component_id, successes, errors);
        
        // Each component should have some successes despite errors in others
        assert!(successes > 0, "Component {} should have some successes", component_id);
    }
}

fn create_error_prone_component(name: &str, error_rate: f64) -> ErrorProneComponent {
    ErrorProneComponent {
        name: name.to_string(),
        error_rate,
        request_count: AtomicU32::new(0),
    }
}

struct ErrorProneComponent {
    name: String,
    error_rate: f64,
    request_count: AtomicU32,
}

impl ErrorProneComponent {
    async fn handle_request(&self, request: TestRequest) -> McpResult<TestResponse> {
        let count = self.request_count.fetch_add(1, Ordering::SeqCst);
        
        if rand::random::<f64>() < self.error_rate {
            Err(McpError::Tool(format!("Error in {}", self.name)))
        } else {
            Ok(TestResponse {
                id: request.id,
                result: format!("processed_by_{}", self.name),
                service_name: Some(self.name.clone()),
                warnings: vec![],
            })
        }
    }
}

struct IsolatedSystem {
    components: Vec<ErrorProneComponent>,
}

impl IsolatedSystem {
    fn new(components: Vec<ErrorProneComponent>) -> Self {
        Self { components }
    }
    
    async fn send_to_component(&self, component_id: usize, request: TestRequest) -> McpResult<TestResponse> {
        if component_id < self.components.len() {
            self.components[component_id].handle_request(request).await
        } else {
            Err(McpError::Tool("Invalid component ID".to_string()))
        }
    }
}