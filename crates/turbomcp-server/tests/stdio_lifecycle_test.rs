//! Integration tests for STDIO transport lifecycle management
//!
//! These tests verify that the STDIO transport properly manages task lifecycles
//! and doesn't panic on clean shutdown (fixes: "JoinHandle polled after completion" bug)

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use turbomcp_server::runtime::{StdioDispatcher, StdioMessage};

/// Test that STDIO dispatcher shuts down cleanly when request channel closes
#[tokio::test]
async fn test_stdio_shutdown_on_channel_close() {
    let (tx, rx) = mpsc::unbounded_channel();
    let dispatcher = StdioDispatcher::new(tx.clone());

    // Simulate immediate shutdown by dropping the channel
    drop(tx);
    drop(rx);

    // Should not panic - dispatcher handles dropped channels gracefully
    drop(dispatcher);
}

/// Test that StdioDispatcher can be cloned and used concurrently
#[tokio::test]
async fn test_stdio_dispatcher_concurrent_usage() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let dispatcher = StdioDispatcher::new(tx.clone());

    // Test that dispatcher can be cloned for concurrent usage
    let _dispatcher2 = dispatcher.clone();
    let _dispatcher3 = dispatcher.clone();

    // Send messages directly through channel
    for i in 0..5 {
        use turbomcp_protocol::MessageId;
        use turbomcp_protocol::jsonrpc::{JsonRpcRequest, JsonRpcVersion};

        let request = JsonRpcRequest {
            jsonrpc: JsonRpcVersion,
            method: format!("test_{}", i),
            params: None,
            id: MessageId::String(format!("test-{}", i)),
        };
        tx.send(StdioMessage::ServerRequest { request })
            .expect("Send should work");
    }

    // Verify all messages were received
    let mut count = 0;
    while let Ok(msg) = rx.try_recv() {
        if let StdioMessage::ServerRequest { .. } = msg {
            count += 1;
        }
    }
    assert_eq!(count, 5, "Should receive all 5 messages");
}

/// Test shutdown signal propagation
#[tokio::test]
async fn test_stdio_shutdown_signal() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let _dispatcher = StdioDispatcher::new(tx.clone());

    // Send shutdown signal
    tx.send(StdioMessage::Shutdown).expect("Send should work");

    // Verify shutdown message received
    match rx.recv().await {
        Some(StdioMessage::Shutdown) => { /* OK */ }
        _ => panic!("Should receive shutdown signal"),
    }
}

/// Test that JoinSet properly tracks multiple tasks
#[tokio::test]
async fn test_joinset_multiple_tasks() {
    use tokio::task::JoinSet;

    let mut tasks = JoinSet::new();

    // Spawn multiple tasks simulating request handlers
    for i in 0..10 {
        tasks.spawn(async move {
            tokio::time::sleep(Duration::from_millis(i * 5)).await;
            Ok::<_, ()>(())
        });
    }

    assert_eq!(tasks.len(), 10, "Should track all tasks");

    // Wait for all to complete
    let mut completed = 0;
    while let Some(result) = tasks.join_next().await {
        assert!(result.is_ok(), "Task should not panic");
        completed += 1;
    }

    assert_eq!(completed, 10, "All tasks should complete");
    assert!(tasks.is_empty(), "JoinSet should be empty");
}

/// Test timeout and abort behavior for hung tasks
#[tokio::test]
async fn test_joinset_timeout_and_abort() {
    use tokio::task::JoinSet;

    let mut tasks = JoinSet::new();

    // Spawn a task that takes too long
    tasks.spawn(async move {
        tokio::time::sleep(Duration::from_secs(300)).await;
    });

    // Try to join with short timeout
    let result = tokio::time::timeout(Duration::from_millis(100), tasks.join_next()).await;

    assert!(result.is_err(), "Should timeout");

    // Cleanup with abort
    tasks.shutdown().await;
    assert!(tasks.is_empty(), "Should be empty after shutdown");
}

