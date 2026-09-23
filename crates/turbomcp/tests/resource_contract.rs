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

/// A read that returns several entries carries a type per entry, chosen by the
/// handler. The declared `mime_type` used to overwrite all of them, so the
/// image below was served as `text/markdown`. It may only fill in the gaps.
#[tokio::test]
async fn declared_mime_type_does_not_overwrite_a_multi_entry_read() {
    #[derive(Clone)]
    struct Bundle;

    #[server(name = "bundle", version = "1.0.0")]
    impl Bundle {
        #[resource("mem://bundle", mime_type = "text/markdown")]
        async fn bundle(&self, uri: String, _ctx: &RequestContext) -> McpResult<ResourceResult> {
            let text = |mime_type: Option<&str>, text: &str| {
                ResourceContents::Text(TextResourceContents {
                    uri: uri.clone(),
                    mime_type: mime_type.map(str::to_string),
                    text: text.to_string(),
                    meta: None,
                })
            };
            Ok(ResourceResult {
                contents: vec![
                    text(None, "# readme"),
                    text(Some("text/csv"), "a,b"),
                    ResourceContents::Blob(BlobResourceContents {
                        uri: uri.clone(),
                        mime_type: Some("image/png".to_string()),
                        blob: "iVBORw0KGgo=".to_string(),
                        meta: None,
                    }),
                ],
                meta: None,
            })
        }
    }

    let response = Bundle
        .handle_request(
            serde_json::json!({
                "jsonrpc": "2.0", "id": 1, "method": "resources/read",
                "params": { "uri": "mem://bundle" }
            }),
            RequestContext::stdio(),
        )
        .await
        .unwrap();
    let contents = &response["result"]["contents"];
    assert_eq!(contents[0]["mimeType"], "text/markdown", "{response}");
    assert_eq!(contents[1]["mimeType"], "text/csv", "{response}");
    assert_eq!(contents[2]["mimeType"], "image/png", "{response}");
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

/// `ResourceTemplate.uriTemplate` is specified as RFC 6570. Dispatch used to
/// take the text before the first `{` as a prefix and after the last `}` as a
/// suffix and ignore everything between, so a template claimed URIs it could
/// never have produced — and the handler, receiving the raw URI, served the
/// wrong resource rather than erroring.
///
/// The concrete resource is declared *after* both templates here on purpose:
/// which of two `#[resource]` attributes comes first in a file is not something
/// an author should have to reason about.
#[derive(Clone)]
struct Db;

#[server(name = "db", version = "1.0.0")]
impl Db {
    #[resource("db://{table}/rows/{id}.json")]
    async fn row(&self, _uri: String, _ctx: &RequestContext) -> McpResult<String> {
        Ok("row".to_string())
    }

    #[resource("db://{table}/meta.json")]
    async fn meta(&self, _uri: String, _ctx: &RequestContext) -> McpResult<String> {
        Ok("meta".to_string())
    }

    #[resource("db://health")]
    async fn health(&self, _uri: String, _ctx: &RequestContext) -> McpResult<String> {
        Ok("health".to_string())
    }
}

async fn db_read(uri: &str) -> serde_json::Value {
    Db.handle_request(
        serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "resources/read",
            "params": { "uri": uri }
        }),
        RequestContext::stdio(),
    )
    .await
    .unwrap()
}

fn body(response: &serde_json::Value) -> &str {
    response["result"]["contents"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("expected a text body, got: {response}"))
}

#[tokio::test]
async fn a_template_only_claims_uris_it_could_produce() {
    assert_eq!(body(&db_read("db://users/rows/7.json").await), "row");

    for impossible in [
        "db://totally/unrelated/path.json",
        "db://x.json",
        "db://.json",
        "db://users/rows/.json",
    ] {
        let response = db_read(impossible).await;
        assert_eq!(
            response["error"]["code"], -32002,
            "{impossible} is not an instance of any template: {response}"
        );
    }
}

/// Two templates sharing a scheme and an extension reduced to the same
/// prefix/suffix pair, so whichever was declared first took both.
#[tokio::test]
async fn sibling_templates_do_not_steal_each_others_traffic() {
    assert_eq!(body(&db_read("db://users/meta.json").await), "meta");
    assert_eq!(body(&db_read("db://orders/rows/3.json").await), "row");
}

/// A concrete resource appears in `resources/list` as something a client can
/// read by name, so a template declared above it must not swallow the URI.
#[tokio::test]
async fn a_concrete_resource_wins_over_a_template_declared_before_it() {
    assert_eq!(body(&db_read("db://health").await), "health");
}

/// A variable is one path segment; spanning them needs `{/var}`, which is not
/// supported. Refusing to route beats quietly handing a handler a value it
/// would use as a path.
#[tokio::test]
async fn traversal_shapes_do_not_route() {
    for hostile in [
        "db://../../etc/rows/1.json",
        "db://%2e%2e/rows/1.json",
        "db://users/rows/..%2fsecret.json",
    ] {
        let response = db_read(hostile).await;
        assert_eq!(
            response["error"]["code"], -32002,
            "{hostile} should not reach a handler: {response}"
        );
    }
}
