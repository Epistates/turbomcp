//! Fault tolerance and error injection tests for TurboMCP

use serde_json::json;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::Duration;
use turbomcp_protocol::{Error, ErrorKind, StateManager};
use turbomcp_transport::core::{Transport, TransportConfig, TransportType};
use turbomcp_transport::stdio::StdioTransport;

#[cfg(test)]
mod fault_injection {
    use super::*;

    #[tokio::test]
    async fn test_state_manager_memory_pressure() {
        let state = StateManager::new();

        // Simulate memory pressure by creating many large objects
        let mut handles = vec![];

        for i in 0..100 {
            let state_clone = state.clone();
            let handle = tokio::spawn(async move {
                // Create large data structures
                let large_data = json!({
                    "id": i,
                    "data": "x".repeat(10000),
                    "array": (0..1000).collect::<Vec<i32>>(),
                    "nested": {
                        "deep": {
                            "deeper": {
                                "value": "nested_data".repeat(100)
                            }
                        }
                    }
                });

                state_clone.set(format!("large_key_{}", i), large_data);

                // Simulate some processing time
                tokio::time::sleep(Duration::from_millis(1)).await;

                // Verify data integrity
                assert!(state_clone.contains(&format!("large_key_{}", i)));
            });
            handles.push(handle);
        }

        // Wait for all tasks and verify no panics
        for handle in handles {
            assert!(handle.await.is_ok());
        }

        // Verify final state
        assert_eq!(state.size(), 100);

        // Clean up - should not cause memory issues
        state.clear();
        assert_eq!(state.size(), 0);
    }

    #[tokio::test]
    async fn test_concurrent_state_corruption() {
        let state = Arc::new(StateManager::new());
        let error_count = Arc::new(AtomicUsize::new(0));
        let success_count = Arc::new(AtomicUsize::new(0));

        // Launch many concurrent operations that could cause race conditions
        let mut handles = vec![];

        for thread_id in 0..20 {
            let state_clone = Arc::clone(&state);
            let error_count_clone = Arc::clone(&error_count);
            let success_count_clone = Arc::clone(&success_count);

            let handle = tokio::spawn(async move {
                for i in 0..50 {
                    let key = format!("thread_{}_key_{}", thread_id, i);
                    let value = json!({
                        "thread_id": thread_id,
                        "iteration": i,
                        "timestamp": chrono::Utc::now().to_rfc3339()
                    });

                    // Rapid fire operations that could cause corruption
                    state_clone.set(key.clone(), value.clone());

                    if let Some(retrieved) = state_clone.get(&key) {
                        // Verify data integrity
                        if retrieved == value {
                            success_count_clone.fetch_add(1, Ordering::Relaxed);
                        } else {
                            error_count_clone.fetch_add(1, Ordering::Relaxed);
                        }
                    } else {
                        error_count_clone.fetch_add(1, Ordering::Relaxed);
                    }

                    // Random operations to increase race condition likelihood
                    match fastrand::u32(0..4) {
                        0 => {
                            state_clone.contains(&key);
                        }
                        1 => {
                            state_clone.remove(&key);
                        }
                        2 => {
                            state_clone.size();
                        }
                        3 => {
                            state_clone.list_keys();
                        }
                        _ => unreachable!(),
                    }
                }
            });
            handles.push(handle);
        }

        // Wait for completion
        for handle in handles {
            handle.await.expect("Task should complete without panic");
        }

        let final_errors = error_count.load(Ordering::Relaxed);
        let final_successes = success_count.load(Ordering::Relaxed);

        // Should have very few or no corruption errors
        assert!(
            final_errors < final_successes / 10,
            "Too many data corruption errors: {} errors vs {} successes",
            final_errors,
            final_successes
        );
    }

    #[tokio::test]
    async fn test_transport_resilience() {
        let mut transport = StdioTransport::new();

        // Test invalid configuration handling
        let invalid_config = TransportConfig {
            transport_type: TransportType::Http, // Wrong type for StdioTransport
            ..Default::default()
        };

        let result = transport.configure(invalid_config).await;
        assert!(result.is_err(), "Should reject invalid configuration");

        // Test state consistency after errors
        assert_eq!(transport.transport_type(), TransportType::Stdio);
        let state = transport.state().await;
        assert_eq!(state.to_string(), "disconnected");
    }

    #[test]
    fn test_error_handling_under_stress() {
        // Create many errors concurrently to test error handling robustness
        let handles: Vec<_> = (0..1000)
            .map(|i| {
                std::thread::spawn(move || {
                    let error = match i % 5 {
                        0 => Error::new(ErrorKind::Transport, format!("Transport error {}", i)),
                        1 => Error::new(ErrorKind::Protocol, format!("Protocol error {}", i)),
                        2 => Error::new(
                            ErrorKind::Serialization,
                            format!("Serialization error {}", i),
                        ),
                        3 => Error::new(
                            ErrorKind::Configuration,
                            format!("Configuration error {}", i),
                        ),
                        4 => Error::new(ErrorKind::Internal, format!("Internal error {}", i)),
                        _ => unreachable!(),
                    };

                    // Add context and verify it doesn't cause issues
                    let contextual_error = error
                        .with_context("thread_id", i)
                        .with_context("operation", "stress_test");

                    // Verify error can be displayed
                    let error_string = contextual_error.to_string();
                    assert!(!error_string.is_empty());
                    assert!(error_string.contains(&format!("error {}", i)));

                    contextual_error
                })
            })
            .collect();

        // Collect all results and verify no panics
        for handle in handles {
            let error = handle.join().expect("Thread should not panic");
            assert!(!error.to_string().is_empty());
        }
    }