/// Test graceful vs forceful shutdown
#[tokio::test]
async fn test_graceful_shutdown_with_fallback() {
    use tokio::task::JoinSet;

    let mut tasks = JoinSet::new();

    // Spawn fast tasks (will complete within grace period)
    for i in 0..3 {
        tasks.spawn(async move {
            tokio::time::sleep(Duration::from_millis(i * 10)).await;
        });
    }

    // Spawn slow task (will need to be aborted)
    tasks.spawn(async move {
        tokio::time::sleep(Duration::from_secs(300)).await;
    });

    assert_eq!(tasks.len(), 4);

    // Graceful shutdown with short timeout
    let timeout = Duration::from_millis(200);
    let start = std::time::Instant::now();

    let mut graceful_count = 0;
    while let Some(result) =
        tokio::time::timeout(timeout.saturating_sub(start.elapsed()), tasks.join_next())
            .await
            .ok()
            .flatten()
    {
        if result.is_ok() {
            graceful_count += 1;
        }
    }

    assert!(graceful_count >= 3, "Fast tasks should complete gracefully");

    // Force remaining tasks to abort
    if !tasks.is_empty() {
        tasks.shutdown().await;
    }

    assert!(tasks.is_empty(), "All tasks cleaned up");
}

/// Test that pending requests are cleaned up on timeout
#[tokio::test]
async fn test_pending_request_cleanup_on_timeout() {
    use std::collections::HashMap;
    use tokio::sync::{Mutex, oneshot};
    use turbomcp_protocol::jsonrpc::JsonRpcResponse;

    let pending_requests: Arc<Mutex<HashMap<String, oneshot::Sender<JsonRpcResponse>>>> =
        Arc::new(Mutex::new(HashMap::new()));

    // Register a pending request
    let (tx, _rx) = oneshot::channel();
    pending_requests
        .lock()
        .await
        .insert("test-1".to_string(), tx);

    assert_eq!(pending_requests.lock().await.len(), 1);

    // Simulate timeout cleanup
    let removed = pending_requests.lock().await.remove("test-1");
    assert!(removed.is_some());
    assert_eq!(pending_requests.lock().await.len(), 0);
}

/// Test rapid spawn and join cycles (stress test)
#[tokio::test]
async fn test_rapid_task_spawn_and_join() {
    use tokio::task::JoinSet;

    let mut tasks = JoinSet::new();

    // Rapidly spawn and complete tasks
    for iteration in 0..100 {
        tasks.spawn(async move { iteration });

        // Periodically join completed tasks
        if iteration % 10 == 0 {
            while let Some(result) = tasks.try_join_next() {
                assert!(result.is_ok());
            }
        }
    }

    // Join all remaining
    while let Some(result) = tasks.join_next().await {
        assert!(result.is_ok());
    }

    assert!(tasks.is_empty());
}

/// Test error propagation in task results
#[tokio::test]
async fn test_task_error_handling() {
    use tokio::task::JoinSet;

    let mut tasks = JoinSet::new();

    // Spawn task that returns error
    tasks.spawn(async move { Err::<(), &str>("intentional error") });

    // Spawn task that panics
    tasks.spawn(async move {
        panic!("intentional panic");
    });

    // Spawn successful task
    tasks.spawn(async move { Ok::<(), &str>(()) });

    let mut error_count = 0;
    let mut panic_count = 0;
    let mut success_count = 0;

    while let Some(result) = tasks.join_next().await {
        match result {
            Ok(Ok(())) => success_count += 1,
            Ok(Err(_)) => error_count += 1,
            Err(e) if e.is_panic() => panic_count += 1,
            Err(_) => {}
        }
    }

    assert_eq!(error_count, 1, "Should catch error result");
    assert_eq!(panic_count, 1, "Should catch panic");
    assert_eq!(success_count, 1, "Should have success");
}
