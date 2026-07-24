//! Client behavior under server responses the happy path never produces:
//! the `Auto` → legacy negotiation fallback, the HeaderMismatch
//! refresh-and-retry-once recovery, the MRTR round cap and no-handler error,
//! packaged `sampling/createMessage` / `roots/list` dispatch, and the
//! auto-driven task terminal-state mapping — all against hand-scripted
//! servers so each branch is reached deterministically.

use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{Map, Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, split};
use turbomcp_client::{Client, ClientBuilder, ClientError, ClientHandler, ConnectMode};
use turbomcp_codec::SerdeJsonCodec;
use turbomcp_core::ProtocolVersion;
use turbomcp_protocol::neutral;
use turbomcp_transport_stdio::LineTransport;

/// Spawn a line-delimited scripted server: `respond(method, frame)` returns
/// `Some({"result": …})` / `Some({"error": …})` to answer, or `None` to stay
/// silent. Notifications (no id) are consumed without an answer.
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
                continue; // notification: nothing to answer
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

fn transport_for(client_io: tokio::io::DuplexStream) -> impl turbomcp_service::Transport {
    let (rd, wr) = split(client_io);
    LineTransport::new(BufReader::new(rd), wr, SerdeJsonCodec)
}

fn discover_ok() -> Value {
    json!({ "result": {
        "capabilities": { "tools": {} },
        "supportedVersions": ["2026-07-28"],
        "resultType": "complete", "cacheScope": "private", "ttlMs": 0
    }})
}

/// Accepts every elicitation with empty content; sampling/roots stay default.
struct AcceptAll;

#[async_trait]
impl ClientHandler for AcceptAll {
    async fn elicit(&self, _request: neutral::ElicitParams) -> neutral::ElicitOutcome {
        neutral::ElicitOutcome::new(neutral::ElicitAction::Accept, Map::new())
    }
}

// ---- Auto → legacy fallback ----------------------------------------------------

/// The point of `ConnectMode::Auto`: when `server/discover` is unavailable
/// (`-32601` from a legacy-only server, or UnsupportedProtocolVersion from a
/// version-strict one), the client falls back to `initialize` and lands on
/// `2025-11-25`. Both the RC's `-32004` and the earlier draft's `-32022` are
/// accepted, so a peer tracking either spec revision still negotiates.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn auto_falls_back_to_legacy_when_discover_is_unavailable() {
    for discover_code in [-32601i64, -32004, -32022] {
        let (client_io, server_io) = tokio::io::duplex(64 * 1024);
        spawn_scripted(server_io, move |method, _| match method {
            "server/discover" => Some(json!({
                "error": { "code": discover_code, "message": "not here" }
            })),
            "initialize" => Some(json!({ "result": {
                "protocolVersion": "2025-11-25",
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "legacy-only", "version": "1.0" }
            }})),
            "tools/list" => Some(json!({ "result": { "tools": [] } })),
            other => panic!("unexpected method from client: {other}"),
        });

        let client = ClientBuilder::new("auto", "1.0.0")
            .with_connect_mode(ConnectMode::Auto)
            .connect(transport_for(client_io))
            .await
            .unwrap_or_else(|e| panic!("fallback handshake (code {discover_code}) failed: {e}"));
        assert_eq!(client.protocol_version(), &ProtocolVersion::V2025_11_25);
        assert_eq!(client.server_info().unwrap().name, "legacy-only");
        // The negotiated legacy wire drives the typed surface.
        let tools = client.list_tools(None).await.expect("legacy list works");
        assert!(tools.tools.is_empty());
    }
}

/// A non-negotiation failure (here `-32603`) must NOT trigger the fallback —
/// it surfaces as the handshake error.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn auto_does_not_swallow_unrelated_discover_failures() {
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    spawn_scripted(server_io, |method, _| match method {
        "server/discover" => Some(json!({
            "error": { "code": -32603, "message": "boom" }
        })),
        other => panic!("no fallback expected, got {other}"),
    });
    let Err(err) = ClientBuilder::new("auto", "1.0.0")
        .with_connect_mode(ConnectMode::Auto)
        .connect(transport_for(client_io))
        .await
    else {
        panic!("internal error is not a fallback trigger");
    };
    assert!(
        matches!(&err, ClientError::Rpc(e) if e.code == -32603),
        "{err:?}"
    );
}

// ---- HeaderMismatch recovery ----------------------------------------------------

