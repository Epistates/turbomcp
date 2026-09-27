//! How `#[server]` reads its markers and what they put in a tool's schema:
//! qualified marker paths register, `schema_extend` merges instead of
//! replacing, and an optional `#[mcp_header]` parameter is typed as the plain
//! primitive a strict client accepts.

use serde_json::{Value, json};
use tower::ServiceExt;
use turbomcp::prelude::*;
use turbomcp::{JsonRpcMessage, JsonRpcRequest};

#[derive(Clone)]
struct Markers;

#[server(name = "markers", version = "1.0.0")]
impl Markers {
    /// Spelled with its full path, as a lint policy might require.
    #[turbomcp::tool]
    async fn qualified(&self) -> String {
        "registered".into()
    }

    #[tool(schema_extend = r#"{"properties":{"age":{"minimum":18}},"required":["age"]}"#)]
    async fn enroll(&self, name: String, age: Option<i64>) -> String {
        format!("{name}:{age:?}")
    }

    #[tool]
    async fn locate(&self, city: String, #[mcp_header] zone: Option<String>) -> String {
        format!("{city}@{zone:?}")
    }
}

async fn tools() -> Vec<Value> {
    let msg: JsonRpcMessage = JsonRpcRequest::new(
        1,
        "tools/list",
        Some(json!({ "_meta": {
            "io.modelcontextprotocol/protocolVersion": "2026-07-28",
            "io.modelcontextprotocol/clientCapabilities": {}
        }})),
    )
    .into();
    let reply = Markers
        .into_server()
        .build()
        .oneshot(msg.into())
        .await
        .unwrap()
        .unwrap();
    serde_json::to_value(reply).unwrap()["result"]["tools"]
        .as_array()
        .unwrap()
        .clone()
}

fn tool<'a>(tools: &'a [Value], name: &str) -> &'a Value {
    tools
        .iter()
        .find(|t| t["name"] == name)
        .unwrap_or_else(|| panic!("no tool {name}: {tools:#?}"))
}

#[tokio::test]
async fn a_path_qualified_marker_registers() {
    let tools = tools().await;
    tool(&tools, "qualified");
}

/// Adding a bound to one property keeps the others, and `required` is the
/// union. A top-level replace used to drop `name` from `properties` while
/// leaving it required under `additionalProperties: false`, so no call could
/// ever satisfy the schema.
#[tokio::test]
async fn schema_extend_merges_into_the_generated_schema() {
    let tools = tools().await;
    let schema = &tool(&tools, "enroll")["inputSchema"];
    assert_eq!(schema["properties"]["age"]["minimum"], 18);
    assert!(
        schema["properties"]["age"].get("type").is_some(),
        "kept: {schema}"
    );
    assert!(schema["properties"].get("name").is_some(), "kept: {schema}");
    let required: Vec<&str> = schema["required"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert!(
        required.contains(&"name") && required.contains(&"age"),
        "{required:?}"
    );
}

/// `Option<String>` renders as `["string", "null"]`, which a strict client
/// rejects as a header target. The mark types it as plain `string`; being
/// absent from `required` already makes it optional.
#[tokio::test]
async fn an_optional_header_param_is_typed_as_its_primitive() {
    let tools = tools().await;
    let zone = &tool(&tools, "locate")["inputSchema"]["properties"]["zone"];
    assert_eq!(zone["type"], "string", "{zone}");
    assert_eq!(zone["x-mcp-header"], "zone");
}
