//! `call_tool_task` — the client side of core Tasks (`2025-11-25`): the
//! request carries the spec's `task` augmentation, the client polls
//! `tasks/get`, and the outcome comes back via `tasks/result`. The happy path
//! runs against a real `VersionDispatcher` with task support; the terminal
//! and degradation branches run against hand-scripted servers so each is
//! reached deterministically.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};

use async_trait::async_trait;
use serde_json::{Map, Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, split};
use turbomcp_client::{Client, ClientBuilder, ClientError, ConnectMode};
use turbomcp_codec::DefaultCodec;
use turbomcp_core::{CancellationToken, Implementation, JsonRpcError, McpResult};
use turbomcp_protocol::neutral;
use turbomcp_server::{
    CallToolContext, LegacySessionAdapter, ListToolsContext, McpServerCore, MethodRouter,
    TaskBackend, TaskError, TaskOutcome, TaskSnapshot, TaskStore, VersionDispatcher, WithTools,
};
use turbomcp_transport_stdio::LineTransport;

// ---- against a real server -----------------------------------------------------

#[derive(Clone)]
struct Sleeper;

impl McpServerCore for Sleeper {
    fn server_info(&self) -> Implementation {
        Implementation::new("sleeper", "1.0.0")
    }
}

impl WithTools for Sleeper {
    async fn list_tools(
        &self,
        _ctx: &ListToolsContext,
        _params: neutral::ListParams,
    ) -> McpResult<neutral::ListToolsResult> {
        Ok(neutral::ListToolsResult::new(vec![neutral::Tool::new(
            "nap",
            json!({"type": "object"}),
        )]))
    }

    async fn call_tool(
        &self,
        _ctx: &CallToolContext,
        _params: neutral::CallToolParams,
    ) -> McpResult<neutral::CallToolResult> {
        // Long enough that the CreateTaskResult returns while the tool is
        // still running, forcing at least one `working` poll.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        Ok(neutral::CallToolResult::text("well rested"))
    }
}

/// The bundled store with a test-speed suggested poll and a `create` counter,
/// so the test can assert the call actually took the task path.
struct FastPoll {
    inner: TaskStore,
    creates: AtomicUsize,
}

#[async_trait]
impl TaskBackend for FastPoll {
    async fn create(
        &self,
        session_id: &str,
        requested_ttl_ms: Option<i64>,
        cancel: CancellationToken,
    ) -> Result<TaskSnapshot, TaskError> {
        self.creates.fetch_add(1, SeqCst);
        self.inner
            .create(session_id.to_string(), requested_ttl_ms, cancel)
    }

    async fn complete(&self, task_id: &str, outcome: TaskOutcome) {
        self.inner.complete(task_id, outcome);
    }

    async fn cancel(&self, session_id: &str, task_id: &str) -> Result<TaskSnapshot, TaskError> {
        self.inner.cancel(session_id, task_id)
    }

    async fn get(&self, session_id: &str, task_id: &str) -> Result<TaskSnapshot, TaskError> {
        self.inner.get(session_id, task_id)
    }

    async fn list(
        &self,
        session_id: &str,
        cursor: Option<&str>,
        page_size: usize,
    ) -> Result<(Vec<TaskSnapshot>, Option<String>), TaskError> {
        self.inner.list(session_id, cursor, page_size)
    }

    async fn wait_result(
        &self,
        session_id: &str,
        task_id: &str,
    ) -> Result<Result<Value, JsonRpcError>, TaskError> {
        self.inner.wait_result(session_id, task_id).await
    }

    fn poll_interval_ms(&self) -> i64 {
        10
    }
}

async fn connect_real(tasks: Option<Arc<FastPoll>>) -> Client {
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    let (s_rd, s_wr) = split(server_io);
    let mut dispatcher = VersionDispatcher::new(Sleeper, MethodRouter::new().with_tools());
    if let Some(backend) = tasks {
        dispatcher = dispatcher.with_task_backend(backend);
    }
    let service = LegacySessionAdapter::new(dispatcher);
    let server_transport = LineTransport::new(BufReader::new(s_rd), s_wr, DefaultCodec::default());
    tokio::spawn(async move {
        let _ = turbomcp_service::serve(server_transport, service).await;
    });

    let (c_rd, c_wr) = split(client_io);
    let transport = LineTransport::new(BufReader::new(c_rd), c_wr, DefaultCodec::default());
    ClientBuilder::new("tasks-legacy", "1.0.0")
        .with_connect_mode(ConnectMode::Legacy)
        .connect(transport)
        .await
        .expect("legacy handshake succeeds")
}