/// Per the transports spec, a HeaderMismatch on `tools/call` means the
/// client's mirrored headers came from a stale schema: refresh `tools/list`
/// (rebuilding the header cache) and retry exactly once. Driven for both the
/// RC's `-32001` and the earlier draft's `-32020`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn header_mismatch_refreshes_tools_list_and_retries_once() {
    for mismatch_code in [-32001i64, -32020] {
        header_mismatch_retry_case(mismatch_code).await;
    }
}

async fn header_mismatch_retry_case(mismatch_code: i64) {
    let calls = Arc::new(AtomicUsize::new(0));
    let lists = Arc::new(AtomicUsize::new(0));
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    {
        let calls = Arc::clone(&calls);
        let lists = Arc::clone(&lists);
        spawn_scripted(server_io, move |method, _| match method {
            "server/discover" => Some(discover_ok()),
            "tools/list" => {
                lists.fetch_add(1, SeqCst);
                Some(json!({ "result": {
                    "tools": [], "resultType": "complete",
                    "cacheScope": "private", "ttlMs": 0
                }}))
            }
            "tools/call" => {
                if calls.fetch_add(1, SeqCst) == 0 {
                    Some(
                        json!({ "error": { "code": mismatch_code, "message": "header mismatch" } }),
                    )
                } else {
                    Some(json!({ "result": {
                        "content": [{ "type": "text", "text": "ok" }],
                        "resultType": "complete"
                    }}))
                }
            }
            other => panic!("unexpected method from client: {other}"),
        });
    }

    let client = ClientBuilder::new("hm", "1.0.0")
        .with_connect_mode(ConnectMode::Modern)
        .connect(transport_for(client_io))
        .await
        .unwrap();
    let result = client
        .call_tool("echo", Map::new())
        .await
        .expect("the single retry succeeds");
    assert!(!result.is_error);
    assert_eq!(calls.load(SeqCst), 2, "exactly one retry");
    assert_eq!(lists.load(SeqCst), 1, "exactly one schema refresh");
}

// ---- MRTR edges -----------------------------------------------------------------

fn input_required_body(requests: Value) -> Value {
    json!({ "result": {
        "resultType": "input_required",
        "inputRequests": requests,
        "requestState": "opaque-resume-state"
    }})
}

/// A server that requires input from a handler-less client is a protocol
/// error, not a hang or a panic.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mrtr_without_a_handler_is_a_protocol_error() {
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    spawn_scripted(server_io, |method, _| match method {
        "server/discover" => Some(discover_ok()),
        "tools/call" => Some(input_required_body(json!({
            "k1": { "method": "elicitation/create",
                    "params": { "message": "hi", "requestedSchema": {} } }
        }))),
        other => panic!("unexpected method from client: {other}"),
    });
    let client = ClientBuilder::new("no-handler", "1.0.0")
        .with_connect_mode(ConnectMode::Modern)
        .connect(transport_for(client_io))
        .await
        .unwrap();
    let err = client
        .call_tool("needs-input", Map::new())
        .await
        .expect_err("no handler to answer with");
    assert!(
        matches!(&err, ClientError::Protocol(m) if m.contains("no handler")),
        "{err:?}"
    );
}

/// A server that answers `input_required` forever hits the round cap and
/// surfaces "did not converge" instead of looping unboundedly.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mrtr_gives_up_after_the_round_cap() {
    let rounds = Arc::new(AtomicUsize::new(0));
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    {
        let rounds = Arc::clone(&rounds);
        spawn_scripted(server_io, move |method, _| match method {
            "server/discover" => Some(discover_ok()),
            "tools/call" => {
                rounds.fetch_add(1, SeqCst);
                Some(input_required_body(json!({
                    // A fresh key every round, so the handler keeps answering.
                    (format!("k{}", rounds.load(SeqCst))): {
                        "method": "elicitation/create",
                        "params": { "message": "again", "requestedSchema": {} }
                    }
                })))
            }
            other => panic!("unexpected method from client: {other}"),
        });
    }
    let client = ClientBuilder::new("looper", "1.0.0")
        .with_connect_mode(ConnectMode::Modern)
        .with_handler(AcceptAll)
        .connect(transport_for(client_io))
        .await
        .unwrap();
    let err = client
        .call_tool("never-settles", Map::new())
        .await
        .expect_err("must not loop forever");
    assert!(
        matches!(&err, ClientError::Protocol(m) if m.contains("did not converge")),
        "{err:?}"
    );
    assert_eq!(rounds.load(SeqCst), 16, "the documented round cap");
}

