//! Comprehensive integration tests for timeout and cancellation system
//!
//! This test suite validates the production-grade timeout management system
//! with cooperative cancellation, covering all security-critical scenarios.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;

use turbomcp_core::RequestContext;
use turbomcp_server::{
    ServerError,
    config::TimeoutConfig,
    metrics::ServerMetrics,
    timeout::{ToolTimeoutError, ToolTimeoutManager},
};

/// Helper function to create a test timeout manager
fn create_test_timeout_manager() -> ToolTimeoutManager {
    let mut config = TimeoutConfig::default();
    config.tool_execution_timeout = Duration::from_millis(500); // 500ms default timeout
    let metrics = Arc::new(ServerMetrics::new());
    ToolTimeoutManager::new(config, metrics)
}

/// Helper function to create a test context
fn create_test_context() -> RequestContext {
    RequestContext::new()
        .with_user_id("test_user")
        .with_session_id("test_session")
}

#[tokio::test]
async fn test_timeout_manager_basic_execution() {
    let manager = create_test_timeout_manager();

    // Test successful execution within timeout
    let result = manager
        .execute_with_timeout("test_tool", async {
            sleep(Duration::from_millis(100)).await;
            Ok("success")
        })
        .await;

    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "success");
}

#[tokio::test]
async fn test_timeout_manager_timeout_exceeded() {
    let manager = create_test_timeout_manager();

    // Test timeout exceeded scenario
    let result = manager
        .execute_with_timeout("slow_tool", async {
            sleep(Duration::from_millis(1000)).await; // Exceeds 500ms timeout
            Ok("should_not_complete")
        })
        .await;

    assert!(result.is_err());
    match result {
        Err(ToolTimeoutError::Timeout {
            tool_name,
            timeout_duration,
            elapsed,
        }) => {
            assert_eq!(tool_name, "slow_tool");
            assert_eq!(timeout_duration, Duration::from_millis(500));
            assert!(elapsed >= Duration::from_millis(500));
        }
        _ => panic!("Expected ToolTimeoutError::Timeout"),
    }
}

#[tokio::test]
async fn test_cancellation_token_creation_and_return() {
    let manager = create_test_timeout_manager();

    let result = manager
        .execute_with_timeout_and_cancellation("test_tool", async {
            sleep(Duration::from_millis(100)).await;
            Ok("success")
        })
        .await;

    assert!(result.is_ok());
    let (value, cancellation_token) = result.unwrap();
    assert_eq!(value, "success");
    assert!(!cancellation_token.is_cancelled());
}

#[tokio::test]
async fn test_external_cancellation_token() {
    let manager = create_test_timeout_manager();
    let external_token = CancellationToken::new();

    // Start a task with external token
    let token_clone = external_token.clone();
    let task_handle = tokio::spawn(async move {
        manager
            .execute_with_external_token(
                "cancellable_tool",
                async {
                    sleep(Duration::from_millis(1000)).await;
                    Ok("should_be_cancelled")
                },
                token_clone,
            )
            .await
    });

    // Cancel after short delay
    sleep(Duration::from_millis(100)).await;
    external_token.cancel();

    let result = task_handle.await.unwrap();
    assert!(result.is_err());
    match result {
        Err(ToolTimeoutError::Cancelled { tool_name, .. }) => {
            assert_eq!(tool_name, "cancellable_tool");
        }
        _ => panic!("Expected ToolTimeoutError::Cancelled, got {:?}", result),
    }
}

