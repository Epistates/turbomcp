use turbomcp_transport_traits::{Transport, TransportType};
use turbomcp_websocket::{WebSocketBidirectionalConfig, WebSocketBidirectionalTransport};

#[tokio::test]
async fn test_websocket_config_builder() {
    let config = WebSocketBidirectionalConfig::client("ws://localhost:8080".to_string())
        .with_compression(true)
        .with_max_concurrent_elicitations(10);

    assert!(config.enable_compression);
    assert_eq!(config.max_concurrent_elicitations, 10);
}

#[tokio::test]
async fn test_transport_type() {
    let config = WebSocketBidirectionalConfig::client("ws://localhost:1".to_string());
    // Note: new() is async because it might perform initial setup
    if let Ok(transport) = WebSocketBidirectionalTransport::new(config).await {
        assert_eq!(transport.transport_type(), TransportType::WebSocket);
    }
}
