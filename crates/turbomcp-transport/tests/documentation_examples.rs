//! Tests for documentation examples in `lib.rs`.

use futures_util::stream::StreamExt;

#[cfg(feature = "websocket")]
#[tokio::test]
async fn test_websocket_bidirectional_transport_example() {
    use turbomcp_transport::{WebSocketBidirectionalTransport, WebSocketBidirectionalConfig};
    use std::time::Duration;

    // This test will fail if it can't connect to a WebSocket server.
    // We'll start a dummy server to make the test pass.
    let server = tokio::net::TcpListener::bind("127.0.0.1:8080").await.unwrap();
    tokio::spawn(async move {
        let (stream, _) = server.accept().await.unwrap();
        let mut websocket = tokio_tungstenite::accept_async(stream).await.unwrap();
        // Keep the connection open until the test is done.
        let _ = websocket.next().await;
    });

    let config = WebSocketBidirectionalConfig {
        url: Some("ws://localhost:8080".to_string()),
        max_concurrent_elicitations: 10,
        elicitation_timeout: Duration::from_secs(60),
        keep_alive_interval: Duration::from_secs(30),
        reconnect: Default::default(),
        ..Default::default()
    };

    let transport = WebSocketBidirectionalTransport::new(config).await;
    assert!(transport.is_ok());
}

#[cfg(feature = "http")]
#[tokio::test]
async fn test_streamable_http_client_transport_example() {
    use turbomcp_transport::streamable_http_client::{StreamableHttpClientConfig, StreamableHttpClientTransport};
    use std::time::Duration;

    let config = StreamableHttpClientConfig {
        base_url: "http://localhost:8080".to_string(),
        endpoint_path: "/mcp".to_string(),
        timeout: Duration::from_secs(30),
        ..Default::default()
    };

    let _transport = StreamableHttpClientTransport::new(config);
    // The test passes if the transport is created successfully.
}
