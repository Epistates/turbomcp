//! Bucket-A A2: the `#[completion]` marker wires a server's `completion/complete`
//! handler end-to-end — the capability is advertised and the handler answers.

use serde_json::json;
use tower::{Service, ServiceExt};
use turbomcp::prelude::*;
use turbomcp::{JsonRpcMessage, JsonRpcRequest};

#[derive(Clone)]
struct Completer;

#[server(name = "completer", version = "1.0.0")]
impl Completer {
    /// A prompt so the completion capability has something to complete against.
    #[prompt]
    async fn greet(&self, name: String) -> McpResult<String> {
        Ok(format!("Hello, {name}"))
    }

    /// Suggest completions: echo the partial value plus a fixed suggestion.
    #[completion]
    async fn complete(
        &self,
        params: neutral::CompleteParams,
    ) -> McpResult<neutral::CompleteResult> {
        Ok(neutral::CompleteResult::new(vec![
            params.argument.value,
            "suggested".to_string(),
        ]))
    }
}

fn draft_meta() -> serde_json::Value {
    json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": {},
    })
}

async fn call(
    svc: &mut turbomcp::VersionDispatcher<Completer>,
    req: JsonRpcRequest,
) -> JsonRpcMessage {
    svc.ready()
        .await
        .unwrap()
        .call(req.into())
        .await
        .unwrap()
        .expect("a response")
}

#[tokio::test]
async fn completion_capability_is_advertised() {
    let mut svc = Completer.into_server().build();
    let req = JsonRpcRequest::new(1, "server/discover", Some(json!({ "_meta": draft_meta() })));
    let JsonRpcMessage::Response(r) = call(&mut svc, req).await else {
        panic!("expected response")
    };
    let caps = &r.result.expect("result")["capabilities"];
    assert!(caps.get("completions").is_some(), "got {caps}");
}

#[tokio::test]
async fn completion_complete_answers() {
    let mut svc = Completer.into_server().build();
    let req = JsonRpcRequest::new(
        2,
        "completion/complete",
        Some(json!({
            "ref": { "type": "ref/prompt", "name": "greet" },
            "argument": { "name": "name", "value": "Al" },
            "_meta": draft_meta(),
        })),
    );
    let JsonRpcMessage::Response(r) = call(&mut svc, req).await else {
        panic!("expected response")
    };
    let completion = &r.result.expect("result")["completion"];
    assert_eq!(
        completion["values"],
        json!(["Al", "suggested"]),
        "got {completion}"
    );
}
