//! WebSocket End-to-End Client/Server Integration Tests
//!
//! These tests validate the COMPLETE request/response flow using BOTH:
//! - Real TurboMCP server (via `#[server]` macro)
//! - Real TurboMCP client (via `turbomcp-client` with `WebSocketBidirectionalTransport`)
//!
//! ## Why These Tests Exist
//!
//! Previous WebSocket tests had a critical gap:
//! - Transport-layer tests used mock echo servers (not real MCP protocol)
//! - Integration tests used raw `tokio_tungstenite` (bypassed the client entirely)
//!
//! This meant the correlation routing bug in `spawn_message_reader_task()` was never
//! caught because no test actually exercised the full client→server→client path.
//!
//! ## What These Tests Cover
//!
//! - ✅ Full MCP protocol handshake (initialize)
//! - ✅ JSON-RPC request/response correlation routing
//! - ✅ Tool listing and tool calls through the client API
//! - ✅ Concurrent tool calls with proper response routing
//! - ✅ Error handling and timeout behavior
//! - ✅ Connection lifecycle (connect, communicate, disconnect)

#![cfg(feature = "websocket")]

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;
use tokio::time::{sleep, timeout};
use turbomcp::prelude::*;
use turbomcp_client::Client;
use turbomcp_transport::websocket_bidirectional::{
    WebSocketBidirectionalConfig, WebSocketBidirectionalTransport,
};

// ============================================================================
// Test Server Implementation
// ============================================================================

/// Test server with multiple tools for comprehensive testing
#[derive(Clone)]
struct E2ETestServer {
    call_count: Arc<AtomicU32>,
}

#[server(
    name = "E2E WebSocket Test Server",
    version = "1.0.0",
    description = "Server for end-to-end WebSocket client/server testing",
    transports = ["websocket"]
)]
impl E2ETestServer {
    fn new() -> Self {
        Self {
            call_count: Arc::new(AtomicU32::new(0)),
        }
    }

    /// Simple echo tool - validates basic request/response correlation
    #[tool("Echo a message back to the caller")]
    async fn echo(&self, message: String) -> McpResult<String> {
        self.call_count.fetch_add(1, Ordering::SeqCst);
        Ok(format!("Echo: {}", message))
    }

    /// Math tool - validates argument parsing and result serialization
    #[tool("Add two numbers together")]
    async fn add(&self, a: i64, b: i64) -> McpResult<i64> {
        self.call_count.fetch_add(1, Ordering::SeqCst);
        Ok(a + b)
    }

    /// Delayed response tool - validates correlation under timing pressure
    #[tool("Respond after a delay (for timeout testing)")]
    async fn delayed_response(&self, delay_ms: u64, message: String) -> McpResult<String> {
        self.call_count.fetch_add(1, Ordering::SeqCst);
        sleep(Duration::from_millis(delay_ms)).await;
        Ok(format!("Delayed: {}", message))
    }

    /// Get the call count - validates server state persistence
    #[tool("Get total number of tool calls")]
    async fn get_call_count(&self) -> McpResult<u32> {
        Ok(self.call_count.load(Ordering::SeqCst))
    }

