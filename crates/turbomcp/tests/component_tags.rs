//! `tags(…)` on `#[tool]` / `#[resource]` / `#[prompt]`.
//!
//! Tags are catalog metadata: they categorize a component so a policy can
//! decide who is offered it. They ride in the component's own `_meta` rather
//! than a compile-time table, because a server's components do not all come
//! from one `#[server]` impl — a mounted sub-server contributes values this
//! process never declared, and only metadata carried on the value covers both.
//!
//! That choice is only sound if `_meta` really survives the wire, so these
//! drive a live server on **all three** revisions and read the tags back off
//! what the client would receive.

use serde_json::{Value, json};
use turbomcp::methods::request;
use turbomcp::prelude::*;
use turbomcp::tower::{Service, ServiceExt};
use turbomcp::{
    JsonRpcMessage, JsonRpcRequest, LegacySessionAdapter, VersionDispatcher, WithTools, tags,
};

#[derive(Clone)]
struct Catalog;

#[server(name = "catalog", version = "1.0.0")]
impl Catalog {
    /// Untagged: the common case must stay absent from `_meta`, not present-and-empty.
    #[tool]
    async fn plain(&self) -> String {
        "ok".into()
    }

    #[tool(tags("admin", "dangerous"))]
    async fn wipe(&self) -> String {
        "wiped".into()
    }

    /// Tags compose with everything else a marker takes.
    #[tool(
        description = "Search the web",
        name = "search.web",
        title = "Search",
        tags("public"),
        read_only
    )]
    async fn search(&self, q: String) -> String {
        q
    }

    #[resource("config://app", mime_type = "application/json", tags("admin"))]
    async fn config(&self) -> McpResult<String> {
        Ok("{}".into())
    }

    #[resource("file://{+path}", tags("public", "files"))]
    async fn file(&self, path: String) -> McpResult<String> {
        Ok(path)
    }

    #[prompt(tags("public"))]
    async fn summarize(&self, text: String) -> String {
        text
    }
}

/// A live server on `version`, past the handshake.
async fn connect(version: &str) -> LegacySessionAdapter<VersionDispatcher<Catalog>> {
    let mut svc = LegacySessionAdapter::new(Catalog.into_server().build());
    let init = JsonRpcRequest::new(
        1,
        request::INITIALIZE,
        Some(json!({
            "protocolVersion": version,
            "capabilities": {},
            "clientInfo": { "name": "t", "version": "1" },
        })),
    );
    let resp = result(&mut svc, init).await;
    assert_eq!(
        resp["protocolVersion"], version,
        "handshake did not settle on {version}"
    );
    svc
}

async fn result<S>(svc: &mut S, req: JsonRpcRequest) -> Value
where
    S: Service<JsonRpcMessage, Response = Option<JsonRpcMessage>> + Clone,
    S::Error: std::fmt::Debug,
{
    let method = req.method.clone();
    match svc
        .clone()
        .oneshot(req.into())
        .await
        .expect("service failed")
    {
        Some(JsonRpcMessage::Response(r)) => {
            assert!(r.error.is_none(), "{method} failed: {:?}", r.error);
            r.result.expect("a success response has a result")
        }
        other => panic!("expected a response for {method}, got {other:?}"),
    }
}

/// The `_meta` tags on the named entry of a wire list, as plain strings.
fn wire_tags<'a>(list: &'a Value, key: &str, name_field: &str, name: &str) -> Vec<&'a str> {
    let entry = list[key]
        .as_array()
        .unwrap_or_else(|| panic!("no `{key}` array in {list}"))
        .iter()
        .find(|e| e[name_field] == json!(name))
        .unwrap_or_else(|| panic!("no {key} entry named {name}"));
    entry["_meta"]["io.turbomcp/tags"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}

