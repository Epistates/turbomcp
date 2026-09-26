//! A hidden resource template the matcher cannot parse.
//!
//! A malformed template matches no URI, so the visibility check used to find
//! "no component" and hand URIs under it to the handler, which a hand-written
//! server may well serve. The policy was told to hide that template; failing
//! open would expose exactly what it hides.

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

    // Outside its literal prefix the handler still decides, as for any URI
    // the server never declared.
    let elsewhere = read(&mut svc, "vault://other").await;
    assert_eq!(
        elsewhere["result"]["contents"][0]["text"], "secret",
        "{elsewhere}"
    );
}
