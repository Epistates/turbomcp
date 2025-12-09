//! Concurrency and Message Ordering Tests
//!
//! These tests verify thread safety and message ordering guarantees
//! for the protocol types under concurrent access.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::thread;
use turbomcp_protocol::types::*;

/// Test concurrent serialization/deserialization doesn't cause data races
#[test]
fn test_concurrent_serialization_safety() {
    let tool = Arc::new(Tool {
        name: "concurrent_test".to_string(),
        title: Some("Concurrent Test Tool".to_string()),
        description: Some("Test tool for concurrent serialization".to_string()),
        input_schema: ToolInputSchema::empty(),
        output_schema: None,
        execution: Some(ToolExecution {
            task_support: Some(TaskSupportMode::Optional),
        }),
        annotations: None,
        #[cfg(feature = "mcp-icons")]
        icons: None,
        meta: None,
    });

    let handles: Vec<_> = (0..100)
        .map(|i| {
            let tool_clone = Arc::clone(&tool);
            thread::spawn(move || {
                // Concurrent serialization
                let json = serde_json::to_string(&*tool_clone)
                    .unwrap_or_else(|_| panic!("Thread {} failed to serialize", i));

                // Concurrent deserialization
                let deserialized: Tool = serde_json::from_str(&json)
                    .unwrap_or_else(|_| panic!("Thread {} failed to deserialize", i));

                // Verify data integrity
                assert_eq!(deserialized.name, "concurrent_test");
                assert_eq!(
                    deserialized.execution.as_ref().unwrap().task_support,
                    Some(TaskSupportMode::Optional)
                );

                i
            })
        })
        .collect();

    // Collect all results
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();

    // Verify all 100 threads completed
    assert_eq!(results.len(), 100);

    println!("✓ Concurrent serialization safety test passed");
}

/// Test message ID uniqueness under concurrent generation
#[test]
fn test_concurrent_message_id_uniqueness() {
    let ids = Arc::new(Mutex::new(HashSet::new()));

    let handles: Vec<_> = (0..50)
        .map(|thread_num| {
            let ids_clone = Arc::clone(&ids);
            thread::spawn(move || {
                for i in 0..100 {
                    // Generate unique request ID
                    let request_id = format!("req-{}-{}", thread_num, i);

                    let mut ids_lock = ids_clone.lock().unwrap();
                    assert!(
                        ids_lock.insert(request_id.clone()),
                        "Duplicate request ID generated: {}",
                        request_id
                    );
                }
            })
        })
        .collect();

    for handle in handles {
        handle.join().unwrap();
    }

    let final_ids = ids.lock().unwrap();
    assert_eq!(final_ids.len(), 5000); // 50 threads * 100 IDs each

    println!("✓ Concurrent message ID uniqueness test passed");
}

/// Test JSON-RPC request/response correlation under concurrent load
#[test]
fn test_concurrent_request_response_correlation() {
    let pending_requests: Arc<Mutex<HashMap<i32, String>>> = Arc::new(Mutex::new(HashMap::new()));
    let completed_responses: Arc<Mutex<Vec<(i32, String)>>> = Arc::new(Mutex::new(Vec::new()));

    // Simulate concurrent request creation
    let request_handles: Vec<_> = (0..50)
        .map(|i| {
            let pending = Arc::clone(&pending_requests);
            thread::spawn(move || {
                let request = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": i,
                    "method": "tools/call",
                    "params": {"name": format!("tool_{}", i)}
                });

                let mut pending_lock = pending.lock().unwrap();
                pending_lock.insert(i, serde_json::to_string(&request).unwrap());
            })
        })
        .collect();

    for handle in request_handles {
        handle.join().unwrap();
    }

    // Simulate concurrent response processing
    let response_handles: Vec<_> = (0..50)
        .map(|i| {
            let pending = Arc::clone(&pending_requests);
            let completed = Arc::clone(&completed_responses);
            thread::spawn(move || {
                let response = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": i,
                    "result": {"status": "success", "tool": format!("tool_{}", i)}
                });

                // Correlate response with request
                let pending_lock = pending.lock().unwrap();
                assert!(
                    pending_lock.contains_key(&i),
                    "Response {} has no matching request",
                    i
                );
                drop(pending_lock);

                let mut completed_lock = completed.lock().unwrap();
                completed_lock.push((i, serde_json::to_string(&response).unwrap()));
            })
        })
        .collect();

    for handle in response_handles {
        handle.join().unwrap();
    }

    let completed = completed_responses.lock().unwrap();
    assert_eq!(completed.len(), 50);

    println!("✓ Concurrent request/response correlation test passed");
}

