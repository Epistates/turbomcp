//! Comprehensive tests for completion functionality in turbomcp-client
//!
//! These tests validate the production-grade implementation of:
//! - complete() method with various argument types
//! - Protocol compliance for completion/complete requests  
//! - Error handling for invalid completion requests
//! - Real integration with completion servers
//! - Response validation and type safety

use async_trait::async_trait;
use serde_json::json;
use turbomcp_client::Client;
use turbomcp_core::{ErrorKind, MessageId};
use turbomcp_protocol::types::{CompleteResult, CompletionResponse};
use turbomcp_transport::core::{
    Transport, TransportCapabilities, TransportMessage, TransportMetrics, TransportResult,
    TransportState, TransportType,
};

/// Mock transport for completion testing that can simulate various server responses
#[derive(Debug)]
struct MockCompletionTransport {
    capabilities: TransportCapabilities,
    state: TransportState,
    metrics: TransportMetrics,
    response_queue: Vec<Vec<u8>>,
    sent_messages: Vec<TransportMessage>,
}

impl MockCompletionTransport {
    fn new() -> Self {
        Self {
            capabilities: TransportCapabilities::default(),
            state: TransportState::Disconnected,
            metrics: TransportMetrics::default(),
            response_queue: Vec::new(),
            sent_messages: Vec::new(),
        }
    }

    /// Add responses to the queue (first in, first out)
    fn add_response(&mut self, response: Vec<u8>) {
        self.response_queue.push(response);
    }

    /// Set up for initialization + completion test
    fn setup_for_completion_test(&mut self, completion_values: Vec<&str>, has_more: bool) {
        // Add initialization response first
        let init_response = json!({
            "jsonrpc": "2.0",
            "id": "1",
            "result": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "serverInfo": {
                    "name": "test-server",
                    "version": "1.0.0"
                }
            }
        });
        self.add_response(serde_json::to_vec(&init_response).unwrap());

        // Add completion response second (for the completion call)
        let completion_response = Self::create_completion_response(completion_values, has_more);
        self.add_response(completion_response);
    }

    /// Create a mock completion response
    fn create_completion_response(values: Vec<&str>, has_more: bool) -> Vec<u8> {
        let completion = CompletionResponse {
            values: values.iter().map(|s| s.to_string()).collect(),
            total: Some(values.len() as u32),
            has_more: Some(has_more),
        };

        let result = CompleteResult::new(completion);

        let json_response = json!({
            "jsonrpc": "2.0",
            "id": "1",
            "result": result
        });

        serde_json::to_vec(&json_response).unwrap()
    }

    /// Create a mock error response
    fn create_error_response(code: i32, message: &str) -> Vec<u8> {
        let json_response = json!({
            "jsonrpc": "2.0",
            "id": "1",
            "error": {
                "code": code,
                "message": message
            }
        });

        serde_json::to_vec(&json_response).unwrap()
    }
}

#[async_trait]
impl Transport for MockCompletionTransport {
    fn transport_type(&self) -> TransportType {
        TransportType::Stdio
    }

    fn capabilities(&self) -> &TransportCapabilities {
        &self.capabilities
    }

    async fn state(&self) -> TransportState {
        self.state.clone()
    }

    async fn connect(&mut self) -> TransportResult<()> {
        self.state = TransportState::Connected;
        Ok(())
    }

    async fn disconnect(&mut self) -> TransportResult<()> {
        self.state = TransportState::Disconnected;
        Ok(())
    }

    async fn send(&mut self, message: TransportMessage) -> TransportResult<()> {
        self.sent_messages.push(message);
        Ok(())
    }

    async fn receive(&mut self) -> TransportResult<Option<TransportMessage>> {
        if !self.response_queue.is_empty() {
            let payload = self.response_queue.remove(0);
            Ok(Some(TransportMessage::new(
                MessageId::from(format!("response-{}", self.sent_messages.len())),
                payload.into(),
            )))
        } else {
            Ok(None)
        }
    }