/// Tags reach the client on every revision this server serves. `_meta` is
/// carried losslessly by all three, so this is the property that lets one
/// policy work across them.
#[tokio::test]
async fn tags_survive_every_protocol_revision() {
    for version in ["2025-06-18", "2025-11-25"] {
        let mut svc = connect(version).await;

        let tools = result(
            &mut svc,
            JsonRpcRequest::new(2, request::TOOLS_LIST, Some(json!({}))),
        )
        .await;
        assert_eq!(
            wire_tags(&tools, "tools", "name", "wipe"),
            ["admin", "dangerous"],
            "tool tags lost on {version}"
        );
        assert_eq!(wire_tags(&tools, "tools", "name", "search.web"), ["public"]);

        let resources = result(
            &mut svc,
            JsonRpcRequest::new(3, request::RESOURCES_LIST, Some(json!({}))),
        )
        .await;
        assert_eq!(
            wire_tags(&resources, "resources", "uri", "config://app"),
            ["admin"],
            "resource tags lost on {version}"
        );

        let templates = result(
            &mut svc,
            JsonRpcRequest::new(4, request::RESOURCES_TEMPLATES_LIST, Some(json!({}))),
        )
        .await;
        assert_eq!(
            wire_tags(
                &templates,
                "resourceTemplates",
                "uriTemplate",
                "file://{+path}"
            ),
            ["public", "files"],
            "template tags lost on {version}"
        );

        let prompts = result(
            &mut svc,
            JsonRpcRequest::new(5, request::PROMPTS_LIST, Some(json!({}))),
        )
        .await;
        assert_eq!(
            wire_tags(&prompts, "prompts", "name", "summarize"),
            ["public"],
            "prompt tags lost on {version}"
        );
    }
}

/// The draft is stateless — no handshake, the version rides each request.
#[tokio::test]
async fn tags_survive_the_draft_wire() {
    let mut svc = Catalog.into_server().build();
    let tools = result(
        &mut svc,
        JsonRpcRequest::new(
            1,
            request::TOOLS_LIST,
            Some(json!({ "_meta": { "io.modelcontextprotocol/protocolVersion": "2026-07-28" } })),
        ),
    )
    .await;
    assert_eq!(
        wire_tags(&tools, "tools", "name", "wipe"),
        ["admin", "dangerous"]
    );
}

/// An untagged component carries no tags key at all. An empty array would be a
/// distinct-looking value a filter then has to special-case.
#[tokio::test]
async fn an_untagged_component_has_no_tags_key() {
    let mut svc = connect("2025-11-25").await;
    let tools = result(
        &mut svc,
        JsonRpcRequest::new(2, request::TOOLS_LIST, Some(json!({}))),
    )
    .await;
    let plain = tools["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == json!("plain"))
        .expect("no `plain` tool");
    assert!(
        plain.get("_meta").is_none(),
        "an untagged tool must not carry a `_meta` object: {plain}"
    );
}

/// Tags do not displace the other metadata on the same marker.
#[tokio::test]
async fn tags_compose_with_the_rest_of_the_marker() {
    let mut svc = connect("2025-11-25").await;
    let tools = result(
        &mut svc,
        JsonRpcRequest::new(2, request::TOOLS_LIST, Some(json!({}))),
    )
    .await;
    let search = tools["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == json!("search.web"))
        .expect("no `search.web` tool");
    assert_eq!(search["title"], json!("Search"));
    assert_eq!(search["description"], json!("Search the web"));
    assert_eq!(search["annotations"]["readOnlyHint"], json!(true));
    assert_eq!(search["_meta"]["io.turbomcp/tags"], json!(["public"]));

    let resources = result(
        &mut svc,
        JsonRpcRequest::new(3, request::RESOURCES_LIST, Some(json!({}))),
    )
    .await;
    let config = resources["resources"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["uri"] == json!("config://app"))
        .expect("no config resource");
    assert_eq!(config["mimeType"], json!("application/json"));
    assert_eq!(config["_meta"]["io.turbomcp/tags"], json!(["admin"]));
}

/// The reader in `turbomcp::tags` is what a policy actually calls; it must agree
/// with the wire, on the neutral values the server hands a filter.
#[tokio::test]
async fn the_reader_agrees_with_what_went_on_the_wire() {
    let listed = Catalog
        .list_tools(
            &turbomcp::ListToolsContext::new(RequestContext::default()),
            neutral::ListParams::default(),
        )
        .await
        .expect("list_tools failed");

    let wipe = listed
        .tools
        .iter()
        .find(|t| t.name == "wipe")
        .expect("no `wipe` tool");
    assert_eq!(
        tags::of(&wipe.meta).collect::<Vec<_>>(),
        ["admin", "dangerous"]
    );
    assert!(tags::has(&wipe.meta, "admin"));
    assert!(tags::has_any(&wipe.meta, &["public", "dangerous"]));
    assert!(tags::has_all(&wipe.meta, &["admin", "dangerous"]));

    let plain = listed
        .tools
        .iter()
        .find(|t| t.name == "plain")
        .expect("no `plain` tool");
    assert_eq!(tags::of(&plain.meta).count(), 0);
}
