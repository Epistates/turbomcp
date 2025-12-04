//! Comprehensive integration tests for Tasks API (MCP 2025-11-25 draft - SEP-1686)
//!
//! This test suite validates the complete task lifecycle including:
//! - Task creation through tool augmentation
//! - Task status retrieval (tasks/get)
//! - Blocking result retrieval (tasks/result)
//! - Task listing (tasks/list)
//! - Task cancellation (tasks/cancel)
//! - Error scenarios and edge cases

#![cfg(feature = "mcp-tasks")]

use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

use serde_json::json;
use turbomcp_protocol::{
    RequestContext,
    jsonrpc::{JsonRpcRequest, JsonRpcVersion},
    types::{RequestId, TaskMetadata, TaskStatus},
};
use turbomcp_server::{
    config::ServerConfig, metrics::ServerMetrics, registry::HandlerRegistry,
    routing::RequestRouter, task_storage::TaskStorage,
};

/// Helper function to create a test router with task storage
fn create_test_router() -> Arc<RequestRouter> {
    let registry = Arc::new(HandlerRegistry::new());
    let metrics = Arc::new(ServerMetrics::new());
    let task_storage = Arc::new(TaskStorage::new(Duration::from_secs(3600)));

    Arc::new(RequestRouter::new(
        registry,
        metrics,
        ServerConfig::default(),
        Some(task_storage),
    ))
}

/// Helper function to create a test context
fn create_test_context(router: &RequestRouter) -> RequestContext {
    router.create_context(None, None, None)
}

// ============================================================================
// Task Status Retrieval Tests (tasks/get)
// ============================================================================

#[tokio::test]
async fn test_tasks_get_retrieves_task_status() {
    let router = create_test_router();

    // Create a task directly in storage
    let task_metadata = TaskMetadata {
        ttl: Some(3_600_000), // 1 hour TTL
    };

    let task_id = router
        .get_task_storage()
        .expect("Task storage should be available")
        .create_task(task_metadata, None)
        .expect("Task creation should succeed");

    // Retrieve task status via tasks/get
    let request = JsonRpcRequest {
        jsonrpc: JsonRpcVersion,
        id: RequestId::String("get-1".to_string()),
        method: "tasks/get".to_string(),
        params: Some(json!({
            "taskId": task_id
        })),
    };

    let ctx = create_test_context(&router);
    let response = router.route(request, ctx).await;

    assert!(response.result().is_some());
    let result = response.result().unwrap();
    assert_eq!(result["taskId"], task_id);
    assert_eq!(result["status"], "working");
}

#[tokio::test]
async fn test_tasks_get_returns_error_for_nonexistent_task() {
    let router = create_test_router();

    let request = JsonRpcRequest {
        jsonrpc: JsonRpcVersion,
        id: RequestId::String("get-2".to_string()),
        method: "tasks/get".to_string(),
        params: Some(json!({
            "taskId": "nonexistent-task-id"
        })),
    };

    let ctx = create_test_context(&router);
    let response = router.route(request, ctx).await;

    assert!(response.error().is_some());
    let error = response.error().unwrap();
    assert!(error.message.contains("not found"));
}

// ============================================================================
// Task Listing Tests (tasks/list)
// ============================================================================

#[tokio::test]
async fn test_tasks_list_returns_all_tasks() {
    let router = create_test_router();

    // Create multiple tasks
    let storage = router.get_task_storage().unwrap();
    for _ in 0..3 {
        let metadata = TaskMetadata {
            ttl: Some(3_600_000),
        };
        storage.create_task(metadata, None).unwrap();
    }

    // List all tasks
    let request = JsonRpcRequest {
        jsonrpc: JsonRpcVersion,
        id: RequestId::String("list-1".to_string()),
        method: "tasks/list".to_string(),
        params: Some(json!({})),
    };

    let ctx = create_test_context(&router);
    let response = router.route(request, ctx).await;

    assert!(response.result().is_some());
    let result = response.result().unwrap();
    assert!(result["tasks"].is_array());
    let tasks = result["tasks"].as_array().unwrap();
    assert_eq!(tasks.len(), 3);
}