// ---- packaged sampling / roots dispatch ------------------------------------------

/// The default handler refuses `sampling/createMessage`; the refusal surfaces
/// as a protocol error on the original call rather than being silently dropped.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn packaged_sampling_is_refused_by_default() {
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    spawn_scripted(server_io, |method, _| match method {
        "server/discover" => Some(discover_ok()),
        "tools/call" => Some(input_required_body(json!({
            "k1": { "method": "sampling/createMessage", "params": { "messages": [] } }
        }))),
        other => panic!("unexpected method from client: {other}"),
    });
    let client = ClientBuilder::new("no-sampling", "1.0.0")
        .with_connect_mode(ConnectMode::Modern)
        .with_handler(AcceptAll) // elicit-only: sampling stays the refusing default
        .connect(transport_for(client_io))
        .await
        .unwrap();
    let err = client
        .call_tool("wants-sampling", Map::new())
        .await
        .expect_err("default handler refuses sampling");
    assert!(
        matches!(&err, ClientError::Protocol(m) if m.contains("does not support sampling")),
        "{err:?}"
    );
}

/// An overriding handler's sampling answer and the default `roots/list` answer
/// both travel back to the server as `inputResponses`, keyed as sent.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn packaged_sampling_and_roots_reach_the_handler_and_return() {
    struct Sampler;
    #[async_trait]
    impl ClientHandler for Sampler {
        async fn elicit(&self, _request: neutral::ElicitParams) -> neutral::ElicitOutcome {
            neutral::ElicitOutcome::new(neutral::ElicitAction::Accept, Map::new())
        }
        async fn create_message(&self, _params: Value) -> Result<Value, ClientError> {
            Ok(json!({
                "role": "assistant",
                "content": { "type": "text", "text": "sampled" },
                "model": "test-model"
            }))
        }
    }

    let seen_responses: Arc<std::sync::Mutex<Option<Value>>> =
        Arc::new(std::sync::Mutex::new(None));
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    {
        let seen = Arc::clone(&seen_responses);
        let mut first_call = true;
        spawn_scripted(server_io, move |method, frame| match method {
            "server/discover" => Some(discover_ok()),
            "tools/call" => {
                if first_call {
                    first_call = false;
                    Some(input_required_body(json!({
                        "k-sample": { "method": "sampling/createMessage",
                                      "params": { "messages": [] } },
                        "k-roots": { "method": "roots/list" }
                    })))
                } else {
                    *seen.lock().unwrap() = Some(frame["params"]["inputResponses"].clone());
                    Some(json!({ "result": {
                        "content": [{ "type": "text", "text": "done" }],
                        "resultType": "complete"
                    }}))
                }
            }
            other => panic!("unexpected method from client: {other}"),
        });
    }

    let client = ClientBuilder::new("sampler", "1.0.0")
        .with_connect_mode(ConnectMode::Modern)
        .with_handler(Sampler)
        .connect(transport_for(client_io))
        .await
        .unwrap();
    let result = client
        .call_tool("wants-both", Map::new())
        .await
        .expect("both answers gathered");
    assert!(matches!(&result.content[0], neutral::Content::Text { text, .. } if text == "done"));

    let responses = seen_responses
        .lock()
        .unwrap()
        .take()
        .expect("second call seen");
    assert_eq!(responses["k-sample"]["model"], "test-model");
    assert_eq!(responses["k-roots"]["roots"], json!([]));
}

// ---- auto-driven task terminal states ---------------------------------------------

/// Connect against a server whose `tools/call` answers a task handle and whose
/// `tasks/get` answers `terminal`.
async fn task_client(terminal: Value, ttl_ms: u64) -> Client {
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    spawn_scripted(server_io, move |method, _| match method {
        "server/discover" => Some(discover_ok()),
        "tools/call" => Some(json!({ "result": {
            "resultType": "task", "taskId": "t1", "status": "working",
            "pollIntervalMs": 1, "ttlMs": ttl_ms
        }})),
        "tasks/get" => Some(json!({ "result": terminal })),
        other => panic!("unexpected method from client: {other}"),
    });
    ClientBuilder::new("tasks", "1.0.0")
        .with_connect_mode(ConnectMode::Modern)
        .connect(transport_for(client_io))
        .await
        .unwrap()
}

