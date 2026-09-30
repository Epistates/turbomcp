//! Under a visibility policy, a resource URI the policy can't judge is
//! refused.
//!
//! A malformed template matches no URI, and a hand-written handler may serve
//! URIs it never lists. The visibility check used to find "no component" and
//! hand such URIs to the handler; the policy was told to hide the template,
//! and for resources it is the only gate. Deny by default; a server that
//! serves unlisted URIs makes them judgeable through `lookup_resource`.

use std::sync::Arc;

use serde_json::{Value, json};
use tower::{Service, ServiceExt};
use turbomcp_core::{Implementation, JsonRpcMessage, JsonRpcRequest, McpResult};
use turbomcp_protocol::neutral;
use turbomcp_server::{
    ListResourceTemplatesContext, ListResourcesContext, McpServerCore, MethodRouter,
    ReadResourceContext, VersionDispatcher, Visibility, WithResources,
};

#[derive(Clone)]
struct HandRolled;

impl McpServerCore for HandRolled {
    fn server_info(&self) -> Implementation {
        Implementation::new("hand-rolled", "0.1.0")
    }
}

impl WithResources for HandRolled {
    async fn list_resources(
        &self,
        _ctx: &ListResourcesContext,
        _params: neutral::ListParams,
    ) -> McpResult<neutral::ListResourcesResult> {
        Ok(neutral::ListResourcesResult::new(Vec::new()))
    }

    async fn list_resource_templates(
        &self,
        _ctx: &ListResourceTemplatesContext,
        _params: neutral::ListParams,
    ) -> McpResult<neutral::ListResourceTemplatesResult> {
        Ok(neutral::ListResourceTemplatesResult::new(vec![
            // Unclosed brace: no URI matches it.
            neutral::ResourceTemplate::new("vault://keys/{name", "keys")
                .with_meta_entry(turbomcp_core::meta::keys::TAGS, json!(["internal"])),
        ]))
    }

    /// Serves anything, the way a server matching URIs by hand might.
    async fn read_resource(
        &self,
        _ctx: &ReadResourceContext,
        params: neutral::ReadResourceParams,
    ) -> McpResult<neutral::ReadResourceResult> {
        Ok(neutral::ReadResourceResult::text(params.uri, "secret"))
    }
}

async fn read(svc: &mut VersionDispatcher<HandRolled>, uri: &str) -> Value {
    let req = JsonRpcRequest::new(
        1,
        "resources/read",
        Some(json!({
            "uri": uri,
            "_meta": {
                "io.modelcontextprotocol/protocolVersion": "2026-07-28",
                "io.modelcontextprotocol/clientCapabilities": {},
            },
        })),
    );
    let Some(JsonRpcMessage::Response(r)) =
        svc.ready().await.unwrap().call(req.into()).await.unwrap()
    else {
        panic!("expected a response");
    };
    serde_json::to_value(r).unwrap()
}

#[tokio::test]
async fn uris_under_a_hidden_malformed_template_stay_hidden() {
    let mut svc = VersionDispatcher::new(HandRolled, MethodRouter::new().with_resources())
        .with_visibility(Arc::new(Visibility::new().hiding_tagged(["internal"])));

    let under = read(&mut svc, "vault://keys/root").await;
    assert_eq!(under["error"]["code"], -32602, "{under}");

    // A URI the server never declared is refused the same way: the policy
    // has nothing to judge it by.
    let elsewhere = read(&mut svc, "vault://other").await;
    assert_eq!(elsewhere["error"]["code"], -32602, "{elsewhere}");
}

#[tokio::test]
async fn without_a_policy_the_handler_decides() {
    let mut svc = VersionDispatcher::new(HandRolled, MethodRouter::new().with_resources());
    let unlisted = read(&mut svc, "vault://other").await;
    assert_eq!(
        unlisted["result"]["contents"][0]["text"], "secret",
        "{unlisted}"
    );
}

/// Serves `note://` URIs it doesn't list, and says what each one is through
/// `lookup_resource`, so a policy can judge it.
#[derive(Clone)]
struct Dynamic;

impl McpServerCore for Dynamic {
    fn server_info(&self) -> Implementation {
        Implementation::new("dynamic", "0.1.0")
    }
}

impl WithResources for Dynamic {
    async fn lookup_resource(
        &self,
        _ctx: &ListResourcesContext,
        uri: String,
    ) -> McpResult<Option<neutral::Resource>> {
        let tags = if uri.starts_with("note://private/") {
            json!(["internal"])
        } else {
            json!([])
        };
        Ok(uri.starts_with("note://").then(|| {
            neutral::Resource::new(uri, "note")
                .with_meta_entry(turbomcp_core::meta::keys::TAGS, tags)
        }))
    }

    async fn list_resources(
        &self,
        _ctx: &ListResourcesContext,
        _params: neutral::ListParams,
    ) -> McpResult<neutral::ListResourcesResult> {
        Ok(neutral::ListResourcesResult::new(Vec::new()))
    }

    async fn read_resource(
        &self,
        _ctx: &ReadResourceContext,
        params: neutral::ReadResourceParams,
    ) -> McpResult<neutral::ReadResourceResult> {
        Ok(neutral::ReadResourceResult::text(params.uri, "note body"))
    }
}

async fn read_dynamic(svc: &mut VersionDispatcher<Dynamic>, uri: &str) -> Value {
    let req = JsonRpcRequest::new(
        1,
        "resources/read",
        Some(json!({
            "uri": uri,
            "_meta": {
                "io.modelcontextprotocol/protocolVersion": "2026-07-28",
                "io.modelcontextprotocol/clientCapabilities": {},
            },
        })),
    );
    let Some(JsonRpcMessage::Response(r)) =
        svc.ready().await.unwrap().call(req.into()).await.unwrap()
    else {
        panic!("expected a response");
    };
    serde_json::to_value(r).unwrap()
}

/// The way to serve unlisted URIs under a policy: `lookup_resource` returns a
/// component for them, and the policy judges it like any other.
#[tokio::test]
async fn an_overridden_lookup_lets_the_policy_judge_unlisted_uris() {
    let mut svc = VersionDispatcher::new(Dynamic, MethodRouter::new().with_resources())
        .with_visibility(Arc::new(Visibility::new().hiding_tagged(["internal"])));
    let public = read_dynamic(&mut svc, "note://public/1").await;
    assert_eq!(
        public["result"]["contents"][0]["text"], "note body",
        "{public}"
    );
    let private = read_dynamic(&mut svc, "note://private/1").await;
    assert_eq!(private["error"]["code"], -32602, "{private}");
}
