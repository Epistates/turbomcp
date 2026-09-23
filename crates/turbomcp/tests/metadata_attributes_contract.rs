//! What the `#[tool]`, `#[resource]`, and `#[prompt]` attribute keys put on the
//! wire.
//!
//! The three markers used to disagree: an explicit `description = "..."` beat
//! the doc comment on `#[tool]` but lost to it on `#[prompt]`, `#[resource]`
//! had no `description` key at all, and none of them could express the spec
//! metadata `ResourceAnnotations`, `Resource.size`, or `Tool.execution` — so a
//! macro-built server could not say what a hand-built one could.

use serde_json::{Value, json};
use turbomcp::prelude::*;

#[derive(Clone)]
struct Catalogue;

#[server(name = "catalogue", version = "1.0.0")]
impl Catalogue {
    /// Doc comment.
    #[tool(description = "Explicit", task_support = "optional")]
    async fn report(&self) -> String {
        String::new()
    }

    #[tool]
    async fn plain(&self) -> String {
        String::new()
    }

    /// Doc comment.
    #[resource(
        "mem://readme",
        description = "Explicit",
        mime_type = "text/markdown",
        audience = ["user", "assistant"],
        priority = 0.8,
        last_modified = "2025-01-12T15:00:58Z",
        size = 1024
    )]
    async fn readme(&self, _uri: String, _ctx: &RequestContext) -> McpResult<String> {
        Ok("# readme".to_string())
    }

    /// Doc comment only.
    #[resource("mem://rows/{id}", audience = ["assistant"], priority = 1)]
    async fn row(&self, _uri: String, _ctx: &RequestContext) -> McpResult<String> {
        Ok("row".to_string())
    }

    /// Doc comment.
    #[prompt(description = "Explicit")]
    async fn summarize(&self, topic: std::option::Option<String>, _ctx: &RequestContext) -> String {
        format!("{topic:?}")
    }
}

async fn request(method: &str, params: Value) -> Value {
    let response = Catalogue
        .handle_request(
            json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params }),
            RequestContext::stdio(),
        )
        .await
        .unwrap();
    assert!(response["error"].is_null(), "{method} failed: {response}");
    response["result"].clone()
}

fn named<'a>(list: &'a Value, key: &str, name: &str) -> &'a Value {
    list[key]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["name"] == name)
        .unwrap_or_else(|| panic!("{name} not in {list}"))
}

#[tokio::test]
async fn explicit_description_wins_over_the_doc_comment_on_every_marker() {
    let tools = request("tools/list", json!({})).await;
    assert_eq!(named(&tools, "tools", "report")["description"], "Explicit");

    let resources = request("resources/list", json!({})).await;
    assert_eq!(
        named(&resources, "resources", "readme")["description"],
        "Explicit"
    );

    let prompts = request("prompts/list", json!({})).await;
    assert_eq!(
        named(&prompts, "prompts", "summarize")["description"],
        "Explicit"
    );

    // With no explicit description the doc comment still applies.
    let templates = request("resources/templates/list", json!({})).await;
    assert_eq!(
        named(&templates, "resourceTemplates", "row")["description"],
        "Doc comment only."
    );
}

#[tokio::test]
async fn a_fully_qualified_option_is_an_optional_prompt_argument() {
    let prompts = request("prompts/list", json!({})).await;
    assert_eq!(
        named(&prompts, "prompts", "summarize")["arguments"][0]["required"],
        false
    );

    let rendered = request("prompts/get", json!({ "name": "summarize" })).await;
    assert_eq!(rendered["messages"][0]["content"]["text"], "None");
}

#[tokio::test]
async fn resource_annotations_and_size_are_listed() {
    let resources = request("resources/list", json!({})).await;
    let readme = named(&resources, "resources", "readme");
    assert_eq!(
        readme["annotations"],
        json!({
            "audience": ["user", "assistant"],
            "priority": 0.8,
            "lastModified": "2025-01-12T15:00:58Z"
        })
    );
    assert_eq!(readme["size"], 1024);

    let templates = request("resources/templates/list", json!({})).await;
    let row = named(&templates, "resourceTemplates", "row");
    assert_eq!(
        row["annotations"],
        json!({ "audience": ["assistant"], "priority": 1.0 })
    );
}

#[tokio::test]
async fn task_support_is_listed_as_tool_execution() {
    let tools = request("tools/list", json!({})).await;
    assert_eq!(
        named(&tools, "tools", "report")["execution"],
        json!({ "taskSupport": "optional" })
    );
    // Undeclared stays absent: "forbidden" is the spec default.
    assert!(named(&tools, "tools", "plain").get("execution").is_none());
}
