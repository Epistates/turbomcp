//! Mounting several servers as one.
//!
//! Composition is only useful if a mounted server needs no knowledge that it is
//! mounted: these drive a real dispatcher over the wire and check that the
//! sub-servers — written as ordinary `#[server]` impls — answer through the
//! composite exactly as they would alone, under their namespaced names.

use serde_json::{Value, json};
use turbomcp::methods::request;
use turbomcp::prelude::*;
use turbomcp::tower::{Service, ServiceExt};
use turbomcp::{
    Composite, CompositeServer, Implementation, JsonRpcMessage, JsonRpcRequest, JsonRpcResponse,
    LegacySessionAdapter, ProtocolVersion, ServerBuilder, VersionDispatcher, neutral,
};

#[derive(Clone)]
struct Weather;

#[server(name = "weather", version = "1.0.0")]
impl Weather {
    #[tool(description = "Tomorrow's forecast", tags("public"))]
    async fn forecast(&self, city: String) -> String {
        format!("{city}: sunny")
    }

    #[resource("weather://today", mime_type = "text/plain")]
    async fn today(&self) -> McpResult<String> {
        Ok("sunny".into())
    }

    #[prompt(description = "Explain the weather")]
    async fn explain(&self, city: String) -> String {
        format!("Explain the weather in {city}")
    }
}

/// Deliberately shares the tool and prompt name `forecast`/`explain` with
/// `Weather`: without namespacing one would shadow the other.
#[derive(Clone)]
struct News;

#[server(name = "news", version = "1.0.0")]
impl News {
    #[tool(description = "Today's headlines")]
    async fn forecast(&self) -> String {
        "no news is good news".into()
    }

    #[resource("news://today")]
    async fn today(&self) -> McpResult<String> {
        Ok("quiet".into())
    }

    #[prompt]
    async fn explain(&self, topic: String) -> String {
        format!("Explain {topic}")
    }
}

/// Tools only — so the composite must not assume every mount has everything.
#[derive(Clone)]
struct Health;

#[server(name = "health", version = "1.0.0")]
impl Health {
    #[tool]
    async fn ping(&self) -> String {
        "ok".into()
    }
}

fn gateway() -> Composite {
    Composite::new(Implementation::new("gateway", "1.0.0"))
        .instructions("Ask weather.* for forecasts and news.* for headlines.")
        .mount("weather", Weather.into_server())
        .expect("mount weather")
        .mount("news", News.into_server())
        .expect("mount news")
        .mount("health", Health.into_server())
        .expect("mount health")
}

async fn connect(composite: Composite) -> LegacySessionAdapter<VersionDispatcher<CompositeServer>> {
    let mut svc = LegacySessionAdapter::new(composite.into_server().build());
    let init = result(
        &mut svc,
        JsonRpcRequest::new(
            1,
            request::INITIALIZE,
            Some(json!({
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": { "name": "t", "version": "1" },
            })),
        ),
    )
    .await;
    assert_eq!(init["serverInfo"]["name"], json!("gateway"));
    svc
}

async fn respond<S>(svc: &mut S, req: JsonRpcRequest) -> JsonRpcResponse
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
        Some(JsonRpcMessage::Response(r)) => r,
        other => panic!("expected a response for {method}, got {other:?}"),
    }
}

async fn result<S>(svc: &mut S, req: JsonRpcRequest) -> Value
where
    S: Service<JsonRpcMessage, Response = Option<JsonRpcMessage>> + Clone,
    S::Error: std::fmt::Debug,
{
    let method = req.method.clone();
    let r = respond(svc, req).await;
    assert!(r.error.is_none(), "{method} failed: {:?}", r.error);
    r.result.expect("a success response has a result")
}

