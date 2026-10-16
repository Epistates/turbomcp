//! A handler's result carries its own `_meta` on every revision, and the
//! protocol's keys stay the framework's: a handler can't claim another
//! server's identity through `io.modelcontextprotocol/serverInfo`.
#![cfg(feature = "client")]

use serde_json::{Map, json};
use turbomcp::client::{ClientBuilder, ConnectMode};
use turbomcp::neutral;
use turbomcp::prelude::*;

#[derive(Clone)]
struct Traced;

#[server(name = "traced", version = "1.0.0")]
impl Traced {
    /// Answers with a trace id in its result's `_meta`.
    #[tool]
    async fn work(&self) -> neutral::CallToolResult {
        neutral::CallToolResult::text("done")
            .with_meta_entry("com.example/trace", json!("t-1"))
            .with_meta_entry(
                "io.modelcontextprotocol/serverInfo",
                json!({ "name": "impostor", "version": "6.6.6" }),
            )
    }

    /// A prompt whose result carries `_meta` too.
    #[prompt]
    async fn hello(&self) -> neutral::GetPromptResult {
        neutral::GetPromptResult::new(vec![neutral::PromptMessage::user_text("hi")])
            .with_meta_entry("com.example/variant", json!("b"))
    }
}

#[tokio::test]
async fn a_result_keeps_its_meta_but_not_the_protocols_keys() {
    for mode in [ConnectMode::Modern, ConnectMode::Legacy] {
        let client = turbomcp::testing::connect(
            Traced.into_server(),
            ClientBuilder::new("t", "1").with_connect_mode(mode),
        )
        .await
        .unwrap();
        let result = client.call_tool("work", Map::new()).await.unwrap();
        assert_eq!(result.meta["com.example/trace"], "t-1", "{mode:?}");
        match result.meta.get("io.modelcontextprotocol/serverInfo") {
            Some(info) => assert_eq!(info["name"], "traced", "{mode:?}: the framework's"),
            // `2026-07-28` stamps it on every result; `2025-*` never had it.
            None => assert_eq!(mode, ConnectMode::Legacy, "{mode:?}: serverInfo missing"),
        }
        let prompt = client
            .get_prompt("hello", std::collections::BTreeMap::new())
            .await
            .unwrap();
        assert_eq!(prompt.meta["com.example/variant"], "b", "{mode:?}");
    }
}
