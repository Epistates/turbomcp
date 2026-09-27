//! What a `tools/list` entry commits the server to, held against its
//! `tools/call` answers: the declared `outputSchema` (clients SHOULD validate
//! structured results against it), and where `x-mcp-header` rules apply.
//!
//! The servers here are scripted, because a real TurboMCP server validates its
//! own output and would never send what these tests need to see refused.

use serde_json::{Map, Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, split};
use turbomcp_client::{Client, ClientBuilder, ClientError, ConnectMode};
use turbomcp_codec::DefaultCodec;
use turbomcp_transport_stdio::LineTransport;

/// Answer each request with `respond(method, frame)`'s `{"result": …}` body;
/// notifications are consumed silently.
async fn connect<F>(mode: ConnectMode, mut respond: F) -> Client
where
    F: FnMut(&str, &Value) -> Value + Send + 'static,
{
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    tokio::spawn(async move {
        let (rd, mut wr) = split(server_io);
        let mut lines = BufReader::new(rd).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let frame: Value = serde_json::from_str(&line).expect("client sends valid json");
            let (Some(method), Some(id)) = (
                frame.get("method").and_then(Value::as_str),
                frame.get("id").cloned(),
            ) else {
                continue;
            };
            let result = match method {
                "server/discover" => json!({
                    "capabilities": { "tools": {} },
                    "supportedVersions": ["2026-07-28"],
                    "resultType": "complete", "cacheScope": "private", "ttlMs": 0
                }),
                "initialize" => json!({
                    "protocolVersion": "2025-11-25",
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": "scripted", "version": "1.0" }
                }),
                other => respond(other, &frame),
            };
            let reply = json!({ "jsonrpc": "2.0", "id": id, "result": result });
            wr.write_all(format!("{reply}\n").as_bytes()).await.unwrap();
        }
    });
    let (rd, wr) = split(client_io);
    ClientBuilder::new("contract", "1.0.0")
        .with_connect_mode(mode)
        .with_response_cache(false)
        .connect(LineTransport::new(
            BufReader::new(rd),
            wr,
            DefaultCodec::default(),
        ))
        .await
        .expect("handshake")
}

fn tools_list(tool: Value) -> Value {
    json!({ "tools": [tool], "resultType": "complete", "cacheScope": "private", "ttlMs": 0 })
}

fn stats_tool() -> Value {
    json!({
        "name": "stats",
        "inputSchema": { "type": "object" },
        "outputSchema": {
            "type": "object",
            "properties": { "n": { "type": "integer" } },
            "required": ["n"]
        }
    })
}

/// A server whose `stats` tool answers `structured` (or no structured content
/// at all), optionally flagged `isError`.
fn stats_server(structured: Option<Value>, is_error: bool) -> impl FnMut(&str, &Value) -> Value {
    move |method, _| match method {
        "tools/list" => tools_list(stats_tool()),
        "tools/call" => {
            let mut result = json!({
                "content": [{ "type": "text", "text": "stats" }],
                "isError": is_error,
                "resultType": "complete"
            });
            if let Some(structured) = &structured {
                result["structuredContent"] = structured.clone();
            }
            result
        }
        other => panic!("unexpected method {other}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn structured_content_matching_the_output_schema_passes() {
    for mode in [ConnectMode::Modern, ConnectMode::Legacy] {
        let client = connect(mode, stats_server(Some(json!({ "n": 3 })), false)).await;
        client.list_tools(None).await.expect("list");
        let result = client.call_tool("stats", Map::new()).await.expect("valid");
        assert_eq!(result.structured_content, Some(json!({ "n": 3 })));
    }
}

/// "Clients SHOULD validate structured results against this schema": a
/// result that breaks it is refused instead of handed on as if it held.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn structured_content_violating_the_output_schema_is_refused() {
    for mode in [ConnectMode::Modern, ConnectMode::Legacy] {
        let client = connect(mode, stats_server(Some(json!({ "n": "three" })), false)).await;
        client.list_tools(None).await.expect("list");
        let err = client
            .call_tool("stats", Map::new())
            .await
            .expect_err("violates the schema");
        assert!(
            matches!(&err, ClientError::OutputSchema { tool, .. } if tool == "stats"),
            "{err}"
        );
    }
}

/// "Servers MUST provide structured results that conform to this schema", so
/// a successful result with none at all breaks the contract too.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_missing_structured_result_is_refused() {
    let client = connect(ConnectMode::Modern, stats_server(None, false)).await;
    client.list_tools(None).await.expect("list");
    let err = client
        .call_tool("stats", Map::new())
        .await
        .expect_err("schema promised structuredContent");
    assert!(matches!(err, ClientError::OutputSchema { .. }), "{err}");
}

/// A tool-level error carries no structured result, and that's not a
/// violation: the model reads the error text and self-corrects.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_tool_error_is_not_held_to_the_output_schema() {
    let client = connect(ConnectMode::Modern, stats_server(None, true)).await;
    client.list_tools(None).await.expect("list");
    let result = client
        .call_tool("stats", Map::new())
        .await
        .expect("isError");
    assert!(result.is_error);
}

/// `x-mcp-header` exists for Streamable HTTP: "Clients using other
/// transports (e.g., stdio) MAY ignore `x-mcp-header` annotations entirely."
/// Rejecting over stdio dropped tools nobody could have mirrored anyway.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn header_annotations_are_ignored_off_http() {
    let tool = json!({
        "name": "locate",
        "inputSchema": {
            "type": "object",
            // A `number` is not a permitted header type; on HTTP this tool
            // would be excluded.
            "properties": { "lat": { "type": "number", "x-mcp-header": "Lat" } }
        }
    });
    for mode in [ConnectMode::Modern, ConnectMode::Legacy] {
        let listed = tool.clone();
        let client = connect(mode, move |method, _| match method {
            "tools/list" => tools_list(listed.clone()),
            other => panic!("unexpected method {other}"),
        })
        .await;
        let tools = client.list_tools(None).await.expect("list");
        assert_eq!(tools.tools.len(), 1, "kept over stdio");
    }
}