/// The text of a `tools/call` result's first content block.
fn text(result: &Value) -> &str {
    result["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("no text content in {result}"))
}

fn names(list: &Value, key: &str) -> Vec<String> {
    list[key]
        .as_array()
        .unwrap_or_else(|| panic!("no `{key}` array in {list}"))
        .iter()
        .map(|e| e["name"].as_str().unwrap_or_default().to_owned())
        .collect()
}

/// The point of prefixing: two mounts declaring the same tool name both survive,
/// and each call reaches the right one.
#[tokio::test]
async fn a_name_collision_across_mounts_is_impossible() {
    let mut svc = connect(gateway()).await;

    let tools = result(
        &mut svc,
        JsonRpcRequest::new(2, request::TOOLS_LIST, Some(json!({}))),
    )
    .await;
    let mut listed = names(&tools, "tools");
    listed.sort();
    assert_eq!(
        listed,
        ["health.ping", "news.forecast", "weather.forecast"],
        "both `forecast` tools must survive, each under its mount"
    );

    let weather = result(
        &mut svc,
        JsonRpcRequest::new(
            3,
            request::TOOLS_CALL,
            Some(json!({ "name": "weather.forecast", "arguments": { "city": "Oslo" } })),
        ),
    )
    .await;
    assert_eq!(text(&weather), "Oslo: sunny");

    let news = result(
        &mut svc,
        JsonRpcRequest::new(
            4,
            request::TOOLS_CALL,
            Some(json!({ "name": "news.forecast", "arguments": {} })),
        ),
    )
    .await;
    assert_eq!(text(&news), "no news is good news");
}

/// Prompts namespace the same way, and route by the same split.
#[tokio::test]
async fn prompts_are_namespaced_and_routed() {
    let mut svc = connect(gateway()).await;

    let prompts = result(
        &mut svc,
        JsonRpcRequest::new(2, request::PROMPTS_LIST, Some(json!({}))),
    )
    .await;
    let mut listed = names(&prompts, "prompts");
    listed.sort();
    assert_eq!(listed, ["news.explain", "weather.explain"]);

    let got = result(
        &mut svc,
        JsonRpcRequest::new(
            3,
            request::PROMPTS_GET,
            Some(json!({ "name": "weather.explain", "arguments": { "city": "Oslo" } })),
        ),
    )
    .await;
    assert!(
        got["messages"][0]["content"]["text"]
            .as_str()
            .unwrap()
            .contains("Oslo"),
        "{got}"
    );
}

/// Resource URIs are *not* rewritten — a URI is a global identifier, and a
/// client that reads `weather://today` off the list must be able to send it back
/// unchanged.
#[tokio::test]
async fn resource_uris_pass_through_unchanged() {
    let mut svc = connect(gateway()).await;

    let resources = result(
        &mut svc,
        JsonRpcRequest::new(2, request::RESOURCES_LIST, Some(json!({}))),
    )
    .await;
    let mut uris: Vec<&str> = resources["resources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["uri"].as_str().unwrap())
        .collect();
    uris.sort_unstable();
    assert_eq!(uris, ["news://today", "weather://today"]);

    for (uri, expected) in [("weather://today", "sunny"), ("news://today", "quiet")] {
        let read = result(
            &mut svc,
            JsonRpcRequest::new(3, request::RESOURCES_READ, Some(json!({ "uri": uri }))),
        )
        .await;
        assert_eq!(
            read["contents"][0]["text"],
            json!(expected),
            "reading {uri}"
        );
    }
}

/// A URI no mount owns is a plain not-found, not the first mount's error — the
/// read walks every mount before giving up.
#[tokio::test]
async fn an_unowned_uri_is_not_found() {
    let mut svc = connect(gateway()).await;
    let r = respond(
        &mut svc,
        JsonRpcRequest::new(
            2,
            request::RESOURCES_READ,
            Some(json!({ "uri": "nowhere://x" })),
        ),
    )
    .await;
    let error = r.error.expect("expected an error response");
    assert_eq!(
        error.code,
        McpError::resource_not_found("x").jsonrpc_code_for(&ProtocolVersion::V2025_11_25),
    );
    assert!(error.message.contains("nowhere://x"), "{}", error.message);
}

/// Advertised capabilities are still derived, not declared: a composite of
/// tools-only servers must not claim resources or prompts.
#[tokio::test]
async fn capabilities_come_from_what_the_mounts_actually_have() {
    let tools_only = Composite::new(Implementation::new("gateway", "1.0.0"))
        .mount("health", Health.into_server())
        .expect("mount health");
    let mut svc = connect(tools_only).await;

    let discovered = result(
        &mut svc,
        JsonRpcRequest::new(2, "server/discover", Some(json!({}))),
    )
    .await;
    let caps = &discovered["capabilities"];
    assert!(caps.get("tools").is_some(), "{caps}");
    assert!(caps.get("resources").is_none(), "{caps}");
    assert!(caps.get("prompts").is_none(), "{caps}");

    // …and the full gateway, whose mounts do have them, advertises all three.
    let mut svc = connect(gateway()).await;
    let discovered = result(
        &mut svc,
        JsonRpcRequest::new(2, "server/discover", Some(json!({}))),
    )
    .await;
    let caps = &discovered["capabilities"];
    for cap in ["tools", "resources", "prompts"] {
        assert!(caps.get(cap).is_some(), "missing `{cap}` in {caps}");
    }
}

/// A tool name with no mount prefix, or one naming an unmounted prefix, answers
/// the same way a `#[server]` impl answers an unknown tool: a tool-level error
/// the model can act on, not a JSON-RPC error.
#[tokio::test]
async fn an_unroutable_tool_name_is_a_tool_level_error() {
    let mut svc = connect(gateway()).await;
    for name in ["forecast", "sports.forecast"] {
        let r = result(
            &mut svc,
            JsonRpcRequest::new(
                2,
                request::TOOLS_CALL,
                Some(json!({ "name": name, "arguments": {} })),
            ),
        )
        .await;
        assert_eq!(r["isError"], json!(true), "calling `{name}`");
        assert!(text(&r).contains("unknown tool"), "calling `{name}`");
    }
}

/// A mount's own metadata survives composition untouched — only the name is
/// rewritten. Tags in particular are what a later visibility policy reads.
#[tokio::test]
async fn a_mounted_components_metadata_is_preserved() {
    let mut svc = connect(gateway()).await;
    let tools = result(
        &mut svc,
        JsonRpcRequest::new(2, request::TOOLS_LIST, Some(json!({}))),
    )
    .await;
    let forecast = tools["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == json!("weather.forecast"))
        .expect("no weather.forecast");
    assert_eq!(forecast["description"], json!("Tomorrow's forecast"));
    assert_eq!(forecast["_meta"]["io.turbomcp/tags"], json!(["public"]));
    assert!(
        forecast["inputSchema"]["properties"]["city"].is_object(),
        "the mounted tool's schema must survive: {forecast}"
    );
}

/// Two mounts claiming one URI is a real ambiguity — nothing in the request says
/// which was meant — so it is reported rather than resolved by mount order.
#[tokio::test]
async fn two_mounts_claiming_one_uri_is_reported() {
    #[derive(Clone)]
    struct Shadow;

    #[server(name = "shadow", version = "1.0.0")]
    impl Shadow {
        #[resource("weather://today")]
        async fn today(&self) -> McpResult<String> {
            Ok("rain".into())
        }
    }

    let clashing = Composite::new(Implementation::new("gateway", "1.0.0"))
        .mount("weather", Weather.into_server())
        .expect("mount weather")
        .mount("shadow", Shadow.into_server())
        .expect("mount shadow");
    let mut svc = connect(clashing).await;

    let r = respond(
        &mut svc,
        JsonRpcRequest::new(2, request::RESOURCES_LIST, Some(json!({}))),
    )
    .await;
    let error = r.error.expect("the collision must be reported");
    assert!(
        error.message.contains("weather://today"),
        "{}",
        error.message
    );
    assert!(error.message.contains("shadow"), "{}", error.message);
}

/// The composite governs negotiation, so `protocols(…)` on it works exactly as
/// `#[server(protocols(…))]` does on a single server.
#[tokio::test]
async fn the_composite_owns_protocol_negotiation() {
    let pinned = Composite::new(Implementation::new("gateway", "1.0.0"))
        .protocols(&[ProtocolVersion::V2025_11_25])
        .mount("health", Health.into_server())
        .expect("mount health");
    let mut svc = LegacySessionAdapter::new(pinned.into_server().build());

    // On the handshake the lifecycle spec has the server answer with a version
    // it *does* serve and let the client decide — so a pinned composite
    // negotiates down rather than refusing.
    let init = result(
        &mut svc,
        JsonRpcRequest::new(
            1,
            request::INITIALIZE,
            Some(json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": { "name": "t", "version": "1" },
            })),
        ),
    )
    .await;
    assert_eq!(init["protocolVersion"], json!("2025-11-25"));

    // On the stateless path the version rides each request, so an excluded one
    // is refused outright — and the error names what *is* served.
    let r = respond(
        &mut svc,
        JsonRpcRequest::new(
            2,
            request::TOOLS_LIST,
            Some(json!({
                "_meta": { "io.modelcontextprotocol/protocolVersion": "2026-07-28" }
            })),
        ),
    )
    .await;
    let error = r.error.expect("an excluded revision must be refused");
    assert_eq!(error.code, -32004);
    assert_eq!(error.data.unwrap()["supported"], json!(["2025-11-25"]));
}

// ---- pagination --------------------------------------------------------------

/// A hand-written server that actually paginates. Nothing `#[server]` generates
/// does — it lists everything in one page — so a composite's cursor handling has
/// no other way to be exercised.
///
/// The cursor shape is deliberately private to this server *and* tagged with its
/// own id, so a cursor handed to the wrong mount is an error rather than a
/// coincidence that happens to parse.
#[derive(Clone)]
struct Paged {
    id: &'static str,
    count: usize,
}

impl McpServerCore for Paged {
    fn server_info(&self) -> Implementation {
        Implementation::new(self.id, "1.0.0")
    }
}

impl WithTools for Paged {
    async fn list_tools(
        &self,
        _ctx: &ListToolsContext,
        params: neutral::ListParams,
    ) -> McpResult<neutral::ListToolsResult> {
        let at: usize = match params.cursor.as_deref() {
            None => 0,
            Some(c) => c
                .strip_prefix(self.id)
                .and_then(|rest| rest.strip_prefix('#'))
                .and_then(|n| n.parse().ok())
                .ok_or_else(|| {
                    McpError::invalid_params(format!(
                        "{} was handed a foreign cursor: {c}",
                        self.id
                    ))
                })?,
        };
        let mut out = neutral::ListToolsResult::new(vec![neutral::Tool::new(
            format!("t{at}"),
            json!({ "type": "object" }),
        )]);
        if at + 1 < self.count {
            out.next_cursor = Some(format!("{}#{}", self.id, at + 1));
        }
        Ok(out)
    }

    async fn call_tool(
        &self,
        _ctx: &CallToolContext,
        params: neutral::CallToolParams,
    ) -> McpResult<neutral::CallToolResult> {
        Ok(neutral::CallToolResult::new(vec![neutral::Content::text(
            format!("{}:{}", self.id, params.name),
        )]))
    }
}

fn paged(id: &'static str, count: usize) -> ServerBuilder<Paged> {
    ServerBuilder::new(Paged { id, count }).with_tools()
}

async fn list_page(
    svc: &mut LegacySessionAdapter<VersionDispatcher<CompositeServer>>,
    id: i64,
    cursor: Option<&str>,
) -> Value {
    let params = cursor.map_or_else(|| json!({}), |c| json!({ "cursor": c }));
    result(
        svc,
        JsonRpcRequest::new(id, request::TOOLS_LIST, Some(params)),
    )
    .await
}

/// The whole point: paging through a composite reaches *every* tool of *every*
/// mount, exactly once. Concatenating each mount's first page and dropping its
/// `next_cursor` — what the composite used to do — would return three of six.
#[tokio::test]
async fn paging_visits_every_mounts_every_page() {
    let mut svc = connect(
        Composite::new(Implementation::new("gateway", "1.0.0"))
            .mount("a", paged("a", 3))
            .expect("mount a")
            .mount("b", paged("b", 2))
            .expect("mount b")
            .mount("c", paged("c", 1))
            .expect("mount c"),
    )
    .await;

    let mut seen = Vec::new();
    let mut cursor: Option<String> = None;
    for id in 0..10 {
        let page = list_page(&mut svc, id, cursor.as_deref()).await;
        seen.extend(names(&page, "tools"));
        cursor = page["nextCursor"].as_str().map(ToOwned::to_owned);
        if cursor.is_none() {
            break;
        }
    }

    assert!(cursor.is_none(), "pagination never terminated");
    assert_eq!(
        seen,
        ["a.t0", "a.t1", "a.t2", "b.t0", "b.t1", "c.t0"],
        "every mount's every page, in mount order, each exactly once"
    );
}

/// A mount's cursor is private to it. The composite must never hand mount `a`'s
/// cursor to mount `b` — `Paged` rejects a foreign cursor outright, so if the
/// composite forwarded the caller's cursor to every mount (the old behaviour)
/// the second request would fail instead of returning a page.
#[tokio::test]
async fn a_mounts_cursor_never_reaches_another_mount() {
    let mut svc = connect(
        Composite::new(Implementation::new("gateway", "1.0.0"))
            .mount("a", paged("a", 2))
            .expect("mount a")
            .mount("b", paged("b", 2))
            .expect("mount b"),
    )
    .await;

    let first = list_page(&mut svc, 1, None).await;
    assert_eq!(names(&first, "tools"), ["a.t0"]);
    let cursor = first["nextCursor"].as_str().expect("more pages").to_owned();

    // A success, not `a` rejecting a cursor it didn't mint — and it *resumes*
    // `a` at t1 rather than restarting it. `b.t0` follows in the same page
    // because `a` is exhausted by then and a page runs on until some mount
    // reports more.
    let second = list_page(&mut svc, 2, Some(&cursor)).await;
    assert_eq!(names(&second, "tools"), ["a.t1", "b.t0"]);
}

/// A cursor the composite did not mint is a client error. Silently starting over
/// would hand back a page the caller already has, and treating it as a mount's
/// own cursor would leak one server's private state into another's parser.
#[tokio::test]
async fn a_cursor_this_server_did_not_issue_is_refused() {
    let mut svc = connect(
        Composite::new(Implementation::new("gateway", "1.0.0"))
            .mount("a", paged("a", 2))
            .expect("mount a"),
    )
    .await;

    for bogus in ["no-separator", "nosuchmount:0", "a#1"] {
        let r = respond(
            &mut svc,
            JsonRpcRequest::new(1, request::TOOLS_LIST, Some(json!({ "cursor": bogus }))),
        )
        .await;
        assert!(
            r.error.is_some(),
            "`{bogus}` should not be accepted as a cursor, got {:?}",
            r.result
        );
    }
}

/// Mounts that don't paginate — every `#[server]` impl — still answer in one
/// page with no cursor at all.
#[tokio::test]
async fn a_composite_of_single_page_mounts_advertises_no_cursor() {
    let mut svc = connect(gateway()).await;
    let page = list_page(&mut svc, 1, None).await;

    assert_eq!(
        names(&page, "tools"),
        ["weather.forecast", "news.forecast", "health.ping"]
    );
    assert!(
        page.get("nextCursor").is_none_or(Value::is_null),
        "a single-page composite must not advertise another page: {page}"
    );
}
