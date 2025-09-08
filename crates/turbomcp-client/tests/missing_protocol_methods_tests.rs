//! Comprehensive tests for missing protocol methods implementation
//!
//! This test suite validates the implementation of all missing MCP protocol methods
//! following TurboMCP's strict TDD standards. Since we can't easily mock the internal
//! transport, we'll test by verifying that the methods:
//! 1. Fail appropriately when client is not initialized
//! 2. Have correct function signatures and error handling
//! 3. Return expected error types for invalid inputs

use turbomcp_client::Client;
use turbomcp_core::ErrorKind;
use turbomcp_protocol::types::LogLevel;
use turbomcp_transport::stdio::StdioTransport;

// ============================================================================
// PING METHOD TESTS
// ============================================================================

#[tokio::test]
async fn test_ping_method_client_not_initialized() {
    // Test that ping() fails when client is not initialized
    let transport = StdioTransport::new();
    let mut client = Client::new(transport);

    // Don't initialize client - call ping directly
    let result = client.ping().await;

    // Should fail with initialization error
    assert!(
        result.is_err(),
        "Ping should fail when client not initialized"
    );

    let error = result.unwrap_err();
    match error.kind {
        ErrorKind::BadRequest => {} // Expected
        _ => panic!(
            "Should be BadRequest error for uninitialized client: {:?}",
            error
        ),
    }
}

// ============================================================================
// READ_RESOURCE METHOD TESTS
// ============================================================================

#[tokio::test]
async fn test_read_resource_method_client_not_initialized() {
    let transport = StdioTransport::new();
    let mut client = Client::new(transport);

    // Don't initialize client
    let result = client.read_resource("file:///test.txt").await;

    // Should fail with initialization error
    assert!(
        result.is_err(),
        "read_resource should fail when client not initialized"
    );

    let error = result.unwrap_err();
    match error.kind {
        ErrorKind::BadRequest => {} // Expected
        _ => panic!("Should be BadRequest error: {:?}", error),
    }
}

#[tokio::test]
async fn test_read_resource_empty_uri() {
    let transport = StdioTransport::new();
    let mut client = Client::new(transport);

    // Test empty URI - should fail even without initialization
    let result = client.read_resource("").await;
    assert!(result.is_err(), "Should fail with empty URI");

    let error = result.unwrap_err();
    match error.kind {
        ErrorKind::BadRequest => {} // Expected
        _ => panic!("Should be BadRequest error for empty URI: {:?}", error),
    }
}

// ============================================================================
// LIST_PROMPTS METHOD TESTS
// ============================================================================

#[tokio::test]
async fn test_list_prompts_method_client_not_initialized() {
    let transport = StdioTransport::new();
    let mut client = Client::new(transport);

    // Don't initialize client
    let result = client.list_prompts().await;

    // Should fail with initialization error
    assert!(
        result.is_err(),
        "list_prompts should fail when client not initialized"
    );

    let error = result.unwrap_err();
    match error.kind {
        ErrorKind::BadRequest => {} // Expected
        _ => panic!("Should be BadRequest error: {:?}", error),
    }
}

// ============================================================================
// GET_PROMPT METHOD TESTS
// ============================================================================

#[tokio::test]
async fn test_get_prompt_method_client_not_initialized() {
    let transport = StdioTransport::new();
    let mut client = Client::new(transport);

    // Don't initialize client
    let result = client.get_prompt("greeting").await;

    // Should fail with initialization error
    assert!(
        result.is_err(),
        "get_prompt should fail when client not initialized"
    );

    let error = result.unwrap_err();
    match error.kind {
        ErrorKind::BadRequest => {} // Expected
        _ => panic!("Should be BadRequest error: {:?}", error),
    }
}

#[tokio::test]
async fn test_get_prompt_empty_name() {
    let transport = StdioTransport::new();
    let mut client = Client::new(transport);

    // Test empty name - should fail even without initialization
    let result = client.get_prompt("").await;
    assert!(result.is_err(), "Should fail with empty prompt name");

    let error = result.unwrap_err();
    match error.kind {
        ErrorKind::BadRequest => {} // Expected
        _ => panic!("Should be BadRequest error for empty name: {:?}", error),
    }
}

