//! Phase 8d, over HTTP: elicitation round-trips on both versions across the
//! Streamable HTTP transport. The draft drives the MRTR loop with the
//! `input_required` result returned as JSON (re-POST); legacy drives inline bidi
//! — the `tools/call` POST upgrades to SSE, the server's `elicitation/create`
//! arrives as an event, the client answers it on a *separate* POST, and the
//! original SSE stream then delivers the final result.

#![cfg(all(feature = "client", feature = "http"))]

use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use serde_json::{Map, json};
use turbomcp::CancellationToken;
use turbomcp::client::{
    Client, ClientBuilder, ConnectMode, ElicitationHandler, async_trait, connect_http,
};
use turbomcp::http::{HttpConfig, ServeHttp};
use turbomcp::prelude::*;

#[derive(Clone)]
struct FileManager;

#[server(name = "file-manager", version = "1.0.0")]
impl FileManager {
    /// Delete a path — but only after the user confirms via elicitation.
    #[tool(description = "Delete a path after user confirmation")]
    async fn delete(&self, ctx: &CallToolContext, path: String) -> McpResult<String> {
        let outcome = ctx
            .client
            .elicit(
                "confirm_delete",
                neutral::ElicitParams::new(
                    format!("Really delete {path}?"),
                    json!({ "type": "object", "properties": { "ok": { "type": "boolean" } } }),
                ),
            )
            .await?;
        if outcome.accepted() && outcome.content.get("ok").and_then(|v| v.as_bool()) == Some(true) {
            Ok(format!("deleted {path}"))
        } else {
            Ok(format!("kept {path}"))
        }
    }
}

struct Confirm {
    ok: bool,
}

#[async_trait]
impl ElicitationHandler for Confirm {
    async fn elicit(&self, _req: neutral::ElicitParams) -> neutral::ElicitOutcome {
        let mut content = Map::new();
        content.insert("ok".into(), json!(self.ok));
        neutral::ElicitOutcome::new(neutral::ElicitAction::Accept, content)
    }
}

async fn spawn_server() -> (String, CancellationToken) {
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    drop(listener);
    let shutdown = CancellationToken::new();
    let config = HttpConfig::new().with_shutdown(shutdown.clone());
    tokio::spawn(FileManager.into_server().run_http(addr, config));
    tokio::time::sleep(Duration::from_millis(100)).await;
    (format!("http://{addr}/mcp"), shutdown)
}

async fn connect(url: &str, mode: ConnectMode, ok: bool) -> Client {
    connect_http(
        ClientBuilder::new("confirmer", "1.0.0")
            .with_connect_mode(mode)
            .with_elicitation(Confirm { ok }),
        url,
    )
    .await
    .expect("connect")
}

async fn call_delete(client: &Client) -> String {
    let mut args = Map::new();
    args.insert("path".into(), json!("/tmp/x"));
    let result = client.call_tool("delete", args).await.expect("call_tool");
    match &result.content[0] {
        neutral::Content::Text { text, .. } => text.clone(),
        other => panic!("unexpected content {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn modern_mrtr_over_http() {
    let (url, shutdown) = spawn_server().await;
    let client = connect(&url, ConnectMode::Modern, true).await;
    assert_eq!(call_delete(&client).await, "deleted /tmp/x");
    shutdown.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn legacy_inline_bidi_over_http() {
    let (url, shutdown) = spawn_server().await;
    let client = connect(&url, ConnectMode::Legacy, true).await;
    assert_eq!(call_delete(&client).await, "deleted /tmp/x");
    shutdown.cancel();
}
