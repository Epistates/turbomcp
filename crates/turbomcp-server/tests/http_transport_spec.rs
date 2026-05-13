#![cfg(feature = "http")]

use futures::StreamExt;
use reqwest::{Client, StatusCode, header};
use serde_json::json;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use turbomcp_core::context::RequestContext as CoreRequestContext;
use turbomcp_core::error::{McpError, McpResult};
use turbomcp_server::McpHandler;
use turbomcp_server::ServerConfig;
use turbomcp_server::transport::http;
use turbomcp_types::{
    CreateMessageRequest, Prompt, PromptResult, Resource, ResourceResult, SamplingContent,
    SamplingMessage, ServerInfo, Tool, ToolResult,
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

    async fn call_tool(
        &self,
        name: &str,
        _args: serde_json::Value,
        _ctx: &CoreRequestContext,
    ) -> McpResult<ToolResult> {
        Err(McpError::tool_not_found(name))
    }

    async fn read_resource(
        &self,
        uri: &str,
        _ctx: &CoreRequestContext,
    ) -> McpResult<ResourceResult> {
        Err(McpError::resource_not_found(uri))
    }

    async fn get_prompt(
        &self,
        name: &str,
        _args: Option<serde_json::Value>,
        _ctx: &CoreRequestContext,
    ) -> McpResult<PromptResult> {
        Err(McpError::prompt_not_found(name))
    }
}

#[derive(Clone)]
struct SamplingHandler;

impl McpHandler for SamplingHandler {
    fn server_info(&self) -> ServerInfo {
        ServerInfo::new("sampling-http-test", "1.0.0")
    }

    fn list_tools(&self) -> Vec<Tool> {
        vec![Tool::new("sample_text", "Sample text from the client")]
    }

    fn list_resources(&self) -> Vec<Resource> {
        Vec::new()
    }

    fn list_prompts(&self) -> Vec<Prompt> {
        Vec::new()
    }

    async fn call_tool<'a>(
        &'a self,
        name: &'a str,
        _args: serde_json::Value,
        ctx: &'a CoreRequestContext,
    ) -> McpResult<ToolResult> {
        if name != "sample_text" {
            return Err(McpError::tool_not_found(name));
        }

        let result = ctx
            .sample(CreateMessageRequest {
                messages: vec![SamplingMessage::user("sample a short response")],
                max_tokens: 32,
                ..Default::default()
            })
            .await?;

        let text = result
            .content
            .to_vec()
            .into_iter()
            .find_map(|content| match content {
                SamplingContent::Text(text) => Some(text.text.clone()),
                _ => None,
            })
            .unwrap_or_else(|| result.model.clone());

        Ok(ToolResult::text(text))
    }

    async fn read_resource(
        &self,
        uri: &str,
        _ctx: &CoreRequestContext,
    ) -> McpResult<ResourceResult> {
        Err(McpError::resource_not_found(uri))
    }

    async fn get_prompt(
        &self,
        name: &str,
        _args: Option<serde_json::Value>,
        _ctx: &CoreRequestContext,
    ) -> McpResult<PromptResult> {
        Err(McpError::prompt_not_found(name))
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

async fn spawn_sampling_server() -> (String, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);

    let addr_string = addr.to_string();
    let handle = tokio::spawn(async move {
        http::run(&SamplingHandler, &addr_string).await.unwrap();
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
    initialize_request_with_capabilities(json!({}))
}

fn initialize_request_with_capabilities(capabilities: serde_json::Value) -> serde_json::Value {
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
            "capabilities": capabilities
        }
    })
}

async fn initialize_session(client: &Client, base_url: &str) -> String {
    initialize_session_with_capabilities(client, base_url, json!({})).await
}

