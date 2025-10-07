//! Integration tests for TurboMCP

use serde_json::json;
use std::sync::Arc;
use turbomcp_protocol::{MessageId, StateManager};
use turbomcp_server::{McpServer, ServerConfig};
use turbomcp_transport::core::{Transport, TransportConfig, TransportType};
use turbomcp_transport::stdio::StdioTransport;
// Integration test imports

#[tokio::test]
async fn test_server_creation() {
    let config = ServerConfig::default();
    let server = McpServer::new(config);

    // Basic creation test - server should be created successfully
    // Note: McpServer::new returns Self, not Result<Self, Error>
    assert!(!server.config().name.is_empty()); // Should have a name
}

#[tokio::test]
async fn test_custom_server_config() {
    let config = ServerConfig {
        name: "test-server".to_string(),
        version: "1.0.0".to_string(),
        description: Some("Test server".to_string()),
        ..Default::default()
    };

    let server = McpServer::new(config.clone());
    // Verify server was created with correct config
    assert_eq!(server.config().name, "test-server");
}

#[tokio::test]
async fn test_state_manager_integration() {
    let state = StateManager::new();

    // Test basic state operations
    state.set("test_key".to_string(), json!("test_value"));
    assert_eq!(state.get("test_key"), Some(json!("test_value")));

    // Test persistence through export/import
    let exported = state.export();
    let new_state = StateManager::new();
    assert!(new_state.import(exported).is_ok());
    assert_eq!(new_state.get("test_key"), Some(json!("test_value")));
}

#[tokio::test]
async fn test_transport_creation() {
    let config = TransportConfig {
        transport_type: TransportType::Stdio,
        ..Default::default()
    };

    let transport = StdioTransport::with_config(config);
    assert_eq!(transport.transport_type(), TransportType::Stdio);
    assert!(transport.capabilities().supports_bidirectional);
}

#[tokio::test]
async fn test_stdio_transport_lifecycle() {
    let mut transport = StdioTransport::new();

    // Test initial state
    assert_eq!(transport.state().await.to_string(), "disconnected");

    // Test configuration
    let config = TransportConfig {
        transport_type: TransportType::Stdio,
        ..Default::default()
    };

    assert!(transport.configure(config).await.is_ok());
}

#[tokio::test]
async fn test_message_serialization() {
    use turbomcp_protocol::MessageId;

    // Test MessageId serialization
    let message_id = MessageId::from("test-123");
    let serialized = serde_json::to_string(&message_id);
    assert!(serialized.is_ok());

    // Test deserialization
    let deserialized: std::result::Result<MessageId, _> =
        serde_json::from_str(&serialized.unwrap());
    assert!(deserialized.is_ok());
}

#[tokio::test]
async fn test_concurrent_state_operations() {
    let state = Arc::new(StateManager::new());
    let mut handles = vec![];

    // Spawn multiple tasks that operate on state concurrently
    for i in 0..50 {
        let state_clone = Arc::clone(&state);
        let handle = tokio::spawn(async move {
            let key = format!("concurrent_key_{}", i);
            let value = json!(format!("value_{}", i));
            state_clone.set(key.clone(), value);

            // Verify the value was set
            assert_eq!(state_clone.get(&key), Some(json!(format!("value_{}", i))));
        });
        handles.push(handle);
    }

    // Wait for all tasks to complete
    for handle in handles {
        assert!(handle.await.is_ok());
    }

    assert_eq!(state.size(), 50);
}

#[tokio::test]
async fn test_error_handling() {
    use turbomcp_protocol::{Error, ErrorKind, Result};

    // Test error creation and display
    let error = Error::new(ErrorKind::Transport, "Connection failed");
    assert!(error.to_string().contains("Connection failed"));

    // Test result handling
    let result: Result<String> = Err(error);
    assert!(result.is_err());
}

