//! Serving on `Http` wires session termination from the server, so a client
//! `DELETE` ends its session (204) without the user touching the terminator,
//! and a layer of middleware doesn't change that. The middleware path used to
//! be `serve_http` with a hand-built service, which answered `DELETE` with
//! `405` and accepted `MCP-Protocol-Version`s the server didn't serve.
#![cfg(feature = "http")]

use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use turbomcp::CancellationToken;
use turbomcp::http::{Http, HttpConfig};
use turbomcp::prelude::*;

#[derive(Clone)]
struct Greeter;

#[server(name = "greeter", version = "0.1.0")]
impl Greeter {
    /// Greet someone.
    #[tool]
    async fn greet(&self, name: String) -> McpResult<String> {
        Ok(format!("Hello, {name}!"))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn serving_on_http_honors_delete() {
    delete_is_honored(|http| tokio::spawn(Greeter.into_server().serve(http))).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_layer_of_middleware_keeps_delete() {
    delete_is_honored(|http| {
        tokio::spawn(
            Greeter
                .into_server()
                .layer(turbomcp::TracingLayer)
                .serve(http),
        )
    })
    .await;
}

type Serving = tokio::task::JoinHandle<Result<(), turbomcp::ProtocolError>>;

async fn delete_is_honored(serve: impl FnOnce(Http) -> Serving) {
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();

    let shutdown = CancellationToken::new();
    let config = HttpConfig::new().with_shutdown(shutdown.clone());
    let server = serve(Http::listener(listener).config(config));

    let url = format!("http://{addr}/mcp");
    let client = reqwest::Client::new();

    // initialize → a 2025-11-25 session is minted in the Mcp-Session-Id header.
    let resp = client
        .post(&url)
        .header("accept", "application/json, text/event-stream")
        .json(&serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": { "name": "c", "version": "1" },
            }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let sid = resp
        .headers()
        .get("mcp-session-id")
        .expect("session header")
        .to_str()
        .unwrap()
        .to_owned();

    // DELETE the session: the runtime wired the terminator, so this is honored.
    let resp = client
        .delete(&url)
        .header("mcp-session-id", &sid)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::NO_CONTENT);

    // The session is gone: a follow-up request 404s (re-initialize).
    let resp = client
        .post(&url)
        .header("accept", "application/json, text/event-stream")
        .header("mcp-session-id", &sid)
        .header("mcp-protocol-version", "2025-11-25")
        .json(&serde_json::json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::NOT_FOUND);

    shutdown.cancel();
    tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .expect("server shuts down")
        .unwrap()
        .expect("serving exits Ok");
}
