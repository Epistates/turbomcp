//! A real `Client` against a real server, over every network transport.
//!
//! Each transport has unit tests of its own, but nothing drove the client's
//! `connect_*` helpers against the server's `run_*` helpers, so a TCP client
//! that failed its first `initialize` against every server shipped unnoticed.
//! These run on the default single-threaded test runtime on purpose: that is
//! where a connection set up inside a task that has not run yet fails every
//! time instead of occasionally.

#![cfg(feature = "full-client")]

use std::collections::HashMap;
use std::future::Future;
use std::time::Duration;

use turbomcp::prelude::*;
use turbomcp_client::Client;
use turbomcp_transport::Transport;

#[derive(Clone)]
struct Echo;

#[turbomcp::server(name = "e2e", version = "1.0.0")]
impl Echo {
    #[tool("Echo a message")]
    async fn echo(&self, message: String) -> McpResult<String> {
        Ok(format!("echo: {message}"))
    }
}

/// A loopback address nothing is listening on yet.
fn free_addr() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().to_string()
}

/// Retry `connect` until the server spawned alongside it is listening.
async fn connect_when_up<T, E, F, Fut>(mut connect: F) -> T
where
    E: std::fmt::Debug,
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, E>>,
{
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        match connect().await {
            Ok(connected) => return connected,
            Err(e) if tokio::time::Instant::now() >= deadline => {
                panic!("server never accepted a client: {e:?}")
            }
            Err(_) => tokio::time::sleep(Duration::from_millis(50)).await,
        }
    }
}

/// List the tools and call one: a request after the handshake, both ways.
async fn exercise<T: Transport + 'static>(client: &Client<T>) {
    let tools = client.list_tools().await.expect("tools/list");
    assert_eq!(
        tools.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
        ["echo"]
    );

    let args = HashMap::from([("message".to_string(), serde_json::json!("hi"))]);
    let result = client
        .call_tool("echo", Some(args), None)
        .await
        .expect("tools/call");
    let result = serde_json::to_value(&result).unwrap();
    assert_eq!(result["content"][0]["text"], "echo: hi", "{result}");
}

#[cfg(feature = "tcp")]
#[tokio::test]
async fn tcp() {
    let addr = free_addr();
    let server_addr = addr.clone();
    tokio::spawn(async move { Echo.run_tcp(&server_addr).await });

    let client = connect_when_up(|| Client::connect_tcp(addr.clone())).await;
    exercise(&client).await;
}

#[cfg(all(unix, feature = "unix"))]
#[tokio::test]
async fn unix() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("e2e.sock");
    let server_path = path.to_string_lossy().into_owned();
    tokio::spawn(async move { Echo.run_unix(&server_path).await });

    let client = connect_when_up(|| Client::connect_unix(path.clone())).await;
    exercise(&client).await;
}

#[cfg(feature = "http")]
#[tokio::test]
async fn http() {
    let addr = free_addr();
    let server_addr = addr.clone();
    tokio::spawn(async move { Echo.run_http(&server_addr).await });

    let url = format!("http://{addr}");
    let client = connect_when_up(|| Client::connect_http(url.clone())).await;
    exercise(&client).await;
}

#[cfg(feature = "websocket")]
#[tokio::test]
async fn websocket() {
    use turbomcp_transport::websocket_bidirectional::{
        WebSocketBidirectionalTransport, config::WebSocketBidirectionalConfig,
    };

    let addr = free_addr();
    let server_addr = addr.clone();
    tokio::spawn(async move { Echo.run_websocket(&server_addr).await });

    let url = format!("ws://{addr}/");
    let client = connect_when_up(|| async {
        let config = WebSocketBidirectionalConfig {
            url: Some(url.clone()),
            ..Default::default()
        };
        let transport = WebSocketBidirectionalTransport::new(config).await?;
        let client = Client::new(transport);
        client
            .initialize()
            .await
            .map(|_| client)
            .map_err(|e| turbomcp_transport::TransportError::ConnectionFailed(e.to_string()))
    })
    .await;
    exercise(&client).await;
}