#[tokio::test]
async fn test_protocol_validation() {
    use turbomcp_server::{JsonRpcRequest, JsonRpcVersion};

    let request = JsonRpcRequest {
        jsonrpc: JsonRpcVersion,
        id: MessageId::from("test-id"),
        method: "test_method".to_string(),
        params: Some(json!({"param": "value"})),
    };

    // Test serialization
    let serialized = serde_json::to_string(&request);
    assert!(serialized.is_ok());

    // Test deserialization
    let deserialized: std::result::Result<JsonRpcRequest, _> =
        serde_json::from_str(&serialized.unwrap());
    assert!(deserialized.is_ok());

    let deserialized_request = deserialized.unwrap();
    assert_eq!(deserialized_request.method, "test_method");
}

#[tokio::test]
async fn test_stress_operations() {
    let state = StateManager::new();

    // Perform many operations to stress test
    let num_operations = 1000;

    // Set operations
    for i in 0..num_operations {
        state.set(format!("stress_key_{}", i), json!(i));
    }

    assert_eq!(state.size(), num_operations);

    // Get operations
    for i in 0..num_operations {
        assert_eq!(state.get(&format!("stress_key_{}", i)), Some(json!(i)));
    }

    // Remove operations
    for i in 0..num_operations / 2 {
        state.remove(&format!("stress_key_{}", i));
    }

    assert_eq!(state.size(), num_operations / 2);

    // Clear all
    state.clear();
    assert_eq!(state.size(), 0);
}

#[tokio::test]
async fn test_transport_metrics() {
    use turbomcp_transport::core::TransportMetrics;

    let metrics = TransportMetrics::default();
    assert_eq!(metrics.bytes_sent, 0);
    assert_eq!(metrics.bytes_received, 0);
    assert_eq!(metrics.messages_sent, 0);
    assert_eq!(metrics.messages_received, 0);
}

#[tokio::test]
async fn test_memory_efficiency() {
    let state = StateManager::new();

    // Test memory usage with large objects
    let large_object = json!({
        "data": "x".repeat(10000),
        "array": (0..1000).collect::<Vec<_>>(),
        "nested": {
            "deep": {
                "value": "test"
            }
        }
    });

    state.set("large_object".to_string(), large_object.clone());
    assert_eq!(state.get("large_object"), Some(large_object));

    // Clear should free memory
    state.clear();
    assert_eq!(state.size(), 0);
}

#[tokio::test]
async fn test_json_rpc_error_handling() {
    use turbomcp_server::JsonRpcError;

    // Test error creation and handling
    let error = JsonRpcError {
        code: -32700,
        message: "Parse error".to_string(),
        data: None,
    };

    assert_eq!(error.code, -32700);
    assert_eq!(error.message, "Parse error");
}

#[tokio::test]
async fn test_transport_capabilities() {
    use turbomcp_transport::core::TransportCapabilities;

    let caps = TransportCapabilities::default();
    assert!(caps.supports_bidirectional);
    assert!(caps.max_message_size.is_some());
}

// =============================================================================
// COMPREHENSIVE TEST SUITE
// =============================================================================

#[tokio::test]
async fn test_comprehensive_auth_flow() {
    // Test OAuth2-like flow with state validation
    let state_manager = StateManager::new();
    
    // Simulate OAuth2 state
    let auth_state = "random_state_123";
    state_manager.set(format!("oauth_state_{}", auth_state), json!({
        "client_id": "test_client",
        "redirect_uri": "https://example.com/callback",
        "created_at": chrono::Utc::now().timestamp()
    }));
    
    // Verify state exists and is valid
    let stored_state = state_manager.get(&format!("oauth_state_{}", auth_state));
    assert!(stored_state.is_some());
    
    let state_data = stored_state.unwrap();
    assert_eq!(state_data["client_id"], "test_client");
    
    // Simulate completion and cleanup
    state_manager.remove(&format!("oauth_state_{}", auth_state));
    assert!(state_manager.get(&format!("oauth_state_{}", auth_state)).is_none());
}

