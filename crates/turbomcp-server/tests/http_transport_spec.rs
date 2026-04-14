#![cfg(feature = "http")]

use reqwest::{Client, StatusCode, header};
use serde_json::json;
use std::future::Future;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use turbomcp_core::context::RequestContext as CoreRequestContext;
use turbomcp_core::error::{McpError, McpResult};
use turbomcp_server::McpHandler;
use turbomcp_server::ServerConfig;
use turbomcp_server::transport::http;
use turbomcp_types::{
    Prompt, PromptResult, Resource, ResourceResult, ServerInfo, Tool, ToolResult,
};

#[derive(Clone)]
struct TestHandler;

impl McpHandler for TestHandler {
    fn server_info(&self) -> ServerInfo {
        ServerInfo::new("http-test", "1.0.0")
    }

    fn list_tools(&self) -> Vec<Tool> {
        Vec::new()
    }

    fn list_resources(&self) -> Vec<Resource> {
        Vec::new()
    }

    fn list_prompts(&self) -> Vec<Prompt> {
        Vec::new()
    }

    fn call_tool(
        &self,
        name: &str,
        _args: serde_json::Value,
        _ctx: &CoreRequestContext,
    ) -> impl Future<Output = McpResult<ToolResult>> + Send {
        let name = name.to_string();
        async move { Err(McpError::tool_not_found(&name)) }
    }

    fn read_resource(
        &self,
        uri: &str,
        _ctx: &CoreRequestContext,
    ) -> impl Future<Output = McpResult<ResourceResult>> + Send {
        let uri = uri.to_string();
        async move { Err(McpError::resource_not_found(&uri)) }
    }

    fn get_prompt(
        &self,
        name: &str,
        _args: Option<serde_json::Value>,
        _ctx: &CoreRequestContext,
    ) -> impl Future<Output = McpResult<PromptResult>> + Send {
        let name = name.to_string();
        async move { Err(McpError::prompt_not_found(&name)) }
    }
}

async fn spawn_server() -> (String, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);

    let addr_string = addr.to_string();
    let handle = tokio::spawn(async move {
        http::run(&TestHandler, &addr_string).await.unwrap();
    });

    tokio::time::sleep(Duration::from_millis(200)).await;
    (format!("http://{}", addr), handle)
}

async fn spawn_server_with_config(config: ServerConfig) -> (String, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);

    let addr_string = addr.to_string();
    let handle = tokio::spawn(async move {
        http::run_with_config(&TestHandler, &addr_string, &config)
            .await
            .unwrap();
    });

    tokio::time::sleep(Duration::from_millis(200)).await;
    (format!("http://{}", addr), handle)
}

fn initialize_request() -> serde_json::Value {
    json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-11-25",
            "clientInfo": {
                "name": "spec-test-client",
                "version": "1.0.0"
            },
            "capabilities": {}
        }
    })
}

async fn initialize_session(client: &Client, base_url: &str) -> String {
    let response = client
        .post(format!("{}/mcp", base_url))
        .header(header::ACCEPT, "application/json, text/event-stream")
        .json(&initialize_request())
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let session_id = response
        .headers()
        .get("mcp-session-id")
        .expect("initialize response should include MCP-Session-Id")
        .to_str()
        .unwrap()
        .to_string();

    let body: serde_json::Value = response.json().await.unwrap();
    assert_eq!(body["result"]["protocolVersion"], "2025-11-25");
    assert!(!session_id.is_empty());
    session_id
}

#[tokio::test]
async fn initialize_returns_session_id_header() {
    let (base_url, handle) = spawn_server().await;
    let client = Client::new();

    let session_id = initialize_session(&client, &base_url).await;
    assert!(!session_id.is_empty());

    handle.abort();
}