/// Test tool list serialization under concurrent access
#[test]
fn test_concurrent_tool_list_access() {
    // Create a shared tool list
    let tools: Arc<Vec<Tool>> = Arc::new(
        (0..100)
            .map(|i| Tool {
                name: format!("tool_{}", i),
                title: Some(format!("Tool {}", i)),
                description: Some(format!("Description for tool {}", i)),
                input_schema: ToolInputSchema::empty(),
                output_schema: None,
                execution: if i % 2 == 0 {
                    Some(ToolExecution {
                        task_support: Some(TaskSupportMode::Optional),
                    })
                } else {
                    None
                },
                annotations: None,
                #[cfg(feature = "mcp-icons")]
                icons: None,
                meta: None,
            })
            .collect(),
    );

    // Concurrent reads
    let read_handles: Vec<_> = (0..50)
        .map(|thread_id| {
            let tools_clone = Arc::clone(&tools);
            thread::spawn(move || {
                for i in 0..100 {
                    let tool_index = (thread_id * 2 + i) % 100;
                    let tool = &tools_clone[tool_index];

                    // Serialize and verify
                    let json = serde_json::to_string(tool).unwrap();
                    let restored: Tool = serde_json::from_str(&json).unwrap();

                    assert_eq!(restored.name, format!("tool_{}", tool_index));
                }
            })
        })
        .collect();

    for handle in read_handles {
        handle.join().unwrap();
    }

    println!("✓ Concurrent tool list access test passed");
}

/// Test notification ordering under concurrent emission
#[test]
fn test_concurrent_notification_ordering() {
    use std::time::Instant;

    #[derive(Debug)]
    struct TimestampedNotification {
        thread_id: usize,
        sequence: usize,
        #[allow(dead_code)]
        timestamp: Instant,
    }

    let notifications: Arc<Mutex<Vec<TimestampedNotification>>> = Arc::new(Mutex::new(Vec::new()));

    let handles: Vec<_> = (0..10)
        .map(|thread_id| {
            let notifications_clone = Arc::clone(&notifications);
            thread::spawn(move || {
                for seq in 0..100 {
                    let notification = TimestampedNotification {
                        thread_id,
                        sequence: seq,
                        timestamp: Instant::now(),
                    };

                    let mut lock = notifications_clone.lock().unwrap();
                    lock.push(notification);
                }
            })
        })
        .collect();

    for handle in handles {
        handle.join().unwrap();
    }

    let final_notifications = notifications.lock().unwrap();
    assert_eq!(final_notifications.len(), 1000); // 10 threads * 100 notifications

    // Verify per-thread ordering (notifications from same thread should be in order)
    let mut per_thread: HashMap<usize, Vec<&TimestampedNotification>> = HashMap::new();

    for notification in final_notifications.iter() {
        per_thread
            .entry(notification.thread_id)
            .or_default()
            .push(notification);
    }

    for (thread_id, thread_notifications) in per_thread {
        for window in thread_notifications.windows(2) {
            assert!(
                window[0].sequence <= window[1].sequence,
                "Thread {} has out-of-order notifications: {} > {}",
                thread_id,
                window[0].sequence,
                window[1].sequence
            );
        }
    }

    println!("✓ Concurrent notification ordering test passed");
}

/// Test JSON-RPC batch processing under concurrent load
#[test]
fn test_concurrent_batch_processing() {
    let processed_batches: Arc<Mutex<Vec<usize>>> = Arc::new(Mutex::new(Vec::new()));

    let handles: Vec<_> = (0..20)
        .map(|batch_id| {
            let processed = Arc::clone(&processed_batches);
            thread::spawn(move || {
                // Create a batch of requests
                let batch: Vec<serde_json::Value> = (0..10)
                    .map(|i| {
                        serde_json::json!({
                            "jsonrpc": "2.0",
                            "id": format!("{}-{}", batch_id, i),
                            "method": "tools/call",
                            "params": {"name": format!("tool_{}", i)}
                        })
                    })
                    .collect();

                // Serialize the batch
                let batch_json = serde_json::to_string(&batch).unwrap();

                // Deserialize and verify
                let restored: Vec<serde_json::Value> = serde_json::from_str(&batch_json).unwrap();
                assert_eq!(restored.len(), 10);

                let mut lock = processed.lock().unwrap();
                lock.push(batch_id);
            })
        })
        .collect();

    for handle in handles {
        handle.join().unwrap();
    }

    let processed = processed_batches.lock().unwrap();
    assert_eq!(processed.len(), 20);

    println!("✓ Concurrent batch processing test passed");
}

/// Test protocol type cloning under concurrent access
#[test]
fn test_concurrent_type_cloning() {
    let original = Arc::new(ServerCapabilities {
        tools: Some(ToolsCapabilities {
            list_changed: Some(true),
        }),
        prompts: Some(PromptsCapabilities {
            list_changed: Some(true),
        }),
        resources: Some(ResourcesCapabilities {
            list_changed: Some(true),
            subscribe: Some(true),
        }),
        logging: Some(LoggingCapabilities {}),
        experimental: None,
        completions: None,
        tasks: None,
    });

    let handles: Vec<_> = (0..100)
        .map(|_| {
            let original_clone = Arc::clone(&original);
            thread::spawn(move || {
                // Clone the capabilities
                let cloned = (*original_clone).clone();

                // Verify the clone is correct
                assert!(cloned.tools.is_some());
                assert!(cloned.prompts.is_some());
                assert!(cloned.resources.is_some());
            })
        })
        .collect();

    for handle in handles {
        handle.join().unwrap();
    }

    println!("✓ Concurrent type cloning test passed");
}
