//! Progressive disclosure: `ServerBuilder::with_visibility`.
//!
//! The claim these guard is "hidden means unreachable". Filtering a list while
//! still answering the call would be theatre — names are guessable and the list
//! is not the only way to learn them — so every test that checks a component is
//! absent from a list also checks that calling it fails, *and* that it fails the
//! way a nonexistent component does rather than with a distinct refusal that
//! would itself disclose what is being hidden.

use std::sync::Arc;

use serde_json::{Value, json};
use turbomcp::methods::request;
use turbomcp::prelude::*;
use turbomcp::tower::{Service, ServiceExt};
use turbomcp::{
    JsonRpcMessage, JsonRpcRequest, JsonRpcResponse, VersionDispatcher, Visibility,
    VisibleComponent,
};

#[derive(Clone)]
struct Catalog;

#[server(name = "catalog", version = "1.0.0")]
impl Catalog {
    #[tool(description = "Anyone may use this")]
    async fn read(&self) -> String {
        "ok".into()
    }

    #[tool(description = "Operators only", tags("internal"))]
    async fn rotate_keys(&self) -> String {
        "rotated".into()
    }

    /// Scope-gated: `#[tool(scopes(…))]` has always refused the *call*; the
    /// visibility policy is what makes the *list* agree.
    #[tool(description = "Wipe everything", scopes("admin"))]
    async fn wipe(&self) -> String {
        "wiped".into()
    }

    #[resource("catalog://public")]
    async fn public(&self) -> McpResult<String> {
        Ok("public".into())
    }

    #[resource("catalog://secret", tags("internal"))]
    async fn secret(&self) -> McpResult<String> {
        Ok("secret".into())
    }

    #[resource("catalog://vault/{+path}", tags("internal"))]
    async fn vault(&self, path: String) -> McpResult<String> {
        Ok(path)
    }

    #[prompt(tags("internal"))]
    async fn debug_prompt(&self, text: String) -> String {
        text
    }

    #[prompt]
    async fn summarize(&self, text: String) -> String {
        text
    }
}

// ---- harness -----------------------------------------------------------------

/// The draft path is stateless — the version and the caller's identity both
/// ride each request's `_meta`, which makes a per-caller assertion a one-liner.
const DRAFT: &str = "2026-07-28";

fn hides_internal() -> Arc<Visibility> {
    Arc::new(Visibility::new().hiding_tagged(["internal"]))
}

fn dispatcher(policy: Option<Arc<Visibility>>) -> VersionDispatcher<Catalog> {
    let builder = Catalog.into_server();
    match policy {
        Some(p) => builder.with_visibility(p).build(),
        None => builder.build(),
    }
}

