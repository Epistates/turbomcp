//! Phase 4 exit criterion, as a test: a single macro-generated `#[server]` type
//! answers `tools/call` identically over **stdio framing** and over the
//! **Streamable HTTP** endpoint — same server value, same build, two transports.
#![cfg(feature = "http")]

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tower::ServiceExt; // oneshot
use turbomcp::http::{HttpConfig, router};
use turbomcp::prelude::*;
use turbomcp::{SerdeJsonCodec, serve};
use turbomcp_transport_stdio::LineTransport;

#[derive(Clone)]
struct Demo;

#[server(name = "demo", version = "1.0.0")]
impl Demo {
    /// Echo a word back, upper-cased.
    #[tool(description = "Shout a word")]
    async fn shout(&self, word: String) -> McpResult<String> {
        Ok(word.to_uppercase())
    }
}

const META: &str = r#"{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}"#;

fn call_frame(id: u64, word: &str) -> String {
    format!(
        r#"{{"jsonrpc":"2.0","id":{id},"method":"tools/call","params":{{"name":"shout","arguments":{{"word":"{word}"}},"_meta":{META}}}}}"#
    )
}

#[tokio::test]
async fn macro_server_answers_tools_call_over_http() {
    let app = router(Demo.into_server().build(), HttpConfig::new());
    let req = Request::builder()
        .method("POST")
        .header("accept", "application/json, text/event-stream")
        .uri("/mcp")
        .header(header::CONTENT_TYPE, "application/json")
        // The draft envelope requires the mirrored request-metadata headers.
        .header("MCP-Protocol-Version", "2026-07-28")
        .header("Mcp-Method", "tools/call")
        .header("Mcp-Name", "shout")
        .body(Body::from(call_frame(1, "hi")))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let v: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["result"]["content"][0]["text"], "HI");
    assert_eq!(v["result"]["isError"], false);
}

#[tokio::test]
async fn macro_server_answers_tools_call_over_stdio() {
    // The SAME server value/build, driven through real line framing + the
    // concurrent serve loop over an in-memory duplex (a stdin/stdout stand-in).
    let (client, server) = tokio::io::duplex(64 * 1024);
    let (server_rx, server_tx) = tokio::io::split(server);
    let transport = LineTransport::new(BufReader::new(server_rx), server_tx, SerdeJsonCodec);
    let task = tokio::spawn(serve(transport, Demo.into_server().build()));

    let (client_rx, mut client_tx) = tokio::io::split(client);
    let mut client_rx = BufReader::new(client_rx);

    client_tx
        .write_all(format!("{}\n", call_frame(1, "hi")).as_bytes())
        .await
        .unwrap();
    client_tx.flush().await.unwrap();
    client_tx.shutdown().await.unwrap(); // EOF so the serve loop ends

    let mut line = String::new();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        client_rx.read_line(&mut line).await.unwrap();
    })
    .await
    .expect("a response should arrive");
    let v: Value = serde_json::from_str(line.trim()).unwrap();
    assert_eq!(v["result"]["content"][0]["text"], "HI");

    task.await.unwrap().expect("serve loop ends cleanly on EOF");
}