#[tokio::test]
async fn test_timeout_vs_cancellation_priority() {
    let manager = ToolTimeoutManager::new(
        TimeoutConfig {
            request_timeout: Duration::from_secs(30),
            connection_timeout: Duration::from_secs(10),
            keep_alive_timeout: Duration::from_secs(60),
            tool_execution_timeout: Duration::from_millis(200), // Short timeout
            tool_timeouts: HashMap::new(),
        },
        Arc::new(ServerMetrics::new()),
    );

    let cancellation_token = CancellationToken::new();

    // Start task that would timeout
    let token_clone = cancellation_token.clone();
    let task_handle = tokio::spawn(async move {
        manager
            .execute_with_external_token(
                "priority_test",
                async {
                    sleep(Duration::from_millis(1000)).await;
                    Ok("never_reached")
                },
                token_clone,
            )
            .await
    });

    // Cancel before timeout would occur
    sleep(Duration::from_millis(50)).await;
    cancellation_token.cancel();

    let result = task_handle.await.unwrap();
    assert!(result.is_err());
    // Should be cancelled, not timed out
    match result {
        Err(ToolTimeoutError::Cancelled { .. }) => {} // Expected
        Err(ToolTimeoutError::Timeout { .. }) => panic!("Expected cancellation, not timeout"),
        _ => panic!("Expected ToolTimeoutError"),
    }
}

#[tokio::test]
async fn test_operation_error_propagation() {
    let manager = create_test_timeout_manager();

    let result = manager
        .execute_with_timeout::<_, ()>("error_tool", async {
            Err(ServerError::handler("operation failed"))
        })
        .await;

    assert!(result.is_err());
    match result {
        Err(ToolTimeoutError::ServerError(server_error)) => {
            assert!(server_error.to_string().contains("operation failed"));
        }
        _ => panic!("Expected ToolTimeoutError::ServerError"),
    }
}

#[tokio::test]
async fn test_concurrent_timeout_operations() {
    let manager = Arc::new(create_test_timeout_manager());
    let mut handles = Vec::new();

    // Start multiple concurrent operations
    for i in 0..10 {
        let manager_clone = manager.clone();
        let handle = tokio::spawn(async move {
            let delay = Duration::from_millis(50 * i); // Varying delays
            manager_clone
                .execute_with_timeout(&format!("concurrent_tool_{}", i), async move {
                    sleep(delay).await;
                    Ok(format!("result_{}", i))
                })
                .await
        });
        handles.push(handle);
    }

    // Wait for all to complete
    let results: Vec<_> = futures::future::join_all(handles).await;

    // All should succeed (delays are under 500ms timeout)
    for (i, result) in results.into_iter().enumerate() {
        let timeout_result = result.unwrap();
        assert!(timeout_result.is_ok());
        assert_eq!(timeout_result.unwrap(), format!("result_{}", i));
    }
}

#[tokio::test]
async fn test_context_integration_with_cancellation_token() {
    let manager = create_test_timeout_manager();

    let result = manager
        .execute_with_timeout_and_cancellation("context_tool", async {
            let mut ctx = create_test_context();

            // In a real scenario, the routing layer would set this
            let token = CancellationToken::new();
            ctx = ctx.with_cancellation_token(Arc::new(token.clone()));

            // Simulate tool checking cancellation token
            if let Some(token) = &ctx.cancellation_token
                && token.is_cancelled()
            {
                return Err(ServerError::handler("cancelled"));
            }

            sleep(Duration::from_millis(100)).await;

            // Check again after work
            if let Some(token) = &ctx.cancellation_token
                && token.is_cancelled()
            {
                return Err(ServerError::handler("cancelled_after_work"));
            }

            Ok("completed_with_context")
        })
        .await;

    assert!(result.is_ok());
    let (value, _) = result.unwrap();
    assert_eq!(value, "completed_with_context");
}

#[tokio::test]
async fn test_timeout_error_conversion_to_server_error() {
    let timeout_error = ToolTimeoutError::Timeout {
        tool_name: "test_tool".to_string(),
        timeout_duration: Duration::from_millis(500),
        elapsed: Duration::from_millis(600),
    };

    let server_error: ServerError = timeout_error.into();

    // Check that error conversion works correctly
    assert!(server_error.to_string().contains("test_tool"));
    assert!(server_error.to_string().contains("500ms"));
}

