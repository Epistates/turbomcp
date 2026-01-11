//! Comprehensive test coverage for the lifespan management system

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::time::Duration;

use turbomcp::lifespan::*;
use turbomcp::prelude::*;

/// Test hook for basic functionality
struct TestHook {
    name: String,
    priority: HookPriority,
    execution_count: Arc<AtomicI32>,
    should_fail: Arc<AtomicBool>,
    startup_enabled: bool,
    shutdown_enabled: bool,
    timeout: Option<Duration>,
}

impl TestHook {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            priority: HookPriority::Normal,
            execution_count: Arc::new(AtomicI32::new(0)),
            should_fail: Arc::new(AtomicBool::new(false)),
            startup_enabled: true,
            shutdown_enabled: true,
            timeout: Some(Duration::from_secs(30)),
        }
    }

    fn with_priority(mut self, priority: HookPriority) -> Self {
        self.priority = priority;
        self
    }

    fn startup_only(mut self) -> Self {
        self.startup_enabled = true;
        self.shutdown_enabled = false;
        self
    }

    fn shutdown_only(mut self) -> Self {
        self.startup_enabled = false;
        self.shutdown_enabled = true;
        self
    }

    fn with_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.timeout = timeout;
        self
    }

    fn fail_next_execution(&self) {
        self.should_fail.store(true, Ordering::SeqCst);
    }

    #[allow(dead_code)]
    fn execution_count(&self) -> i32 {
        self.execution_count.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl LifespanHook for TestHook {
    fn name(&self) -> &str {
        &self.name
    }

    fn priority(&self) -> HookPriority {
        self.priority
    }

    async fn execute(&self, _event: LifespanEvent) -> McpResult<()> {
        self.execution_count.fetch_add(1, Ordering::SeqCst);

        if self.should_fail.load(Ordering::SeqCst) {
            self.should_fail.store(false, Ordering::SeqCst);
            return Err(McpError::context(format!(
                "Hook {} intentionally failed",
                self.name
            )));
        }

        // Simulate some work
        tokio::time::sleep(Duration::from_millis(10)).await;
        Ok(())
    }

    fn handles_startup(&self) -> bool {
        self.startup_enabled
    }

    fn handles_shutdown(&self) -> bool {
        self.shutdown_enabled
    }

    fn timeout(&self) -> Option<Duration> {
        self.timeout
    }
}

/// Test basic lifespan manager functionality
#[tokio::test]
async fn test_lifespan_manager_basic() {
    let manager = LifespanManager::new();

    // Test empty manager
    assert_eq!(manager.hook_count().await, 0);
    assert!(manager.get_hook_names().await.is_empty());
    assert!(manager.get_execution_results().await.is_empty());

    // Test executing with no hooks
    let result = manager.execute_hooks(LifespanEvent::Startup).await;
    assert!(result.is_ok());

    // Test registering a hook
    let hook = TestHook::new("test_hook");
    let hook_execution_count = Arc::clone(&hook.execution_count);

    manager.register_hook(Box::new(hook)).await;
    assert_eq!(manager.hook_count().await, 1);

    let hook_names = manager.get_hook_names().await;
    assert_eq!(hook_names.len(), 1);
    assert_eq!(hook_names[0], "test_hook");

    // Test executing startup hooks
    let result = manager.execute_hooks(LifespanEvent::Startup).await;
    assert!(result.is_ok());
    assert_eq!(hook_execution_count.load(Ordering::SeqCst), 1);

    // Test executing shutdown hooks
    let result = manager.execute_hooks(LifespanEvent::Shutdown).await;
    assert!(result.is_ok());
    assert_eq!(hook_execution_count.load(Ordering::SeqCst), 2);

    // Check execution results
    let results = manager.get_execution_results().await;
    assert_eq!(results.len(), 2);
    assert!(results[0].success);
    assert!(results[1].success);
    assert_eq!(results[0].event, LifespanEvent::Startup);
    assert_eq!(results[1].event, LifespanEvent::Shutdown);
}

/// Test hook priority ordering
#[tokio::test]
async fn test_hook_priority_ordering() {
    let manager = LifespanManager::new();

    let _execution_order = Arc::new(tokio::sync::Mutex::new(Vec::<String>::new()));

    // Create hooks with different priorities
    let critical_hook = TestHook::new("critical").with_priority(HookPriority::Critical);
    let high_hook = TestHook::new("high").with_priority(HookPriority::High);
    let normal_hook = TestHook::new("normal").with_priority(HookPriority::Normal);
    let low_hook = TestHook::new("low").with_priority(HookPriority::Low);

    // Register hooks in random order
    manager.register_hook(Box::new(normal_hook)).await;
    manager.register_hook(Box::new(critical_hook)).await;
    manager.register_hook(Box::new(low_hook)).await;
    manager.register_hook(Box::new(high_hook)).await;

    // Hook names should be ordered by priority
    let hook_names = manager.get_hook_names().await;
    assert_eq!(hook_names, vec!["critical", "high", "normal", "low"]);

    // Test startup execution order
    manager.execute_hooks(LifespanEvent::Startup).await.unwrap();

    let results = manager.get_execution_results().await;
    let startup_results: Vec<_> = results
        .iter()
        .filter(|r| r.event == LifespanEvent::Startup)
        .collect();

    assert_eq!(startup_results.len(), 4);
    assert_eq!(startup_results[0].hook_name, "critical");
    assert_eq!(startup_results[1].hook_name, "high");
    assert_eq!(startup_results[2].hook_name, "normal");
    assert_eq!(startup_results[3].hook_name, "low");

    // Clear results and test shutdown order (should be reversed)
    manager.clear_execution_results().await;
    manager
        .execute_hooks(LifespanEvent::Shutdown)
        .await
        .unwrap();

    let results = manager.get_execution_results().await;
    let shutdown_results: Vec<_> = results
        .iter()
        .filter(|r| r.event == LifespanEvent::Shutdown)
        .collect();

    assert_eq!(shutdown_results.len(), 4);
    assert_eq!(shutdown_results[0].hook_name, "low");
    assert_eq!(shutdown_results[1].hook_name, "normal");
    assert_eq!(shutdown_results[2].hook_name, "high");
    assert_eq!(shutdown_results[3].hook_name, "critical");
}

/// Test hook event filtering
#[tokio::test]
async fn test_hook_event_filtering() {
    let manager = LifespanManager::new();

    let startup_hook = TestHook::new("startup_only").startup_only();
    let shutdown_hook = TestHook::new("shutdown_only").shutdown_only();
    let both_hook = TestHook::new("both_events");

    let startup_count = Arc::clone(&startup_hook.execution_count);
    let shutdown_count = Arc::clone(&shutdown_hook.execution_count);
    let both_count = Arc::clone(&both_hook.execution_count);

    manager.register_hook(Box::new(startup_hook)).await;
    manager.register_hook(Box::new(shutdown_hook)).await;
    manager.register_hook(Box::new(both_hook)).await;

    // Execute startup - only startup_only and both_events should run
    manager.execute_hooks(LifespanEvent::Startup).await.unwrap();
    assert_eq!(startup_count.load(Ordering::SeqCst), 1);
    assert_eq!(shutdown_count.load(Ordering::SeqCst), 0);
    assert_eq!(both_count.load(Ordering::SeqCst), 1);

    // Execute shutdown - only shutdown_only and both_events should run
    manager
        .execute_hooks(LifespanEvent::Shutdown)
        .await
        .unwrap();
    assert_eq!(startup_count.load(Ordering::SeqCst), 1);
    assert_eq!(shutdown_count.load(Ordering::SeqCst), 1);
    assert_eq!(both_count.load(Ordering::SeqCst), 2);

    // Check execution results
    let results = manager.get_execution_results().await;
    assert_eq!(results.len(), 4); // 2 for startup, 2 for shutdown

    let startup_results: Vec<_> = results
        .iter()
        .filter(|r| r.event == LifespanEvent::Startup)
        .map(|r| &r.hook_name)
        .collect();
    assert_eq!(startup_results, vec!["startup_only", "both_events"]);

    let shutdown_results: Vec<_> = results
        .iter()
        .filter(|r| r.event == LifespanEvent::Shutdown)
        .map(|r| &r.hook_name)
        .collect();
    assert_eq!(shutdown_results, vec!["both_events", "shutdown_only"]);
}

/// Test hook failure handling
#[tokio::test]
async fn test_hook_failure_handling() {
    let manager = LifespanManager::new();

    let good_hook = TestHook::new("good_hook");
    let bad_hook = TestHook::new("bad_hook");
    let another_good_hook = TestHook::new("another_good");

    bad_hook.fail_next_execution();

    manager.register_hook(Box::new(good_hook)).await;
    manager.register_hook(Box::new(bad_hook)).await;
    manager.register_hook(Box::new(another_good_hook)).await;

    // Execute hooks - should fail due to bad_hook
    let result = manager.execute_hooks(LifespanEvent::Startup).await;
    assert!(result.is_err());

    // Check execution results
    let results = manager.get_execution_results().await;
    assert_eq!(results.len(), 3);

    // First hook should succeed
    assert!(results[0].success);
    assert_eq!(results[0].hook_name, "good_hook");

    // Second hook should fail
    assert!(!results[1].success);
    assert_eq!(results[1].hook_name, "bad_hook");
    assert!(results[1].error.is_some());

    // Third hook should still execute and succeed
    assert!(results[2].success);
    assert_eq!(results[2].hook_name, "another_good");
}

/// Test hook timeout handling
#[tokio::test]
async fn test_hook_timeout() {
    let manager = LifespanManager::new();

    // Create a slow hook that will timeout
    struct SlowHook {
        name: String,
        delay: Duration,
        timeout: Option<Duration>,
    }

    #[async_trait]
    impl LifespanHook for SlowHook {
        fn name(&self) -> &str {
            &self.name
        }

        async fn execute(&self, _event: LifespanEvent) -> McpResult<()> {
            tokio::time::sleep(self.delay).await;
            Ok(())
        }

        fn timeout(&self) -> Option<Duration> {
            self.timeout
        }
    }

    let slow_hook = SlowHook {
        name: "slow_hook".to_string(),
        delay: Duration::from_millis(200),
        timeout: Some(Duration::from_millis(50)), // Will timeout
    };

    manager.register_hook(Box::new(slow_hook)).await;

    let start = std::time::Instant::now();
    let result = manager.execute_hooks(LifespanEvent::Startup).await;
    let duration = start.elapsed();

    // Should fail due to timeout
    assert!(result.is_err());
    // Should complete quickly due to timeout
    assert!(duration < Duration::from_millis(100));

    // Check execution results
    let results = manager.get_execution_results().await;
    assert_eq!(results.len(), 1);
    assert!(!results[0].success);
    assert!(results[0].error.is_some());
    assert!(
        results[0]
            .error
            .as_ref()
            .unwrap()
            .to_string()
            .contains("timed out")
    );
}

/// Test hook with no timeout
#[tokio::test]
async fn test_hook_no_timeout() {
    let manager = LifespanManager::new();

    let hook = TestHook::new("no_timeout").with_timeout(None);
    manager.register_hook(Box::new(hook)).await;

    // Should execute successfully without timeout
    let result = manager.execute_hooks(LifespanEvent::Startup).await;
    assert!(result.is_ok());

    let results = manager.get_execution_results().await;
    assert_eq!(results.len(), 1);
    assert!(results[0].success);
}

/// Test function hook wrapper
#[tokio::test]
async fn test_function_hook() {
    let execution_counter = Arc::new(AtomicI32::new(0));
    let counter_clone = Arc::clone(&execution_counter);

    let func_hook = FunctionHook::new("function_test", move |event| {
        let counter = Arc::clone(&counter_clone);
        Box::pin(async move {
            counter.fetch_add(1, Ordering::SeqCst);
            match event {
                LifespanEvent::Startup => Ok(()),
                LifespanEvent::Shutdown => Ok(()),
            }
        })
    });

    // Test basic properties
    assert_eq!(func_hook.name(), "function_test");
    assert_eq!(func_hook.priority(), HookPriority::Normal);
    assert!(func_hook.handles_startup());
    assert!(func_hook.handles_shutdown());
    assert_eq!(func_hook.timeout(), Some(Duration::from_secs(30)));

    // Test execution
    func_hook.execute(LifespanEvent::Startup).await.unwrap();
    assert_eq!(execution_counter.load(Ordering::SeqCst), 1);

    func_hook.execute(LifespanEvent::Shutdown).await.unwrap();
    assert_eq!(execution_counter.load(Ordering::SeqCst), 2);
}

/// Test function hook configuration
#[tokio::test]
async fn test_function_hook_configuration() {
    let func_hook = FunctionHook::new("configured_hook", |_| Box::pin(async { Ok(()) }))
        .with_priority(HookPriority::Critical)
        .startup_only()
        .with_timeout(Duration::from_secs(10));

    assert_eq!(func_hook.priority(), HookPriority::Critical);
    assert!(func_hook.handles_startup());
    assert!(!func_hook.handles_shutdown());
    assert_eq!(func_hook.timeout(), Some(Duration::from_secs(10)));

    // Test shutdown_only configuration
    let shutdown_hook = FunctionHook::new("shutdown_hook", |_| Box::pin(async { Ok(()) }))
        .shutdown_only()
        .no_timeout();

    assert!(!shutdown_hook.handles_startup());
    assert!(shutdown_hook.handles_shutdown());
    assert_eq!(shutdown_hook.timeout(), None);
}

/// Test the example hooks provided in the module
#[tokio::test]
async fn test_example_hooks() {
    // Test DatabaseHook
    let db_hook = DatabaseHook::new("postgres://localhost/test".to_string());
    assert_eq!(db_hook.name(), "database");
    assert_eq!(db_hook.priority(), HookPriority::Critical);

    let result = db_hook.execute(LifespanEvent::Startup).await;
    assert!(result.is_ok());

    let result = db_hook.execute(LifespanEvent::Shutdown).await;
    assert!(result.is_ok());

    // Test CacheWarmupHook
    let cache_hook = CacheWarmupHook::new(1000);
    assert_eq!(cache_hook.name(), "cache_warmup");
    assert_eq!(cache_hook.priority(), HookPriority::Low);
    assert!(cache_hook.handles_shutdown());

    let result = cache_hook.execute(LifespanEvent::Startup).await;
    assert!(result.is_ok());

    let result = cache_hook.execute(LifespanEvent::Shutdown).await;
    assert!(result.is_ok());

    // Test MetricsHook
    let metrics_hook = MetricsHook;
    assert_eq!(metrics_hook.name(), "metrics");
    assert_eq!(metrics_hook.priority(), HookPriority::High);

    let result = metrics_hook.execute(LifespanEvent::Startup).await;
    assert!(result.is_ok());

    let result = metrics_hook.execute(LifespanEvent::Shutdown).await;
    assert!(result.is_ok());
}

/// Test lifespan event and priority enums
#[tokio::test]
async fn test_enums_and_traits() {
    // Test LifespanEvent
    assert_eq!(LifespanEvent::Startup, LifespanEvent::Startup);
    assert_ne!(LifespanEvent::Startup, LifespanEvent::Shutdown);

    // Test LifespanEvent debug formatting
    assert_eq!(format!("{:?}", LifespanEvent::Startup), "Startup");
    assert_eq!(format!("{:?}", LifespanEvent::Shutdown), "Shutdown");

    // Test HookPriority ordering
    assert!(HookPriority::Critical < HookPriority::High);
    assert!(HookPriority::High < HookPriority::Normal);
    assert!(HookPriority::Normal < HookPriority::Low);

    // Test HookPriority default
    assert_eq!(HookPriority::default(), HookPriority::Normal);

    // Test HookPriority debug formatting
    assert_eq!(format!("{:?}", HookPriority::Critical), "Critical");
    assert_eq!(format!("{:?}", HookPriority::High), "High");
    assert_eq!(format!("{:?}", HookPriority::Normal), "Normal");
    assert_eq!(format!("{:?}", HookPriority::Low), "Low");

    // Test HookPriority values
    assert_eq!(HookPriority::Critical as u32, 0);
    assert_eq!(HookPriority::High as u32, 100);
    assert_eq!(HookPriority::Normal as u32, 500);
    assert_eq!(HookPriority::Low as u32, 900);
}

/// Test HookExecutionResult
#[tokio::test]
async fn test_hook_execution_result() {
    let success_result = HookExecutionResult {
        hook_name: "test_hook".to_string(),
        event: LifespanEvent::Startup,
        success: true,
        duration: Duration::from_millis(100),
        error: None,
    };

    assert_eq!(success_result.hook_name, "test_hook");
    assert_eq!(success_result.event, LifespanEvent::Startup);
    assert!(success_result.success);
    assert_eq!(success_result.duration, Duration::from_millis(100));
    assert!(success_result.error.is_none());

    let error_result = HookExecutionResult {
        hook_name: "failing_hook".to_string(),
        event: LifespanEvent::Shutdown,
        success: false,
        duration: Duration::from_millis(50),
        error: Some(McpError::context("Something went wrong".to_string())),
    };

    assert_eq!(error_result.hook_name, "failing_hook");
    assert_eq!(error_result.event, LifespanEvent::Shutdown);
    assert!(!error_result.success);
    assert!(error_result.error.is_some());

    // Test cloning
    let cloned_result = success_result.clone();
    assert_eq!(cloned_result.hook_name, success_result.hook_name);
    assert_eq!(cloned_result.success, success_result.success);
}

/// Test concurrent hook execution safety
#[tokio::test]
async fn test_concurrent_safety() {
    let manager = Arc::new(LifespanManager::new());

    // Register multiple hooks concurrently
    let mut handles = vec![];
    for i in 0..10 {
        let manager_clone = Arc::clone(&manager);
        let handle = tokio::spawn(async move {
            let hook = TestHook::new(&format!("hook_{i}"));
            manager_clone.register_hook(Box::new(hook)).await;
        });
        handles.push(handle);
    }

    // Wait for all registrations to complete
    for handle in handles {
        handle.await.unwrap();
    }

    assert_eq!(manager.hook_count().await, 10);

    // Execute hooks concurrently
    let manager1 = Arc::clone(&manager);
    let manager2 = Arc::clone(&manager);

    let startup_handle =
        tokio::spawn(async move { manager1.execute_hooks(LifespanEvent::Startup).await });

    let shutdown_handle =
        tokio::spawn(async move { manager2.execute_hooks(LifespanEvent::Shutdown).await });

    let startup_result = startup_handle.await.unwrap();
    let shutdown_result = shutdown_handle.await.unwrap();

    assert!(startup_result.is_ok());
    assert!(shutdown_result.is_ok());

    // Check that we have results from both executions
    let results = manager.get_execution_results().await;
    assert_eq!(results.len(), 20); // 10 hooks * 2 events
}

/// Test lifespan manager default implementation
#[tokio::test]
async fn test_manager_default() {
    let manager = LifespanManager::default();
    assert_eq!(manager.hook_count().await, 0);

    let hook = TestHook::new("default_test");
    manager.register_hook(Box::new(hook)).await;
    assert_eq!(manager.hook_count().await, 1);
}

/// Test execution results management
#[tokio::test]
async fn test_execution_results_management() {
    let manager = LifespanManager::new();

    let hook = TestHook::new("results_test");
    manager.register_hook(Box::new(hook)).await;

    // Execute multiple times
    manager.execute_hooks(LifespanEvent::Startup).await.unwrap();
    manager
        .execute_hooks(LifespanEvent::Shutdown)
        .await
        .unwrap();
    manager.execute_hooks(LifespanEvent::Startup).await.unwrap();

    let results = manager.get_execution_results().await;
    assert_eq!(results.len(), 3);

    // Clear results
    manager.clear_execution_results().await;
    let results = manager.get_execution_results().await;
    assert_eq!(results.len(), 0);

    // Execute again and verify results are recorded
    manager
        .execute_hooks(LifespanEvent::Shutdown)
        .await
        .unwrap();
    let results = manager.get_execution_results().await;
    assert_eq!(results.len(), 1);
}

/// Test complex hook registration and execution scenario
#[tokio::test]
async fn test_complex_scenario() {
    let manager = LifespanManager::new();

    // Create a complex set of hooks with different configurations
    let db_hook = DatabaseHook::new("postgres://localhost/app".to_string());
    let cache_hook = CacheWarmupHook::new(5000);
    let metrics_hook = MetricsHook;

    let custom_hook = FunctionHook::new("custom_initialization", |event| {
        Box::pin(async move {
            match event {
                LifespanEvent::Startup => {
                    // Simulate complex startup logic
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    Ok(())
                }
                LifespanEvent::Shutdown => {
                    // Simulate cleanup
                    tokio::time::sleep(Duration::from_millis(30)).await;
                    Ok(())
                }
            }
        })
    })
    .with_priority(HookPriority::High)
    .with_timeout(Duration::from_secs(5));

    // Register hooks
    manager.register_hook(Box::new(cache_hook)).await;
    manager.register_hook(Box::new(db_hook)).await;
    manager.register_hook(Box::new(metrics_hook)).await;
    manager.register_hook(Box::new(custom_hook)).await;

    assert_eq!(manager.hook_count().await, 4);

    // Execute startup sequence
    let start_time = std::time::Instant::now();
    let result = manager.execute_hooks(LifespanEvent::Startup).await;
    let _startup_duration = start_time.elapsed();

    assert!(result.is_ok());

    // Verify execution order (Critical, High, Normal, Low)
    let results = manager.get_execution_results().await;
    let startup_results: Vec<_> = results
        .iter()
        .filter(|r| r.event == LifespanEvent::Startup)
        .map(|r| &r.hook_name)
        .collect();

    assert_eq!(
        startup_results,
        vec![
            "database",
            "metrics",
            "custom_initialization",
            "cache_warmup"
        ]
    );

    // Execute shutdown sequence
    manager.clear_execution_results().await;
    let result = manager.execute_hooks(LifespanEvent::Shutdown).await;
    assert!(result.is_ok());

    // Verify shutdown order (reversed)
    let results = manager.get_execution_results().await;
    let shutdown_results: Vec<_> = results
        .iter()
        .filter(|r| r.event == LifespanEvent::Shutdown)
        .map(|r| &r.hook_name)
        .collect();

    assert_eq!(
        shutdown_results,
        vec![
            "cache_warmup",
            "custom_initialization",
            "metrics",
            "database"
        ]
    );

    // Verify all executions were successful
    assert!(results.iter().all(|r| r.success));
}