#[tokio::test]
async fn test_tasks_list_returns_empty_when_no_tasks() {
    let router = create_test_router();

    let request = JsonRpcRequest {
        jsonrpc: JsonRpcVersion,
        id: RequestId::String("list-2".to_string()),
        method: "tasks/list".to_string(),
        params: Some(json!({})),
    };

    let ctx = create_test_context(&router);
    let response = router.route(request, ctx).await;

    assert!(response.result().is_some());
    let result = response.result().unwrap();
    assert!(result["tasks"].is_array());
    let tasks = result["tasks"].as_array().unwrap();
    assert_eq!(tasks.len(), 0);
}

// ============================================================================
// Task Cancellation Tests (tasks/cancel)
// ============================================================================

#[tokio::test]
async fn test_tasks_cancel_cancels_pending_task() {
    let router = create_test_router();

    // Create a task
    let storage = router.get_task_storage().unwrap();
    let metadata = TaskMetadata {
        ttl: Some(3_600_000),
    };
    let task_id = storage.create_task(metadata, None).unwrap();

    // Cancel the task
    let request = JsonRpcRequest {
        jsonrpc: JsonRpcVersion,
        id: RequestId::String("cancel-1".to_string()),
        method: "tasks/cancel".to_string(),
        params: Some(json!({
            "taskId": task_id
        })),
    };

    let ctx = create_test_context(&router);
    let response = router.route(request, ctx).await;

    assert!(response.result().is_some());
    let result = response.result().unwrap();
    assert_eq!(result["taskId"], task_id);
    assert_eq!(result["status"], "cancelled");
}

#[tokio::test]
async fn test_tasks_cancel_returns_error_for_completed_task() {
    let router = create_test_router();

    // Create and complete a task
    let storage = router.get_task_storage().unwrap();
    let metadata = TaskMetadata {
        ttl: Some(3_600_000),
    };
    let task_id = storage.create_task(metadata, None).unwrap();
    storage
        .complete_task(&task_id, json!({"result": "success"}), None)
        .unwrap();

    // Attempt to cancel
    let request = JsonRpcRequest {
        jsonrpc: JsonRpcVersion,
        id: RequestId::String("cancel-2".to_string()),
        method: "tasks/cancel".to_string(),
        params: Some(json!({
            "taskId": task_id
        })),
    };

    let ctx = create_test_context(&router);
    let response = router.route(request, ctx).await;

    assert!(response.error().is_some());
    let error = response.error().unwrap();
    assert!(
        error.message.contains("Cannot cancel")
            || error.message.contains("already in terminal state")
    );
}

// ============================================================================
// Blocking Result Retrieval Tests (tasks/result)
// ============================================================================

#[tokio::test]
async fn test_tasks_result_returns_completed_result() {
    let router = create_test_router();

    // Create and complete a task
    let storage = router.get_task_storage().unwrap();
    let metadata = TaskMetadata {
        ttl: Some(3_600_000),
    };
    let task_id = storage.create_task(metadata, None).unwrap();
    let result_value = json!({"answer": 42, "status": "success"});
    storage
        .complete_task(&task_id, result_value.clone(), None)
        .unwrap();

    // Get result via tasks/result
    let request = JsonRpcRequest {
        jsonrpc: JsonRpcVersion,
        id: RequestId::String("result-1".to_string()),
        method: "tasks/result".to_string(),
        params: Some(json!({
            "taskId": task_id
        })),
    };

    let ctx = create_test_context(&router);
    let response = router.route(request, ctx).await;

    assert!(response.result().is_some());
    let result = response.result().unwrap();
    assert_eq!(result["answer"], 42);
    assert_eq!(result["status"], "success");
}

