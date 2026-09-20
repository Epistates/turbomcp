//! WebSocket server-to-client operations, over a real socket.
//!
//! Until 3.5.0 the WebSocket transport built its `RequestContext` with no
//! `McpSession` attached, so `sample()`, `elicit_form()`, `elicit_url()` and
//! `notify_client()` all failed with `capability_not_supported` — the one
//! transport where server-initiated work was impossible. A test double would
//! not have caught that, because the bug was the absence of the wiring, so this
//! drives an actual connection.

#![cfg(feature = "websocket")]
// `McpHandler`'s methods return `impl Future`, so hand-implementing it needs
// the explicit form; `async fn` would not satisfy the trait's `MaybeSend` bound.
#![allow(clippy::manual_async_fn)]

use std::time::Duration;

use futures::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;
use turbomcp_core::context::RequestContext;
use turbomcp_core::error::{McpError, McpResult};
use turbomcp_core::handler::McpHandler;
use turbomcp_types::{
    Prompt, PromptResult, Resource, ResourceResult, ServerInfo, Tool, ToolResult,
};

#[derive(Clone)]
struct RootsEchoHandler;

impl McpHandler for RootsEchoHandler {
    fn server_info(&self) -> ServerInfo {
        ServerInfo::new("ws-session", "1.0.0")
    }

    fn list_tools(&self) -> Vec<Tool> {
        vec![Tool {
            name: "count_roots".to_string(),
            ..Default::default()
        }]
    }

    fn list_resources(&self) -> Vec<Resource> {
        Vec::new()
    }

    fn list_prompts(&self) -> Vec<Prompt> {
        Vec::new()
    }

    fn call_tool<'a>(
        &'a self,
        name: &'a str,
        _args: Value,
        ctx: &'a RequestContext,
    ) -> impl std::future::Future<Output = McpResult<ToolResult>> + Send + 'a {
        async move {
            match name {
                // Round-trip: server asks the client a question and awaits the
                // answer. Exercises `call` plus response correlation.
                "count_roots" => {
                    let roots = ctx.list_roots().await?;
                    ctx.notify_client("notifications/message", json!({ "level": "info" }))
                        .await?;
                    Ok(ToolResult::text(format!("{}", roots.len())))
                }
                other => Err(McpError::tool_not_found(other)),
            }
        }
    }

    fn read_resource<'a>(
        &'a self,
        uri: &'a str,
        _ctx: &'a RequestContext,
    ) -> impl std::future::Future<Output = McpResult<ResourceResult>> + Send + 'a {
        async move { Err(McpError::resource_not_found(uri)) }
    }

    fn get_prompt<'a>(
        &'a self,
        name: &'a str,
        _args: Option<Value>,
        _ctx: &'a RequestContext,
    ) -> impl std::future::Future<Output = McpResult<PromptResult>> + Send + 'a {
        async move { Err(McpError::prompt_not_found(name)) }
    }
}

/// Bind an ephemeral port, then serve on it so the test never races another.
async fn spawn_server() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);

    let bind = addr.to_string();
    let serve_addr = bind.clone();
    tokio::spawn(async move {
        let _ = turbomcp_server::transport::websocket::run(&RootsEchoHandler, &serve_addr).await;
    });

    // Wait for the listener to come up rather than sleeping a fixed amount.
    for _ in 0..100 {
        if TcpListener::bind(addr).await.is_err() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    format!("ws://{bind}/ws")
}

async fn recv_json(
    socket: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) -> Value {
    loop {
        let msg = tokio::time::timeout(Duration::from_secs(5), socket.next())
            .await
            .expect("timed out waiting for a frame")
            .expect("socket closed")
            .expect("websocket error");
        if let Message::Text(text) = msg {
            return serde_json::from_str(&text).expect("server sent invalid JSON");
        }
    }
}

#[tokio::test]
async fn websocket_handlers_can_reach_the_client() {
    let url = spawn_server().await;
    let (mut socket, _) = tokio_tungstenite::connect_async(&url)
        .await
        .expect("failed to connect");

    // Declare the roots capability so the server's capability gate passes.
    socket
        .send(Message::Text(
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-11-25",
                    "clientInfo": { "name": "test-client", "version": "1.0.0" },
                    "capabilities": { "roots": { "listChanged": true } }
                }
            })
            .to_string()
            .into(),
        ))
        .await
        .unwrap();

    let init = recv_json(&mut socket).await;
    assert_eq!(init["id"], 1, "expected the initialize response");
    assert!(init["result"]["protocolVersion"].is_string());

    // Now call the tool. The server must ask us for roots before answering.
    socket
        .send(Message::Text(
            json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/call",
                "params": { "name": "count_roots", "arguments": {} }
            })
            .to_string()
            .into(),
        ))
        .await
        .unwrap();

    // The next frame is a server-initiated *request*, not the tool response.
    let roots_request = recv_json(&mut socket).await;
    assert_eq!(
        roots_request["method"], "roots/list",
        "the handler's ctx.list_roots() must reach the socket"
    );
    let server_request_id = roots_request["id"].clone();
    assert!(!server_request_id.is_null(), "a request must carry an id");

    // Answer it. The server has to correlate this back to the parked handler.
    socket
        .send(Message::Text(
            json!({
                "jsonrpc": "2.0",
                "id": server_request_id,
                "result": {
                    "roots": [
                        { "uri": "file:///a", "name": "a" },
                        { "uri": "file:///b", "name": "b" }
                    ]
                }
            })
            .to_string()
            .into(),
        ))
        .await
        .unwrap();

    // Then a notification, then the tool's response.
    let notification = recv_json(&mut socket).await;
    assert_eq!(notification["method"], "notifications/message");
    assert!(
        notification.get("id").is_none() || notification["id"].is_null(),
        "notifications carry no id"
    );

    let tool_response = recv_json(&mut socket).await;
    assert_eq!(tool_response["id"], 2);
    assert_eq!(
        tool_response["result"]["content"][0]["text"], "2",
        "the handler saw both roots we sent"
    );
}