#[tokio::test]
async fn initialized_notification_returns_202_without_body() {
    let (base_url, handle) = spawn_server().await;
    let client = Client::new();
    let session_id = initialize_session(&client, &base_url).await;

    let response = client
        .post(format!("{}/mcp", base_url))
        .header(header::ACCEPT, "application/json, text/event-stream")
        .header("Mcp-Session-Id", &session_id)
        .header("MCP-Protocol-Version", "2025-11-25")
        .json(&json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert!(response.text().await.unwrap().is_empty());

    handle.abort();
}

#[tokio::test]
async fn client_jsonrpc_response_post_returns_202() {
    let (base_url, handle) = spawn_server().await;
    let client = Client::new();
    let session_id = initialize_session(&client, &base_url).await;

    let response = client
        .post(format!("{}/mcp", base_url))
        .header(header::ACCEPT, "application/json, text/event-stream")
        .header("Mcp-Session-Id", &session_id)
        .header("MCP-Protocol-Version", "2025-11-25")
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 99,
            "result": {
                "ok": true
            }
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert!(response.text().await.unwrap().is_empty());

    handle.abort();
}

#[tokio::test]
async fn get_and_delete_use_same_endpoint_session() {
    let (base_url, handle) = spawn_server().await;
    let client = Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let session_id = initialize_session(&client, &base_url).await;

    let sse_response = client
        .get(format!("{}/mcp", base_url))
        .header(header::ACCEPT, "text/event-stream")
        .header("Mcp-Session-Id", &session_id)
        .header("MCP-Protocol-Version", "2025-11-25")
        .send()
        .await
        .unwrap();

    assert_eq!(sse_response.status(), StatusCode::OK);
    assert_eq!(
        sse_response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("text/event-stream")
    );

    drop(sse_response);

    let delete_response = client
        .delete(format!("{}/mcp", base_url))
        .header("Mcp-Session-Id", &session_id)
        .header("MCP-Protocol-Version", "2025-11-25")
        .send()
        .await
        .unwrap();

    assert_eq!(delete_response.status(), StatusCode::NO_CONTENT);

    let after_delete = client
        .get(format!("{}/mcp", base_url))
        .header(header::ACCEPT, "text/event-stream")
        .header("Mcp-Session-Id", &session_id)
        .header("MCP-Protocol-Version", "2025-11-25")
        .send()
        .await
        .unwrap();

    assert_eq!(after_delete.status(), StatusCode::NOT_FOUND);

    handle.abort();
}

#[tokio::test]
async fn rejects_untrusted_origin() {
    let (base_url, handle) = spawn_server().await;
    let client = Client::new();

    let response = client
        .post(format!("{}/mcp", base_url))
        .header(header::ACCEPT, "application/json, text/event-stream")
        .header(header::ORIGIN, "https://evil.example")
        .json(&initialize_request())
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    handle.abort();
}

#[tokio::test]
async fn allows_configured_origin() {
    let config = ServerConfig::builder()
        .allow_origin("https://app.example.com")
        .allow_localhost_origins(false)
        .build();
    let (base_url, handle) = spawn_server_with_config(config).await;
    let client = Client::new();

    let response = client
        .post(format!("{}/mcp", base_url))
        .header(header::ACCEPT, "application/json, text/event-stream")
        .header(header::ORIGIN, "https://app.example.com")
        .json(&initialize_request())
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    handle.abort();
}

#[tokio::test]
async fn duplicate_request_ids_are_rejected() {
    let (base_url, handle) = spawn_server().await;
    let client = Client::new();
    let session_id = initialize_session(&client, &base_url).await;

    let request = json!({
        "jsonrpc": "2.0",
        "id": 7,
        "method": "tools/list"
    });

    let first = client
        .post(format!("{}/mcp", base_url))
        .header(header::ACCEPT, "application/json, text/event-stream")
        .header("Mcp-Session-Id", &session_id)
        .header("MCP-Protocol-Version", "2025-11-25")
        .json(&request)
        .send()
        .await
        .unwrap();

    assert_eq!(first.status(), StatusCode::OK);
    let first_body: serde_json::Value = first.json().await.unwrap();
    assert!(first_body.get("result").is_some());

    let duplicate = client
        .post(format!("{}/mcp", base_url))
        .header(header::ACCEPT, "application/json, text/event-stream")
        .header("Mcp-Session-Id", &session_id)
        .header("MCP-Protocol-Version", "2025-11-25")
        .json(&request)
        .send()
        .await
        .unwrap();

    assert_eq!(duplicate.status(), StatusCode::OK);
    let duplicate_body: serde_json::Value = duplicate.json().await.unwrap();
    assert_eq!(duplicate_body["error"]["code"], -32600);
    assert!(
        duplicate_body["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("already used"))
    );

    handle.abort();
}
