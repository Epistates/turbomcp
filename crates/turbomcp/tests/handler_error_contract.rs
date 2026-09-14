//! What a handler returns, and what the client can recover from it.
//!
//! Covers the contract gaps reported by a downstream consumer against 3.3.0:
//! a tool's error kind was flattened to display text, `McpError` had no way to
//! carry structured detail to a JSON-RPC `data` member, `Json<T>` never
//! populated `structuredContent`, the server-level `instructions` string had no
//! outlet, and a `#[prompt]` returning `Err` rendered it as a user message
//! instead of propagating.

use turbomcp::prelude::*;
use turbomcp_core::meta_keys;

#[derive(Clone, serde::Serialize, schemars::JsonSchema)]
struct Stats {
    count: u64,
}

#[derive(Clone)]
struct Contract;

#[server(
    name = "contract",
    version = "1.0.0",
    title = "Contract Server",
    instructions = "Call `boom` to see a typed failure.",
    website_url = "https://example.com/contract",
    icons = ["https://example.com/icon.png"]
)]
impl Contract {
    /// Fails with a caller-fault kind.
    #[tool]
    async fn bad_input(&self) -> McpResult<String> {
        Err(McpError::invalid_params("start must precede end")
            .with_data(serde_json::json!({ "field": "start" })))
    }

    /// Fails with a server-fault kind.
    #[tool]
    async fn broken(&self) -> McpResult<String> {
        Err(McpError::internal("upstream timed out"))
    }

    /// Returns structured output.
    #[tool]
    async fn stats(&self) -> Json<Stats> {
        Json(Stats { count: 3 })
    }

    /// Returns a JSON array, which may not occupy `structuredContent`.
    #[tool]
    async fn listing(&self) -> Json<Vec<u64>> {
        Json(vec![1, 2, 3])
    }

    /// Succeeds, to prove the happy path is untouched.
    #[tool]
    async fn greet(&self, name: String) -> McpResult<String> {
        Ok(format!("Hello, {name}!"))
    }

    #[prompt]
    async fn explain(&self, topic: String, _ctx: &RequestContext) -> McpResult<PromptResult> {
        if topic.is_empty() {
            return Err(McpError::invalid_params("topic must not be empty"));
        }
        Ok(PromptResult::user(format!("Explain {topic}")))
    }
}

fn meta_of(result: &ToolResult) -> &std::collections::HashMap<String, serde_json::Value> {
    result
        .meta
        .as_ref()
        .expect("a failed tool call must carry classification in _meta")
}

// ── 1. The error kind survives the `isError` convention ────────────────────

#[tokio::test]
async fn tool_error_kind_reaches_the_client() {
    let ctx = RequestContext::stdio();

    let bad = Contract
        .call_tool("bad_input", serde_json::json!({}), &ctx)
        .await
        .expect("a tool that runs and fails reports isError, not a protocol error");
    let broken = Contract
        .call_tool("broken", serde_json::json!({}), &ctx)
        .await
        .unwrap();

    // Both are tool execution errors per SEP-1303 ...
    assert!(bad.is_error());
    assert!(broken.is_error());

    // ... but a client can still tell a caller fault from a server fault.
    assert_eq!(meta_of(&bad)[meta_keys::ERROR_CODE], -32602);
    assert_eq!(meta_of(&bad)[meta_keys::ERROR_KIND], "invalid_params");
    assert_eq!(meta_of(&broken)[meta_keys::ERROR_CODE], -32603);
    assert_eq!(meta_of(&broken)[meta_keys::ERROR_KIND], "internal");

    // The message still reads naturally for the model.
    assert!(bad.first_text().unwrap().contains("start must precede end"));
}

/// The typed assertions above would still pass if `_meta` never serialized, so
/// pin the JSON a client actually receives.
#[tokio::test]
async fn tool_error_classification_is_on_the_wire() {
    let response = Contract
        .handle_request(
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "tools/call",
                "params": { "name": "bad_input", "arguments": {} }
            }),
            RequestContext::stdio(),
        )
        .await
        .unwrap();

    // A tool execution error is a *successful* JSON-RPC response ...
    assert!(response.get("error").is_none());
    let result = &response["result"];
    assert_eq!(result["isError"], true);

    // ... carrying its classification under the spec's `_meta` member.
    let meta = &result["_meta"];
    assert_eq!(meta[meta_keys::ERROR_CODE], -32602);
    assert_eq!(meta[meta_keys::ERROR_KIND], "invalid_params");
    assert_eq!(meta[meta_keys::ERROR_DATA]["field"], "start");
}

#[tokio::test]
async fn argument_validation_failures_are_classified_too() {
    let ctx = RequestContext::stdio();

    // Missing required parameter: still SEP-1303 (isError, so the model can
    // retry), and now labelled so the client knows it was a caller fault.
    let result = Contract
        .call_tool("greet", serde_json::json!({}), &ctx)
        .await
        .expect("validation failure surfaces as a tool execution error");

    assert!(result.is_error());
    assert_eq!(meta_of(&result)[meta_keys::ERROR_CODE], -32602);
}

#[tokio::test]
async fn successful_calls_carry_no_error_metadata() {
    let ctx = RequestContext::stdio();
    let result = Contract
        .call_tool("greet", serde_json::json!({ "name": "Ada" }), &ctx)
        .await
        .unwrap();

    assert!(!result.is_error());
    assert_eq!(result.first_text(), Some("Hello, Ada!"));
    assert!(result.meta.is_none());
}

// ── 2. McpError carries structured detail onto the JSON-RPC error object ───