// ============================================================================
// LIST_ROOTS METHOD TESTS
// ============================================================================

#[tokio::test]
async fn test_list_roots_method_client_not_initialized() {
    let transport = StdioTransport::new();
    let mut client = Client::new(transport);

    // Don't initialize client
    let result = client.list_roots().await;

    // Should fail with initialization error
    assert!(
        result.is_err(),
        "list_roots should fail when client not initialized"
    );

    let error = result.unwrap_err();
    match error.kind {
        ErrorKind::BadRequest => {} // Expected
        _ => panic!("Should be BadRequest error: {:?}", error),
    }
}

// ============================================================================
// SET_LOG_LEVEL METHOD TESTS
// ============================================================================

#[tokio::test]
async fn test_set_log_level_method_client_not_initialized() {
    let transport = StdioTransport::new();
    let mut client = Client::new(transport);

    // Don't initialize client
    let result = client.set_log_level(LogLevel::Debug).await;

    // Should fail with initialization error
    assert!(
        result.is_err(),
        "set_log_level should fail when client not initialized"
    );

    let error = result.unwrap_err();
    match error.kind {
        ErrorKind::BadRequest => {} // Expected
        _ => panic!("Should be BadRequest error: {:?}", error),
    }
}

#[tokio::test]
async fn test_set_log_level_all_levels_client_not_initialized() {
    let levels = vec![
        LogLevel::Error,
        LogLevel::Warning,
        LogLevel::Info,
        LogLevel::Debug,
    ];

    for level in levels {
        let transport = StdioTransport::new();
        let mut client = Client::new(transport);

        // Test each level without initialization
        let result = client.set_log_level(level).await;
        assert!(
            result.is_err(),
            "Should fail for {:?} level without initialization",
            level
        );

        let error = result.unwrap_err();
        match error.kind {
            ErrorKind::BadRequest => {} // Expected
            _ => panic!("Should be BadRequest error for {:?}: {:?}", level, error),
        }
    }
}

// ============================================================================
// SUBSCRIPTION METHOD TESTS
// ============================================================================

#[tokio::test]
async fn test_subscribe_method_client_not_initialized() {
    let transport = StdioTransport::new();
    let mut client = Client::new(transport);

    // Don't initialize client
    let result = client.subscribe("file:///watch/directory").await;

    // Should fail with initialization error
    assert!(
        result.is_err(),
        "subscribe should fail when client not initialized"
    );

    let error = result.unwrap_err();
    match error.kind {
        ErrorKind::BadRequest => {} // Expected
        _ => panic!("Should be BadRequest error: {:?}", error),
    }
}

#[tokio::test]
async fn test_subscribe_empty_uri() {
    let transport = StdioTransport::new();
    let mut client = Client::new(transport);

    // Test empty URI - should fail even without initialization
    let result = client.subscribe("").await;
    assert!(result.is_err(), "Should fail with empty subscription URI");

    let error = result.unwrap_err();
    match error.kind {
        ErrorKind::BadRequest => {} // Expected
        _ => panic!("Should be BadRequest error for empty URI: {:?}", error),
    }
}

#[tokio::test]
async fn test_unsubscribe_method_client_not_initialized() {
    let transport = StdioTransport::new();
    let mut client = Client::new(transport);

    // Don't initialize client
    let result = client.unsubscribe("file:///watch/directory").await;

    // Should fail with initialization error
    assert!(
        result.is_err(),
        "unsubscribe should fail when client not initialized"
    );

    let error = result.unwrap_err();
    match error.kind {
        ErrorKind::BadRequest => {} // Expected
        _ => panic!("Should be BadRequest error: {:?}", error),
    }
}

