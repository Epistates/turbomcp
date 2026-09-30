//! `mask_internal_errors` on a `#[server]`: the path most internal errors take
//! (`?` on a driver error inside a `#[tool]`) reaches the log, not the model.

use serde_json::{Value, json};
use turbomcp::prelude::*;
use turbomcp::tower::{Service, ServiceExt};
use turbomcp::{JsonRpcMessage, JsonRpcRequest};

#[derive(Clone)]
struct Store;

#[server(name = "store", version = "1.0.0")]
impl Store {
    /// Look something up.
    #[tool]
    async fn lookup(&self, key: String) -> McpResult<String> {
        let _ = key;
        let parsed: Value = serde_json::from_str("{ upstream said: dsn=secret-dsn")?;
        Ok(parsed.to_string())
    }
}

async fn lookup(masked: bool) -> Value {
    let builder = Store.into_server();
    let builder = if masked {
        builder.mask_internal_errors()
    } else {
        builder
    };
    let mut svc = builder.build();
    let request = JsonRpcRequest::new(
        1,
        "tools/call",
        Some(json!({
            "name": "lookup", "arguments": { "key": "k" },
            "_meta": {
                "io.modelcontextprotocol/protocolVersion": "2026-07-28",
                "io.modelcontextprotocol/clientCapabilities": {},
            },
        })),
    );
    let reply = svc
        .ready()
        .await
        .unwrap()
        .call(request.into())
        .await
        .unwrap();
    let Some(JsonRpcMessage::Response(response)) = reply else {
        panic!("expected a response");
    };
    response
        .result
        .expect("an isError result, not a JSON-RPC error")
}

#[tokio::test]
async fn a_tools_serde_failure_is_masked() {
    let result = lookup(true).await;
    assert_eq!(result["isError"], true);
    let text = result["content"][0]["text"].as_str().unwrap();
    assert!(text.starts_with("internal error (ref: "), "{text}");
}

#[tokio::test]
async fn and_visible_when_not() {
    let result = lookup(false).await;
    assert_eq!(result["isError"], true);
    assert!(
        result["content"][0]["text"]
            .as_str()
            .unwrap()
            .starts_with("internal error: ")
    );
}
