#![cfg(feature = "http")]

use futures::StreamExt;
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
async fn sse_sends_primer_event_with_id() {
    use tokio::io::AsyncReadExt;

    let (base_url, handle) = spawn_server().await;
    let client = Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let session_id = initialize_session(&client, &base_url).await;

    let response = client
        .get(format!("{}/mcp", base_url))
        .header(header::ACCEPT, "text/event-stream")
        .header("Mcp-Session-Id", &session_id)
        .header("MCP-Protocol-Version", "2025-11-25")
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    // Read just enough bytes to see the first event (up to the second
    // blank line). An SSE primer event has an `id:` line and an empty
    // `data:` line followed by the record-terminating blank line.
    let mut stream = tokio_util::io::StreamReader::new(
        response
            .bytes_stream()
            .map(|r| r.map_err(std::io::Error::other)),
    );
    let mut buf = vec![0u8; 512];
    let mut collected = String::new();
    for _ in 0..8 {
        let n = tokio::time::timeout(Duration::from_secs(1), stream.read(&mut buf))
            .await
            .expect("read timed out")
            .expect("read error");
        if n == 0 {
            break;
        }
        collected.push_str(std::str::from_utf8(&buf[..n]).unwrap());
        if collected.contains("\n\n") {
            break;
        }
    }

    // Per SSE spec, a valid primer event carries only an `id:` line and no
    // `data:` line (equivalent to an empty data field). axum's Sse writer
    // drops `data:` lines when the payload is empty, so we require the id
    // and require the event to carry no non-empty data line.
    let first_event = collected.split("\n\n").next().unwrap_or("");
    assert!(
        first_event.lines().any(|line| line.starts_with("id:")),
        "first SSE event should carry an id for Last-Event-ID resume, got: {collected:?}"
    );
    assert!(
        first_event
            .lines()
            .all(|line| !line.starts_with("data:")
                || line.trim_start_matches("data:").trim().is_empty()),
        "primer event should have no non-empty data field, got: {collected:?}"
    );

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
async fn oversized_body_returns_413() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let (base_url, handle) = spawn_server().await;
    // Strip scheme so we can raw-socket connect.
    let host_port = base_url
        .trim_start_matches("http://")
        .trim_start_matches("https://");
    let addr: std::net::SocketAddr = host_port.parse().unwrap();

    // Advertise a huge Content-Length but only upload a few hundred bytes
    // so reqwest's "write the whole body before reading the response" race
    // doesn't apply. We use a raw TCP stream so the server can reject via
    // headers alone (tower-http's RequestBodyLimitLayer will 413 without
    // waiting for the full body, since it consults the declared length).
    let declared_len = (11 * 1024 * 1024) as usize;
    let request = format!(
        "POST /mcp HTTP/1.1\r\n\
         Host: {host_port}\r\n\
         Accept: application/json, text/event-stream\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {declared_len}\r\n\
         Connection: close\r\n\
         \r\n\
         {{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\"}}"
    );

    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream.write_all(request.as_bytes()).await.unwrap();
    // Best-effort close the write half so the server doesn't block waiting
    // for more body. Ignore errors — the server may already have 413'd us.
    let _ = stream.shutdown().await;

    let mut buf = Vec::with_capacity(1024);
    let _ = tokio::time::timeout(Duration::from_secs(2), stream.read_to_end(&mut buf)).await;

    let response_head = String::from_utf8_lossy(&buf);
    assert!(
        response_head.starts_with("HTTP/1.1 413") || response_head.starts_with("HTTP/1.0 413"),
        "expected 413 Payload Too Large, got: {response_head:?}"
    );

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