/// The auto-drive mapping: `failed` → the task's JSON-RPC error, `cancelled`
/// → a protocol error naming the task, `completed` without a `result` → a
/// decode error, and a task that outlives its `ttlMs` → `Timeout`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn driven_task_terminal_states_map_to_client_errors() {
    let client = task_client(
        json!({ "taskId": "t1", "status": "failed",
                "error": { "code": -32050, "message": "boom" } }),
        5_000,
    )
    .await;
    let err = client
        .call_tool("t", Map::new())
        .await
        .expect_err("failed task");
    match &err {
        ClientError::Rpc(e) => {
            assert_eq!(e.code, -32050);
            assert_eq!(e.message, "boom");
        }
        other => panic!("expected Rpc, got {other:?}"),
    }

    let client = task_client(json!({ "taskId": "t1", "status": "cancelled" }), 5_000).await;
    let err = client
        .call_tool("t", Map::new())
        .await
        .expect_err("cancelled task");
    assert!(
        matches!(&err, ClientError::Protocol(m) if m.contains("t1") && m.contains("cancelled")),
        "{err:?}"
    );

    let client = task_client(json!({ "taskId": "t1", "status": "completed" }), 5_000).await;
    let err = client
        .call_tool("t", Map::new())
        .await
        .expect_err("completed without result");
    assert!(matches!(&err, ClientError::Decode(_)), "{err:?}");

    // Never completes; the finite ttlMs is the polling backstop.
    let client = task_client(
        json!({ "taskId": "t1", "status": "working", "pollIntervalMs": 1 }),
        25,
    )
    .await;
    let err = tokio::time::timeout(Duration::from_secs(5), client.call_tool("t", Map::new()))
        .await
        .expect("ttl backstop fires promptly")
        .expect_err("ttl exceeded");
    assert!(matches!(&err, ClientError::Timeout), "{err:?}");
}

// ---- elicitation/complete routing -----------------------------------------------

/// Records the ids handed to the dedicated `on_elicitation_complete` hook, and
/// separately every notification method seen by the generic hook.
#[derive(Clone, Default)]
struct CompletionSpy {
    ids: Arc<Mutex<Vec<String>>>,
    methods: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl ClientHandler for CompletionSpy {
    async fn elicit(&self, _request: neutral::ElicitParams) -> neutral::ElicitOutcome {
        neutral::ElicitOutcome::new(neutral::ElicitAction::Decline, Map::new())
    }
    async fn on_elicitation_complete(&self, elicitation_id: String) {
        self.ids.lock().unwrap().push(elicitation_id);
    }
    async fn on_notification(&self, method: String, _params: Option<Value>) {
        self.methods.lock().unwrap().push(method);
    }
}

/// A server-pushed `notifications/elicitation/complete` reaches the dedicated
/// hook with its `elicitationId` — and still reaches the generic notification
/// hook. A malformed one (no string id) is an unknown id: generic hook only,
/// never the typed one (clients MUST ignore unknown ids).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn elicitation_complete_reaches_the_typed_hook() {
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    tokio::spawn(async move {
        let (rd, mut wr) = split(server_io);
        let mut lines = BufReader::new(rd).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let frame: Value = serde_json::from_str(&line).expect("valid json");
            let Some(id) = frame.get("id").cloned() else {
                continue;
            };
            if frame.get("method").and_then(Value::as_str) != Some("server/discover") {
                continue;
            }
            let mut reply = json!({ "jsonrpc": "2.0", "id": id });
            reply
                .as_object_mut()
                .unwrap()
                .extend(discover_ok().as_object().unwrap().clone());
            wr.write_all(format!("{reply}\n").as_bytes()).await.unwrap();
            for params in [
                json!({ "elicitationId": "eid-7" }),
                json!({ "elicitationId": 42 }), // malformed → unknown id
            ] {
                let note = json!({
                    "jsonrpc": "2.0",
                    "method": "notifications/elicitation/complete",
                    "params": params,
                });
                wr.write_all(format!("{note}\n").as_bytes()).await.unwrap();
            }
        }
    });

    let spy = CompletionSpy::default();
    let _client = ClientBuilder::new("ec", "1.0.0")
        .with_connect_mode(ConnectMode::Modern)
        .with_handler(spy.clone())
        .connect(transport_for(client_io))
        .await
        .unwrap();

    // Both notifications are in flight behind the handshake response.
    for _ in 0..50 {
        if spy.methods.lock().unwrap().len() == 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(
        *spy.ids.lock().unwrap(),
        vec!["eid-7".to_owned()],
        "only the well-formed id reaches the typed hook"
    );
    assert_eq!(
        spy.methods.lock().unwrap().len(),
        2,
        "both still reach the generic hook"
    );
}
