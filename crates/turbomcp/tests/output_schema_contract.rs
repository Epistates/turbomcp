//! `outputSchema` and `structuredContent` must agree.
//!
//! The spec makes declaring `outputSchema` a promise: a tool that declares one
//! MUST return conforming `structuredContent`. And `structuredContent` is typed
//! `{ [key: string]: unknown }` in every wire this SDK speaks, so a schema for a
//! non-object payload is a promise that cannot be kept. These tests pin that the
//! declaration and the payload are decided by the same object test, and so can
//! never disagree.

use turbomcp::prelude::*;

#[derive(Clone, serde::Serialize, schemars::JsonSchema)]
struct Stats {
    count: u64,
    label: String,
}

#[derive(Clone, serde::Serialize, schemars::JsonSchema)]
struct Wrapped {
    items: Vec<u64>,
}

#[derive(Clone)]
struct Typed;

#[server(name = "typed", version = "1.0.0")]
impl Typed {
    /// Returns an object — schema and structured content both apply.
    #[tool]
    async fn stats(&self) -> Json<Stats> {
        Json(Stats {
            count: 3,
            label: "ok".into(),
        })
    }

    /// Same, behind a Result.
    #[tool]
    async fn fallible_stats(&self) -> McpResult<Json<Stats>> {
        Ok(Json(Stats {
            count: 7,
            label: "via result".into(),
        }))
    }

    /// Returns a JSON array, which may not occupy `structuredContent`.
    #[tool]
    async fn listing(&self) -> Json<Vec<u64>> {
        Json(vec![1, 2, 3])
    }

    /// An array nested in an object is fine — the top level is what matters.
    #[tool]
    async fn wrapped(&self) -> Json<Wrapped> {
        Json(Wrapped {
            items: vec![1, 2, 3],
        })
    }

    /// Not typed output at all.
    #[tool]
    async fn plain(&self) -> String {
        "hello".into()
    }
}

async fn tool_named(name: &str) -> serde_json::Value {
    let response = Typed
        .handle_request(
            serde_json::json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }),
            RequestContext::stdio(),
        )
        .await
        .unwrap();
    response["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == name)
        .unwrap_or_else(|| panic!("tool {name} not listed"))
        .clone()
}

async fn call(name: &str) -> serde_json::Value {
    Typed
        .handle_request(
            serde_json::json!({
                "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                "params": { "name": name, "arguments": {} }
            }),
            RequestContext::stdio(),
        )
        .await
        .unwrap()["result"]
        .clone()
}

// ── Json<T> over an object declares and delivers ───────────────────────────

#[tokio::test]
async fn json_object_return_declares_output_schema() {
    let tool = tool_named("stats").await;

    let schema = &tool["outputSchema"];
    assert!(
        schema.is_object(),
        "Json<T> over a struct should advertise outputSchema, got {tool}"
    );
    assert_eq!(schema["type"], "object");
    assert!(schema["properties"]["count"].is_object());
    assert!(schema["properties"]["label"].is_object());
}

#[tokio::test]
async fn declared_schema_is_matched_by_the_result() {
    let tool = tool_named("stats").await;
    let result = call("stats").await;

    // The promise...
    assert_eq!(tool["outputSchema"]["type"], "object");
    // ...and the payload that keeps it.
    let structured = &result["structuredContent"];
    assert!(structured.is_object(), "got {result}");
    assert_eq!(structured["count"], 3);
    assert_eq!(structured["label"], "ok");
}

#[tokio::test]
async fn inference_sees_through_result() {
    let tool = tool_named("fallible_stats").await;
    assert_eq!(
        tool["outputSchema"]["type"], "object",
        "McpResult<Json<T>> should infer the same schema as Json<T>"
    );

    let result = call("fallible_stats").await;
    assert_eq!(result["structuredContent"]["count"], 7);
}

#[tokio::test]
async fn nested_arrays_are_fine_when_the_top_level_is_an_object() {
    let tool = tool_named("wrapped").await;
    assert_eq!(tool["outputSchema"]["type"], "object");

    let result = call("wrapped").await;
    assert_eq!(
        result["structuredContent"]["items"],
        serde_json::json!([1, 2, 3])
    );
}

// ── The cases where nothing may be promised ────────────────────────────────

/// The heart of it: a non-object payload gets no schema, because it could never
/// satisfy one.
#[tokio::test]
async fn json_array_return_declares_no_output_schema() {
    let tool = tool_named("listing").await;
    let result = call("listing").await;

    assert!(
        tool.get("outputSchema").is_none() || tool["outputSchema"].is_null(),
        "Json<Vec<_>> must not advertise a schema it cannot satisfy, got {tool}"
    );
    // And correspondingly no structured content — the two decisions agree.
    assert!(
        result.get("structuredContent").is_none() || result["structuredContent"].is_null(),
        "got {result}"
    );
    // The data still reaches the model as text.
    assert!(result["content"][0]["text"].as_str().unwrap().contains('['));
}

#[tokio::test]
async fn untyped_returns_declare_no_output_schema() {
    let tool = tool_named("plain").await;
    assert!(
        tool.get("outputSchema").is_none() || tool["outputSchema"].is_null(),
        "a String-returning tool has no schema to advertise, got {tool}"
    );
}

/// Across every tool on the server, declaring a schema and populating
/// structured content must be the same decision.
#[tokio::test]
async fn declaration_and_payload_never_disagree() {
    for name in ["stats", "fallible_stats", "listing", "wrapped", "plain"] {
        let tool = tool_named(name).await;
        let result = call(name).await;

        let declares = tool.get("outputSchema").is_some_and(|s| !s.is_null());
        let delivers = result
            .get("structuredContent")
            .is_some_and(|s| !s.is_null());

        assert_eq!(
            declares, delivers,
            "{name}: declares outputSchema={declares} but delivers structuredContent={delivers}"
        );
    }
}