fn text_of(result: &neutral::CallToolResult) -> &str {
    match &result.content[0] {
        neutral::Content::Text { text, .. } => text,
        other => panic!("expected text content, got {other:?}"),
    }
}

/// The full round trip against the real dispatcher: `tools/call` + `task` →
/// `CreateTaskResult` → `tasks/get` polling → `tasks/result` — and the result
/// is exactly what the un-augmented call would have produced. A plain
/// `call_tool` never touches the store.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn task_augmented_call_round_trips_against_a_real_server() {
    let backend = Arc::new(FastPoll {
        inner: TaskStore::default(),
        creates: AtomicUsize::new(0),
    });
    let client = connect_real(Some(Arc::clone(&backend))).await;

    let result = client
        .call_tool_task("nap", Map::new(), Some(60_000))
        .await
        .expect("task-augmented call settles");
    assert_eq!(text_of(&result), "well rested");
    assert_eq!(backend.creates.load(SeqCst), 1, "took the task path");

    let result = client.call_tool("nap", Map::new()).await.expect("inline");
    assert_eq!(text_of(&result), "well rested");
    assert_eq!(backend.creates.load(SeqCst), 1, "plain call stays inline");
}

/// Spec §Task Support and Handling: a server without Tasks enabled ignores
/// the `task` augmentation entirely and answers inline — `call_tool_task`
/// degrades to a plain call.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn without_task_support_the_augmented_call_degrades_to_inline() {
    let client = connect_real(None).await;
    let result = client
        .call_tool_task("nap", Map::new(), Some(60_000))
        .await
        .expect("inline degradation");
    assert_eq!(text_of(&result), "well rested");
}

// ---- scripted terminal branches --------------------------------------------------

/// Spawn a line-delimited scripted server: `respond(method, frame)` returns
/// `Some({"result": …})` / `Some({"error": …})` to answer. Notifications (no
/// id) are consumed without an answer.
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

fn initialize_ok() -> Value {
    json!({ "result": {
        "protocolVersion": "2025-11-25",
        "capabilities": { "tools": {}, "tasks": {} },
        "serverInfo": { "name": "scripted", "version": "1.0" }
    }})
}

fn task_handle(status: &str, ttl: u64) -> Value {
    json!({ "result": { "task": {
        "taskId": "t-1", "status": status,
        "createdAt": "2026-01-01T00:00:00Z", "lastUpdatedAt": "2026-01-01T00:00:00Z",
        "ttl": ttl, "pollInterval": 10
    }}})
}

fn task_state(status: &str) -> Value {
    json!({ "result": {
        "taskId": "t-1", "status": status,
        "createdAt": "2026-01-01T00:00:00Z", "lastUpdatedAt": "2026-01-01T00:00:00Z",
        "ttl": 60000, "pollInterval": 10
    }})
}

async fn connect_scripted<F>(respond: F) -> Client
where
    F: FnMut(&str, &Value) -> Option<Value> + Send + 'static,
{
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    spawn_scripted(server_io, respond);
    let (rd, wr) = split(client_io);
    let transport = LineTransport::new(BufReader::new(rd), wr, DefaultCodec::default());
    ClientBuilder::new("tasks-scripted", "1.0.0")
        .with_connect_mode(ConnectMode::Legacy)
        .connect(transport)
        .await
        .expect("scripted handshake succeeds")
}

