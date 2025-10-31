//! Integration tests for turbomcp-proxy
//!
//! Tests the complete proxy flow: initialization, introspection, and message routing.

use turbomcp_proxy::{
    prelude::*,
    proxy::{BackendConfig, BackendTransport},
};

#[tokio::test]
async fn test_proxy_service_creation() {
    // This test verifies that ProxyService can be created
    // Integration tests for backends require running subprocess servers,
    // which are covered by end-to-end testing in the main examples.

    // Placeholder for now - full integration tests require working stdio_server
    // ProxyService module structure verified - this test ensures the module compiles
}

#[tokio::test]
async fn test_backend_config_validation() {
    // Test BackendConfig creation and validation
    let backend_config = BackendConfig {
        transport: BackendTransport::Stdio {
            command: "echo".to_string(),
            args: vec!["test".to_string()],
            working_dir: None,
        },
        client_name: "test-client".to_string(),
        client_version: "1.0.0".to_string(),
    };

    // Verify config can be created
    assert_eq!(
        match &backend_config.transport {
            BackendTransport::Stdio { command, .. } => command.as_str(),
            _ => "unknown",
        },
        "echo"
    );
}

#[test]
fn test_backend_transport_types() {
    // Test that all backend transport variants can be created
    let _stdio = BackendTransport::Stdio {
        command: "cmd".to_string(),
        args: vec![],
        working_dir: None,
    };

    let _http = BackendTransport::Http {
        url: "http://localhost:3000".to_string(),
        auth_token: None,
    };

    let _tcp = BackendTransport::Tcp {
        host: "localhost".to_string(),
        port: 5000,
    };

    let _unix = BackendTransport::Unix {
        path: "/tmp/mcp.sock".to_string(),
    };

    let _ws = BackendTransport::WebSocket {
        url: "ws://localhost:3000".to_string(),
    };

    // All variants construct successfully - this test verifies enum variants are complete
}

#[tokio::test]
async fn test_id_translator_functionality() {
    use turbomcp_protocol::MessageId;

    // Test IdTranslator for message ID mapping
    let translator = IdTranslator::new();

    // Allocate a backend ID for a frontend request
    let frontend_id = MessageId::String("frontend-123".to_string());
    let backend_id = translator.allocate(frontend_id.clone()).unwrap();

    // Verify backend ID is sequential number
    assert!(matches!(backend_id, MessageId::Number(1)));

    // Reverse lookup
    let remapped = translator.get_frontend_id(&backend_id);
    assert_eq!(remapped, Some(frontend_id));
}
