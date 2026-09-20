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

/// The two long-running tool shapes: one that parks until cancelled, one that
/// reports progress while it works.
#[derive(Clone)]
struct CancellableHandler;

impl McpHandler for CancellableHandler {
    fn server_info(&self) -> ServerInfo {
        ServerInfo::new("cancellable-http-test", "1.0.0")
    }

    fn list_tools(&self) -> Vec<Tool> {
        vec![
            Tool::new("park", "Wait until cancelled"),
            Tool::new("report", "Report progress while working"),
        ]
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
        match name {
            "park" => {
                for _ in 0..30 {
                    if ctx.is_cancelled() {
                        return Ok(ToolResult::text("cancelled"));
                    }
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
                Ok(ToolResult::text("completed"))
            }
            "report" => {
                ctx.report_progress(1.0, Some(2.0), Some("halfway")).await?;
                Ok(ToolResult::text("done"))
            }
            _ => Err(McpError::tool_not_found(name)),
        }
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

async fn spawn_cancellable_server() -> (String, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);

    let addr_string = addr.to_string();
    let handle = tokio::spawn(async move {
        http::run(&CancellableHandler, &addr_string).await.unwrap();
    });

    tokio::time::sleep(Duration::from_millis(200)).await;
    (format!("http://{}", addr), handle)
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

type SseBytes = futures::stream::BoxStream<'static, std::io::Result<bytes::Bytes>>;
type SseReader = tokio::io::BufReader<tokio_util::io::StreamReader<SseBytes, bytes::Bytes>>;

/// Pull JSON-RPC payloads off an SSE stream one at a time.
///
/// A POST-initiated stream carries several records (primer, any server-initiated
/// traffic, then the response), so reading has to be resumable rather than
/// one-shot.
fn sse_reader(response: reqwest::Response) -> SseReader {
    tokio::io::BufReader::new(tokio_util::io::StreamReader::new(
        response
            .bytes_stream()
            .map(|r| r.map_err(std::io::Error::other))
            .boxed(),
    ))
}

struct SseRecord {
    id: Option<String>,
    data: String,
}

/// Read one SSE record, skipping comment-only frames such as `: connected`.
async fn next_sse_record(reader: &mut SseReader) -> SseRecord {
    use tokio::io::AsyncBufReadExt;

    let mut record = SseRecord {
        id: None,
        data: String::new(),
    };

    loop {
        let mut line = String::new();
        let n = tokio::time::timeout(Duration::from_secs(5), reader.read_line(&mut line))
            .await
            .expect("timed out reading SSE line")
            .expect("SSE read error");
        assert_ne!(n, 0, "SSE stream closed before the record ended");

        let line = line.trim_end_matches(&['\r', '\n'][..]);
        if line.is_empty() {
            if record.id.is_some() || !record.data.is_empty() {
                return record;
            }
            continue;
        }

        if let Some(rest) = line.strip_prefix("id:") {
            record.id = Some(rest.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("data:") {
            record.data.push_str(rest.trim_start());
        }
    }
}

async fn next_sse_json(reader: &mut SseReader) -> serde_json::Value {
    loop {
        let record = next_sse_record(reader).await;
        if !record.data.is_empty() {
            return serde_json::from_str(&record.data).expect("SSE data should be JSON");
        }
    }
}

// MCP 2025-11-25 §Resumability: SSE event IDs MUST be globally unique within a
// session. The GET listening stream opens with a `: connected` comment (for
// older RMCP/Codex clients) followed by a primer event carrying an event ID and
// an empty data field, giving the client an immediate `Last-Event-ID` anchor.
// The primer is the stream's seq-0 cursor; real messages must start at seq 1 so
// they can never reuse the primer's ID on replay.
#[tokio::test]
async fn get_sse_stream_primes_client_with_unique_event_id() {
    use tokio::io::AsyncBufReadExt;

    let (base_url, _handle) = spawn_server().await;
    let client = Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();
    let session_id = initialize_session(&client, &base_url).await;

    let sse_response = client
        .get(format!("{}/mcp", base_url))
        .header(header::ACCEPT, "text/event-stream")
        .header("Mcp-Session-Id", &session_id)
        .send()
        .await
        .expect("GET SSE stream");
    assert_eq!(sse_response.status(), StatusCode::OK);

    let mut reader = tokio::io::BufReader::new(tokio_util::io::StreamReader::new(
        sse_response
            .bytes_stream()
            .map(|r| r.map_err(std::io::Error::other)),
    ));

    // The first two frames are deterministic: the `: connected` comment, then
    // the primer event (`id:` + empty `data:`). Read raw lines so we can inspect
    // the comment and the event ID, which `read_next_sse_json` would discard.
    let mut saw_comment = false;
    let mut primer_id: Option<String> = None;
    let mut empty_data = false;
    for _ in 0..6 {
        let mut line = String::new();
        let n = tokio::time::timeout(Duration::from_secs(5), reader.read_line(&mut line))
            .await
            .expect("timed out reading SSE line")
            .expect("SSE read error");
        assert_ne!(n, 0, "stream closed before the primer event");
        let line = line.trim_end_matches(&['\r', '\n'][..]);
        if line == ": connected" {
            saw_comment = true;
        } else if let Some(id) = line.strip_prefix("id: ") {
            primer_id = Some(id.to_string());
        } else if line == "data:" {
            empty_data = true;
        }
        if saw_comment && primer_id.is_some() && empty_data {
            break;
        }
    }

    assert!(
        saw_comment,
        "primer event must be preceded by the `: connected` comment"
    );
    let primer_id = primer_id.expect("primer event must carry an `id:` field");
    assert!(empty_data, "primer event must have an empty data field");
    assert!(
        primer_id.ends_with("-0"),
        "primer must be the stream's seq-0 cursor so message IDs (seq 1+) never collide, got {primer_id}"
    );
}

/// Drive a full sampling round trip **without ever issuing a GET**.
///
/// §Sending Messages item 6 puts request-related server messages on the stream
/// the POST itself opened, and §Listening for Messages item 4 reserves the
/// standalone GET stream for messages unrelated to a running request. Before
/// 3.5.0 this server always answered a POST with `application/json` and pushed
/// `ctx.sample()` onto the GET stream, so a client that only POSTs — which the
/// spec permits, since the GET is a MAY — got `-32603 No active SSE stream`
/// from every sample. Opening no GET here is the point of the test.
async fn run_sampling_round_trip(client_sampling_payload: serde_json::Value) -> serde_json::Value {
    let (base_url, handle) = spawn_sampling_server().await;
    let client = Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();
    let session_id =
        initialize_session_with_capabilities(&client, &base_url, json!({ "sampling": {} })).await;

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
    .expect("the POST should answer with headers promptly, not block on the handler")
    .unwrap();

    assert_eq!(tool_response.status(), StatusCode::OK);
    assert_eq!(
        tool_response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|v| v.split(';').next().unwrap_or("").trim().to_string()),
        Some("text/event-stream".to_string()),
        "a handler that talks back must upgrade the POST to a stream"
    );

    let mut reader = sse_reader(tool_response);
    let server_request = next_sse_json(&mut reader).await;
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

    let ack = client
        .post(format!("{}/mcp", base_url))
        .header(header::ACCEPT, "application/json, text/event-stream")
        .header("Mcp-Session-Id", &session_id)
        .header("MCP-Protocol-Version", "2025-11-25")
        .json(&serde_json::Value::Object(response))
        .send()
        .await
        .unwrap();
    assert_eq!(ack.status(), StatusCode::ACCEPTED);

    // §Sending Messages item 6: "The SSE stream SHOULD eventually include a
    // JSON-RPC response for the JSON-RPC request sent in the POST body."
    let body = next_sse_json(&mut reader).await;
    assert_eq!(body["id"], 2, "the stream must conclude with our response");
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
    // The point here is that the request was *routed* despite the missing
    // header; the tool simply does not exist. tools.mdx assigns -32602 to an
    // unknown tool name.
    assert_eq!(body["error"]["code"], -32602);

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

    let mut reader = sse_reader(tool_response);

    // §Sending Messages item 6: the stream opens with an event ID and an empty
    // data field, priming the client with a `Last-Event-ID` anchor.
    let primer = next_sse_record(&mut reader).await;
    let primer_id = primer.id.expect("primer must carry an event ID");
    assert!(primer.data.is_empty(), "primer carries no data");
    assert!(
        primer_id.starts_with(&format!("{}-", session_id)),
        "event ids are scoped to the session, got: {primer_id}"
    );
    assert!(primer_id.ends_with("-0"), "the primer is the seq-0 cursor");

    let message = next_sse_record(&mut reader).await;
    let event_id = message.id.expect("message must carry an event ID");
    let server_request: serde_json::Value =
        serde_json::from_str(&message.data).expect("SSE data should be JSON");
    assert_eq!(server_request["method"], "sampling/createMessage");
    assert!(
        event_id.ends_with("-1"),
        "the stream's primer event holds sequence 0, so the first JSON-RPC \
         message must use sequence 1 to keep event IDs unique, got: {event_id}"
    );
    assert_eq!(
        primer_id.rsplit_once('-').map(|(head, _)| head),
        event_id.rsplit_once('-').map(|(head, _)| head),
        "both events belong to the same stream, so the ids share a prefix"
    );

    let ack = client
        .post(format!("{}/mcp", base_url))
        .header(header::ACCEPT, "application/json, text/event-stream")
        .header("Mcp-Session-Id", &session_id)
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
    assert_eq!(ack.status(), StatusCode::ACCEPTED);

    let final_record = next_sse_record(&mut reader).await;
    assert_eq!(
        final_record
            .id
            .as_deref()
            .and_then(|id| id.rsplit_once('-')),
        Some((event_id.rsplit_once('-').unwrap().0, "2")),
        "the response continues the same stream's cursor"
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

/// Reusing a request id within a session is served, not refused — the exact
/// shape reported in #25, where a client recycling ids was locked out of an
/// otherwise working server after its first few calls.
///
/// The spec's "MUST NOT have been previously used" binds the **requestor**; a
/// receiver's only obligation is to echo the id back, so there is nothing for
/// the server to gain by policing it. The old per-session "every id ever seen"
/// set also grew without bound for the life of the session.
#[tokio::test]
async fn a_reused_request_id_is_served_rather_than_refused() {
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
    assert!(
        duplicate_body.get("error").is_none(),
        "a reused id must not be refused, got: {duplicate_body}"
    );
    assert_eq!(duplicate_body["id"], 7, "the id is echoed back verbatim");
    assert_eq!(
        duplicate_body["result"], first_body["result"],
        "the repeat is served exactly as the first was"
    );

    // And it keeps working — the failure in #25 showed up as a session that
    // died after a handful of calls, so one repeat is not enough to prove it.
    for _ in 0..3 {
        let again = client
            .post(format!("{}/mcp", base_url))
            .header(header::ACCEPT, "application/json, text/event-stream")
            .header("Mcp-Session-Id", &session_id)
            .header("MCP-Protocol-Version", "2025-11-25")
            .json(&request)
            .send()
            .await
            .unwrap();
        let body: serde_json::Value = again.json().await.unwrap();
        assert!(body.get("result").is_some(), "got: {body}");
    }

    handle.abort();
}

/// MCP §Cancellation: a receiver SHOULD stop processing a cancelled request.
///
/// The other three transports each kept an in-flight registry; HTTP had none,
/// so `ctx.is_cancelled()` was permanently false on the transport where long
/// tool calls are most common. A client that abandoned a ten-minute call still
/// paid for it server-side, and the 202 made the cancel look accepted.
#[tokio::test]
async fn a_posted_cancellation_signals_the_running_handler() {
    let (base_url, handle) = spawn_cancellable_server().await;
    let client = Client::new();
    let session_id = initialize_session(&client, &base_url).await;

    let call = tokio::spawn({
        let client = client.clone();
        let base_url = base_url.clone();
        let session_id = session_id.clone();
        async move {
            client
                .post(format!("{}/mcp", base_url))
                .header(header::ACCEPT, "application/json, text/event-stream")
                .header("Mcp-Session-Id", &session_id)
                .header("MCP-Protocol-Version", "2025-11-25")
                .json(&json!({
                    "jsonrpc": "2.0",
                    "id": 42,
                    "method": "tools/call",
                    "params": { "name": "park", "arguments": {} }
                }))
                .send()
                .await
                .unwrap()
        }
    });

    // Let the handler get into its wait before cancelling it.
    tokio::time::sleep(Duration::from_millis(150)).await;

    // The id goes out as a string here while the request carried a number:
    // JSON-RPC does not constrain which, so both must land in the same slot.
    let cancel = client
        .post(format!("{}/mcp", base_url))
        .header(header::ACCEPT, "application/json, text/event-stream")
        .header("Mcp-Session-Id", &session_id)
        .header("MCP-Protocol-Version", "2025-11-25")
        .json(&json!({
            "jsonrpc": "2.0",
            "method": "notifications/cancelled",
            "params": { "requestId": "42", "reason": "user abandoned it" }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(cancel.status(), StatusCode::ACCEPTED);

    let body: serde_json::Value = tokio::time::timeout(Duration::from_secs(5), call)
        .await
        .expect("the cancelled call should return promptly, not run to its timeout")
        .unwrap()
        .json()
        .await
        .unwrap();

    assert_eq!(
        body["result"]["content"][0]["text"], "cancelled",
        "the handler must observe ctx.is_cancelled(), got: {body}"
    );

    handle.abort();
}

/// §Resumability: "the server MAY use this header to replay messages that would
/// have been sent after the last event ID, *on the stream that was
/// disconnected*, and to resume the stream from that point."
///
/// Before 3.5.0 `Last-Event-ID` was never read, so a reconnect got a brand-new
/// empty stream and the client never learned it had missed anything — while the
/// event IDs the server emits advertised resumability the whole time.
#[tokio::test]
async fn a_dropped_stream_resumes_from_last_event_id() {
    let (base_url, handle) = spawn_cancellable_server().await;
    let client = Client::new();
    let session_id = initialize_session(&client, &base_url).await;

    // A POST stream is the easy one to disconnect mid-flight: the handler emits
    // progress, then the response. Drop after the progress event.
    let response = client
        .post(format!("{}/mcp", base_url))
        .header(header::ACCEPT, "application/json, text/event-stream")
        .header("Mcp-Session-Id", &session_id)
        .header("MCP-Protocol-Version", "2025-11-25")
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "tools/call",
            "params": {
                "name": "report",
                "arguments": {},
                "_meta": { "progressToken": "tok-2" }
            }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let mut reader = sse_reader(response);
    let primer = next_sse_record(&mut reader).await;
    let primer_id = primer.id.expect("primer carries an event ID");
    let progress = next_sse_record(&mut reader).await;
    let progress_id = progress.id.expect("progress carries an event ID");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&progress.data).unwrap()["method"],
        "notifications/progress"
    );

    // Hang up without reading the response, then let the handler finish.
    drop(reader);
    tokio::time::sleep(Duration::from_millis(200)).await;

    // §Resumability: "Resumption is always via HTTP GET with Last-Event-ID",
    // whichever way the stream was originally opened.
    let resumed = client
        .get(format!("{}/mcp", base_url))
        .header(header::ACCEPT, "text/event-stream")
        .header("Mcp-Session-Id", &session_id)
        .header("MCP-Protocol-Version", "2025-11-25")
        .header("Last-Event-ID", &progress_id)
        .send()
        .await
        .unwrap();
    assert_eq!(resumed.status(), StatusCode::OK);

    let mut resumed_reader = sse_reader(resumed);
    let replayed = next_sse_record(&mut resumed_reader).await;
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&replayed.data).unwrap()["id"],
        5,
        "the response the client missed must be redelivered"
    );
    assert_eq!(
        replayed.id.as_deref(),
        Some(format!("{}-2", progress_id.rsplit_once('-').unwrap().0).as_str()),
        "a replayed event keeps its original id, and continues this stream's cursor"
    );
    assert_ne!(
        replayed.id.as_deref(),
        Some(primer_id.as_str()),
        "already-received events are not replayed"
    );

    handle.abort();
}

/// A `Last-Event-ID` naming another session's stream replays nothing.
///
/// §Resumability: "The server MUST NOT replay messages that would have been
/// delivered on a different stream." Falling through to a fresh stream, rather
/// than erroring, is also what a client that sent no header would have got.
#[tokio::test]
async fn a_last_event_id_from_another_session_replays_nothing() {
    let (base_url, handle) = spawn_cancellable_server().await;
    let client = Client::new();
    let victim = initialize_session(&client, &base_url).await;
    let attacker = initialize_session(&client, &base_url).await;

    let response = client
        .post(format!("{}/mcp", base_url))
        .header(header::ACCEPT, "application/json, text/event-stream")
        .header("Mcp-Session-Id", &victim)
        .header("MCP-Protocol-Version", "2025-11-25")
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 6,
            "method": "tools/call",
            "params": {
                "name": "report",
                "arguments": {},
                "_meta": { "progressToken": "tok-3" }
            }
        }))
        .send()
        .await
        .unwrap();

    let mut reader = sse_reader(response);
    next_sse_record(&mut reader).await; // primer
    let progress_id = next_sse_record(&mut reader)
        .await
        .id
        .expect("progress carries an event ID");
    drop(reader);
    tokio::time::sleep(Duration::from_millis(200)).await;

    let resumed = client
        .get(format!("{}/mcp", base_url))
        .header(header::ACCEPT, "text/event-stream")
        .header("Mcp-Session-Id", &attacker)
        .header("MCP-Protocol-Version", "2025-11-25")
        .header("Last-Event-ID", &progress_id)
        .send()
        .await
        .unwrap();
    assert_eq!(resumed.status(), StatusCode::OK);

    // A fresh stream: the first record is this session's own primer, not any of
    // the victim's traffic.
    let mut resumed_reader = sse_reader(resumed);
    let first = next_sse_record(&mut resumed_reader).await;
    assert!(
        first.data.is_empty(),
        "a cross-session Last-Event-ID must open a fresh stream, got: {}",
        first.data
    );
    assert!(
        first
            .id
            .as_deref()
            .is_some_and(|id| id.starts_with(&format!("{attacker}-")) && id.ends_with("-0")),
        "the fresh stream is primed under the requesting session, got: {:?}",
        first.id
    );

    handle.abort();
}

/// The upgrade to SSE is lazy: a request whose handler says nothing back keeps
/// the single-JSON-object form. §Sending Messages item 5 permits either, and
/// making every POST a stream would be a needless change for every client.
#[tokio::test]
async fn a_request_that_needs_no_stream_still_answers_with_json() {
    let (base_url, _handle) = spawn_cancellable_server().await;
    let client = Client::new();
    let session_id = initialize_session(&client, &base_url).await;

    let response = client
        .post(format!("{}/mcp", base_url))
        .header(header::ACCEPT, "application/json, text/event-stream")
        .header("Mcp-Session-Id", &session_id)
        .header("MCP-Protocol-Version", "2025-11-25")
        .json(&json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/list" }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|v| v.split(';').next().unwrap_or("").trim().to_string()),
        Some("application/json".to_string())
    );
    let body: serde_json::Value = response.json().await.unwrap();
    assert_eq!(body["id"], 3);
}

/// Progress is request-related traffic, so it belongs on the POST's own stream.
///
/// It used to go to the standalone GET stream, which means `report_progress`
/// returned `-32603 No active SSE stream` on a client that never issued a GET —
/// failing the very request it was describing.
#[tokio::test]
async fn progress_rides_the_stream_opened_by_its_own_request() {
    let (base_url, handle) = spawn_cancellable_server().await;
    let client = Client::new();
    let session_id = initialize_session(&client, &base_url).await;

    let response = client
        .post(format!("{}/mcp", base_url))
        .header(header::ACCEPT, "application/json, text/event-stream")
        .header("Mcp-Session-Id", &session_id)
        .header("MCP-Protocol-Version", "2025-11-25")
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/call",
            "params": {
                "name": "report",
                "arguments": {},
                "_meta": { "progressToken": "tok-1" }
            }
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let mut reader = sse_reader(response);

    let progress = next_sse_json(&mut reader).await;
    assert_eq!(progress["method"], "notifications/progress");
    assert_eq!(progress["params"]["progressToken"], "tok-1");
    assert_eq!(progress["params"]["progress"], 1.0);

    // Ordering is the point: a progress report that lands after the response it
    // describes is worse than none.
    let result = next_sse_json(&mut reader).await;
    assert_eq!(result["id"], 4);
    assert_eq!(result["result"]["content"][0]["text"], "done");

    handle.abort();
}

/// A cancellation is scoped to the session that sent it. The registry is
/// per-session precisely so one client cannot cancel another's request by
/// guessing a JSON-RPC id — ids are only unique within a session.
#[tokio::test]
async fn a_cancellation_cannot_reach_another_session() {
    let (base_url, handle) = spawn_cancellable_server().await;
    let client = Client::new();
    let victim = initialize_session(&client, &base_url).await;
    let attacker = initialize_session(&client, &base_url).await;
    assert_ne!(victim, attacker);

    let call = tokio::spawn({
        let client = client.clone();
        let base_url = base_url.clone();
        async move {
            client
                .post(format!("{}/mcp", base_url))
                .header(header::ACCEPT, "application/json, text/event-stream")
                .header("Mcp-Session-Id", &victim)
                .header("MCP-Protocol-Version", "2025-11-25")
                .json(&json!({
                    "jsonrpc": "2.0",
                    "id": 42,
                    "method": "tools/call",
                    "params": { "name": "park", "arguments": {} }
                }))
                .send()
                .await
                .unwrap()
        }
    });

    tokio::time::sleep(Duration::from_millis(150)).await;

    let cancel = client
        .post(format!("{}/mcp", base_url))
        .header(header::ACCEPT, "application/json, text/event-stream")
        .header("Mcp-Session-Id", &attacker)
        .header("MCP-Protocol-Version", "2025-11-25")
        .json(&json!({
            "jsonrpc": "2.0",
            "method": "notifications/cancelled",
            "params": { "requestId": 42 }
        }))
        .send()
        .await
        .unwrap();
    // Still 202: an id the sender never used is a no-op, not an error.
    assert_eq!(cancel.status(), StatusCode::ACCEPTED);

    let body: serde_json::Value = tokio::time::timeout(Duration::from_secs(5), call)
        .await
        .expect("the victim's call should finish on its own")
        .unwrap()
        .json()
        .await
        .unwrap();

    assert_eq!(
        body["result"]["content"][0]["text"], "completed",
        "another session's cancel must not touch this request, got: {body}"
    );

    handle.abort();
}
