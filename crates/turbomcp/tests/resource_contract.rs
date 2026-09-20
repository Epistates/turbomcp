//! What a `#[resource]` advertises must match what it returns.

use turbomcp::prelude::*;

#[derive(Clone)]
struct Files;

#[server(name = "files", version = "1.0.0")]
impl Files {
    /// Declares a MIME type, so the read must agree with the listing.
    #[resource("mem://config", mime_type = "application/json")]
    async fn config(&self, _uri: String, _ctx: &RequestContext) -> McpResult<String> {
        Ok(r#"{"ok":true}"#.to_string())
    }

    /// Declares none, so the conversion's guess stands.
    #[resource("mem://notes")]
    async fn notes(&self, _uri: String, _ctx: &RequestContext) -> McpResult<String> {
        Ok("plain".to_string())
    }
}

async fn read(uri: &str) -> serde_json::Value {
    Files
        .handle_request(
            serde_json::json!({
                "jsonrpc": "2.0", "id": 1, "method": "resources/read",
                "params": { "uri": uri }
            }),
            RequestContext::stdio(),
        )
        .await
        .unwrap()
}

async fn listed(uri: &str) -> serde_json::Value {
    let response = Files
        .handle_request(
            serde_json::json!({ "jsonrpc": "2.0", "id": 1, "method": "resources/list" }),
            RequestContext::stdio(),
        )
        .await
        .unwrap();
    response["result"]["resources"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["uri"] == uri)
        .unwrap_or_else(|| panic!("{uri} not listed"))
        .clone()
}

/// The catalogue and the content must describe the resource the same way —
/// a client that trusts the listing otherwise mis-parses the body.
#[tokio::test]
async fn declared_mime_type_reaches_the_read() {
    let entry = listed("mem://config").await;
    assert_eq!(entry["mimeType"], "application/json");

    let result = read("mem://config").await;
    assert_eq!(
        result["result"]["contents"][0]["mimeType"], "application/json",
        "the read must agree with the listing, got {}",
        result["result"]
    );
}

#[tokio::test]
async fn undeclared_mime_type_keeps_the_inferred_one() {
    let result = read("mem://notes").await;
    assert_eq!(result["result"]["contents"][0]["mimeType"], "text/plain");
}

/// `uri` is schema-required; omitting it is a malformed request, not a
/// lookup for the empty URI that then reports "not found".
#[tokio::test]
async fn a_missing_uri_is_invalid_params() {
    let response = Files
        .handle_request(
            serde_json::json!({
                "jsonrpc": "2.0", "id": 1, "method": "resources/read", "params": {}
            }),
            RequestContext::stdio(),
        )
        .await
        .unwrap();
    assert_eq!(response["error"]["code"], -32602, "got {response}");
}

/// The spec assigns -32002 to this condition specifically.
#[tokio::test]
async fn an_unknown_uri_is_resource_not_found() {
    let response = read("mem://nope").await;
    assert_eq!(response["error"]["code"], -32002, "got {response}");
}
