//! `turbomcp::testing::connect`: a server and the real client, in memory, on
//! every revision, with or without middleware.
#![cfg(feature = "client")]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Map, json};
use turbomcp::McpRequest;
use turbomcp::client::{ClientBuilder, ConnectMode};
use turbomcp::prelude::*;
use turbomcp_core::ProtocolVersion;

#[derive(Clone)]
struct Adder;

#[server(name = "adder", version = "1.0.0")]
impl Adder {
    /// Add two numbers.
    #[tool]
    async fn add(&self, a: i64, b: i64) -> i64 {
        a + b
    }
}

fn args() -> Map<String, serde_json::Value> {
    json!({ "a": 2, "b": 3 }).as_object().unwrap().clone()
}

#[tokio::test]
async fn every_revision_connects_in_memory() {
    for (mode, version) in [
        (ConnectMode::Modern, ProtocolVersion::V2026_07_28),
        (ConnectMode::Legacy, ProtocolVersion::V2025_11_25),
    ] {
        let client = turbomcp::testing::connect(
            Adder.into_server(),
            ClientBuilder::new("t", "1.0.0").with_connect_mode(mode),
        )
        .await
        .expect("handshake");
        assert_eq!(client.protocol_version(), &version);
        let result = client.call_tool("add", args()).await.expect("call");
        assert_eq!(result.text_content().as_deref(), Some("5"));
    }
}

#[tokio::test]
async fn a_server_with_middleware_connects_too() {
    let seen = Arc::new(AtomicUsize::new(0));
    let count = {
        let seen = Arc::clone(&seen);
        move |req: McpRequest| {
            seen.fetch_add(1, Ordering::SeqCst);
            req
        }
    };
    let client = turbomcp::testing::connect(
        Adder
            .into_server()
            .layer(tower::util::MapRequestLayer::new(count)),
        ClientBuilder::new("t", "1.0.0"),
    )
    .await
    .expect("handshake");
    client.call_tool("add", args()).await.expect("call");
    assert!(
        seen.load(Ordering::SeqCst) >= 2,
        "the layer saw the traffic"
    );
}