#[tokio::test]
async fn test_cancellation_error_conversion_to_server_error() {
    let cancellation_error = ToolTimeoutError::Cancelled {
        tool_name: "cancelled_tool".to_string(),
        elapsed: Duration::from_millis(300),
    };

    let server_error: ServerError = cancellation_error.into();

    // Check that error conversion works correctly
    assert!(server_error.to_string().contains("cancelled_tool"));
}

#[tokio::test]
async fn test_high_load_timeout_management() {
    let manager = Arc::new(create_test_timeout_manager());
    let mut handles = Vec::new();

    // Create high load scenario with mixed success/timeout
    for i in 0..20 {
        let manager_clone = manager.clone();
        let handle = tokio::spawn(async move {
            let delay = if i % 5 == 0 {
                Duration::from_millis(1000) // These will timeout
            } else {
                Duration::from_millis(100) // These will succeed
            };

            manager_clone
                .execute_with_timeout(&format!("load_test_{}", i), async move {
                    sleep(delay).await;
                    Ok(i)
                })
                .await
        });
        handles.push(handle);
    }

    let results: Vec<_> = futures::future::join_all(handles).await;

    let mut successes = 0;
    let mut timeouts = 0;

    for result in results {
        match result.unwrap() {
            Ok(_) => successes += 1,
            Err(ToolTimeoutError::Timeout { .. }) => timeouts += 1,
            Err(e) => panic!("Unexpected error: {:?}", e),
        }
    }

    assert_eq!(successes, 16); // 80% success rate
    assert_eq!(timeouts, 4); // 20% timeout rate
}

#[tokio::test]
async fn test_timeout_manager_drop_cleanup() {
    // Test that dropping the timeout manager doesn't cause issues
    let manager = create_test_timeout_manager();

    let result = manager
        .execute_with_timeout("drop_test", async {
            sleep(Duration::from_millis(100)).await;
            Ok("success")
        })
        .await;

    assert!(result.is_ok());

    // Manager drops here - should be clean
    drop(manager);

    // No panics or resource leaks should occur
}

#[tokio::test]
async fn test_active_executions_monitoring() {
    let manager = Arc::new(create_test_timeout_manager());
    let manager_clone = manager.clone();

    // Start a long-running task
    let task_handle = tokio::spawn(async move {
        manager_clone
            .execute_with_timeout("monitoring_test", async {
                sleep(Duration::from_millis(200)).await;
                Ok("completed")
            })
            .await
    });

    // Check active executions while task is running
    sleep(Duration::from_millis(50)).await;
    let active_executions = manager.get_active_executions().await;
    assert_eq!(active_executions.len(), 1);
    assert_eq!(active_executions[0].tool_name, "monitoring_test");
    assert!(!active_executions[0].cancelled);

    // Wait for completion
    let result = task_handle.await.unwrap();
    assert!(result.is_ok());

    // Check that execution was cleaned up
    let active_executions = manager.get_active_executions().await;
    assert_eq!(active_executions.len(), 0);
}

#[tokio::test]
async fn test_cancel_all_executions() {
    let manager = Arc::new(create_test_timeout_manager());
    let mut handles = Vec::new();

    // Start multiple long-running tasks
    for i in 0..3 {
        let manager_clone = manager.clone();
        let handle = tokio::spawn(async move {
            manager_clone
                .execute_with_timeout(&format!("batch_cancel_test_{}", i), async {
                    sleep(Duration::from_millis(1000)).await;
                    Ok("should_be_cancelled")
                })
                .await
        });
        handles.push(handle);
    }

    // Let tasks start
    sleep(Duration::from_millis(50)).await;

    // Check that all tasks are active
    let active_executions = manager.get_active_executions().await;
    assert_eq!(active_executions.len(), 3);

    // Cancel all executions
    manager.cancel_all_executions().await;

    // Wait for results
    let results: Vec<_> = futures::future::join_all(handles).await;

    // All should be cancelled
    for result in results {
        let timeout_result = result.unwrap();
        assert!(timeout_result.is_err());
        match timeout_result {
            Err(ToolTimeoutError::Cancelled { .. }) => {} // Expected
            other => panic!("Expected cancellation, got {:?}", other),
        }
    }
}
