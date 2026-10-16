//! Real-socket smoke test for [`serve_http`]: bind an ephemeral port, drive a
//! `tools/call` round-trip with a real HTTP client, then verify graceful
//! shutdown via the cancellation token. The `oneshot` tests cover the handler
//! stack; this covers the bind + accept loop + shutdown glue, and the limits the
//! accept loop enforces before any request exists.

use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use serde_json::{Value, json};
use turbomcp_core::{Implementation, McpResult};
use turbomcp_protocol::neutral;
use turbomcp_server::{
    CallToolContext, ListToolsContext, McpServerCore, MethodRouter, VersionDispatcher, WithTools,
};
use turbomcp_service::CancellationToken;
use turbomcp_transport_http::{HttpConfig, serve_http};

#[derive(Clone)]
struct Echo;

impl McpServerCore for Echo {
    fn server_info(&self) -> Implementation {
        Implementation::new("echo", "0.1.0")
    }
}

impl WithTools for Echo {
    async fn list_tools(
        &self,
        _ctx: &ListToolsContext,
        _params: neutral::ListParams,
    ) -> McpResult<neutral::ListToolsResult> {
        Ok(neutral::ListToolsResult::new(vec![neutral::Tool::new(
            "shout",
            json!({"type": "object"}),
        )]))
    }

    async fn call_tool(
        &self,
        _ctx: &CallToolContext,
        params: neutral::CallToolParams,
    ) -> McpResult<neutral::CallToolResult> {
        let word = params
            .arguments
            .get("word")
            .and_then(Value::as_str)
            .unwrap_or("");
        Ok(neutral::CallToolResult::text(word.to_uppercase()))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn round_trip_and_graceful_shutdown_over_a_real_socket() {
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    drop(listener); // free the port for serve_http to re-bind

    let shutdown = CancellationToken::new();
    let dispatcher = VersionDispatcher::new(Echo, MethodRouter::new().with_tools());
    let config = HttpConfig::new().with_shutdown(shutdown.clone());
    let server = tokio::spawn(serve_http(addr, dispatcher, config));

    // Give the listener a moment to bind.
    tokio::time::sleep(Duration::from_millis(100)).await;

    let url = format!("http://{addr}/mcp");
    let client = reqwest::Client::new();
    let resp = client
        .post(&url)
        .header("accept", "application/json, text/event-stream")
        // The draft envelope requires the mirrored request-metadata headers.
        .header("MCP-Protocol-Version", "2026-07-28")
        .header("Mcp-Method", "tools/call")
        .header("Mcp-Name", "shout")
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": "shout",
                "arguments": { "word": "hello" },
                "_meta": {
                    "io.modelcontextprotocol/protocolVersion": "2026-07-28",
                    "io.modelcontextprotocol/clientCapabilities": {},
                }
            }
        }))
        .send()
        .await
        .expect("request should reach the server");
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["result"]["content"][0]["text"], "HELLO");

    // Ask the server to stop; it should return cleanly.
    shutdown.cancel();
    let result = tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .expect("server should shut down promptly")
        .unwrap();
    result.expect("serve_http exits Ok on graceful shutdown");
}

/// Serve `Echo` on an ephemeral port with `config`; the address and the
/// shutdown token.
async fn serve(config: HttpConfig) -> (SocketAddr, CancellationToken) {
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let shutdown = CancellationToken::new();
    let dispatcher = VersionDispatcher::new(Echo, MethodRouter::new().with_tools());
    let http = turbomcp_transport_http::Http::listener(listener)
        .config(config.with_shutdown(shutdown.clone()));
    tokio::spawn(async move {
        use turbomcp_service::Serve as _;
        let _ = http.serve(dispatcher).await;
    });
    (addr, shutdown)
}

/// Whether the server has closed `stream` (a read returns EOF or an error)
/// within `within`.
async fn closed_within(stream: &mut tokio::net::TcpStream, within: Duration) -> bool {
    use tokio::io::AsyncReadExt as _;
    let mut buf = [0u8; 256];
    loop {
        match tokio::time::timeout(within, stream.read(&mut buf)).await {
            Ok(Ok(0) | Err(_)) => return true,
            Ok(Ok(_)) => {}
            Err(_) => return false,
        }
    }
}

/// A peer that dribbles its headers is cut off at the header-read timeout,
/// and so is one that connects and says nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_connection_slow_to_send_headers_is_closed() {
    use tokio::io::AsyncWriteExt as _;
    let (addr, shutdown) =
        serve(HttpConfig::new().header_read_timeout(Duration::from_millis(300))).await;

    let mut idle = tokio::net::TcpStream::connect(addr).await.unwrap();
    let mut slow = tokio::net::TcpStream::connect(addr).await.unwrap();
    slow.write_all(b"POST /mcp HTTP/1.1\r\nHost: x\r\n")
        .await
        .unwrap();
    assert!(
        closed_within(&mut idle, Duration::from_secs(3)).await,
        "idle"
    );
    assert!(
        closed_within(&mut slow, Duration::from_secs(3)).await,
        "slow"
    );
    shutdown.cancel();
}

/// At the connection cap the listener stops accepting: a further client is
/// not served until a connection closes.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_connection_cap_holds_new_clients_until_one_closes() {
    let (addr, shutdown) = serve(HttpConfig::new().max_connections(1)).await;
    let health = |addr: SocketAddr| async move {
        reqwest::Client::new()
            .post(format!("http://{addr}/mcp"))
            .header("accept", "application/json, text/event-stream")
            .header("content-type", "application/json")
            .body("{}")
            .timeout(Duration::from_millis(500))
            .send()
            .await
    };

    let hog = tokio::net::TcpStream::connect(addr).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        health(addr).await.is_err(),
        "a second connection is not served while the first holds the only slot"
    );
    drop(hog);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(health(addr).await.is_ok(), "served once the slot frees");
    shutdown.cancel();
}