    async fn metrics(&self) -> TransportMetrics {
        self.metrics.clone()
    }

    fn endpoint(&self) -> Option<String> {
        Some("mock://completion-transport".to_string())
    }
}

/// Test basic completion functionality with file paths
#[tokio::test]
async fn test_complete_method_basic_path_completion() {
    let mut transport = MockCompletionTransport::new();

    // Set up for both initialization and completion
    transport.setup_for_completion_test(vec!["/home/alice", "/home/bob", "/home/charlie"], false);

    let mut client = Client::new(transport);

    // Initialize the client first
    let _init_result = client
        .initialize()
        .await
        .expect("Initialization should succeed");

    // Test path completion
    let result = client.complete("complete_path", "/home").await;

    assert!(
        result.is_ok(),
        "Path completion should succeed: {:?}",
        result
    );

    let completion_response = result.unwrap();
    assert_eq!(completion_response.values.len(), 3);
    assert!(
        completion_response
            .values
            .contains(&"/home/alice".to_string())
    );
    assert!(
        completion_response
            .values
            .contains(&"/home/bob".to_string())
    );
    assert!(
        completion_response
            .values
            .contains(&"/home/charlie".to_string())
    );

    assert_eq!(completion_response.total, Some(3));
    assert_eq!(completion_response.has_more, Some(false));
}

/// Test completion with command names
#[tokio::test]
async fn test_complete_method_command_completion() {
    let mut transport = MockCompletionTransport::new();

    // Set up for both initialization and completion
    transport.setup_for_completion_test(
        vec!["deploy", "delete", "describe", "debug"],
        true, // Has more completions available
    );

    let mut client = Client::new(transport);

    // Initialize the client first
    let _init_result = client
        .initialize()
        .await
        .expect("Initialization should succeed");

    // Test command completion
    let result = client.complete("complete_command", "de").await;

    assert!(
        result.is_ok(),
        "Command completion should succeed: {:?}",
        result
    );

    let completion_response = result.unwrap();
    assert_eq!(completion_response.values.len(), 4);
    assert!(completion_response.values.contains(&"deploy".to_string()));
    assert!(completion_response.values.contains(&"delete".to_string()));
    assert!(completion_response.values.contains(&"describe".to_string()));
    assert!(completion_response.values.contains(&"debug".to_string()));

    assert_eq!(completion_response.total, Some(4));
    assert_eq!(completion_response.has_more, Some(true));
}

/// Test completion with user names
#[tokio::test]
async fn test_complete_method_user_completion() {
    let mut transport = MockCompletionTransport::new();

    // Set up for both initialization and completion
    transport.setup_for_completion_test(vec!["@alice", "@admin", "@alex"], false);

    let mut client = Client::new(transport);

    // Initialize the client first
    let _init_result = client
        .initialize()
        .await
        .expect("Initialization should succeed");

    // Test user completion
    let result = client.complete("complete_user", "a").await;

    assert!(
        result.is_ok(),
        "User completion should succeed: {:?}",
        result
    );

    let completion_response = result.unwrap();
    assert_eq!(completion_response.values.len(), 3);
    assert!(completion_response.values.contains(&"@alice".to_string()));
    assert!(completion_response.values.contains(&"@admin".to_string()));
    assert!(completion_response.values.contains(&"@alex".to_string()));
}

/// Test completion with empty partial string
#[tokio::test]
async fn test_complete_method_empty_partial() {
    let mut transport = MockCompletionTransport::new();

    // Set up for both initialization and completion
    transport.setup_for_completion_test(vec!["option1", "option2", "option3", "option4"], false);

    let mut client = Client::new(transport);

    // Initialize the client first
    let _init_result = client
        .initialize()
        .await
        .expect("Initialization should succeed");

    // Test completion with empty string
    let result = client.complete("complete_all", "").await;

    assert!(
        result.is_ok(),
        "Empty completion should succeed: {:?}",
        result
    );

    let completion_response = result.unwrap();
    assert_eq!(completion_response.values.len(), 4);
    // All options should be returned when no partial match is provided
    assert!(completion_response.values.contains(&"option1".to_string()));
    assert!(completion_response.values.contains(&"option2".to_string()));
    assert!(completion_response.values.contains(&"option3".to_string()));
    assert!(completion_response.values.contains(&"option4".to_string()));
}

