//! The `list_all_*` helpers: follow `nextCursor` to the last page, and refuse
//! to loop forever against a server whose cursor never terminates.
//!
//! The single-page `list_*` methods are the primitive; these wrappers are what
//! most callers want, because `list_tools(None)` against a paginating server
//! silently yields only the first page.

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, split};
use turbomcp_client::{Client, ClientBuilder, ClientError};
use turbomcp_codec::SerdeJsonCodec;
use turbomcp_transport_stdio::LineTransport;

/// Spawn a line-delimited scripted server (see `negotiation_and_recovery.rs`).
fn spawn_scripted<F>(server_io: tokio::io::DuplexStream, mut respond: F)
where
    F: FnMut(&str, &Value) -> Option<Value> + Send + 'static,
{
    tokio::spawn(async move {
        let (rd, mut wr) = split(server_io);
        let mut lines = BufReader::new(rd).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let frame: Value = serde_json::from_str(&line).expect("client sends valid json");
            let Some(method) = frame.get("method").and_then(Value::as_str) else {
                continue;
            };
            let Some(id) = frame.get("id").cloned() else {
                continue;
            };
            if let Some(body) = respond(method, &frame) {
                let mut reply = json!({ "jsonrpc": "2.0", "id": id });
                reply
                    .as_object_mut()
                    .unwrap()
                    .extend(body.as_object().unwrap().clone());
                wr.write_all(format!("{reply}\n").as_bytes()).await.unwrap();
            }
        }
    });
}

/// The SEP-2549 fields every draft list result carries.
const CACHE_FIELDS: &str = r#""resultType": "complete", "cacheScope": "private", "ttlMs": 0"#;

/// Build a `{"result": …}` body with the required cache fields merged in.
fn page(body: Value) -> Value {
    let mut result = body;
    let extra: Value = serde_json::from_str(&format!("{{{CACHE_FIELDS}}}")).unwrap();
    result
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    json!({ "result": result })
}

fn discover_ok() -> Value {
    json!({ "result": {
        "capabilities": { "tools": {}, "resources": {}, "prompts": {} },
        "supportedVersions": ["2026-07-28"],
        "resultType": "complete", "cacheScope": "private", "ttlMs": 0
    }})
}

/// Connect a client to a scripted server that answers `server/discover` and
/// delegates everything else to `respond`.
async fn client_against<F>(mut respond: F) -> Client
where
    F: FnMut(&str, &Value) -> Option<Value> + Send + 'static,
{
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    spawn_scripted(server_io, move |method, frame| match method {
        "server/discover" => Some(discover_ok()),
        other => respond(other, frame),
    });
    let (rd, wr) = split(client_io);
    ClientBuilder::new("pager", "1.0.0")
        // The response cache keys on the cursor, but these tests want every
        // page to reach the scripted server verbatim.
        .with_response_cache(false)
        .connect(LineTransport::new(BufReader::new(rd), wr, SerdeJsonCodec))
        .await
        .expect("handshake")
}

