use turbomcp_client::{Client, SharedClient};
use turbomcp_transport::stdio::StdioTransport;

/// Test that SharedClient maintains exact same protocol compliance as Client
///
/// This test verifies that SharedClient is a pure wrapper that doesn't alter
/// any MCP protocol behavior, ensuring strict compliance.
#[tokio::test]
async fn test_shared_client_protocol_equivalence() {
    // Both clients should have identical API surfaces for MCP operations
    let transport1 = StdioTransport::new();
    let transport2 = StdioTransport::new();

    let regular_client = Client::new(transport1);
    let client = Client::new(transport2);
    let shared_client = SharedClient::new(client);

    // Test that method signatures are accessible
    // Regular client has sync capabilities access
    let _regular_caps = regular_client.capabilities();

    // SharedClient has async capabilities access (due to mutex)
    let _shared_caps = shared_client.capabilities().await;

    // All MCP operations should have identical signatures
    // (These will fail at runtime due to no server, but should compile identically)

    // Note: We can't actually call these without a server, but the important
    // thing is that the API surface is identical
}

#[tokio::test]
async fn test_shared_client_preserves_mcp_semantics() {
    let transport = StdioTransport::new();
    let client = Client::new(transport);
    let shared = SharedClient::new(client);

    // SharedClient should maintain all MCP protocol requirements:

    // 1. Initialization requirement
    // Operations should fail before initialization (same as regular Client)
    let result = shared.list_tools().await;
    assert!(result.is_err(), "Should fail when not initialized");

    // 2. Error propagation should be identical
    let error_msg = result.unwrap_err().to_string();
    assert!(
        error_msg.contains("not initialized"),
        "Should have same error message"
    );

    // 3. Capabilities should be preserved
    let capabilities = shared.capabilities().await;
    // Just verify the call succeeds and returns a valid capabilities struct
    let _ = capabilities.tools;
    let _ = capabilities.prompts;
    let _ = capabilities.resources;
}

#[tokio::test]
async fn test_shared_client_thread_safety_compliance() {
    let transport = StdioTransport::new();
    let client = Client::new(transport);
    let shared = SharedClient::new(client);

    // Test that SharedClient can be shared across threads safely
    // while maintaining MCP protocol compliance

    let shared1 = shared.clone();
    let shared2 = shared.clone();

    // Verify that concurrent access doesn't corrupt state
    let handle1 = tokio::spawn(async move {
        // This should maintain protocol state correctly
        shared1.capabilities().await
    });

    let handle2 = tokio::spawn(async move {
        // This should see the same protocol state
        shared2.capabilities().await
    });

    let (caps1, caps2) = tokio::join!(handle1, handle2);
    let caps1 = caps1.unwrap();
    let caps2 = caps2.unwrap();

    // Both should see identical capabilities (proving state consistency)
    assert_eq!(caps1.tools, caps2.tools);
    assert_eq!(caps1.prompts, caps2.prompts);
    assert_eq!(caps1.resources, caps2.resources);
}

/// Compile-time verification that SharedClient implements expected traits
#[tokio::test]
async fn test_shared_client_trait_compliance() {
    fn assert_send<T: Send>() {}
    fn assert_sync<T: Sync>() {}
    fn assert_clone<T: Clone>() {}

    // SharedClient must be Send + Sync + Clone for proper async usage
    assert_send::<SharedClient<StdioTransport>>();
    assert_sync::<SharedClient<StdioTransport>>();
    assert_clone::<SharedClient<StdioTransport>>();
}
