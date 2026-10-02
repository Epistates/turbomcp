//! Each transport tells the server where a request came from
//! ([`NetworkFacts`]), for the network attributes telemetry records.
#![cfg(all(feature = "http", feature = "websocket", feature = "client"))]

use std::sync::{Arc, Mutex};

use turbomcp::client::{ClientBuilder, ConnectMode, connect_http, connect_websocket};
use turbomcp::http::{Http, HttpConfig, WebSocketConfig};
use turbomcp::prelude::*;
use turbomcp::{CancellationToken, McpRequest};
use turbomcp_service::NetworkFacts;

#[derive(Clone)]
struct Echo;

#[server(name = "echo", version = "1.0.0")]
impl Echo {
    /// Echo.
    #[tool]
    async fn echo(&self, text: String) -> String {
        text
    }
}

/// Serve `Echo` over HTTP (and WebSocket at `/ws`), recording the facts on
/// every request it receives.
async fn serve() -> (String, Arc<Mutex<Vec<NetworkFacts>>>, CancellationToken) {
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let record = {
        let seen = Arc::clone(&seen);
        move |req: McpRequest| {
            if let Some(facts) = req.extensions.get::<NetworkFacts>() {
                seen.lock().unwrap().push(facts.clone());
            }
            req
        }
    };
    let shutdown = CancellationToken::new();
    tokio::spawn(
        Echo.into_server()
            .layer(tower::util::MapRequestLayer::new(record))
            .serve(
                Http::listener(listener).config(
                    HttpConfig::new()
                        .with_websocket(WebSocketConfig::new("/ws"))
                        .with_shutdown(shutdown.clone()),
                ),
            ),
    );
    (addr.to_string(), seen, shutdown)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn http_requests_carry_the_client_and_the_http_version() {
    let (addr, seen, shutdown) = serve().await;
    let client = connect_http(
        ClientBuilder::new("c", "1.0.0").with_connect_mode(ConnectMode::Legacy),
        &format!("http://{addr}/mcp"),
    )
    .await
    .expect("connect");
    client.list_tools(None).await.expect("list");
    let seen = seen.lock().unwrap();
    let facts = seen.last().expect("facts on the request");
    assert_eq!(facts.transport, "tcp");
    assert_eq!(facts.protocol_name, Some("http"));
    assert_eq!(facts.protocol_version, Some("1.1"));
    assert_eq!(facts.peer_address.as_deref(), Some("127.0.0.1"));
    assert!(facts.peer_port.is_some());
    shutdown.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn websocket_requests_carry_the_client() {
    let (addr, seen, shutdown) = serve().await;
    let transport = connect_websocket(&format!("ws://{addr}/ws"))
        .await
        .expect("upgrade");
    let client = ClientBuilder::new("c", "1.0.0")
        .with_connect_mode(ConnectMode::Legacy)
        .connect(transport)
        .await
        .expect("connect");
    client.list_tools(None).await.expect("list");
    let seen = seen.lock().unwrap();
    let facts = seen.last().expect("facts on the request");
    assert_eq!(facts.transport, "tcp");
    assert_eq!(facts.protocol_name, Some("websocket"));
    assert_eq!(facts.peer_address.as_deref(), Some("127.0.0.1"));
    assert!(facts.peer_port.is_some());
    shutdown.cancel();
}