/// A request as `scopes` (space-separated; empty = anonymous).
fn as_caller(id: i64, method: &str, params: Value, scopes: &str) -> JsonRpcRequest {
    let mut meta = json!({ "io.modelcontextprotocol/protocolVersion": DRAFT });
    if !scopes.is_empty() {
        meta["io.turbomcp.internal/identity"] =
            json!({ "sub": "alice", "claims": { "scope": scopes } });
    }
    let mut params = params;
    params["_meta"] = meta;
    JsonRpcRequest::new(id, method, Some(params))
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

fn field(list: &Value, key: &str, name_field: &str) -> Vec<String> {
    let mut out: Vec<String> = list[key]
        .as_array()
        .unwrap_or_else(|| panic!("no `{key}` array in {list}"))
        .iter()
        .map(|e| e[name_field].as_str().unwrap_or_default().to_owned())
        .collect();
    out.sort();
    out
}

// ---- tests -------------------------------------------------------------------

/// Without a policy nothing changes — the feature is entirely opt-in, and a
/// scope-gated tool is still *listed* (and still refused on call, as always).
#[tokio::test]
async fn without_a_policy_everything_is_visible() {
    let mut svc = dispatcher(None);
    let tools = result(&mut svc, as_caller(1, request::TOOLS_LIST, json!({}), "")).await;
    assert_eq!(
        field(&tools, "tools", "name"),
        ["read", "rotate_keys", "wipe"]
    );
}

/// A tagged tool leaves the list *and* becomes uncallable — and the refusal is
/// the same one an unknown tool gets.
#[tokio::test]
async fn a_hidden_tool_is_absent_and_unreachable() {
    let mut svc = dispatcher(Some(hides_internal()));

    let tools = result(&mut svc, as_caller(1, request::TOOLS_LIST, json!({}), "")).await;
    assert_eq!(field(&tools, "tools", "name"), ["read", "wipe"]);

    let called = result(
        &mut svc,
        as_caller(
            2,
            request::TOOLS_CALL,
            json!({ "name": "rotate_keys", "arguments": {} }),
            "",
        ),
    )
    .await;
    assert_eq!(called["isError"], json!(true));

    // Byte-identical to what a name that was never declared produces: a
    // distinct refusal would disclose the tool the policy is hiding.
    let unknown = result(
        &mut svc,
        as_caller(
            3,
            request::TOOLS_CALL,
            json!({ "name": "no_such_tool", "arguments": {} }),
            "",
        ),
    )
    .await;
    assert_eq!(
        called["content"][0]["text"].as_str().unwrap(),
        "unknown tool: rotate_keys"
    );
    assert_eq!(
        unknown["content"][0]["text"].as_str().unwrap(),
        "unknown tool: no_such_tool"
    );
    assert_eq!(called["isError"], unknown["isError"]);

    // A visible tool still works.
    let ok = result(
        &mut svc,
        as_caller(
            4,
            request::TOOLS_CALL,
            json!({ "name": "read", "arguments": {} }),
            "",
        ),
    )
    .await;
    assert_eq!(ok["content"][0]["text"], json!("ok"));
}

/// The finding this closes: `#[tool(scopes(…))]` refused the call but the tool
/// still appeared in `tools/list` for a caller who could never use it.
#[tokio::test]
async fn declared_scopes_now_filter_the_list_too() {
    let policy = Arc::new(Visibility::new().requiring_declared_scopes());
    let mut svc = dispatcher(Some(policy));

    // Unauthorized: the scope-gated tool is gone from the catalog…
    let tools = result(
        &mut svc,
        as_caller(1, request::TOOLS_LIST, json!({}), "read"),
    )
    .await;
    assert_eq!(field(&tools, "tools", "name"), ["read", "rotate_keys"]);

    // …and unreachable, as an unknown tool.
    let refused = result(
        &mut svc,
        as_caller(
            2,
            request::TOOLS_CALL,
            json!({ "name": "wipe", "arguments": {} }),
            "read",
        ),
    )
    .await;
    assert_eq!(refused["content"][0]["text"], json!("unknown tool: wipe"));

    // Authorized: listed and callable.
    let tools = result(
        &mut svc,
        as_caller(3, request::TOOLS_LIST, json!({}), "read admin"),
    )
    .await;
    assert_eq!(
        field(&tools, "tools", "name"),
        ["read", "rotate_keys", "wipe"]
    );
    let allowed = result(
        &mut svc,
        as_caller(
            4,
            request::TOOLS_CALL,
            json!({ "name": "wipe", "arguments": {} }),
            "read admin",
        ),
    )
    .await;
    assert_eq!(allowed["content"][0]["text"], json!("wiped"));
}

/// One policy, one dispatcher, every listable kind.
#[tokio::test]
async fn resources_templates_and_prompts_are_filtered_too() {
    let mut svc = dispatcher(Some(hides_internal()));

    let resources = result(
        &mut svc,
        as_caller(1, request::RESOURCES_LIST, json!({}), ""),
    )
    .await;
    assert_eq!(field(&resources, "resources", "uri"), ["catalog://public"]);

    let templates = result(
        &mut svc,
        as_caller(2, request::RESOURCES_TEMPLATES_LIST, json!({}), ""),
    )
    .await;
    assert!(
        templates["resourceTemplates"]
            .as_array()
            .unwrap()
            .is_empty(),
        "{templates}"
    );

    let prompts = result(&mut svc, as_caller(3, request::PROMPTS_LIST, json!({}), "")).await;
    assert_eq!(field(&prompts, "prompts", "name"), ["summarize"]);
}

/// Reads are refused for a hidden concrete resource *and* for a URI a hidden
/// template would have produced — a template's URIs are not enumerable, so
/// checking only the concrete list would leave the whole template readable.
#[tokio::test]
async fn a_hidden_resource_and_its_templates_uris_are_not_readable() {
    let mut svc = dispatcher(Some(hides_internal()));

    for uri in ["catalog://secret", "catalog://vault/keys.txt"] {
        let r = respond(
            &mut svc,
            as_caller(1, request::RESOURCES_READ, json!({ "uri": uri }), ""),
        )
        .await;
        let error = r.error.unwrap_or_else(|| panic!("{uri} was readable"));
        assert_eq!(
            error.code,
            McpError::resource_not_found("x").jsonrpc_code_for(&turbomcp::ProtocolVersion::Draft),
            "reading {uri}"
        );
    }

    let ok = result(
        &mut svc,
        as_caller(
            2,
            request::RESOURCES_READ,
            json!({ "uri": "catalog://public" }),
            "",
        ),
    )
    .await;
    assert_eq!(ok["contents"][0]["text"], json!("public"));
}

/// Same guarantee for prompts.
#[tokio::test]
async fn a_hidden_prompt_is_not_gettable() {
    let mut svc = dispatcher(Some(hides_internal()));

    let hidden = respond(
        &mut svc,
        as_caller(
            1,
            request::PROMPTS_GET,
            json!({ "name": "debug_prompt", "arguments": { "text": "x" } }),
            "",
        ),
    )
    .await;
    let unknown = respond(
        &mut svc,
        as_caller(
            2,
            request::PROMPTS_GET,
            json!({ "name": "no_such_prompt", "arguments": {} }),
            "",
        ),
    )
    .await;
    let hidden = hidden.error.expect("the hidden prompt must be refused");
    let unknown = unknown.error.expect("an unknown prompt must be refused");
    assert_eq!(hidden.code, unknown.code);
    assert!(
        hidden.message.ends_with("unknown prompt: debug_prompt"),
        "{}",
        hidden.message
    );
    assert!(
        unknown.message.ends_with("unknown prompt: no_such_prompt"),
        "{}",
        unknown.message
    );

    let ok = result(
        &mut svc,
        as_caller(
            3,
            request::PROMPTS_GET,
            json!({ "name": "summarize", "arguments": { "text": "hi" } }),
            "",
        ),
    )
    .await;
    assert_eq!(ok["messages"][0]["content"]["text"], json!("hi"));
}

/// The escape hatch v3's leaking session map should have been: a policy is a
/// function of `(component, request)`, so per-caller state is the deployment's
/// to store and to expire.
#[tokio::test]
async fn a_custom_policy_sees_the_whole_request() {
    // "Unlock `rotate_keys` only for callers whose identity says so" — the
    // shape of progressive disclosure, with no framework-owned map.
    let policy = Arc::new(|c: &VisibleComponent<'_>| {
        c.id != "rotate_keys" || c.request.identity.subject() == Some("alice")
    });
    let mut svc = Catalog.into_server().with_visibility(policy).build();

    let anon = result(&mut svc, as_caller(1, request::TOOLS_LIST, json!({}), "")).await;
    assert_eq!(field(&anon, "tools", "name"), ["read", "wipe"]);

    let alice = result(
        &mut svc,
        as_caller(2, request::TOOLS_LIST, json!({}), "any"),
    )
    .await;
    assert_eq!(
        field(&alice, "tools", "name"),
        ["read", "rotate_keys", "wipe"]
    );
}

/// The policy applies on every protocol revision, since it runs on the neutral
/// result before the wire conversion.
#[tokio::test]
async fn filtering_happens_on_every_revision() {
    let mut svc = turbomcp::LegacySessionAdapter::new(dispatcher(Some(hides_internal())));
    for version in ["2025-06-18", "2025-11-25"] {
        let init = result(
            &mut svc,
            JsonRpcRequest::new(
                1,
                request::INITIALIZE,
                Some(json!({
                    "protocolVersion": version,
                    "capabilities": {},
                    "clientInfo": { "name": "t", "version": "1" },
                })),
            ),
        )
        .await;
        assert_eq!(init["protocolVersion"], json!(version));

        let tools = result(
            &mut svc,
            JsonRpcRequest::new(2, request::TOOLS_LIST, Some(json!({}))),
        )
        .await;
        assert_eq!(
            field(&tools, "tools", "name"),
            ["read", "wipe"],
            "not filtered on {version}"
        );
    }
}
