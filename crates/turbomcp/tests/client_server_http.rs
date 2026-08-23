//! Phase 8c exit criterion: the typed [`Client`] drives a macro `#[server]` over
//! **Streamable HTTP** (real socket), both protocol versions. The same neutral
//! API as the stdio test — only the transport differs — proving the HTTP client
//! transport (POST + SSE + session-header capture) round-trips against the
//! dual-stack HTTP server.

#![cfg(all(feature = "client", feature = "http"))]

use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use serde_json::{Map, json};
use turbomcp::CancellationToken;
use turbomcp::client::{Client, ClientBuilder, ConnectMode, connect_http};
use turbomcp::http::{HttpConfig, ServeHttp};
use turbomcp::prelude::*;
use turbomcp_core::ProtocolVersion;

#[derive(Clone)]
struct Demo;

#[server(name = "demo", version = "1.0.0")]
impl Demo {
    /// Shout a word back, upper-cased.
    #[tool(description = "Shout a word")]
    async fn shout(&self, word: String) -> McpResult<String> {
        Ok(word.to_uppercase())
    }

    /// A fixed greeting resource.
    #[resource("demo://greeting")]
    async fn greeting(&self) -> McpResult<String> {
        Ok("hello there".to_string())
    }
}

/// Spawn the dual-stack HTTP server on an ephemeral port; return its `/mcp` URL
/// and the shutdown token.
async fn spawn_server() -> (String, CancellationToken) {
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    drop(listener);

    let shutdown = CancellationToken::new();
    let config = HttpConfig::new().with_shutdown(shutdown.clone());
    tokio::spawn(Demo.into_server().run_http(addr, config));
    // Give the listener a moment to bind.
    tokio::time::sleep(Duration::from_millis(100)).await;
    (format!("http://{addr}/mcp"), shutdown)
}

async fn exercise(client: &Client) {
    assert_eq!(client.server_info().map(|i| i.name.as_str()), Some("demo"));
    client.ping().await.expect("ping");

    let tools = client.list_tools(None).await.expect("list_tools");
    assert!(tools.tools.iter().any(|t| t.name == "shout"));

    let mut args = Map::new();
    args.insert("word".into(), json!("hi"));
    let result = client.call_tool("shout", args).await.expect("call_tool");
    assert!(matches!(&result.content[0], neutral::Content::Text { text, .. } if text == "HI"));

    let read = client
        .read_resource("demo://greeting")
        .await
        .expect("read_resource");
    assert!(
        matches!(&read.contents[0], neutral::ResourceContents::Text { text, .. } if text == "hello there")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn modern_over_http() {
    let (url, shutdown) = spawn_server().await;
    let client = connect_http(
        ClientBuilder::new("c", "1.0.0").with_connect_mode(ConnectMode::Modern),
        &url,
    )
    .await
    .expect("connect modern");
    assert_eq!(client.protocol_version(), &ProtocolVersion::V2026_07_28);
    exercise(&client).await;
    shutdown.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn legacy_over_http() {
    let (url, shutdown) = spawn_server().await;
    let client = connect_http(
        ClientBuilder::new("c", "1.0.0").with_connect_mode(ConnectMode::Legacy),
        &url,
    )
    .await
    .expect("connect legacy");
    assert_eq!(client.protocol_version(), &ProtocolVersion::V2025_11_25);
    exercise(&client).await;
    shutdown.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn auto_over_http_resolves_to_modern() {
    let (url, shutdown) = spawn_server().await;
    let client = connect_http(ClientBuilder::new("c", "1.0.0"), &url)
        .await
        .expect("connect auto");
    assert_eq!(client.protocol_version(), &ProtocolVersion::V2026_07_28);
    exercise(&client).await;
    shutdown.cancel();
}