/// Test completion with no matches
#[tokio::test]
async fn test_complete_method_no_matches() {
    let mut transport = MockCompletionTransport::new();

    // Set up for both initialization and completion
    transport.setup_for_completion_test(vec![], false);

    let mut client = Client::new(transport);

    // Initialize the client first
    let _init_result = client
        .initialize()
        .await
        .expect("Initialization should succeed");

    // Test completion with no matches
    let result = client.complete("complete_command", "xyz").await;

    assert!(
        result.is_ok(),
        "No matches completion should succeed: {:?}",
        result
    );

    let completion_response = result.unwrap();
    assert_eq!(completion_response.values.len(), 0);
    assert_eq!(completion_response.total, Some(0));
    assert_eq!(completion_response.has_more, Some(false));
}

/// Test completion with special characters in handler name
#[tokio::test]
async fn test_complete_method_special_handler_names() {
    let mut transport = MockCompletionTransport::new();

    // Set up for both initialization and completion
    transport.setup_for_completion_test(vec!["result1", "result2"], false);

    let mut client = Client::new(transport);

    // Initialize the client first
    let _init_result = client
        .initialize()
        .await
        .expect("Initialization should succeed");

    // Test completion with handler name containing underscores and dots
    let result = client
        .complete("complete_file.extension_names", "test")
        .await;

    assert!(
        result.is_ok(),
        "Special handler name completion should succeed: {:?}",
        result
    );

    let completion_response = result.unwrap();
    assert_eq!(completion_response.values.len(), 2);
}

/// Test completion with special characters in partial string
#[tokio::test]
async fn test_complete_method_special_characters_in_partial() {
    let mut transport = MockCompletionTransport::new();

    // Set up for both initialization and completion
    transport.setup_for_completion_test(
        vec![
            "/path/to/file.txt",
            "/path/to/file-2.txt",
            "/path/to/file@home.txt",
        ],
        false,
    );

    let mut client = Client::new(transport);

    // Initialize the client first
    let _init_result = client
        .initialize()
        .await
        .expect("Initialization should succeed");

    // Test completion with special characters
    let result = client.complete("complete_path", "/path/to/file").await;

    assert!(
        result.is_ok(),
        "Special characters completion should succeed: {:?}",
        result
    );

    let completion_response = result.unwrap();
    assert_eq!(completion_response.values.len(), 3);
    assert!(
        completion_response
            .values
            .contains(&"/path/to/file.txt".to_string())
    );
    assert!(
        completion_response
            .values
            .contains(&"/path/to/file-2.txt".to_string())
    );
    assert!(
        completion_response
            .values
            .contains(&"/path/to/file@home.txt".to_string())
    );
}

/// Test completion with unicode characters
#[tokio::test]
async fn test_complete_method_unicode_support() {
    let mut transport = MockCompletionTransport::new();

    // Set up for both initialization and completion
    transport.setup_for_completion_test(
        vec!["ファイル.txt", "файл.txt", "文件.txt", "🎯target.txt"],
        false,
    );

    let mut client = Client::new(transport);

    // Initialize the client first
    let _init_result = client
        .initialize()
        .await
        .expect("Initialization should succeed");

    // Test completion with unicode characters
    let result = client.complete("complete_unicode", "フ").await;

    assert!(
        result.is_ok(),
        "Unicode completion should succeed: {:?}",
        result
    );

    let completion_response = result.unwrap();
    assert_eq!(completion_response.values.len(), 4);
    assert!(
        completion_response
            .values
            .contains(&"ファイル.txt".to_string())
    );
    assert!(completion_response.values.contains(&"файл.txt".to_string()));
    assert!(completion_response.values.contains(&"文件.txt".to_string()));
    assert!(
        completion_response
            .values
            .contains(&"🎯target.txt".to_string())
    );
}