#[tokio::test]
async fn test_tasks_result_blocks_until_completion() {
    let router = create_test_router();

    // Create a task
    let storage = router.get_task_storage().unwrap();
    let metadata = TaskMetadata {
        ttl: Some(3_600_000),
    };
    let task_id = storage.create_task(metadata, None).unwrap();

    // Spawn a task to complete it after delay
    let storage_clone = storage.clone();
    let task_id_clone = task_id.clone();
    tokio::spawn(async move {
        sleep(Duration::from_millis(100)).await;
        storage_clone
            .complete_task(&task_id_clone, json!({"delayed": true}), None)
            .unwrap();
    });

    // Request result (should block)
    let request = JsonRpcRequest {
        jsonrpc: JsonRpcVersion,
        id: RequestId::String("result-2".to_string()),
        method: "tasks/result".to_string(),
        params: Some(json!({
            "taskId": task_id
        })),
    };

    let ctx = create_test_context(&router);
    let start = std::time::Instant::now();
    let response = router.route(request, ctx).await;
    let elapsed = start.elapsed();

    // Should have blocked for at least 100ms
    assert!(elapsed >= Duration::from_millis(100));
    assert!(response.result().is_some());
    let result = response.result().unwrap();
    assert_eq!(result["delayed"], true);
}

#[tokio::test]
async fn test_tasks_result_returns_error_for_failed_task() {
    let router = create_test_router();

    // Create and fail a task
    let storage = router.get_task_storage().unwrap();
    let metadata = TaskMetadata {
        ttl: Some(3_600_000),
    };
    let task_id = storage.create_task(metadata, None).unwrap();
    storage
        .fail_task(&task_id, "Task execution failed".to_string(), None)
        .unwrap();

    // Get result via tasks/result
    let request = JsonRpcRequest {
        jsonrpc: JsonRpcVersion,
        id: RequestId::String("result-3".to_string()),
        method: "tasks/result".to_string(),
        params: Some(json!({
            "taskId": task_id
        })),
    };

    let ctx = create_test_context(&router);
    let response = router.route(request, ctx).await;

    assert!(response.error().is_some());
    let error = response.error().unwrap();
    assert!(error.message.contains("failed"));
}

// ============================================================================
// Task Lifecycle Integration Tests
// ============================================================================

#[tokio::test]
async fn test_complete_task_lifecycle() {
    let router = create_test_router();

    // 1. Create task
    let storage = router.get_task_storage().unwrap();
    let metadata = TaskMetadata {
        ttl: Some(3_600_000),
    };
    let task_id = storage.create_task(metadata, None).unwrap();

    // 2. Verify initial status
    let task = storage.get_task(&task_id, None).unwrap();
    assert_eq!(task.status, TaskStatus::Working);

    // 3. Update status with progress
    storage
        .update_status(
            &task_id,
            TaskStatus::Working,
            Some("Processing data".to_string()),
            None,
        )
        .unwrap();

    // 4. Complete task
    storage
        .complete_task(&task_id, json!({"result": "success"}), None)
        .unwrap();

    // 5. Verify final status
    let task = storage.get_task(&task_id, None).unwrap();
    assert_eq!(task.status, TaskStatus::Completed);

    // 6. Verify cannot cancel completed task
    let cancel_result = storage.cancel_task(&task_id, None, None);
    assert!(cancel_result.is_err());
}

// Note: TTL cleanup is comprehensively tested in unit tests (task_storage::tests::test_task_lifecycle)
// Integration test removed due to timing complexity with background cleanup task

// ============================================================================
// Error Handling Tests
// ============================================================================

#[tokio::test]
async fn test_invalid_task_id_format() {
    let router = create_test_router();

    let request = JsonRpcRequest {
        jsonrpc: JsonRpcVersion,
        id: RequestId::String("error-1".to_string()),
        method: "tasks/get".to_string(),
        params: Some(json!({
            "taskId": ""
        })),
    };

    let ctx = create_test_context(&router);
    let response = router.route(request, ctx).await;

    assert!(response.error().is_some());
}

#[tokio::test]
async fn test_missing_task_id_parameter() {
    let router = create_test_router();

    let request = JsonRpcRequest {
        jsonrpc: JsonRpcVersion,
        id: RequestId::String("error-2".to_string()),
        method: "tasks/get".to_string(),
        params: Some(json!({})),
    };

    let ctx = create_test_context(&router);
    let response = router.route(request, ctx).await;

    assert!(response.error().is_some());
}

