//! Phase 2 exit criterion: a hand-written `McpServerCore + WithTools` server
//! answers `server/discover` + `tools/list` + `tools/call` driven entirely
//! through the real stack — `LineTransport` framing, the `serve` loop, and the
//! `VersionDispatcher` — over an in-memory duplex pipe (a stand-in for the
//! stdin/stdout byte streams, exercising identical framing code).

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use turbomcp_core::{Implementation, McpResult};
use turbomcp_protocol::neutral;
use turbomcp_server::{
    CallToolContext, ListToolsContext, McpServerCore, MethodRouter, VersionDispatcher, WithTools,
};
use turbomcp_transport_stdio::{LineTransport, StdioError};

#[derive(Clone)]
struct Calculator;

impl McpServerCore for Calculator {
    fn server_info(&self) -> Implementation {
        Implementation::new("calculator", "0.1.0")
    }
}

impl WithTools for Calculator {
    async fn list_tools(
        &self,
        _ctx: &ListToolsContext,
        _params: neutral::ListParams,
    ) -> McpResult<neutral::ListToolsResult> {
        Ok(neutral::ListToolsResult::new(vec![neutral::Tool::new(
            "add",
            json!({"type": "object", "properties": {"a": {"type": "number"}, "b": {"type": "number"}}}),
        )]))
    }

    async fn call_tool(
        &self,
        _ctx: &CallToolContext,
        params: neutral::CallToolParams,
    ) -> McpResult<neutral::CallToolResult> {
        let a = params
            .arguments
            .get("a")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        let b = params
            .arguments
            .get("b")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        Ok(neutral::CallToolResult::text((a + b).to_string()))
    }
}

const DRAFT_META: &str = r#"{"io.modelcontextprotocol/protocolVersion":"2026-07-28"}"#;

#[tokio::test]
async fn server_handles_discover_list_and_call_over_stdio_framing() {
    let dispatcher = VersionDispatcher::new(Calculator, MethodRouter::new().with_tools());

    // An in-memory bidirectional pipe stands in for stdin/stdout. The server's
    // half is split into read/write and wrapped in the SAME LineTransport that
    // `stdio()` uses — only the byte source differs.
    let (client, server) = tokio::io::duplex(64 * 1024);
    let (server_rx, server_tx) = tokio::io::split(server);
    let transport = LineTransport::new(
        BufReader::new(server_rx),
        server_tx,
        turbomcp_codec::SerdeJsonCodec,
    );

    let server_task = tokio::spawn(turbomcp_service::serve(transport, dispatcher));

    let (client_rx, mut client_tx) = tokio::io::split(client);
    let mut client_rx = BufReader::new(client_rx);

    // Three requests, newline-framed.
    let requests = [
        r#"{"jsonrpc":"2.0","id":1,"method":"server/discover"}"#.to_string(),
        format!(
            r#"{{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{{"_meta":{DRAFT_META}}}}}"#
        ),
        format!(
            r#"{{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{{"name":"add","arguments":{{"a":2,"b":40}},"_meta":{DRAFT_META}}}}}"#
        ),
    ];
    for req in &requests {
        client_tx.write_all(req.as_bytes()).await.unwrap();
        client_tx.write_all(b"\n").await.unwrap();
    }
    client_tx.flush().await.unwrap();
    // Shut down the write direction so the server reads EOF after the three
    // frames and the serve loop ends. (Dropping a split `WriteHalf` alone won't
    // close the duplex — the `ReadHalf` keeps it open — so shut down explicitly.)
    client_tx.shutdown().await.unwrap();

    // Read three response lines (whole test bounded so a regression can't hang CI).
    let mut responses = Vec::new();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        for _ in 0..3 {
            let mut line = String::new();
            let n = client_rx.read_line(&mut line).await.unwrap();
            assert!(n > 0, "expected a response line");
            responses.push(serde_json::from_str::<Value>(line.trim()).unwrap());
        }
    })
    .await
    .expect("responses should arrive within timeout");

    let serve_result = server_task.await.unwrap();
    serve_result.expect("serve loop should end cleanly on EOF");

    // discover (id 1)
    let discover = responses.iter().find(|r| r["id"] == 1).unwrap();
    assert_eq!(discover["result"]["serverInfo"]["name"], "calculator");
    assert_eq!(
        discover["result"]["capabilities"]["tools"]["listChanged"],
        true
    );

    // tools/list (id 2)
    let list = responses.iter().find(|r| r["id"] == 2).unwrap();
    assert_eq!(list["result"]["tools"][0]["name"], "add");

    // tools/call (id 3) → 2 + 40 = 42
    let call = responses.iter().find(|r| r["id"] == 3).unwrap();
    assert_eq!(call["result"]["content"][0]["text"], "42");
    assert_eq!(call["result"]["isError"], false);
}

/// The transport surfaces a malformed frame as a codec error (which the serve
/// loop turns into a transport-level failure, ending the session).
#[tokio::test]
async fn malformed_frame_is_a_codec_error() {
    use turbomcp_service::Transport;

    let (client, server) = tokio::io::duplex(1024);
    let (server_rx, server_tx) = tokio::io::split(server);
    let mut transport = LineTransport::new(
        BufReader::new(server_rx),
        server_tx,
        turbomcp_codec::SerdeJsonCodec,
    );

    let (_client_rx, mut client_tx) = tokio::io::split(client);
    client_tx.write_all(b"{not json}\n").await.unwrap();
    client_tx.flush().await.unwrap();

    let err = transport.recv().await.unwrap_err();
    assert!(matches!(err, StdioError::Codec(_)));
}