async fn initialize_session_with_capabilities(
    client: &Client,
    base_url: &str,
    capabilities: serde_json::Value,
) -> String {
    let response = client
        .post(format!("{}/mcp", base_url))
        .header(header::ACCEPT, "application/json, text/event-stream")
        .json(&initialize_request_with_capabilities(capabilities))
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
async fn ping_before_initialize_is_allowed() {
    let (base_url, handle) = spawn_server().await;
    let client = Client::new();

    let response = client
        .post(format!("{}/mcp", base_url))
        .header(header::ACCEPT, "application/json, text/event-stream")
        .json(&json!({
            "jsonrpc": "2.0",
            "id": "ping-1",
            "method": "ping"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value = response.json().await.unwrap();
    assert_eq!(body["id"], "ping-1");
    assert_eq!(body["result"], json!({}));

    handle.abort();
}

async fn read_next_sse_json(response: reqwest::Response) -> serde_json::Value {
    use tokio::io::AsyncBufReadExt;

    let mut reader = tokio::io::BufReader::new(tokio_util::io::StreamReader::new(
        response
            .bytes_stream()
            .map(|r| r.map_err(std::io::Error::other)),
    ));
    let mut data = String::new();

    loop {
        let mut line = String::new();
        let n = tokio::time::timeout(Duration::from_secs(5), reader.read_line(&mut line))
            .await
            .expect("timed out reading SSE line")
            .expect("SSE read error");
        assert_ne!(n, 0, "SSE stream closed before a JSON-RPC message");

        let line = line.trim_end_matches(&['\r', '\n'][..]);
        if line.is_empty() {
            if !data.is_empty() {
                return serde_json::from_str(&data).expect("SSE data should be JSON");
            }
            continue;
        }

        if let Some(rest) = line.strip_prefix("data:") {
            data.push_str(rest.trim_start());
        }
    }
}

async fn run_sampling_round_trip(client_sampling_payload: serde_json::Value) -> serde_json::Value {
    let (base_url, handle) = spawn_sampling_server().await;
    let client = Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();
    let session_id =
        initialize_session_with_capabilities(&client, &base_url, json!({ "sampling": {} })).await;

    let sse_response = client
        .get(format!("{}/mcp", base_url))
        .header(header::ACCEPT, "text/event-stream")
        .header("Mcp-Session-Id", &session_id)
        .header("MCP-Protocol-Version", "2025-11-25")
        .send()
        .await
        .unwrap();
    assert_eq!(sse_response.status(), StatusCode::OK);

    let responder_client = client.clone();
    let responder_base_url = base_url.clone();
    let responder_session_id = session_id.clone();
    let responder = tokio::spawn(async move {
        let server_request = read_next_sse_json(sse_response).await;
        assert_eq!(server_request["method"], "sampling/createMessage");
        assert!(
            server_request["id"]
                .as_str()
                .is_some_and(|id| id.starts_with("s-")),
            "server request id should be string-prefixed: {server_request:?}"
        );

        let mut response = serde_json::Map::new();
        response.insert("jsonrpc".to_string(), json!("2.0"));
        response.insert("id".to_string(), server_request["id"].clone());
        if let Some(result) = client_sampling_payload.get("result") {
            response.insert("result".to_string(), result.clone());
        } else if let Some(error) = client_sampling_payload.get("error") {
            response.insert("error".to_string(), error.clone());
        } else {
            panic!("client sampling payload must contain result or error");
        }

        let response = responder_client
            .post(format!("{}/mcp", responder_base_url))
            .header(header::ACCEPT, "application/json, text/event-stream")
            .header("Mcp-Session-Id", &responder_session_id)
            .header("MCP-Protocol-Version", "2025-11-25")
            .json(&serde_json::Value::Object(response))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::ACCEPTED);
    });

    let tool_response = tokio::time::timeout(
        Duration::from_secs(5),
        client
            .post(format!("{}/mcp", base_url))
            .header(header::ACCEPT, "application/json, text/event-stream")
            .header("Mcp-Session-Id", &session_id)
            .header("MCP-Protocol-Version", "2025-11-25")
            .json(&json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/call",
                "params": {
                    "name": "sample_text",
                    "arguments": {}
                }
            }))
            .send(),
    )
    .await
    .expect("tool call should complete without waiting for timeout")
    .unwrap();

    assert_eq!(tool_response.status(), StatusCode::OK);
    let body = tool_response.json().await.unwrap();
    responder.await.unwrap();
    handle.abort();
    body
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
async fn initialized_notification_allows_missing_protocol_header_after_negotiation() {
    let (base_url, handle) = spawn_server().await;
    let client = Client::new();
    let session_id = initialize_session(&client, &base_url).await;

    let response = client
        .post(format!("{}/mcp", base_url))
        .header(header::ACCEPT, "application/json, text/event-stream")
        .header("Mcp-Session-Id", &session_id)
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
async fn post_init_requests_allow_missing_protocol_header_after_negotiation() {
    let (base_url, handle) = spawn_server().await;
    let client = Client::new();
    let session_id = initialize_session(&client, &base_url).await;

    for (id, method) in [(2, "tools/list"), (3, "resources/list")] {
        let response = client
            .post(format!("{}/mcp", base_url))
            .header(header::ACCEPT, "application/json, text/event-stream")
            .header("Mcp-Session-Id", &session_id)
            .json(&json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": method
            }))
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value = response.json().await.unwrap();
        assert!(body.get("result").is_some(), "{method} body: {body}");
    }

    let call_response = client
        .post(format!("{}/mcp", base_url))
        .header(header::ACCEPT, "application/json, text/event-stream")
        .header("Mcp-Session-Id", &session_id)
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/call",
            "params": {
                "name": "missing",
                "arguments": {}
            }
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(call_response.status(), StatusCode::OK);
    let body: serde_json::Value = call_response.json().await.unwrap();
    assert_eq!(body["error"]["code"], -32001);

    handle.abort();
}

#[tokio::test]
async fn unknown_client_jsonrpc_response_post_returns_400() {
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

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(response.text().await.unwrap().is_empty());

    handle.abort();
}

#[tokio::test]
async fn ctx_sample_round_trips_over_streamable_http_sse() {
    let body = run_sampling_round_trip(json!({
        "result": {
            "role": "assistant",
            "content": {
                "type": "text",
                "text": "sampled over sse"
            },
            "model": "fake-model",
            "stopReason": "endTurn"
        }
    }))
    .await;

    assert_eq!(body["result"]["content"][0]["text"], "sampled over sse");
}

#[tokio::test]
async fn ctx_sample_requires_declared_client_sampling_capability() {
    let (base_url, handle) = spawn_sampling_server().await;
    let client = Client::new();
    let session_id = initialize_session(&client, &base_url).await;

    let response = client
        .post(format!("{}/mcp", base_url))
        .header(header::ACCEPT, "application/json, text/event-stream")
        .header("Mcp-Session-Id", &session_id)
        .header("MCP-Protocol-Version", "2025-11-25")
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "sample_text",
                "arguments": {}
            }
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value = response.json().await.unwrap();
    assert!(
        body["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("sampling capability"))
    );

    handle.abort();
}

#[tokio::test]
async fn rejected_sampling_response_returns_before_timeout() {
    let started = std::time::Instant::now();
    let body = run_sampling_round_trip(json!({
        "error": {
            "code": -1,
            "message": "User rejected sampling"
        }
    }))
    .await;

    assert!(
        started.elapsed() < Duration::from_secs(5),
        "rejected sampling should resolve promptly"
    );
    assert_eq!(body["error"]["code"], -1);
    assert_eq!(body["error"]["message"], "User rejected sampling");
}

async fn read_first_sse_record(client: &Client, base_url: &str, session_id: &str) -> String {
    use tokio::io::AsyncReadExt;

    let response = client
        .get(format!("{}/mcp", base_url))
        .header(header::ACCEPT, "text/event-stream")
        .header("Mcp-Session-Id", session_id)
        .header("MCP-Protocol-Version", "2025-11-25")
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    // Read just enough bytes to see the first SSE record.
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

    let collected = collected.replace("\r\n", "\n");
    collected.split("\n\n").next().unwrap_or("").to_string()
}

fn assert_startup_record_is_comment(record: &str) {
    assert!(
        record.lines().any(|line| line.starts_with(':')),
        "startup SSE record should be a comment, got: {record:?}"
    );
    assert!(
        record.lines().all(|line| line.starts_with(':')),
        "startup SSE record should contain only comment lines, got: {record:?}"
    );
    assert!(
        !record.lines().any(|line| line.starts_with("data:")),
        "startup SSE record must not dispatch an empty data event, got: {record:?}"
    );
    assert!(
        !record.lines().any(|line| line.starts_with("id:")),
        "startup SSE record must not advance Last-Event-ID before a JSON-RPC message, got: {record:?}"
    );
    assert!(
        !record.lines().any(|line| line.starts_with("retry:")),
        "startup SSE record must not rely on a synthetic retry event, got: {record:?}"
    );
}

#[tokio::test]
async fn sse_starts_with_comment_not_empty_data_event() {
    let (base_url, handle) = spawn_server().await;
    let client = Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let session_id = initialize_session(&client, &base_url).await;

    let first_event = read_first_sse_record(&client, &base_url, &session_id).await;
    assert_startup_record_is_comment(&first_event);

    handle.abort();
}

// Streamable HTTP startup traffic must not synthesize an SSE data event. Some
// RMCP/Codex client versions try to parse `data:\n\n` as JSON-RPC during
// startup and close the worker before sending `notifications/initialized`.
#[tokio::test]
async fn concurrent_sse_streams_start_with_comments_not_synthetic_events() {
    let (base_url, handle) = spawn_server().await;
    let client = Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let session_id = initialize_session(&client, &base_url).await;

    let first_event = read_first_sse_record(&client, &base_url, &session_id).await;
    let second_event = read_first_sse_record(&client, &base_url, &session_id).await;

    assert_startup_record_is_comment(&first_event);
    assert_startup_record_is_comment(&second_event);

    handle.abort();
}

#[tokio::test]
async fn server_initiated_sse_messages_have_resumable_event_ids() {
    let (base_url, handle) = spawn_sampling_server().await;
    let client = Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();
    let session_id =
        initialize_session_with_capabilities(&client, &base_url, json!({ "sampling": {} })).await;

    let sse_response = client
        .get(format!("{}/mcp", base_url))
        .header(header::ACCEPT, "text/event-stream")
        .header("Mcp-Session-Id", &session_id)
        .header("MCP-Protocol-Version", "2025-11-25")
        .send()
        .await
        .unwrap();
    assert_eq!(sse_response.status(), StatusCode::OK);

    let responder_client = client.clone();
    let responder_base_url = base_url.clone();
    let responder_session_id = session_id.clone();
    let responder = tokio::spawn(async move {
        use tokio::io::AsyncBufReadExt;

        let mut reader = tokio::io::BufReader::new(tokio_util::io::StreamReader::new(
            sse_response
                .bytes_stream()
                .map(|r| r.map_err(std::io::Error::other)),
        ));
        let mut event_id = String::new();
        let mut data = String::new();

        loop {
            let mut line = String::new();
            let n = tokio::time::timeout(Duration::from_secs(5), reader.read_line(&mut line))
                .await
                .expect("timed out reading SSE line")
                .expect("SSE read error");
            assert_ne!(n, 0, "SSE stream closed before a JSON-RPC message");

            let line = line.trim_end_matches(&['\r', '\n'][..]);
            if line.is_empty() {
                if !data.is_empty() {
                    break;
                }
                continue;
            }

            if let Some(rest) = line.strip_prefix("id:") {
                event_id = rest.trim().to_string();
            } else if let Some(rest) = line.strip_prefix("data:") {
                data.push_str(rest.trim_start());
            }
        }

        let server_request: serde_json::Value =
            serde_json::from_str(&data).expect("SSE data should be JSON");
        assert_eq!(server_request["method"], "sampling/createMessage");
        assert!(
            event_id.starts_with(&format!("{}-", responder_session_id)),
            "server-initiated event id should be scoped to the session, got: {event_id}"
        );

        let response = responder_client
            .post(format!("{}/mcp", responder_base_url))
            .header(header::ACCEPT, "application/json, text/event-stream")
            .header("Mcp-Session-Id", &responder_session_id)
            .header("MCP-Protocol-Version", "2025-11-25")
            .json(&json!({
                "jsonrpc": "2.0",
                "id": server_request["id"],
                "result": {
                    "role": "assistant",
                    "content": {
                        "type": "text",
                        "text": "sampled"
                    },
                    "model": "test-model"
                }
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        event_id
    });

    let tool_response = client
        .post(format!("{}/mcp", base_url))
        .header(header::ACCEPT, "application/json, text/event-stream")
        .header("Mcp-Session-Id", &session_id)
        .header("MCP-Protocol-Version", "2025-11-25")
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "sample_text",
                "arguments": {}
            }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(tool_response.status(), StatusCode::OK);

    let event_id = responder.await.unwrap();
    assert!(
        event_id.ends_with("-0"),
        "first JSON-RPC message on a stream should use sequence 0, got: {event_id}"
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