// ============================================================================
// Tool Call Task Creation Tests (Automatic)
// ============================================================================

struct MockSlowTool {
    name: String,
    delay: Duration,
}

#[async_trait::async_trait]
impl turbomcp_server::handlers::ToolHandler for MockSlowTool {
    async fn handle(
        &self,
        _request: turbomcp_protocol::types::CallToolRequest,
        _ctx: RequestContext,
    ) -> turbomcp_server::ServerResult<turbomcp_protocol::types::CallToolResult> {
        sleep(self.delay).await;
        Ok(turbomcp_protocol::types::CallToolResult {
            content: vec![turbomcp_protocol::types::Content::Text(
                turbomcp_protocol::types::TextContent {
                    text: "Tool finished".to_string(),
                    annotations: None,
                    meta: None,
                },
            )],
            ..Default::default()
        })
    }

    fn tool_definition(&self) -> turbomcp_protocol::types::Tool {
        turbomcp_protocol::types::Tool {
            name: self.name.clone(),
            description: Some("A slow tool".to_string()),
            input_schema: turbomcp_protocol::types::ToolInputSchema::default(),
            ..Default::default()
        }
    }
}

#[tokio::test]
async fn test_tool_call_auto_creates_task() {
    // 1. Setup Router with Slow Tool
    let registry = Arc::new(HandlerRegistry::new());
    let tool = MockSlowTool {
        name: "slow_tool".to_string(),
        delay: Duration::from_millis(500),
    };
    registry.register_tool("slow_tool", tool).unwrap();

    let metrics = Arc::new(ServerMetrics::new());
    let task_storage = Arc::new(TaskStorage::new(Duration::from_secs(3600)));
    let router = Arc::new(RequestRouter::new(
        registry,
        metrics,
        ServerConfig::default(),
        Some(task_storage.clone()),
    ));

    // 2. Call Tool with Task Metadata
    let request = JsonRpcRequest {
        jsonrpc: JsonRpcVersion,
        id: RequestId::String("call-1".to_string()),
        method: "tools/call".to_string(),
        params: Some(json!({
            "name": "slow_tool",
            "arguments": {},
            "task": {
                "ttl": 3600000
            }
        })),
    };

    let start = std::time::Instant::now();
    let ctx = create_test_context(&router);
    let response = router.route(request, ctx).await;
    let duration = start.elapsed();

    // 3. Verify Immediate Return (Non-blocking)
    // IMPORTANT: If this fails (>500ms), it means the handler blocked!
    assert!(
        duration.as_millis() < 100,
        "Handler blocked for {:?}, expected immediate return (<100ms)",
        duration
    );

    // 4. Verify Response is CreateTaskResult
    assert!(response.result().is_some());
    let result = response.result().unwrap();
    // Verify it looks like a task (has taskId, status)
    assert!(result.get("task").is_some());
    let task = result.get("task").unwrap();
    let task_id = task.get("taskId").unwrap().as_str().unwrap().to_string();
    assert_eq!(task.get("status").unwrap().as_str().unwrap(), "working");

    // 5. Verify Task Eventually Completes
    // Wait for the tool to finish (500ms + buffer)
    sleep(Duration::from_millis(700)).await;

    // Check status via storage directly
    let stored_task = task_storage.get_task(&task_id, None).expect("Task should exist");
    assert_eq!(
        stored_task.status,
        TaskStatus::Completed,
        "Task should be completed after delay"
    );

    // 6. Verify Result Data
    // We can use get_task_result (which returns immediately for completed tasks)
    let result_state = task_storage.get_task_result(&task_id, None).await.unwrap();
    match result_state {
        turbomcp_server::task_storage::TaskResultState::Completed(val) => {
            let content = val.get("content").unwrap().as_array().unwrap();
            assert_eq!(content[0].get("text").unwrap().as_str().unwrap(), "Tool finished");
        },
        _ => panic!("Expected completed task result"),
    }
}
