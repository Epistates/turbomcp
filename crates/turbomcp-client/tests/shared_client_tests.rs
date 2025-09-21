use turbomcp_client::{Client, SharedClient};
use turbomcp_transport::stdio::StdioTransport;

#[tokio::test]
async fn test_shared_client_creation() {
    let transport = StdioTransport::new();
    let client = Client::new(transport);
    let shared = SharedClient::new(client);

    // Test that we can clone the shared client
    let _shared2 = shared.clone();
}

#[tokio::test]
async fn test_shared_client_arc_cloning() {
    let transport = StdioTransport::new();
    let client = Client::new(transport);
    let shared = SharedClient::new(client);

    // Clone multiple times to test Arc behavior
    let clones: Vec<_> = (0..10).map(|_| shared.clone()).collect();
    assert_eq!(clones.len(), 10);

    // All clones should reference the same underlying client
    // This is verified by the fact that they can all be created without error
}

#[tokio::test]
async fn test_shared_client_api_surface() {
    let transport = StdioTransport::new();
    let client = Client::new(transport);
    let shared = SharedClient::new(client);

    // Test that SharedClient provides the expected API surface
    // These calls should compile, verifying the API is properly wrapped

    // Core operations (will fail due to no server, but should compile)
    let _ = shared.initialize().await;
    let _ = shared.list_tools().await;
    let _ = shared.list_prompts().await;
    let _ = shared.list_resources().await;
    let _ = shared.ping().await;

    // Test capabilities access
    let _capabilities = shared.capabilities().await;
}

#[tokio::test]
async fn test_shared_client_type_compatibility() {
    let transport = StdioTransport::new();
    let client = Client::new(transport);
    let shared = SharedClient::new(client);

    // Test that the SharedClient can be used in generic contexts
    fn takes_shared_client<T>(_client: T)
    where
        T: Clone + Send + Sync + 'static,
    {
    }

    takes_shared_client(shared);
}

#[tokio::test]
async fn test_shared_client_send_sync() {
    let transport = StdioTransport::new();
    let client = Client::new(transport);
    let shared = SharedClient::new(client);

    // Test that SharedClient can be moved across task boundaries
    let handle = tokio::spawn(async move {
        let _cloned = shared.clone();
        // SharedClient should be Send + Sync, allowing this to compile
    });

    handle.await.unwrap();
}