/// A reply the server has no pending request for must be dropped quietly, not
/// answered with a parse error.
#[tokio::test]
async fn unsolicited_responses_are_ignored() {
    let url = spawn_server().await;
    let (mut socket, _) = tokio_tungstenite::connect_async(&url)
        .await
        .expect("failed to connect");

    socket
        .send(Message::Text(
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-11-25",
                    "clientInfo": { "name": "test-client", "version": "1.0.0" },
                    "capabilities": {}
                }
            })
            .to_string()
            .into(),
        ))
        .await
        .unwrap();
    let _ = recv_json(&mut socket).await;

    // Reply to nothing.
    socket
        .send(Message::Text(
            json!({ "jsonrpc": "2.0", "id": "s-999", "result": {} })
                .to_string()
                .into(),
        ))
        .await
        .unwrap();

    // The connection must still serve requests afterwards.
    socket
        .send(Message::Text(
            json!({ "jsonrpc": "2.0", "id": 3, "method": "ping" })
                .to_string()
                .into(),
        ))
        .await
        .unwrap();

    let pong = recv_json(&mut socket).await;
    assert_eq!(pong["id"], 3);
    assert!(
        pong["error"].is_null(),
        "stray response poisoned the stream"
    );
}

/// A panicking handler must still produce a response. Before 3.5.0 the spawned
/// task unwound before reaching the send, so the client waited forever on an id
/// that would never be answered — and most MCP clients have no per-request
/// timeout, which made that wait permanent.
#[tokio::test]
async fn a_panicking_handler_still_answers() {
    #[derive(Clone)]
    struct Panicky;

    impl McpHandler for Panicky {
        fn server_info(&self) -> ServerInfo {
            ServerInfo::new("panicky", "1.0.0")
        }
        fn list_tools(&self) -> Vec<Tool> {
            vec![Tool {
                name: "boom".to_string(),
                ..Default::default()
            }]
        }
        fn list_resources(&self) -> Vec<Resource> {
            Vec::new()
        }
        fn list_prompts(&self) -> Vec<Prompt> {
            Vec::new()
        }
        fn call_tool<'a>(
            &'a self,
            _name: &'a str,
            _args: Value,
            _ctx: &'a RequestContext,
        ) -> impl std::future::Future<Output = McpResult<ToolResult>> + Send + 'a {
            async move { panic!("handler exploded") }
        }
        fn read_resource<'a>(
            &'a self,
            uri: &'a str,
            _ctx: &'a RequestContext,
        ) -> impl std::future::Future<Output = McpResult<ResourceResult>> + Send + 'a {
            async move { Err(McpError::resource_not_found(uri)) }
        }
        fn get_prompt<'a>(
            &'a self,
            name: &'a str,
            _args: Option<Value>,
            _ctx: &'a RequestContext,
        ) -> impl std::future::Future<Output = McpResult<PromptResult>> + Send + 'a {
            async move { Err(McpError::prompt_not_found(name)) }
        }
    }

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let bind = addr.to_string();
    let serve = bind.clone();
    tokio::spawn(async move {
        let _ = turbomcp_server::transport::websocket::run(&Panicky, &serve).await;
    });
    for _ in 0..100 {
        if TcpListener::bind(addr).await.is_err() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    let (mut socket, _) = tokio_tungstenite::connect_async(&format!("ws://{bind}/ws"))
        .await
        .expect("failed to connect");

    socket
        .send(Message::Text(
            json!({
                "jsonrpc": "2.0", "id": 1, "method": "initialize",
                "params": {
                    "protocolVersion": "2025-11-25",
                    "clientInfo": { "name": "t", "version": "1" },
                    "capabilities": {}
                }
            })
            .to_string()
            .into(),
        ))
        .await
        .unwrap();
    let _ = recv_json(&mut socket).await;

    socket
        .send(Message::Text(
            json!({
                "jsonrpc": "2.0", "id": 7, "method": "tools/call",
                "params": { "name": "boom", "arguments": {} }
            })
            .to_string()
            .into(),
        ))
        .await
        .unwrap();

    // The point of the test: this returns rather than timing out.
    let response = recv_json(&mut socket).await;
    assert_eq!(
        response["id"], 7,
        "the panic must be answered on its own id"
    );
    assert_eq!(
        response["error"]["code"], -32603,
        "a panic is a server fault, got {response}"
    );

    // And the connection survives it.
    socket
        .send(Message::Text(
            json!({ "jsonrpc": "2.0", "id": 8, "method": "ping" })
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    assert_eq!(recv_json(&mut socket).await["id"], 8);
}
