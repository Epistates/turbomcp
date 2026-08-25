//! Phase 10d-3: a `-> Json<T>` tool produces structured output end-to-end —
//! `tools/list` advertises the generated `outputSchema`, and `tools/call`
//! returns `structuredContent` plus a JSON text mirror.

use schemars::JsonSchema;
use serde::Serialize;
use serde_json::json;
use tower::{Service, ServiceExt};
use turbomcp::prelude::*;
use turbomcp::{JsonRpcMessage, JsonRpcRequest};

#[derive(Serialize, JsonSchema)]
struct Point {
    x: i64,
    y: i64,
}

#[derive(Clone)]
struct Geo;

#[server(name = "geo", version = "1.0.0")]
impl Geo {
    /// Echo a point back as structured output.
    #[tool(description = "Make a point")]
    async fn make_point(&self, x: i64, y: i64) -> McpResult<Json<Point>> {
        Ok(Json(Point { x, y }))
    }
}

fn draft_meta() -> serde_json::Value {
    json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": {},
    })
}

async fn call(req: JsonRpcRequest) -> serde_json::Value {
    let mut svc = Geo.into_server().build();
    let JsonRpcMessage::Response(r) = svc
        .ready()
        .await
        .unwrap()
        .call(req.into())
        .await
        .unwrap()
        .expect("a response")
    else {
        panic!("expected a response")
    };
    r.result.expect("a result")
}

#[tokio::test]
async fn list_tools_advertises_output_schema() {
    let result = call(JsonRpcRequest::new(
        1,
        "tools/list",
        Some(json!({ "_meta": draft_meta() })),
    ))
    .await;
    let tool = &result["tools"][0];
    assert_eq!(tool["name"], "make_point");
    let schema = &tool["outputSchema"];
    assert_eq!(schema["type"], "object", "got {schema}");
    assert!(schema["properties"]["x"].is_object());
    assert!(schema["properties"]["y"].is_object());
}

#[tokio::test]
async fn call_tool_returns_structured_content_and_mirror() {
    let result = call(JsonRpcRequest::new(
        2,
        "tools/call",
        Some(json!({
            "name": "make_point",
            "arguments": { "x": 3, "y": 4 },
            "_meta": draft_meta(),
        })),
    ))
    .await;
    assert_eq!(result["structuredContent"], json!({ "x": 3, "y": 4 }));
    // The text mirror is the compact JSON rendering of the same value.
    let mirror: serde_json::Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(mirror, json!({ "x": 3, "y": 4 }));
    assert_eq!(result["isError"], false);
}