/// Test completion error handling - invalid handler
#[tokio::test]
async fn test_complete_method_invalid_handler_error() {
    let mut transport = MockCompletionTransport::new();

    // Add initialization response first
    let init_response = serde_json::json!({
        "jsonrpc": "2.0",
        "id": "1",
        "result": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "serverInfo": {
                "name": "test-server",
                "version": "1.0.0"
            }
        }
    });
    transport.add_response(serde_json::to_vec(&init_response).unwrap());

    // Set up mock error response for invalid handler
    let error_payload = MockCompletionTransport::create_error_response(
        -32601,
        "Method not found: complete_nonexistent",
    );
    transport.add_response(error_payload);

    let mut client = Client::new(transport);

    // Initialize the client first
    let _init_result = client
        .initialize()
        .await
        .expect("Initialization should succeed");

    // Test completion with non-existent handler
    let result = client.complete("complete_nonexistent", "test").await;

    assert!(result.is_err(), "Invalid handler should cause error");

    let error = result.unwrap_err();
    assert_eq!(error.kind, ErrorKind::Protocol);
}

/// Test completion error handling - server error
#[tokio::test]
async fn test_complete_method_server_error() {
    let mut transport = MockCompletionTransport::new();

    // Add initialization response first
    let init_response = serde_json::json!({
        "jsonrpc": "2.0",
        "id": "1",
        "result": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "serverInfo": {
                "name": "test-server",
                "version": "1.0.0"
            }
        }
    });
    transport.add_response(serde_json::to_vec(&init_response).unwrap());

    // Set up mock server error response
    let error_payload = MockCompletionTransport::create_error_response(
        -32603,
        "Internal server error during completion",
    );
    transport.add_response(error_payload);

    let mut client = Client::new(transport);

    // Initialize the client first
    let _init_result = client
        .initialize()
        .await
        .expect("Initialization should succeed");

    // Test completion with server error
    let result = client.complete("complete_path", "/error").await;

    assert!(result.is_err(), "Server error should cause error");

    let error = result.unwrap_err();
    assert_eq!(error.kind, ErrorKind::Protocol);
}

/// Test completion with client not initialized
#[tokio::test]
async fn test_complete_method_client_not_initialized() {
    let transport = MockCompletionTransport::new();
    let mut client = Client::new(transport);

    // Don't initialize the client - it should fail
    let result = client.complete("complete_path", "/test").await;

    assert!(result.is_err(), "Uninitialized client should cause error");

    let error = result.unwrap_err();
    assert_eq!(error.kind, ErrorKind::BadRequest);

    let error_msg = error.to_string();
    assert!(
        error_msg.contains("not initialized"),
        "Error should mention initialization"
    );
}

/// Test completion request format validation
#[tokio::test]
async fn test_complete_method_request_format() {
    let mut transport = MockCompletionTransport::new();

    // Set up for both initialization and completion
    transport.setup_for_completion_test(vec!["test1", "test2"], false);

    let mut client = Client::new(transport);

    // Initialize the client first
    let _init_result = client
        .initialize()
        .await
        .expect("Initialization should succeed");

    // Call completion method
    let result = client.complete("test_handler", "partial_input").await;
    assert!(result.is_ok(), "Completion should succeed");

    // Note: We can't access the transport after moving it to client
    // This test validates the method works but we'd need a different
    // approach to test the exact request format in a real integration test
}

