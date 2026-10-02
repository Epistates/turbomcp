//! The extension seam across revisions (SEP-2133): an extension is advertised
//! and routed on the revisions it speaks, a session's `initialize`
//! declaration is what a stateful client opts in with, handlers can ask
//! whether the client declared one, and colliding registrations are refused
//! at build time.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};
use tower::{Service, ServiceExt};
use turbomcp_core::{
    Implementation, JsonRpcMessage, JsonRpcRequest, JsonRpcResponse, McpResult, ProtocolVersion,
};
use turbomcp_protocol::neutral;
use turbomcp_server::{
    CallToolContext, Extension, ExtensionRequest, LegacySessionAdapter, ListToolsContext,
    McpServerCore, MethodRouter, VersionDispatcher, WithTools,
};

const EVERYWHERE: &[ProtocolVersion] = &[
    ProtocolVersion::V2025_06_18,
    ProtocolVersion::V2025_11_25,
    ProtocolVersion::V2026_07_28,
];

/// An extension with one method, speaking the revisions it is given.
struct Echo {
    id: &'static str,
    methods: &'static [&'static str],
    versions: &'static [ProtocolVersion],
}

#[async_trait]
impl Extension for Echo {
    fn id(&self) -> &'static str {
        self.id
    }
    fn settings(&self) -> Value {
        json!({ "flavour": "echo" })
    }
    fn methods(&self) -> &'static [&'static str] {
        self.methods
    }
    fn protocol_versions(&self) -> &'static [ProtocolVersion] {
        self.versions
    }
    async fn dispatch(&self, request: ExtensionRequest) -> JsonRpcMessage {
        JsonRpcResponse::success(request.request.id, json!({ "echoed": self.id })).into()
    }
}

fn echo(id: &'static str, versions: &'static [ProtocolVersion]) -> Arc<Echo> {
    Arc::new(Echo {
        id,
        methods: &["echo/ping"],
        versions,
    })
}

/// A tool that reports whether the client declared `com.example/apps`.
#[derive(Clone)]
struct Probe;

impl McpServerCore for Probe {
    fn server_info(&self) -> Implementation {
        Implementation::new("probe", "1.0.0")
    }
}

impl WithTools for Probe {
    async fn list_tools(
        &self,
        _ctx: &ListToolsContext,
        _params: neutral::ListParams,
    ) -> McpResult<neutral::ListToolsResult> {
        Ok(neutral::ListToolsResult::new(vec![neutral::Tool::new(
            "probe",
            json!({ "type": "object" }),
        )]))
    }

    async fn call_tool(
        &self,
        ctx: &CallToolContext,
        _params: neutral::CallToolParams,
    ) -> McpResult<neutral::CallToolResult> {
        Ok(neutral::CallToolResult::text(
            ctx.base.supports_extension("com.example/apps").to_string(),
        ))
    }
}

fn dispatcher() -> VersionDispatcher<Probe> {
    VersionDispatcher::new(Probe, MethodRouter::new().with_tools())
        .with_extension(echo("com.example/apps", EVERYWHERE))
        .with_extension(Arc::new(Echo {
            id: "com.example/modern",
            methods: &["modern/ping"],
            versions: &[ProtocolVersion::V2026_07_28],
        }))
}

type Svc = LegacySessionAdapter<VersionDispatcher<Probe>>;

async fn reply(svc: &mut Svc, req: JsonRpcRequest) -> JsonRpcResponse {
    match svc.ready().await.unwrap().call(req.into()).await.unwrap() {
        Some(JsonRpcMessage::Response(r)) => r,
        other => panic!("expected a response, got {other:?}"),
    }
}

/// A session on `version`, declaring `extensions` in `initialize`. Returns
/// the service and the `initialize` result.
async fn session(version: &str, extensions: Value) -> (Svc, Value) {
    let mut svc = LegacySessionAdapter::new(dispatcher());
    let init = reply(
        &mut svc,
        JsonRpcRequest::new(
            0,
            "initialize",
            Some(json!({
                "protocolVersion": version,
                "capabilities": { "extensions": extensions },
                "clientInfo": { "name": "host", "version": "1" },
            })),
        ),
    )
    .await;
    (svc, init.result.expect("initialized"))
}

