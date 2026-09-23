//! Handler and parameter names are the user's, not the macro's.
//!
//! Two ways a perfectly ordinary Rust signature used to break `#[server]`:
//!
//! - A raw identifier (`r#type`) panicked the macro, because the generated
//!   code rebuilt identifiers from their string form and `Ident::new` rejects
//!   `r#type`. The wire name must be the unraw'd `type` — that is what a client
//!   sends — while the generated code keeps calling the method `r#type`.
//! - The dispatch code bound the arguments map as `args` and the request
//!   context as `ctx`, in the same scope as the user's own parameters. A tool
//!   parameter called `args` shadowed the map, so every parameter extracted
//!   after it read from a `String`; one called `ctx` shadowed the context a
//!   `&RequestContext` parameter is handed.

use serde_json::{Value, json};
use turbomcp::prelude::*;

#[derive(Clone)]
struct Hygiene;

#[server(name = "hygiene", version = "1.0.0")]
impl Hygiene {
    /// Named with a keyword, taking a keyword-named parameter.
    #[tool]
    async fn r#type(&self, r#type: String, r#match: Option<u32>) -> String {
        format!("{type}:{match:?}")
    }

    /// Parameters named after what the generated code used to bind, declared
    /// before a parameter that has to be read from the arguments map after
    /// them, plus a context parameter that must still get the real context.
    #[tool]
    async fn collide(
        &self,
        args: String,
        ctx: String,
        after: u32,
        context: &RequestContext,
    ) -> String {
        format!("{args}|{ctx}|{after}|{}", !context.request_id().is_empty())
    }

    #[prompt]
    async fn r#loop(
        &self,
        r#type: String,
        args: Option<String>,
        ctx: String,
        _context: &RequestContext,
    ) -> String {
        format!("{type}|{args:?}|{ctx}")
    }

    #[resource("mem://kw")]
    async fn r#static(&self, _uri: String, _ctx: &RequestContext) -> McpResult<String> {
        Ok("kw".to_string())
    }
}

async fn request(method: &str, params: Value) -> Value {
    let response = Hygiene
        .handle_request(
            json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params }),
            RequestContext::stdio(),
        )
        .await
        .unwrap();
    assert!(response["error"].is_null(), "{method} failed: {response}");
    response["result"].clone()
}

#[tokio::test]
async fn raw_identifiers_are_listed_under_their_unraw_names() {
    let tools = request("tools/list", json!({})).await;
    let r#type = tools["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "type")
        .unwrap_or_else(|| panic!("tool `type` not listed: {tools}"));
    let properties = &r#type["inputSchema"]["properties"];
    assert!(properties["type"].is_object(), "{properties}");
    assert!(properties["match"].is_object(), "{properties}");
    assert_eq!(r#type["inputSchema"]["required"], json!(["type"]));

    let prompts = request("prompts/list", json!({})).await;
    let r#loop = &prompts["prompts"][0];
    assert_eq!(r#loop["name"], "loop");
    assert_eq!(r#loop["arguments"][0]["name"], "type");

    let resources = request("resources/list", json!({})).await;
    assert_eq!(resources["resources"][0]["name"], "static");
}

#[tokio::test]
async fn raw_identifiers_dispatch_by_their_unraw_names() {
    let result = request(
        "tools/call",
        json!({ "name": "type", "arguments": { "type": "kw", "match": 3 } }),
    )
    .await;
    assert_eq!(result["isError"], Value::Null, "{result}");
    assert_eq!(result["content"][0]["text"], "kw:Some(3)");

    let prompt = request(
        "prompts/get",
        json!({ "name": "loop", "arguments": { "type": "t", "ctx": "c" } }),
    )
    .await;
    assert_eq!(prompt["messages"][0]["content"]["text"], "t|None|c");
}

#[tokio::test]
async fn parameters_named_args_and_ctx_do_not_shadow_the_generated_bindings() {
    let result = request(
        "tools/call",
        json!({
            "name": "collide",
            "arguments": { "args": "a", "ctx": "c", "after": 7 }
        }),
    )
    .await;
    assert_eq!(result["isError"], Value::Null, "{result}");
    assert_eq!(result["content"][0]["text"], "a|c|7|true");

    let prompt = request(
        "prompts/get",
        json!({ "name": "loop", "arguments": { "type": "t", "args": "a", "ctx": "c" } }),
    )
    .await;
    assert_eq!(prompt["messages"][0]["content"]["text"], "t|Some(\"a\")|c");
}