    #[test]
    fn test_json_parsing_edge_cases() {
        // Store deeply nested structures in variables to avoid temporary value issues
        let deep_brace = "{".repeat(1000) + &"}".repeat(1000);
        let deep_bracket = "[".repeat(1000) + &"]".repeat(1000);

        let edge_cases = vec![
            // Empty cases
            "",
            "{}",
            "[]",
            "null",
            // Malformed JSON
            "{",
            "}",
            "[",
            "]",
            "{{",
            "}}",
            "{key: value}",
            "{'key': 'value'}",
            // Very large numbers
            "999999999999999999999999999999999999999999",
            "-999999999999999999999999999999999999999999",
            // Unicode edge cases
            "\u{0000}",
            "\u{FFFF}",
            "\"\\u0000\"",
            "\"\\uFFFF\"",
            // Deeply nested structures
            deep_brace.as_str(),
            deep_bracket.as_str(),
        ];

        for (i, case) in edge_cases.iter().enumerate() {
            let result: std::result::Result<serde_json::Value, _> = serde_json::from_str(case);

            // We don't expect all to succeed, but none should cause panics
            match result {
                Ok(_) => {
                    // Valid JSON is fine
                }
                Err(_) => {
                    // Invalid JSON should produce proper errors
                }
            }

            // Edge case handled without panic - validated by reaching this point
        }
    }

    #[tokio::test]
    async fn test_resource_exhaustion() {
        let state = StateManager::new();

        // Simulate resource exhaustion by creating many operations
        let mut operation_count = 0;
        let max_operations = 10000;

        for i in 0..max_operations {
            // Create increasingly complex data
            let complexity_factor = (i / 100) + 1;
            let value = json!({
                "id": i,
                "data": "x".repeat(complexity_factor * 10),
                "nested": (0..complexity_factor).map(|j| {
                    json!({
                        "index": j,
                        "value": "nested".repeat(j + 1)
                    })
                }).collect::<Vec<_>>()
            });

            state.set(format!("resource_key_{}", i), value);
            operation_count += 1;

            // Periodically check system health
            if i % 1000 == 0 {
                // Test that basic operations still work
                assert!(state.contains(&format!("resource_key_{}", i)));

                // Simulate memory pressure relief
                if i > 5000 {
                    // Remove some old entries
                    for j in (i - 1000)..i {
                        if j % 3 == 0 {
                            state.remove(&format!("resource_key_{}", j));
                        }
                    }
                }
            }
        }

        assert!(operation_count == max_operations);

        // Final cleanup should work without issues
        state.clear();
        assert_eq!(state.size(), 0);
    }

    #[tokio::test]
    async fn test_timeout_handling() {
        // Test operations that should timeout gracefully

        // Simulate a long-running operation
        let long_operation = async {
            tokio::time::sleep(Duration::from_secs(10)).await;
            "completed"
        };

        // Test timeout behavior
        let timeout_result = tokio::time::timeout(Duration::from_millis(100), long_operation).await;

        assert!(timeout_result.is_err(), "Operation should timeout");

        // Verify system is still responsive after timeout
        let state = StateManager::new();
        state.set("after_timeout".to_string(), json!("still_working"));
        assert_eq!(state.get("after_timeout"), Some(json!("still_working")));
    }

    #[test]
    fn test_stack_overflow_protection() {
        // Test deeply nested JSON that could cause stack overflow
        fn create_deep_json(depth: usize) -> serde_json::Value {
            if depth == 0 {
                json!("leaf")
            } else {
                json!({
                    "level": depth,
                    "nested": create_deep_json(depth - 1)
                })
            }
        }

        // Test various depths (keeping within serde_json limits)
        for depth in [10, 50, 100].iter() {
            let deep_json = create_deep_json(*depth);

            // Should be able to serialize/deserialize without stack overflow
            let serialized = serde_json::to_string(&deep_json).expect("Should serialize");
            let deserialized_result: Result<serde_json::Value, _> =
                serde_json::from_str(&serialized);

            // For deep nesting, we expect it might fail due to recursion limits, which is acceptable
            if *depth <= 100 {
                deserialized_result.expect("Should deserialize within reasonable limits");
            } else {
                // For very deep nesting, we just verify it doesn't crash
                let _ = deserialized_result;
            }

            // Test with state manager
            let state = StateManager::new();
            state.set(format!("deep_key_{}", depth), deep_json);
            assert!(state.contains(&format!("deep_key_{}", depth)));
        }
    }

    #[test]
    fn test_panic_recovery() {
        // Test that panics in one thread don't affect others
        use std::panic;

        let state = Arc::new(StateManager::new());

        // Thread that will panic
        let state_clone = Arc::clone(&state);
        let panicking_handle = std::thread::spawn(move || {
            // Set some data first
            state_clone.set("before_panic".to_string(), json!("set_before"));

            // Cause a controlled panic
            panic!("Intentional panic for testing");
        });

        // Thread that continues working
        let state_clone2 = Arc::clone(&state);
        let working_handle = std::thread::spawn(move || {
            // This should work despite the other thread panicking
            state_clone2.set("after_panic".to_string(), json!("still_working"));
            state_clone2.get("after_panic")
        });

        // The panicking thread should fail
        assert!(panicking_handle.join().is_err());

        // The working thread should succeed
        let result = working_handle
            .join()
            .expect("Working thread should succeed");
        assert_eq!(result, Some(json!("still_working")));

        // State should still be accessible
        assert!(state.contains("before_panic"));
        assert!(state.contains("after_panic"));
        assert_eq!(state.size(), 2);
    }
}