#[tokio::test]
async fn initialize_advertises_what_the_negotiated_revision_speaks() {
    for version in ["2025-11-25", "2025-06-18"] {
        let (_, init) = session(version, json!({})).await;
        let extensions = &init["capabilities"]["extensions"];
        assert_eq!(
            extensions["com.example/apps"],
            json!({ "flavour": "echo" }),
            "{version}: {init}"
        );
        assert!(
            extensions.get("com.example/modern").is_none(),
            "{version}: a 2026-07-28-only extension stays out: {init}"
        );
    }
}

#[tokio::test]
async fn discover_advertises_what_the_stateless_revision_speaks() {
    let mut svc = dispatcher();
    let out = svc
        .ready()
        .await
        .unwrap()
        .call(
            JsonRpcRequest::new(
                1,
                "server/discover",
                Some(json!({ "_meta": {
                    "io.modelcontextprotocol/protocolVersion": "2026-07-28",
                    "io.modelcontextprotocol/clientCapabilities": {},
                }})),
            )
            .into(),
        )
        .await
        .unwrap();
    let Some(JsonRpcMessage::Response(r)) = out else {
        panic!("expected a response");
    };
    let extensions = &r.result.unwrap()["capabilities"]["extensions"];
    assert!(extensions.get("com.example/apps").is_some());
    assert!(extensions.get("com.example/modern").is_some());
}

#[tokio::test]
async fn a_session_routes_an_extension_it_declared_in_initialize() {
    let (mut svc, _) = session("2025-11-25", json!({ "com.example/apps": {} })).await;
    let r = reply(&mut svc, JsonRpcRequest::new(1, "echo/ping", None)).await;
    assert_eq!(r.result.unwrap()["echoed"], "com.example/apps");

    // The handler sees the declaration too.
    let r = reply(
        &mut svc,
        JsonRpcRequest::new(2, "tools/call", Some(json!({ "name": "probe" }))),
    )
    .await;
    assert_eq!(r.result.unwrap()["content"][0]["text"], "true");
}

#[tokio::test]
async fn a_session_that_did_not_declare_it_is_told_what_to_declare() {
    let (mut svc, _) = session("2025-11-25", json!({})).await;
    let r = reply(&mut svc, JsonRpcRequest::new(1, "echo/ping", None)).await;
    let error = r.error.expect("refused");
    // Invalid Params: `-32021` exists only on the stateless wire.
    assert_eq!(error.code, -32602);
    assert_eq!(
        error.data.unwrap()["requiredCapabilities"]["extensions"],
        json!({ "com.example/apps": {} })
    );
    let r = reply(
        &mut svc,
        JsonRpcRequest::new(2, "tools/call", Some(json!({ "name": "probe" }))),
    )
    .await;
    assert_eq!(r.result.unwrap()["content"][0]["text"], "false");
}

#[tokio::test]
async fn a_revision_the_extension_does_not_speak_does_not_route_to_it() {
    let (mut svc, _) = session("2025-11-25", json!({ "com.example/modern": {} })).await;
    let r = reply(&mut svc, JsonRpcRequest::new(1, "modern/ping", None)).await;
    assert_eq!(r.error.expect("unknown here").code, -32601);
}

#[test]
#[should_panic(expected = "registered twice")]
fn a_second_extension_with_the_same_id_is_refused() {
    let _ = dispatcher().with_extension(echo("com.example/apps", EVERYWHERE));
}

#[test]
#[should_panic(expected = "which the core protocol defines")]
fn an_extension_claiming_a_core_method_is_refused() {
    let _ = VersionDispatcher::new(Probe, MethodRouter::new()).with_extension(Arc::new(Echo {
        id: "com.example/hijack",
        methods: &["tools/call"],
        versions: EVERYWHERE,
    }));
}

#[test]
#[should_panic(expected = "both claim `echo/ping`")]
fn two_extensions_claiming_one_method_on_one_revision_are_refused() {
    let _ = dispatcher().with_extension(echo("com.example/other", &[ProtocolVersion::V2025_11_25]));
}

/// The Tasks extension's shape: `tasks/get` is core on `2025-11-25`, so an
/// extension may claim it only on the revision where it isn't.
#[test]
fn a_method_core_on_another_revision_may_be_claimed_where_it_is_not() {
    let _ = VersionDispatcher::new(Probe, MethodRouter::new()).with_extension(Arc::new(Echo {
        id: "com.example/tasks",
        methods: &["tasks/get"],
        versions: &[ProtocolVersion::V2026_07_28],
    }));
}
