//! `testing::serve` without the client: a server-only crate sends frames to
//! a server in memory and reads its answers.

use serde_json::json;
use turbomcp::prelude::*;
use turbomcp::{JsonRpcMessage, JsonRpcRequest, Transport as _};

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

#[tokio::test]
async fn a_server_answers_frames_in_memory() {
    let mut end = turbomcp::testing::serve(Adder.into_server());
    let request = JsonRpcRequest::new(
        1,
        "tools/call",
        Some(json!({
            "name": "add",
            "arguments": { "a": 2, "b": 3 },
            "_meta": {
                "io.modelcontextprotocol/protocolVersion": "2026-07-28",
                "io.modelcontextprotocol/clientCapabilities": {},
            },
        })),
    );
    end.send(request.into()).await.unwrap();
    let Some(JsonRpcMessage::Response(response)) = end.recv().await.unwrap() else {
        panic!("expected a response");
    };
    assert_eq!(response.result.unwrap()["content"][0]["text"], "5");
}