/// The wire contract itself: the augmented `tools/call` carries `task.ttl`,
/// `working` keeps polling, and the settled value comes from `tasks/result`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_augmented_call_carries_ttl_and_settles_via_tasks_result() {
    let sent_task = Arc::new(Mutex::new(None::<Value>));
    let sent = Arc::clone(&sent_task);
    let gets = Arc::new(AtomicUsize::new(0));
    let gets_in = Arc::clone(&gets);
    let client = connect_scripted(move |method, frame| match method {
        "initialize" => Some(initialize_ok()),
        "tools/call" => {
            *sent.lock().unwrap() = frame.get("params").and_then(|p| p.get("task")).cloned();
            Some(task_handle("working", 60000))
        }
        "tasks/get" => Some(match gets_in.fetch_add(1, SeqCst) {
            0 => task_state("working"),
            _ => task_state("completed"),
        }),
        "tasks/result" => Some(json!({ "result": {
            "content": [ { "type": "text", "text": "task-done" } ]
        }})),
        other => panic!("unexpected method: {other}"),
    })
    .await;

    let result = client
        .call_tool_task("nap", Map::new(), Some(45_000))
        .await
        .expect("drives to completion");
    match &result.content[0] {
        neutral::Content::Text { text, .. } => assert_eq!(text, "task-done"),
        other => panic!("expected text, got {other:?}"),
    }
    assert_eq!(
        sent_task.lock().unwrap().take(),
        Some(json!({ "ttl": 45000 })),
        "the request carried the task augmentation"
    );
    assert_eq!(gets.load(SeqCst), 2, "polled through working to completed");
}

/// A `failed` task surfaces the underlying call's JSON-RPC error verbatim —
/// `tasks/result` answers it, and it propagates as `ClientError::Rpc`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failed_task_surfaces_the_underlying_rpc_error() {
    let client = connect_scripted(|method, _| match method {
        "initialize" => Some(initialize_ok()),
        "tools/call" => Some(task_handle("working", 60000)),
        "tasks/get" => Some(task_state("failed")),
        "tasks/result" => Some(json!({ "error": { "code": -32050, "message": "boom" } })),
        other => panic!("unexpected method: {other}"),
    })
    .await;

    match client.call_tool_task("nap", Map::new(), None).await {
        Err(ClientError::Rpc(e)) => {
            assert_eq!(e.code, -32050);
            assert_eq!(e.message, "boom");
        }
        other => panic!("expected the task's error, got {other:?}"),
    }
}

/// A task that lands `cancelled` (e.g. another connection cancelled it) is a
/// protocol error — there is no result to retrieve.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cancelled_task_is_a_protocol_error() {
    let client = connect_scripted(|method, _| match method {
        "initialize" => Some(initialize_ok()),
        "tools/call" => Some(task_handle("cancelled", 60000)),
        other => panic!("unexpected method: {other}"),
    })
    .await;

    match client.call_tool_task("nap", Map::new(), None).await {
        Err(ClientError::Protocol(msg)) => {
            assert!(msg.contains("t-1") && msg.contains("cancelled"), "{msg}");
        }
        other => panic!("expected a protocol error, got {other:?}"),
    }
}

/// The server-reported `ttl` is the polling backstop: a task that never
/// leaves `working` times out rather than polling forever.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_reported_ttl_bounds_polling() {
    let client = connect_scripted(|method, _| match method {
        "initialize" => Some(initialize_ok()),
        "tools/call" => Some(task_handle("working", 30)),
        "tasks/get" => Some(task_state("working")),
        other => panic!("unexpected method: {other}"),
    })
    .await;

    match client.call_tool_task("nap", Map::new(), None).await {
        Err(ClientError::Timeout) => {}
        other => panic!("expected a ttl timeout, got {other:?}"),
    }
}

/// Robustness: a spec-violating server that answers a *plain* `tools/call`
/// with a `CreateTaskResult` is still driven to the final result (be liberal
/// in what we accept), and the plain call sent no `task` field.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unsolicited_task_handle_is_still_driven() {
    let sent_task = Arc::new(Mutex::new(Some(json!("sentinel"))));
    let sent = Arc::clone(&sent_task);
    let client = connect_scripted(move |method, frame| match method {
        "initialize" => Some(initialize_ok()),
        "tools/call" => {
            *sent.lock().unwrap() = frame.get("params").and_then(|p| p.get("task")).cloned();
            Some(task_handle("working", 60000))
        }
        "tasks/get" => Some(task_state("completed")),
        "tasks/result" => Some(json!({ "result": {
            "content": [ { "type": "text", "text": "surprise" } ]
        }})),
        other => panic!("unexpected method: {other}"),
    })
    .await;

    let result = client
        .call_tool("nap", Map::new())
        .await
        .expect("driven anyway");
    match &result.content[0] {
        neutral::Content::Text { text, .. } => assert_eq!(text, "surprise"),
        other => panic!("expected text, got {other:?}"),
    }
    assert_eq!(sent_task.lock().unwrap().take(), None, "no task field sent");
}
