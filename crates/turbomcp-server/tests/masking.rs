//! `ServerBuilder::mask_internal_errors`: an internal error's text reaches the
//! log, not the client, on both paths an error can take out of a handler.

use serde_json::{Value, json};
use tower::{Service, ServiceExt};
use turbomcp_core::{
    Implementation, JsonRpcMessage, JsonRpcRequest, McpError, McpRequest, McpResult, codes,
};
use turbomcp_protocol::neutral;
use turbomcp_server::{
    CallToolContext, IntoServerBuilder, ListResourcesContext, ListToolsContext, McpServerCore,
    MethodRouter, ReadResourceContext, WithResources, WithTools,
};

const SECRET: &str = "postgres://admin:hunter2@db.internal:5432";

#[derive(Clone)]
struct Leaky;

impl McpServerCore for Leaky {
    fn server_info(&self) -> Implementation {
        Implementation::new("leaky", "1.0.0")
    }

    fn register(router: MethodRouter<Self>) -> MethodRouter<Self> {
        router.with_tools().with_resources()
    }
}

impl WithTools for Leaky {
    async fn list_tools(
        &self,
        _ctx: &ListToolsContext,
        _params: neutral::ListParams,
    ) -> McpResult<neutral::ListToolsResult> {
        let schema = json!({ "type": "object" });
        Ok(neutral::ListToolsResult::new(vec![
            neutral::Tool::new("query", schema.clone()),
            neutral::Tool::new("quota", schema),
        ]))
    }

    async fn call_tool(
        &self,
        _ctx: &CallToolContext,
        params: neutral::CallToolParams,
    ) -> McpResult<neutral::CallToolResult> {
        match params.name.as_str() {
            "query" => Err(McpError::internal(format!("connect {SECRET}: refused"))),
            _ => Err(McpError::tool_execution_failed(
                "quota",
                "daily quota used up",
            )),
        }
    }
}

impl WithResources for Leaky {
    async fn list_resources(
        &self,
        _ctx: &ListResourcesContext,
        _params: neutral::ListParams,
    ) -> McpResult<neutral::ListResourcesResult> {
        Ok(neutral::ListResourcesResult::new(vec![
            neutral::Resource::new("db://rows", "rows"),
        ]))
    }

    async fn read_resource(
        &self,
        _ctx: &ReadResourceContext,
        _params: neutral::ReadResourceParams,
    ) -> McpResult<neutral::ReadResourceResult> {
        Err(McpError::internal(format!("read {SECRET}: timeout")))
    }
}

async fn call(masked: bool, method: &str, params: Value) -> Value {
    let builder = Leaky.into_server();
    let builder = if masked {
        builder.mask_internal_errors()
    } else {
        builder
    };
    let mut svc = builder.build();
    let mut params = params;
    params["_meta"] = json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": {},
    });
    let request: McpRequest = JsonRpcRequest::new(1, method, Some(params)).into();
    let reply = svc.ready().await.unwrap().call(request).await.unwrap();
    let Some(JsonRpcMessage::Response(response)) = reply else {
        panic!("expected a response");
    };
    serde_json::to_value(response).unwrap()
}

fn tool(name: &str) -> Value {
    json!({ "name": name, "arguments": {} })
}

fn is_reference(text: &str) -> bool {
    text.strip_prefix("internal error (ref: ")
        .and_then(|rest| rest.strip_suffix(')'))
        .is_some_and(uuid_like)
}

fn uuid_like(id: &str) -> bool {
    id.len() == 36 && id.chars().filter(|c| *c == '-').count() == 4
}

/// A hand-written tool's internal error is a protocol error (`#[tool]`'s
/// becomes an `isError` result; the facade tests that path).
#[tokio::test]
async fn a_hand_written_tools_internal_error_is_masked_on_the_wire() {
    let reply = call(true, "tools/call", tool("query")).await;
    let error = &reply["error"];
    assert_eq!(error["code"], codes::INTERNAL_ERROR, "{reply}");
    assert!(is_reference(error["message"].as_str().unwrap()), "{reply}");
    assert!(!reply.to_string().contains("hunter2"), "{reply}");
}

#[tokio::test]
async fn a_resources_internal_error_is_masked_on_the_wire() {
    let reply = call(true, "resources/read", json!({ "uri": "db://rows" })).await;
    let error = &reply["error"];
    assert_eq!(error["code"], codes::INTERNAL_ERROR, "{reply}");
    assert!(is_reference(error["message"].as_str().unwrap()), "{reply}");
    assert!(error.get("data").is_none());
    assert!(!reply.to_string().contains("hunter2"), "{reply}");
}

/// Errors a tool means the model to read are not the operator's secrets.
#[tokio::test]
async fn a_tool_failure_is_not_masked() {
    let reply = call(true, "tools/call", tool("quota")).await;
    assert_eq!(
        reply["result"]["content"][0]["text"],
        "tool 'quota' failed: daily quota used up"
    );
}

/// Off by default.
#[tokio::test]
async fn unmasked_by_default() {
    let reply = call(false, "tools/call", tool("query")).await;
    assert!(
        reply["error"]["message"]
            .as_str()
            .unwrap()
            .contains("hunter2")
    );

    let reply = call(false, "resources/read", json!({ "uri": "db://rows" })).await;
    assert!(
        reply["error"]["message"]
            .as_str()
            .unwrap()
            .contains("hunter2")
    );
}