#[test]
fn mcp_error_data_becomes_jsonrpc_error_data() {
    use turbomcp_core::jsonrpc::JsonRpcError;

    // Absent by default, so errors that attach nothing keep their old shape.
    let plain: JsonRpcError = McpError::invalid_params("nope").into();
    assert_eq!(plain.data, None);

    let detailed: JsonRpcError = McpError::invalid_params("nope")
        .with_data(serde_json::json!({ "field": "start", "retryable": false }))
        .into();
    assert_eq!(detailed.code, -32602);
    assert_eq!(detailed.data.unwrap()["field"], "start");
}

/// Server-side context is deliberately *not* forwarded: only what the author
/// passed to `with_data` reaches the client.
#[test]
fn error_context_stays_server_side() {
    use turbomcp_core::jsonrpc::JsonRpcError;

    let err: JsonRpcError = McpError::internal("boom")
        .with_operation("charge_card")
        .with_component("billing")
        .with_source_location("billing.rs:42")
        .into();

    assert_eq!(err.data, None);
    assert!(!err.message.contains("billing.rs"));
}

// ── 3. Json<T> emits structuredContent ─────────────────────────────────────

#[tokio::test]
async fn json_tool_emits_structured_content() {
    let ctx = RequestContext::stdio();
    let result = Contract
        .call_tool("stats", serde_json::json!({}), &ctx)
        .await
        .unwrap();

    let structured = result
        .structured_content
        .as_ref()
        .expect("Json<T> over an object must populate structuredContent");
    assert_eq!(structured["count"], 3);

    // The text mirror stays, for clients that ignore structured output.
    assert!(result.first_text().unwrap().contains("\"count\""));
}

#[tokio::test]
async fn json_tool_omits_non_object_structured_content() {
    let ctx = RequestContext::stdio();
    let result = Contract
        .call_tool("listing", serde_json::json!({}), &ctx)
        .await
        .unwrap();

    // `structuredContent` is typed `{ [key: string]: unknown }`, so an array
    // there would make the whole result invalid. It travels as text instead.
    assert_eq!(result.structured_content, None);
    assert!(result.first_text().unwrap().contains('['));
}

#[test]
fn tool_result_json_applies_the_same_object_rule() {
    let object = ToolResult::json(&serde_json::json!({ "a": 1 })).unwrap();
    assert!(object.structured_content.is_some());

    let array = ToolResult::json(&[1, 2, 3]).unwrap();
    assert_eq!(array.structured_content, None);
    assert!(array.first_text().is_some());
}

// ── 4. Server-level instructions and identity metadata ─────────────────────

#[tokio::test]
async fn initialize_emits_instructions_and_server_metadata() {
    let ctx = RequestContext::stdio();
    let response = Contract
        .handle_request(
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-11-25",
                    "clientInfo": { "name": "test", "version": "1.0.0" },
                    "capabilities": {}
                }
            }),
            ctx,
        )
        .await
        .unwrap();

    let result = &response["result"];
    assert_eq!(
        result["instructions"],
        "Call `boom` to see a typed failure."
    );
    assert_eq!(result["serverInfo"]["title"], "Contract Server");
    assert_eq!(
        result["serverInfo"]["websiteUrl"],
        "https://example.com/contract"
    );
    assert_eq!(
        result["serverInfo"]["icons"][0]["src"],
        "https://example.com/icon.png"
    );
}

#[test]
fn instructions_default_to_absent() {
    #[derive(Clone)]
    struct Quiet;

    #[server(name = "quiet", version = "1.0.0")]
    impl Quiet {
        #[tool]
        async fn noop(&self) -> String {
            String::new()
        }
    }

    assert_eq!(Quiet.instructions(), None);
}

// ── 5. Prompt errors propagate instead of becoming a message ───────────────

#[tokio::test]
async fn prompt_error_propagates() {
    let ctx = RequestContext::stdio();

    let err = Contract
        .get_prompt("explain", Some(serde_json::json!({ "topic": "" })), &ctx)
        .await
        .expect_err("an McpError from a prompt body must propagate, not render");
    assert_eq!(err.kind, turbomcp_core::error::ErrorKind::InvalidParams);

    let ok = Contract
        .get_prompt(
            "explain",
            Some(serde_json::json!({ "topic": "tides" })),
            &ctx,
        )
        .await
        .unwrap();
    assert_eq!(ok.messages[0].content.as_text(), Some("Explain tides"));
}

// ── 6. Server identity can be computed, not only written as a literal ──────

#[test]
fn server_metadata_accepts_expressions() {
    #[derive(Clone)]
    struct Computed;

    const TITLE: &str = "Computed Title";

    #[server(
        name = concat!("svc", "-", "computed"),
        version = env!("CARGO_PKG_VERSION"),
        title = TITLE
    )]
    impl Computed {
        #[tool]
        async fn noop(&self) -> String {
            String::new()
        }
    }

    let info = Computed.server_info();
    assert_eq!(info.name, "svc-computed");
    assert_eq!(info.version, env!("CARGO_PKG_VERSION"));
    assert_eq!(info.title.as_deref(), Some(TITLE));
}

// ── 7. Content types are reachable from the facade ─────────────────────────

#[test]
fn content_types_are_reachable_without_depending_on_turbomcp_types() {
    // Building a resource result by hand needs the concrete variant type;
    // previously callers had to reach through `ResourceContents::Text`.
    let text = TextResourceContents {
        uri: "mem://a".into(),
        mime_type: Some("text/plain".into()),
        text: "body".into(),
        meta: None,
    };
    let result = ResourceResult {
        contents: vec![ResourceContents::Text(text)],
        meta: None,
    };
    assert_eq!(result.first_text(), Some("body"));

    // And reading a content block needs `Content` itself.
    let block: Content = Content::text("hi");
    assert_eq!(block.as_text(), Some("hi"));
    assert!(matches!(block, Content::Text(TextContent { .. })));
}