/// The cursor a request asked for (`None` on the first page).
fn cursor_of(frame: &Value) -> Option<String> {
    frame
        .get("params")?
        .get("cursor")?
        .as_str()
        .map(str::to_owned)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn list_all_tools_follows_every_page() {
    let client = client_against(|method, frame| match method {
        "tools/list" => Some(match cursor_of(frame).as_deref() {
            None => page(json!({
                "tools": [{ "name": "a", "inputSchema": { "type": "object" } }],
                "nextCursor": "p2"
            })),
            Some("p2") => page(json!({
                "tools": [{ "name": "b", "inputSchema": { "type": "object" } }],
                "nextCursor": "p3"
            })),
            Some("p3") => page(json!({
                "tools": [{ "name": "c", "inputSchema": { "type": "object" } }]
            })),
            Some(other) => panic!("unexpected cursor {other}"),
        }),
        other => panic!("unexpected method {other}"),
    })
    .await;

    let names: Vec<_> = client
        .list_all_tools()
        .await
        .expect("pages are followed")
        .into_iter()
        .map(|t| t.name)
        .collect();
    assert_eq!(names, ["a", "b", "c"]);

    // The single-page primitive still returns exactly one page — that
    // difference is the whole reason `list_all_*` exists.
    let first = client.list_tools(None).await.expect("single page");
    assert_eq!(first.tools.len(), 1);
    assert_eq!(first.next_cursor.as_deref(), Some("p2"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn list_all_resources_and_prompts_and_templates_follow_pages() {
    let client = client_against(|method, frame| {
        let second = cursor_of(frame).is_some();
        Some(match (method, second) {
            ("resources/list", false) => page(json!({
                "resources": [{ "uri": "file://1", "name": "one" }], "nextCursor": "n"
            })),
            ("resources/list", true) => page(json!({
                "resources": [{ "uri": "file://2", "name": "two" }]
            })),
            ("resources/templates/list", false) => page(json!({
                "resourceTemplates": [{ "uriTemplate": "file://{a}", "name": "t1" }],
                "nextCursor": "n"
            })),
            ("resources/templates/list", true) => page(json!({
                "resourceTemplates": [{ "uriTemplate": "file://{b}", "name": "t2" }]
            })),
            ("prompts/list", false) => page(json!({
                "prompts": [{ "name": "p1" }], "nextCursor": "n"
            })),
            ("prompts/list", true) => page(json!({ "prompts": [{ "name": "p2" }] })),
            (other, _) => panic!("unexpected method {other}"),
        })
    })
    .await;

    let resources = client.list_all_resources().await.expect("resources");
    assert_eq!(resources.len(), 2);
    let templates = client
        .list_all_resource_templates()
        .await
        .expect("templates");
    assert_eq!(templates.len(), 2);
    let prompts = client.list_all_prompts().await.expect("prompts");
    assert_eq!(prompts.len(), 2);
}

/// A server that keeps handing back the cursor it was just given would page
/// forever, accumulating duplicates. Fail loudly instead — and never return a
/// partial list that reads as complete.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cursor_that_does_not_advance_is_rejected() {
    let client = client_against(|method, _| match method {
        "tools/list" => Some(page(json!({
            "tools": [{ "name": "a", "inputSchema": { "type": "object" } }],
            "nextCursor": "stuck"
        }))),
        other => panic!("unexpected method {other}"),
    })
    .await;

    // First page returns `stuck`; the second is requested *with* `stuck` and
    // answers `stuck` again — that is the non-advancing case.
    let err = client
        .list_all_tools()
        .await
        .expect_err("a non-advancing cursor must not loop");
    assert!(
        matches!(&err, ClientError::Protocol(m) if m.contains("does not advance")),
        "unexpected error: {err}"
    );
}

/// An empty-string `nextCursor` is the same trap in different clothing: it is
/// `Some`, so it reads as "more pages", but it names no page.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_empty_cursor_is_rejected() {
    let client = client_against(|method, _| match method {
        "prompts/list" => Some(page(json!({
            "prompts": [{ "name": "p" }], "nextCursor": ""
        }))),
        other => panic!("unexpected method {other}"),
    })
    .await;

    let err = client
        .list_all_prompts()
        .await
        .expect_err("an empty cursor must not loop");
    assert!(
        matches!(&err, ClientError::Protocol(m) if m.contains("does not advance")),
        "unexpected error: {err}"
    );
}

/// An RPC failure mid-pagination propagates rather than silently truncating.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_mid_pagination_error_propagates() {
    let client = client_against(|method, frame| match method {
        "tools/list" => Some(match cursor_of(frame) {
            None => page(json!({
                "tools": [{ "name": "a", "inputSchema": { "type": "object" } }],
                "nextCursor": "p2"
            })),
            Some(_) => json!({ "error": { "code": -32603, "message": "boom" } }),
        }),
        other => panic!("unexpected method {other}"),
    })
    .await;

    let err = client
        .list_all_tools()
        .await
        .expect_err("the second page fails");
    assert_eq!(err.rpc_code(), Some(-32603));
}