#[tokio::test]
async fn test_unsubscribe_empty_uri() {
    let transport = StdioTransport::new();
    let mut client = Client::new(transport);

    // Test empty URI - should fail even without initialization
    let result = client.unsubscribe("").await;
    assert!(result.is_err(), "Should fail with empty unsubscription URI");

    let error = result.unwrap_err();
    match error.kind {
        ErrorKind::BadRequest => {} // Expected
        _ => panic!("Should be BadRequest error for empty URI: {:?}", error),
    }
}

// ============================================================================
// LIST_RESOURCE_TEMPLATES METHOD TESTS
// ============================================================================

#[tokio::test]
async fn test_list_resource_templates_method_client_not_initialized() {
    let transport = StdioTransport::new();
    let mut client = Client::new(transport);

    // Don't initialize client
    let result = client.list_resource_templates().await;

    // Should fail with initialization error
    assert!(
        result.is_err(),
        "list_resource_templates should fail when client not initialized"
    );

    let error = result.unwrap_err();
    match error.kind {
        ErrorKind::BadRequest => {} // Expected
        _ => panic!("Should be BadRequest error: {:?}", error),
    }
}

// ============================================================================
// INTEGRATION TESTS - METHOD SIGNATURES AND ERROR HANDLING
// ============================================================================

#[tokio::test]
async fn test_all_missing_methods_exist_and_handle_uninitialized_client() {
    // This test ensures all methods exist with correct signatures and error handling
    let transport = StdioTransport::new();
    let mut client = Client::new(transport);

    // Test that all methods exist and fail appropriately when client is uninitialized
    let ping_result = client.ping().await;
    assert!(ping_result.is_err());

    let read_resource_result = client.read_resource("file:///test").await;
    assert!(read_resource_result.is_err());

    let list_prompts_result = client.list_prompts().await;
    assert!(list_prompts_result.is_err());

    let get_prompt_result = client.get_prompt("test").await;
    assert!(get_prompt_result.is_err());

    let list_roots_result = client.list_roots().await;
    assert!(list_roots_result.is_err());

    let set_log_level_result = client.set_log_level(LogLevel::Info).await;
    assert!(set_log_level_result.is_err());

    let subscribe_result = client.subscribe("file:///test").await;
    assert!(subscribe_result.is_err());

    let unsubscribe_result = client.unsubscribe("file:///test").await;
    assert!(unsubscribe_result.is_err());

    let list_resource_templates_result = client.list_resource_templates().await;
    assert!(list_resource_templates_result.is_err());
}

#[tokio::test]
async fn test_all_methods_validate_empty_string_parameters() {
    // Test that methods properly validate empty string parameters
    let transport = StdioTransport::new();
    let mut client = Client::new(transport);

    // These should fail due to empty parameters, not initialization
    let read_resource_result = client.read_resource("").await;
    assert!(read_resource_result.is_err());

    let get_prompt_result = client.get_prompt("").await;
    assert!(get_prompt_result.is_err());

    let subscribe_result = client.subscribe("").await;
    assert!(subscribe_result.is_err());

    let unsubscribe_result = client.unsubscribe("").await;
    assert!(unsubscribe_result.is_err());
}

// Test that methods have correct return types by compiling successful calls
#[tokio::test]
async fn test_method_return_types_compilation() {
    // This test ensures methods have correct return types - it won't run successfully
    // due to transport issues, but it will compile correctly if types are right
    let transport = StdioTransport::new();
    let mut client = Client::new(transport);

    // These calls verify return types compile correctly
    let _: Result<_, _> = client.ping().await;
    let _: Result<_, _> = client.read_resource("file:///test").await;
    let _: Result<Vec<String>, _> = client.list_prompts().await;
    let _: Result<_, _> = client.get_prompt("test").await;
    let _: Result<Vec<String>, _> = client.list_roots().await;
    let _: Result<_, _> = client.set_log_level(LogLevel::Info).await;
    let _: Result<_, _> = client.subscribe("file:///test").await;
    let _: Result<_, _> = client.unsubscribe("file:///test").await;
    let _: Result<Vec<String>, _> = client.list_resource_templates().await;
}
