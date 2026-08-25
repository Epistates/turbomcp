//! Bucket-A A4: templated resource URIs (RFC 6570). A `#[resource("…/{var}")]`
//! method is advertised via resources/templates/list and served by matching an
//! incoming resources/read URI and binding the extracted variables.

use serde_json::{Value, json};
use tower::{Service, ServiceExt};
use turbomcp::prelude::*;
use turbomcp::{JsonRpcMessage, JsonRpcRequest};

#[derive(Clone)]
struct Files;

#[server(name = "files", version = "1.0.0")]
impl Files {
    /// A fixed resource, to prove fixed + templated coexist.
    #[resource("config://app")]
    async fn config(&self) -> McpResult<String> {
        Ok("cfg".into())
    }

    /// A templated resource: the `{name}` segment binds the `name` argument.
    #[resource("note://{name}")]
    async fn note(&self, name: String) -> McpResult<String> {
        Ok(format!("note:{name}"))
    }

    /// A reserved-expansion template spanning slashes.
    #[resource("file://{+path}")]
    async fn file(&self, path: String) -> McpResult<String> {
        Ok(format!("file@{path}"))
    }
}

fn draft_meta() -> Value {
    json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": {},
    })
}

async fn call(svc: &mut turbomcp::VersionDispatcher<Files>, req: JsonRpcRequest) -> Value {
    let JsonRpcMessage::Response(r) = svc
        .ready()
        .await
        .unwrap()
        .call(req.into())
        .await
        .unwrap()
        .expect("response")
    else {
        panic!("expected response")
    };
    r.result.expect("result")
}

#[tokio::test]
async fn templates_are_advertised_and_fixed_are_not() {
    let mut svc = Files.into_server().build();
    let templates = call(
        &mut svc,
        JsonRpcRequest::new(
            1,
            "resources/templates/list",
            Some(json!({ "_meta": draft_meta() })),
        ),
    )
    .await;
    let uris: Vec<&str> = templates["resourceTemplates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["uriTemplate"].as_str().unwrap())
        .collect();
    assert!(uris.contains(&"note://{name}"), "got {templates}");
    assert!(uris.contains(&"file://{+path}"), "got {templates}");

    // The fixed resource is in resources/list, not templates/list.
    let list = call(
        &mut svc,
        JsonRpcRequest::new(2, "resources/list", Some(json!({ "_meta": draft_meta() }))),
    )
    .await;
    let list_uris: Vec<&str> = list["resources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["uri"].as_str().unwrap())
        .collect();
    assert_eq!(list_uris, vec!["config://app"]);
}

async fn read(svc: &mut turbomcp::VersionDispatcher<Files>, id: i64, uri: &str) -> Value {
    call(
        svc,
        JsonRpcRequest::new(
            id,
            "resources/read",
            Some(json!({ "uri": uri, "_meta": draft_meta() })),
        ),
    )
    .await
}

#[tokio::test]
async fn templated_read_binds_variables() {
    let mut svc = Files.into_server().build();

    let note = read(&mut svc, 3, "note://hello").await;
    assert_eq!(note["contents"][0]["text"], "note:hello", "got {note}");

    let file = read(&mut svc, 4, "file:///etc/hosts").await;
    assert_eq!(file["contents"][0]["text"], "file@/etc/hosts", "got {file}");

    // The fixed resource still works.
    let cfg = read(&mut svc, 5, "config://app").await;
    assert_eq!(cfg["contents"][0]["text"], "cfg", "got {cfg}");
}