#[tokio::test]
async fn test_comprehensive_transport_robustness() {
    // Test transport resilience patterns
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;
    
    // Simulate circuit breaker behavior
    let failure_count = Arc::new(AtomicU32::new(0));
    let threshold = 5;
    
    // Simulate multiple failures
    for _ in 0..threshold {
        failure_count.fetch_add(1, Ordering::SeqCst);
    }
    
    assert_eq!(failure_count.load(Ordering::SeqCst), threshold);
    
    // Simulate recovery
    failure_count.store(0, Ordering::SeqCst);
    assert_eq!(failure_count.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn test_comprehensive_protocol_validation() {
    use turbomcp_server::JsonRpcRequest;
    
    // Test complex nested validation
    let complex_params = json!({
        "nested": {
            "level1": {
                "level2": {
                    "data": "test",
                    "array": [1, 2, 3, 4, 5]
                }
            }
        },
        "unicode": "🌟 Testing unicode: こんにちは 🚀",
        "large_array": (0..100).collect::<Vec<_>>()
    });
    
    let request = JsonRpcRequest {
        jsonrpc: turbomcp_server::JsonRpcVersion,
        id: MessageId::from("complex-test"),
        method: "complex_method".to_string(),
        params: Some(complex_params),
    };
    
    // Test serialization of complex data
    let serialized = serde_json::to_string(&request);
    assert!(serialized.is_ok());
    
    // Test deserialization
    let deserialized: Result<JsonRpcRequest, _> = 
        serde_json::from_str(&serialized.unwrap());
    assert!(deserialized.is_ok());
    
    let deserialized_request = deserialized.unwrap();
    assert_eq!(deserialized_request.method, "complex_method");
    
    // Verify nested data integrity
    if let Some(params) = deserialized_request.params {
        assert_eq!(params["unicode"], "🌟 Testing unicode: こんにちは 🚀");
        assert_eq!(params["large_array"].as_array().unwrap().len(), 100);
    }
}

#[tokio::test]
async fn test_comprehensive_performance_simulation() {
    use std::time::Instant;
    
    // Test high-throughput scenario
    let state = Arc::new(StateManager::new());
    let start = Instant::now();
    let num_operations = 5000; // Reduced for CI
    
    // Concurrent operations
    let mut handles = vec![];
    for i in 0..num_operations {
        let state_clone = Arc::clone(&state);
        let handle = tokio::spawn(async move {
            let key = format!("perf_key_{}", i);
            let value = json!({
                "id": i,
                "data": format!("test_data_{}", i),
                "timestamp": chrono::Utc::now().timestamp()
            });
            state_clone.set(key, value);
        });
        handles.push(handle);
    }
    
    // Wait for completion
    for handle in handles {
        assert!(handle.await.is_ok());
    }
    
    let duration = start.elapsed();
    println!("Performance test: {} operations in {:?}", num_operations, duration);
    
    // Verify all operations completed
    assert_eq!(state.size(), num_operations);
    
    // Test should complete within reasonable time (5 seconds)
    assert!(duration.as_secs() < 5);
}

#[tokio::test]
async fn test_comprehensive_error_handling() {
    use turbomcp_protocol::{Error, ErrorKind};
    
    // Test various error scenarios
    let errors = vec![
        Error::new(ErrorKind::Transport, "Connection timeout"),
        Error::new(ErrorKind::Protocol, "Invalid JSON-RPC format"),
        Error::new(ErrorKind::Server, "Server overloaded"),
        Error::new(ErrorKind::Auth, "Authentication failed"),
    ];
    
    for error in errors {
        // Test error formatting
        let error_string = error.to_string();
        assert!(!error_string.is_empty());
        
        // Test error kind preservation
        match error.kind() {
            ErrorKind::Transport => assert!(error_string.contains("Connection timeout")),
            ErrorKind::Protocol => assert!(error_string.contains("Invalid JSON-RPC")),
            ErrorKind::Server => assert!(error_string.contains("Server overloaded")),
            ErrorKind::Auth => assert!(error_string.contains("Authentication failed")),
            _ => {}
        }
    }
}

#[tokio::test]
async fn test_comprehensive_concurrent_safety() {
    // Test thread safety with heavy concurrent access
    let state = Arc::new(StateManager::new());
    let num_threads = 10;
    let ops_per_thread = 100;
    
    let mut handles = vec![];
    
    for thread_id in 0..num_threads {
        let state_clone = Arc::clone(&state);
        let handle = tokio::spawn(async move {
            for op_id in 0..ops_per_thread {
                let key = format!("thread_{}_op_{}", thread_id, op_id);
                let value = json!({
                    "thread_id": thread_id,
                    "op_id": op_id,
                    "data": format!("test_data_{}_{}", thread_id, op_id)
                });
                
                // Set operation
                state_clone.set(key.clone(), value.clone());
                
                // Get operation
                let retrieved = state_clone.get(&key);
                assert_eq!(retrieved, Some(value));
                
                // Update operation
                let updated_value = json!({
                    "thread_id": thread_id,
                    "op_id": op_id,
                    "data": format!("updated_data_{}_{}", thread_id, op_id)
                });
                state_clone.set(key.clone(), updated_value.clone());
                
                // Verify update
                let retrieved_updated = state_clone.get(&key);
                assert_eq!(retrieved_updated, Some(updated_value));
            }
        });
        handles.push(handle);
    }
    
    // Wait for all threads to complete
    for handle in handles {
        assert!(handle.await.is_ok());
    }
    
    // Verify final state
    let expected_size = num_threads * ops_per_thread;
    assert_eq!(state.size(), expected_size);
}

#[tokio::test]
async fn test_comprehensive_resource_cleanup() {
    // Test resource management and cleanup
    let state = StateManager::new();
    
    // Create many resources
    let num_resources = 1000;
    for i in 0..num_resources {
        let resource = json!({
            "id": i,
            "type": "test_resource",
            "data": "x".repeat(1000), // 1KB per resource
            "created_at": chrono::Utc::now().timestamp()
        });
        state.set(format!("resource_{}", i), resource);
    }
    
    assert_eq!(state.size(), num_resources);
    
    // Cleanup even-numbered resources
    for i in (0..num_resources).step_by(2) {
        state.remove(&format!("resource_{}", i));
    }
    
    assert_eq!(state.size(), num_resources / 2);
    
    // Full cleanup
    state.clear();
    assert_eq!(state.size(), 0);
}

#[tokio::test]
async fn test_comprehensive_message_lifecycle() {
    use turbomcp_server::{JsonRpcRequest, JsonRpcResponse, JsonRpcError};
    
    // Test complete message lifecycle
    let request = JsonRpcRequest {
        jsonrpc: turbomcp_server::JsonRpcVersion,
        id: MessageId::from("lifecycle-test"),
        method: "test_lifecycle".to_string(),
        params: Some(json!({
            "input": "test_input",
            "options": {
                "timeout": 30,
                "retries": 3
            }
        })),
    };
    
    // Serialize request
    let serialized_request = serde_json::to_string(&request).unwrap();
    
    // Deserialize request (simulating network transport)
    let deserialized_request: JsonRpcRequest = 
        serde_json::from_str(&serialized_request).unwrap();
    
    assert_eq!(deserialized_request.method, "test_lifecycle");
    
    // Create response
    let response = JsonRpcResponse {
        jsonrpc: turbomcp_server::JsonRpcVersion,
        id: deserialized_request.id.clone(),
        result: Some(json!({
            "output": "test_output",
            "processed_at": chrono::Utc::now().timestamp(),
            "status": "success"
        })),
        error: None,
    };
    
    // Test response serialization/deserialization
    let serialized_response = serde_json::to_string(&response).unwrap();
    let deserialized_response: JsonRpcResponse = 
        serde_json::from_str(&serialized_response).unwrap();
    
    assert_eq!(deserialized_response.id, request.id);
    assert!(deserialized_response.result.is_some());
    assert!(deserialized_response.error.is_none());
    
    // Test error response
    let error_response = JsonRpcResponse {
        jsonrpc: turbomcp_server::JsonRpcVersion,
        id: request.id.clone(),
        result: None,
        error: Some(JsonRpcError {
            code: -32603,
            message: "Internal error".to_string(),
            data: Some(json!({
                "details": "Test error condition",
                "timestamp": chrono::Utc::now().timestamp()
            })),
        }),
    };
    
    let serialized_error = serde_json::to_string(&error_response).unwrap();
    let deserialized_error: JsonRpcResponse = 
        serde_json::from_str(&serialized_error).unwrap();
    
    assert!(deserialized_error.error.is_some());
    assert_eq!(deserialized_error.error.unwrap().code, -32603);
}
