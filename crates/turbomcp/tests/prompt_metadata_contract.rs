//! What a client is told about a prompt's arguments, and how big a completion
//! response may be.
//!
//! Both are display-layer concerns, and both are places where the SDK quietly
//! emitted less than the wire allows: `PromptArgument.title` was hardcoded to
//! `None` with no attribute to set it, and nothing held a `#[completion]`
//! handler to the spec's 100-value ceiling.

use turbomcp::prelude::*;
use turbomcp_core::handler::McpHandler;

#[derive(Clone)]
struct Docs;

#[server(name = "docs", version = "1.0.0")]
impl Docs {
    /// Reviews a pull request.
    #[prompt]
    async fn review(
        &self,
        #[title("Repository URL")]
        #[description("Where the code lives")]
        repo_url: String,
        #[description("Only the diff, not the whole file")] terse: Option<String>,
        _ctx: &RequestContext,
    ) -> McpResult<String> {
        Ok(format!("review {repo_url} terse={terse:?}"))
    }

    /// Answers with more suggestions than the wire permits.
    #[completion]
    async fn suggest(
        &self,
        _params: serde_json::Value,
        _ctx: &RequestContext,
    ) -> McpResult<serde_json::Value> {
        let values: Vec<String> = (0..250).map(|n| format!("value-{n}")).collect();
        Ok(serde_json::json!({ "completion": { "values": values, "total": 250 } }))
    }
}

async fn call<H: McpHandler>(
    handler: &H,
    method: &str,
    params: serde_json::Value,
) -> serde_json::Value {
    handler
        .handle_request(
            serde_json::json!({
                "jsonrpc": "2.0", "id": 1, "method": method, "params": params
            }),
            RequestContext::stdio(),
        )
        .await
        .unwrap()
}

/// SEP-973 `title`: a display label. Without one, a client building the
/// slash-command form the spec illustrates has nothing but the raw Rust
/// identifier to label the field, so the user is shown `repo_url`.
#[tokio::test]
async fn a_prompt_argument_title_reaches_the_wire() {
    let response = call(&Docs, "prompts/list", serde_json::json!({})).await;
    let arguments = &response["result"]["prompts"][0]["arguments"];

    assert_eq!(arguments[0]["name"], "repo_url");
    assert_eq!(arguments[0]["title"], "Repository URL");
    assert_eq!(arguments[0]["description"], "Where the code lives");
    assert_eq!(arguments[0]["required"], true);
}

/// `title` stays absent when unset rather than becoming an empty string — the
/// field is optional, and "" is a label a client would render.
#[tokio::test]
async fn an_argument_without_a_title_omits_the_field() {
    let response = call(&Docs, "prompts/list", serde_json::json!({})).await;
    let arguments = &response["result"]["prompts"][0]["arguments"];

    assert_eq!(arguments[1]["name"], "terse");
    assert!(
        arguments[1]
            .get("title")
            .is_none_or(serde_json::Value::is_null),
        "an unset title must not be serialized: {}",
        arguments[1]
    );
    assert_eq!(
        arguments[1]["required"], false,
        "Option<T> makes the argument optional"
    );
}

/// MCP §Completion Results: "Maximum 100 items per response". That is a
/// property of the wire rather than of any one handler, so it is held here
/// instead of being left to every `#[completion]` body to remember.
#[tokio::test]
async fn a_completion_response_is_capped_at_a_hundred_values() {
    let response = call(
        &Docs,
        "completion/complete",
        serde_json::json!({
            "ref": { "type": "ref/prompt", "name": "review" },
            "argument": { "name": "repo_url", "value": "v" }
        }),
    )
    .await;

    let completion = &response["result"]["completion"];
    assert_eq!(completion["values"].as_array().unwrap().len(), 100);
    assert_eq!(
        completion["hasMore"], true,
        "truncating is exactly what hasMore is for"
    );
    assert_eq!(
        completion["total"], 250,
        "a handler that reported a total was telling the truth about how many exist"
    );
}

/// A response already inside the ceiling is passed through untouched — in
/// particular `hasMore` is not invented for it.
#[tokio::test]
async fn a_short_completion_response_is_left_alone() {
    #[derive(Clone)]
    struct Few;

    #[server(name = "few", version = "1.0.0")]
    impl Few {
        #[completion]
        async fn suggest(
            &self,
            _params: serde_json::Value,
            _ctx: &RequestContext,
        ) -> McpResult<serde_json::Value> {
            Ok(serde_json::json!({ "completion": { "values": ["a", "b"] } }))
        }
    }

    let response = call(
        &Few,
        "completion/complete",
        serde_json::json!({
            "ref": { "type": "ref/prompt", "name": "anything" },
            "argument": { "name": "x", "value": "" }
        }),
    )
    .await;

    let completion = &response["result"]["completion"];
    assert_eq!(completion["values"].as_array().unwrap().len(), 2);
    assert!(
        completion
            .get("hasMore")
            .is_none_or(serde_json::Value::is_null),
        "hasMore must not be invented for a complete response: {completion}"
    );
}