    /// Error tool - validates error propagation through correlation
    #[tool("Always returns an error")]
    async fn always_fails(&self) -> McpResult<String> {
        self.call_count.fetch_add(1, Ordering::SeqCst);
        Err(turbomcp::McpError::internal(
            "Intentional error for testing",
        ))
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Convert JSON value to HashMap for tool arguments
fn json_args(args: serde_json::Value) -> Option<HashMap<String, serde_json::Value>> {
    match args {
        serde_json::Value::Object(map) => Some(map.into_iter().collect()),
        serde_json::Value::Null => None,
        _ => panic!("Arguments must be a JSON object"),
    }
}

/// Start server and return the port it's listening on
async fn start_test_server(port: u16) -> tokio::task::JoinHandle<()> {
    let server = E2ETestServer::new();
    let addr = format!("127.0.0.1:{}", port);

    tokio::spawn(async move {
        // Server runs until task is aborted - result intentionally ignored
        // because the server never returns Ok(()) during normal operation,
        // it only returns when the task is aborted via JoinHandle::abort()
        if let Err(e) = server.run_websocket(&addr).await {
            // Log error only if it's unexpected (not from task abort)
            tracing::debug!("WebSocket server stopped: {}", e);
        }
    })
}

/// Create a client connected to the test server
async fn create_connected_client(
    port: u16,
) -> Result<Client<WebSocketBidirectionalTransport>, Box<dyn std::error::Error + Send + Sync>> {
    let url = format!("ws://127.0.0.1:{}/ws", port);
    let config = WebSocketBidirectionalConfig::client(url);

    let transport = WebSocketBidirectionalTransport::new(config).await?;
    transport.connect().await?;

    let client = Client::new(transport);
    client.initialize().await?;

    Ok(client)
}

// ============================================================================
// End-to-End Tests
// ============================================================================

/// Test basic client-server connection and tool call
///
/// This is the CRITICAL test that would have caught the correlation bug.
/// It validates that:
/// 1. Client can connect via WebSocket
/// 2. Initialize handshake completes
/// 3. Tool call request is sent
/// 4. Response is correctly routed back through correlation
#[tokio::test]
async fn test_e2e_websocket_basic_tool_call() {
    let port = 19101;
    let server_task = start_test_server(port).await;
    sleep(Duration::from_millis(500)).await; // Wait for server startup

    // Create client using the REAL WebSocketBidirectionalTransport
    let client = create_connected_client(port)
        .await
        .expect("Failed to create connected client");

    // Call the echo tool through the client API
    let result = client
        .call_tool(
            "echo",
            json_args(serde_json::json!({"message": "Hello E2E!"})),
        )
        .await
        .expect("Tool call failed");

    // Validate response
    assert!(!result.content.is_empty(), "Response should have content");

    let text = match &result.content[0] {
        turbomcp_protocol::types::Content::Text(t) => &t.text,
        _ => panic!("Expected text content"),
    };
    assert_eq!(text, "Echo: Hello E2E!");

    // Cleanup
    server_task.abort();
}

/// Test listing tools through the client
#[tokio::test]
async fn test_e2e_websocket_list_tools() {
    let port = 19102;
    let server_task = start_test_server(port).await;
    sleep(Duration::from_millis(500)).await;

    let client = create_connected_client(port)
        .await
        .expect("Failed to create connected client");

    // List tools
    let tools = client.list_tools().await.expect("Failed to list tools");

    // Validate tools are returned
    assert!(tools.len() >= 4, "Should have at least 4 tools");

    let tool_names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    assert!(tool_names.contains(&"echo"), "Should have echo tool");
    assert!(tool_names.contains(&"add"), "Should have add tool");
    assert!(
        tool_names.contains(&"delayed_response"),
        "Should have delayed_response tool"
    );
    assert!(
        tool_names.contains(&"get_call_count"),
        "Should have get_call_count tool"
    );

    server_task.abort();
}

/// Test multiple sequential tool calls
///
/// Validates that correlation routing works correctly across multiple requests
#[tokio::test]
async fn test_e2e_websocket_sequential_tool_calls() {
    let port = 19103;
    let server_task = start_test_server(port).await;
    sleep(Duration::from_millis(500)).await;

    let client = create_connected_client(port)
        .await
        .expect("Failed to create connected client");

    // Make multiple sequential calls
    for i in 1..=5i64 {
        let result = client
            .call_tool("add", json_args(serde_json::json!({"a": i, "b": i * 10})))
            .await
            .expect("Tool call failed");

        let text = match &result.content[0] {
            turbomcp_protocol::types::Content::Text(t) => &t.text,
            _ => panic!("Expected text content"),
        };

        let expected = i + i * 10;
        assert_eq!(
            text,
            &expected.to_string(),
            "Call {} should return {}",
            i,
            expected
        );
    }

    // Verify all calls were counted
    let count_result = client
        .call_tool("get_call_count", None)
        .await
        .expect("Failed to get call count");

    let count_text = match &count_result.content[0] {
        turbomcp_protocol::types::Content::Text(t) => &t.text,
        _ => panic!("Expected text content"),
    };

    // 5 add calls (get_call_count doesn't increment)
    assert_eq!(count_text, "5", "Should have made 5 tool calls");

    server_task.abort();
}

/// Test concurrent tool calls
///
/// This validates that correlation routing correctly matches responses
/// to their respective requests when multiple requests are in flight
#[tokio::test]
async fn test_e2e_websocket_concurrent_tool_calls() {
    let port = 19104;
    let server_task = start_test_server(port).await;
    sleep(Duration::from_millis(500)).await;

    let client = Arc::new(
        create_connected_client(port)
            .await
            .expect("Failed to create connected client"),
    );

    // Launch 10 concurrent tool calls
    let mut handles = vec![];
    for i in 0..10 {
        let client_clone = Arc::clone(&client);
        let handle = tokio::spawn(async move {
            let result = client_clone
                .call_tool(
                    "echo",
                    json_args(serde_json::json!({"message": format!("concurrent-{}", i)})),
                )
                .await
                .expect("Concurrent tool call failed");

            let text = match &result.content[0] {
                turbomcp_protocol::types::Content::Text(t) => t.text.clone(),
                _ => panic!("Expected text content"),
            };

            (i, text)
        });
        handles.push(handle);
    }

    // Collect results
    let mut results = vec![];
    for handle in handles {
        let (i, text) = handle.await.expect("Task panicked");
        results.push((i, text));
    }

    // Verify each response matches its request
    for (i, text) in results {
        assert_eq!(
            text,
            format!("Echo: concurrent-{}", i),
            "Response {} should match request {}",
            text,
            i
        );
    }

    server_task.abort();
}

/// Test error handling through correlation
#[tokio::test]
async fn test_e2e_websocket_error_handling() {
    let port = 19105;
    let server_task = start_test_server(port).await;
    sleep(Duration::from_millis(500)).await;

    let client = create_connected_client(port)
        .await
        .expect("Failed to create connected client");

    // Call tool that always fails
    let result = client.call_tool("always_fails", None).await;

    // Should receive error
    assert!(result.is_err(), "Should receive error from failing tool");

    // A successful call should still work after error
    let success_result = client
        .call_tool(
            "echo",
            json_args(serde_json::json!({"message": "after error"})),
        )
        .await
        .expect("Tool call after error should succeed");

    let text = match &success_result.content[0] {
        turbomcp_protocol::types::Content::Text(t) => &t.text,
        _ => panic!("Expected text content"),
    };
    assert_eq!(text, "Echo: after error");

    server_task.abort();
}

/// Test client timeout behavior
#[tokio::test]
async fn test_e2e_websocket_timeout_behavior() {
    let port = 19106;
    let server_task = start_test_server(port).await;
    sleep(Duration::from_millis(500)).await;

    // Create client with default config
    let url = format!("ws://127.0.0.1:{}/ws", port);
    let config = WebSocketBidirectionalConfig::client(url);

    let transport = WebSocketBidirectionalTransport::new(config)
        .await
        .expect("Failed to create transport");
    transport.connect().await.expect("Failed to connect");

    let client = Client::new(transport);
    client.initialize().await.expect("Initialize failed");

    // Call delayed tool with a short external timeout
    let result = timeout(
        Duration::from_millis(500),
        client.call_tool(
            "delayed_response",
            json_args(serde_json::json!({"delay_ms": 2000, "message": "timeout test"})),
        ),
    )
    .await;

    // Should timeout
    assert!(result.is_err(), "Should have timed out");

    server_task.abort();
}

/// Test reconnection scenario
#[tokio::test]
async fn test_e2e_websocket_connection_lifecycle() {
    let port = 19107;
    let server_task = start_test_server(port).await;
    sleep(Duration::from_millis(500)).await;

    // First connection
    let client1 = create_connected_client(port)
        .await
        .expect("Failed to create first client");

    let result1 = client1
        .call_tool("echo", json_args(serde_json::json!({"message": "first"})))
        .await
        .expect("First call failed");

    let text1 = match &result1.content[0] {
        turbomcp_protocol::types::Content::Text(t) => &t.text,
        _ => panic!("Expected text content"),
    };
    assert_eq!(text1, "Echo: first");

    // Second connection (simulating reconnect)
    let client2 = create_connected_client(port)
        .await
        .expect("Failed to create second client");

    let result2 = client2
        .call_tool("echo", json_args(serde_json::json!({"message": "second"})))
        .await
        .expect("Second call failed");

    let text2 = match &result2.content[0] {
        turbomcp_protocol::types::Content::Text(t) => &t.text,
        _ => panic!("Expected text content"),
    };
    assert_eq!(text2, "Echo: second");

    server_task.abort();
}

/// Test multiple concurrent clients
#[tokio::test]
async fn test_e2e_websocket_multiple_clients() {
    let port = 19108;
    let server_task = start_test_server(port).await;
    sleep(Duration::from_millis(500)).await;

    // Create 3 clients
    let mut handles = vec![];
    for client_id in 0..3 {
        let handle = tokio::spawn(async move {
            let client = create_connected_client(port)
                .await
                .expect("Failed to create client");

            // Each client makes 3 calls
            for call_id in 0..3 {
                let result = client
                    .call_tool(
                        "echo",
                        json_args(serde_json::json!({
                            "message": format!("client{}-call{}", client_id, call_id)
                        })),
                    )
                    .await
                    .expect("Tool call failed");

                let text = match &result.content[0] {
                    turbomcp_protocol::types::Content::Text(t) => t.text.clone(),
                    _ => panic!("Expected text content"),
                };

                assert_eq!(
                    text,
                    format!("Echo: client{}-call{}", client_id, call_id),
                    "Response should match request for client {} call {}",
                    client_id,
                    call_id
                );
            }

            client_id
        });
        handles.push(handle);
    }

    // Wait for all clients to complete
    for handle in handles {
        handle.await.expect("Client task panicked");
    }

    server_task.abort();
}

/// Test that validates the specific correlation routing fix
///
/// This test specifically validates that responses are routed to the
/// correct waiting request, not just any waiting request. It does this
/// by having concurrent requests with different expected results.
#[tokio::test]
async fn test_e2e_websocket_correlation_routing_correctness() {
    let port = 19109;
    let server_task = start_test_server(port).await;
    sleep(Duration::from_millis(500)).await;

    let client = Arc::new(
        create_connected_client(port)
            .await
            .expect("Failed to create connected client"),
    );

    // Launch concurrent calls with DIFFERENT expected results
    let client1 = Arc::clone(&client);
    let handle1 = tokio::spawn(async move {
        client1
            .call_tool("add", json_args(serde_json::json!({"a": 1, "b": 2})))
            .await
    });

    let client2 = Arc::clone(&client);
    let handle2 = tokio::spawn(async move {
        client2
            .call_tool("add", json_args(serde_json::json!({"a": 100, "b": 200})))
            .await
    });

    let client3 = Arc::clone(&client);
    let handle3 = tokio::spawn(async move {
        client3
            .call_tool("add", json_args(serde_json::json!({"a": 1000, "b": 2000})))
            .await
    });

    // Collect results
    let result1 = handle1
        .await
        .expect("Task 1 panicked")
        .expect("Call 1 failed");
    let result2 = handle2
        .await
        .expect("Task 2 panicked")
        .expect("Call 2 failed");
    let result3 = handle3
        .await
        .expect("Task 3 panicked")
        .expect("Call 3 failed");

    // Extract text from results
    let text1 = match &result1.content[0] {
        turbomcp_protocol::types::Content::Text(t) => &t.text,
        _ => panic!("Expected text content"),
    };
    let text2 = match &result2.content[0] {
        turbomcp_protocol::types::Content::Text(t) => &t.text,
        _ => panic!("Expected text content"),
    };
    let text3 = match &result3.content[0] {
        turbomcp_protocol::types::Content::Text(t) => &t.text,
        _ => panic!("Expected text content"),
    };

    // CRITICAL: Each result must match its specific request
    // If correlation routing is broken, results would be mismatched
    assert_eq!(text1, "3", "1+2 should equal 3");
    assert_eq!(text2, "300", "100+200 should equal 300");
    assert_eq!(text3, "3000", "1000+2000 should equal 3000");

    server_task.abort();
}
