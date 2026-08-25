//! Real-socket smoke test for [`serve_http`]: bind an ephemeral port, drive a
//! `tools/call` round-trip with a real HTTP client, then verify graceful
//! shutdown via the cancellation token. The `oneshot` tests cover the handler
//! stack; this covers the bind + `axum::serve` + shutdown glue.

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