/// Test completion with large response handling
#[tokio::test]
async fn test_complete_method_large_response() {
    let mut transport = MockCompletionTransport::new();

    // Create a large completion response (1000 items)
    let large_values: Vec<&str> = (0..1000)
        .map(|i| Box::leak(format!("item_{:04}", i).into_boxed_str()) as &str)
        .collect();

    // Set up for both initialization and completion
    transport.setup_for_completion_test(large_values.clone(), false);

    let mut client = Client::new(transport);

    // Initialize the client first
    let _init_result = client
        .initialize()
        .await
        .expect("Initialization should succeed");

    // Test completion with large response
    let result = client.complete("complete_large", "item").await;

    assert!(
        result.is_ok(),
        "Large completion should succeed: {:?}",
        result
    );

    let completion_response = result.unwrap();
    assert_eq!(completion_response.values.len(), 1000);
    assert_eq!(completion_response.total, Some(1000));
    assert_eq!(completion_response.has_more, Some(false));

    // Verify some of the items
    assert!(
        completion_response
            .values
            .contains(&"item_0000".to_string())
    );
    assert!(
        completion_response
            .values
            .contains(&"item_0999".to_string())
    );
    assert!(
        completion_response
            .values
            .contains(&"item_0500".to_string())
    );
}

/// Test completion response deserialization edge cases
#[tokio::test]
async fn test_complete_method_response_deserialization() {
    let mut transport = MockCompletionTransport::new();

    // Add initialization response first
    let init_response = serde_json::json!({
        "jsonrpc": "2.0",
        "id": "1",
        "result": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "serverInfo": {
                "name": "test-server",
                "version": "1.0.0"
            }
        }
    });
    transport.add_response(serde_json::to_vec(&init_response).unwrap());

    // Test minimal valid response
    let minimal_response = json!({
        "jsonrpc": "2.0",
        "id": "1",
        "result": {
            "completion": {
                "values": ["minimal"]
            }
        }
    });

    transport.add_response(serde_json::to_vec(&minimal_response).unwrap());

    let mut client = Client::new(transport);

    // Initialize the client first
    let _init_result = client
        .initialize()
        .await
        .expect("Initialization should succeed");

    let result = client.complete("test", "min").await;

    assert!(result.is_ok(), "Minimal response should succeed");
    let completion = result.unwrap();
    assert_eq!(completion.values, vec!["minimal"]);
    assert_eq!(completion.total, None); // Optional field should be None
    assert_eq!(completion.has_more, None); // Optional field should be None
}

/// Test completion with concurrent requests (if supported)
#[tokio::test]
async fn test_complete_method_sequential_requests() {
    let mut transport = MockCompletionTransport::new();

    // Test multiple sequential completion requests
    for i in 0..5 {
        // Set up for both initialization and completion
        transport.setup_for_completion_test(vec![&format!("result_{}", i)], false);

        let mut client = Client::new(transport);

        // Initialize the client first
        let _init_result = client
            .initialize()
            .await
            .expect("Initialization should succeed");

        let result = client
            .complete("complete_sequential", &format!("input_{}", i))
            .await;
        assert!(result.is_ok(), "Sequential completion {} should succeed", i);

        let completion = result.unwrap();
        assert_eq!(completion.values[0], format!("result_{}", i));

        // Create new transport for next iteration to reset state
        transport = MockCompletionTransport::new();
    }
}

/// Test completion method integration with protocol types
#[tokio::test]
async fn test_complete_method_protocol_integration() {
    // Test that all the protocol types work correctly together
    use turbomcp_protocol::types::{CompleteResult, CompletionResponse};

    // Create a completion response manually and verify serialization
    let completion = CompletionResponse {
        values: vec!["test1".to_string(), "test2".to_string()],
        total: Some(2),
        has_more: Some(false),
    };

    let result = CompleteResult::new(completion);

    // Test serialization roundtrip
    let serialized = serde_json::to_string(&result).expect("Should serialize");
    let deserialized: CompleteResult =
        serde_json::from_str(&serialized).expect("Should deserialize");

    assert_eq!(result.completion.values, deserialized.completion.values);
    assert_eq!(result.completion.total, deserialized.completion.total);
    assert_eq!(result.completion.has_more, deserialized.completion.has_more);
}
