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
use turbomcp_client::{
    Client, ClientBuilder, ClientError, ConnectMode, ElicitationHandler, NotificationHandler,
    RootsHandler, SamplingHandler,
};
use turbomcp_core::ProtocolVersion;
use turbomcp_core::codec::SerdeJsonCodec;
use turbomcp_protocol::neutral;
use turbomcp_service::io::LineTransport;

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
impl ElicitationHandler for AcceptAll {
    async fn elicit(&self, _request: neutral::ElicitParams) -> neutral::ElicitOutcome {
        neutral::ElicitOutcome::new(neutral::ElicitAction::Accept, Map::new())
    }
}

// ---- Auto → legacy fallback ----------------------------------------------------

/// "The server returns any other error, or does not respond within a
/// reasonable timeout: the server is legacy … The fallback MUST NOT be keyed to
/// one specific error code" (2026-07-28 stdio §Backward Compatibility). Each
/// case is what a real legacy SDK answers `server/discover` with: `-32601`,
/// python-sdk/FastMCP's `-32602`, `-32600`, an implementation-defined
/// `-32603`, and an unsupported-version error listing only stateful
/// revisions. Falling back on `-32601` alone stranded all but the first.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn auto_falls_back_on_any_non_modern_answer() {
    let answers = [
        json!({ "code": -32601, "message": "Method not found" }),
        json!({ "code": -32602, "message": "Invalid request parameters" }),
        json!({ "code": -32600, "message": "Invalid Request" }),
        json!({ "code": -32603, "message": "boom" }),
        json!({ "code": -32022, "message": "Unsupported protocol version",
                "data": { "supported": ["2025-06-18", "2025-11-25"], "requested": "2026-07-28" } }),
    ];
    for answer in answers {
        let (client_io, server_io) = tokio::io::duplex(64 * 1024);
        let discover = answer.clone();
        spawn_scripted(server_io, move |method, _| match method {
            "server/discover" => Some(json!({ "error": discover.clone() })),
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
            .unwrap_or_else(|e| panic!("fallback after {answer} failed: {e}"));
        assert_eq!(client.protocol_version(), &ProtocolVersion::V2025_11_25);
        assert_eq!(client.server_info().unwrap().name, "legacy-only");
        let tools = client.list_tools(None).await.expect("legacy list works");
        assert!(tools.tools.is_empty());
    }
}

/// A legacy server that ignores unknown methods never answers the probe. That
/// is the spec's other legacy signal; it used to cost the full request timeout
/// and then fail the connect.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn auto_falls_back_when_the_probe_goes_unanswered() {
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    spawn_scripted(server_io, |method, _| match method {
        "server/discover" => None,
        "initialize" => Some(json!({ "result": {
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "serverInfo": { "name": "silent", "version": "1.0" }
        }})),
        other => panic!("unexpected method from client: {other}"),
    });
    let client = ClientBuilder::new("auto", "1.0.0")
        .with_connect_mode(ConnectMode::Auto)
        .with_timeout(Duration::from_millis(300))
        .connect(transport_for(client_io))
        .await
        .expect("a silent probe means legacy");
    assert_eq!(client.protocol_version(), &ProtocolVersion::V2025_11_25);
}

/// A recognized modern error is a modern server, and falling back to
/// `initialize` is exactly what the spec says not to do.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn auto_does_not_fall_back_on_a_modern_error() {
    for answer in [
        json!({ "code": -32021, "message": "Missing required client capability",
                "data": { "requiredCapabilities": { "sampling": {} } } }),
        json!({ "code": -32022, "message": "Unsupported protocol version",
                "data": { "supported": ["2099-01-01"], "requested": "2026-07-28" } }),
    ] {
        let (client_io, server_io) = tokio::io::duplex(64 * 1024);
        let discover = answer.clone();
        spawn_scripted(server_io, move |method, _| match method {
            "server/discover" => Some(json!({ "error": discover.clone() })),
            other => panic!("no fallback expected after {discover}, got {other}"),
        });
        let Err(err) = ClientBuilder::new("auto", "1.0.0")
            .with_connect_mode(ConnectMode::Auto)
            .connect(transport_for(client_io))
            .await
        else {
            panic!("{answer} is not a fallback trigger");
        };
        assert!(
            matches!(&err, ClientError::Rpc(e) if e.code == -32021)
                || matches!(&err, ClientError::Protocol(_)),
            "{err:?}"
        );
    }
}

// ---- HeaderMismatch recovery ----------------------------------------------------

/// Per the transports spec, a HeaderMismatch (`-32020`) on `tools/call` means
/// the client's mirrored headers came from a stale schema: refresh
/// `tools/list` (rebuilding the header cache) and retry exactly once.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn header_mismatch_refreshes_tools_list_and_retries_once() {
    header_mismatch_retry_case(-32020).await;
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

/// "If the `InputRequiredResult` does not contain a `requestState` field, the
/// client MUST NOT include one in the retry." The retry used to keep the
/// previous round's state, echoing a token the server had stopped issuing.
/// And a state-only round needs no handler at all: there is nothing to answer.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn each_mrtr_retry_carries_only_what_its_round_issued() {
    let seen = Arc::new(Mutex::new(Vec::<Value>::new()));
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    {
        let seen = Arc::clone(&seen);
        spawn_scripted(server_io, move |method, frame| match method {
            "server/discover" => Some(discover_ok()),
            "tools/call" => {
                let mut seen = seen.lock().unwrap();
                seen.push(frame["params"].clone());
                Some(match seen.len() {
                    // Round 1: a token only, no questions.
                    1 => {
                        json!({ "result": { "resultType": "input_required", "requestState": "s1" } })
                    }
                    // Round 2: a question and no token.
                    2 => input_required_no_state(),
                    _ => json!({ "result": {
                        "resultType": "complete",
                        "content": [{ "type": "text", "text": "done" }]
                    }}),
                })
            }
            other => panic!("unexpected method from client: {other}"),
        });
    }
    let client = ClientBuilder::new("mrtr", "1.0.0")
        .with_connect_mode(ConnectMode::Modern)
        .with_elicitation(AcceptAll)
        .connect(transport_for(client_io))
        .await
        .unwrap();
    client.call_tool("t", Map::new()).await.expect("converges");

    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 3);
    assert_eq!(seen[1]["requestState"], "s1", "round 1's token comes back");
    assert!(seen[1].get("inputResponses").is_none(), "nothing was asked");
    assert!(
        seen[2].get("requestState").is_none(),
        "round 2 issued no token, so none goes back: {}",
        seen[2]
    );
    assert!(seen[2]["inputResponses"].get("k").is_some());
}

fn input_required_no_state() -> Value {
    json!({ "result": {
        "resultType": "input_required",
        "inputRequests": { "k": { "method": "elicitation/create",
            "params": { "message": "?", "requestedSchema": { "type": "object", "properties": {} } } } }
    }})
}

/// A handler-less client can still follow a state-only round.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_state_only_round_needs_no_handler() {
    let rounds = Arc::new(AtomicUsize::new(0));
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    {
        let rounds = Arc::clone(&rounds);
        spawn_scripted(server_io, move |method, _| match method {
            "server/discover" => Some(discover_ok()),
            "tools/call" => Some(if rounds.fetch_add(1, SeqCst) == 0 {
                json!({ "result": { "resultType": "input_required", "requestState": "later" } })
            } else {
                json!({ "result": { "resultType": "complete", "content": [] } })
            }),
            other => panic!("unexpected method from client: {other}"),
        });
    }
    let client = ClientBuilder::new("bare", "1.0.0")
        .with_connect_mode(ConnectMode::Modern)
        .connect(transport_for(client_io))
        .await
        .unwrap();
    client
        .call_tool("t", Map::new())
        .await
        .expect("no questions, so no handler needed");
}

/// "A `resultType` of any value unrecognized by the client MUST be considered
/// invalid." It used to be read as a plain result.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unrecognized_result_type_is_invalid() {
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    spawn_scripted(server_io, |method, _| match method {
        "server/discover" => Some(discover_ok()),
        "tools/call" => Some(json!({ "result": { "resultType": "banana", "content": [] } })),
        other => panic!("unexpected method from client: {other}"),
    });
    let client = ClientBuilder::new("strict", "1.0.0")
        .with_connect_mode(ConnectMode::Modern)
        .connect(transport_for(client_io))
        .await
        .unwrap();
    let err = client.call_tool("t", Map::new()).await.unwrap_err();
    assert!(
        matches!(&err, ClientError::Protocol(m) if m.contains("banana")),
        "{err:?}"
    );
}

/// `-32001` is implementation-defined (FastMCP answers "Not found" with it),
/// and re-issuing `tools/call` on it ran the tool twice.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_implementation_defined_code_does_not_rerun_the_tool() {
    let calls = Arc::new(AtomicUsize::new(0));
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    {
        let calls = Arc::clone(&calls);
        spawn_scripted(server_io, move |method, _| match method {
            "server/discover" => Some(discover_ok()),
            "tools/call" => {
                calls.fetch_add(1, SeqCst);
                Some(json!({ "error": { "code": -32001, "message": "Not found" } }))
            }
            other => panic!("unexpected method from client: {other}"),
        });
    }
    let client = ClientBuilder::new("once", "1.0.0")
        .with_connect_mode(ConnectMode::Modern)
        .connect(transport_for(client_io))
        .await
        .unwrap();
    let err = client.call_tool("t", Map::new()).await.unwrap_err();
    assert_eq!(err.rpc_code(), Some(-32001));
    assert_eq!(calls.load(SeqCst), 1, "the tool ran exactly once");
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
        .with_elicitation(AcceptAll)
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
        .with_elicitation(AcceptAll) // elicit-only: sampling is never declared
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
    impl ElicitationHandler for Sampler {
        async fn elicit(&self, _request: neutral::ElicitParams) -> neutral::ElicitOutcome {
            neutral::ElicitOutcome::new(neutral::ElicitAction::Accept, Map::new())
        }
    }
    #[async_trait]
    impl SamplingHandler for Sampler {
        async fn create_message(
            &self,
            _params: neutral::CreateMessageParams,
        ) -> Result<neutral::CreateMessageResult, ClientError> {
            Ok(neutral::CreateMessageResult::text("test-model", "sampled"))
        }
    }
    #[async_trait]
    impl RootsHandler for Sampler {
        async fn list_roots(&self) -> Result<Vec<neutral::Root>, ClientError> {
            Ok(Vec::new())
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
                                      "params": { "messages": [], "maxTokens": 64 } },
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
        .with_elicitation(Sampler)
        .with_sampling(Sampler)
        .with_roots(Sampler)
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
    // A server may answer `resultType: "task"` only to a client that declared
    // the Tasks extension; to anyone else it is an unrecognized result type.
    ClientBuilder::new("tasks", "1.0.0")
        .with_connect_mode(ConnectMode::Modern)
        .with_extension("io.modelcontextprotocol/tasks", json!({}))
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
impl ElicitationHandler for CompletionSpy {
    async fn elicit(&self, _request: neutral::ElicitParams) -> neutral::ElicitOutcome {
        neutral::ElicitOutcome::new(neutral::ElicitAction::Decline, Map::new())
    }
    fn supports_url_mode(&self) -> bool {
        true
    }
    async fn elicit_url(&self, _request: neutral::ElicitUrlParams) -> neutral::ElicitOutcome {
        neutral::ElicitOutcome::new(neutral::ElicitAction::Accept, Map::new())
    }
    async fn on_elicitation_complete(&self, elicitation_id: String) {
        self.ids.lock().unwrap().push(elicitation_id);
    }
}

#[async_trait]
impl NotificationHandler for CompletionSpy {
    async fn on_notification(&self, method: String, _params: Option<Value>) {
        self.methods.lock().unwrap().push(method);
    }
}

/// A `notifications/elicitation/complete` reaches the dedicated hook only when
/// it names an elicitation this client was actually sent.
///
/// "Clients MUST ignore completion notifications for unknown or
/// already-completed elicitation IDs." An id the server invented, a repeat of
/// one already completed, and a malformed one are all unknown: they reach the
/// generic notification observer, never the typed hook.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn elicitation_complete_reaches_the_typed_hook() {
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    tokio::spawn(async move {
        let (rd, mut wr) = split(server_io);
        let mut lines = BufReader::new(rd).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let frame: Value = serde_json::from_str(&line).expect("valid json");
            let method = frame.get("method").and_then(Value::as_str);
            if method == Some("initialize") {
                let reply = json!({ "jsonrpc": "2.0", "id": frame["id"], "result": {
                    "protocolVersion": "2025-11-25",
                    "capabilities": {},
                    "serverInfo": { "name": "ec", "version": "1.0" }
                }});
                wr.write_all(format!("{reply}\n").as_bytes()).await.unwrap();
                continue;
            }
            // `elicitation/complete` exists on 2025-11-25 only, and a compliant
            // server asks nothing before the client says it is initialized.
            if method != Some("notifications/initialized") {
                continue;
            }

            // Only `eid-7` is ever asked of this client.
            let ask = json!({
                "jsonrpc": "2.0",
                "id": "url-1",
                "method": "elicitation/create",
                "params": {
                    "mode": "url",
                    "message": "Sign in",
                    "url": "https://auth.example/go",
                    "elicitationId": "eid-7",
                },
            });
            wr.write_all(format!("{ask}\n").as_bytes()).await.unwrap();

            for params in [
                json!({ "elicitationId": "eid-7" }),
                json!({ "elicitationId": "eid-7" }), // repeat → already completed
                json!({ "elicitationId": "eid-9" }), // never asked → unknown
                json!({ "elicitationId": 42 }),      // malformed → unknown
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
        .with_connect_mode(ConnectMode::Legacy)
        .with_elicitation(spy.clone())
        .with_notifications(spy.clone())
        .connect(transport_for(client_io))
        .await
        .unwrap();

    // All four notifications are in flight behind the handshake response.
    for _ in 0..50 {
        if spy.methods.lock().unwrap().len() == 4 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(
        *spy.ids.lock().unwrap(),
        vec!["eid-7".to_owned()],
        "only the id this client was actually sent, and only once"
    );
    assert_eq!(
        spy.methods.lock().unwrap().len(),
        4,
        "every notification still reaches the generic hook"
    );
}

/// A client that serves roots declares them, and `roots.listChanged` only when
/// the handler says it emits the notification — which it then actually does.
///
/// The notification is the one list-changed message that travels client→server,
/// and before this it existed nowhere but the generated wire types: no
/// constant, nothing that sent it, nothing that accepted it. Driven by a
/// bespoke loop because `spawn_scripted` drops notifications before its
/// callback ever sees them.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_roots_client_declares_and_emits_list_changed() {
    struct Watched;
    #[async_trait]
    impl RootsHandler for Watched {
        async fn list_roots(&self) -> Result<Vec<neutral::Root>, ClientError> {
            Ok(vec![
                neutral::Root::new("file:///work")
                    .expect("a file URI")
                    .with_name("work"),
            ])
        }
        fn list_changed(&self) -> bool {
            true
        }
    }

    let declared: Arc<std::sync::Mutex<Option<Value>>> = Arc::new(std::sync::Mutex::new(None));
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    {
        let declared = Arc::clone(&declared);
        tokio::spawn(async move {
            let (rd, mut wr) = split(server_io);
            let mut lines = BufReader::new(rd).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let frame: Value = serde_json::from_str(&line).expect("valid json");
                let Some(method) = frame.get("method").and_then(Value::as_str) else {
                    continue;
                };
                let _ = tx.send(method.to_owned());
                if method == "initialize" {
                    *declared.lock().unwrap() = Some(frame["params"]["capabilities"].clone());
                    let reply = json!({
                        "jsonrpc": "2.0",
                        "id": frame["id"].clone(),
                        "result": {
                            "protocolVersion": "2025-11-25",
                            "capabilities": {},
                            "serverInfo": { "name": "s", "version": "1" }
                        }
                    });
                    wr.write_all(format!("{reply}\n").as_bytes()).await.unwrap();
                }
            }
        });
    }

    let client = ClientBuilder::new("rooted", "1.0.0")
        .with_connect_mode(ConnectMode::Legacy)
        .with_roots(Watched)
        .connect(transport_for(client_io))
        .await
        .expect("handshake");

    let declared = declared.lock().unwrap().clone().expect("handshake seen");
    assert_eq!(
        declared,
        json!({ "roots": { "listChanged": true } }),
        "registering a roots handler is what declares roots"
    );

    client.notify_roots_changed().await.unwrap();
    let mut seen = Vec::new();
    while let Ok(Some(method)) =
        tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv()).await
    {
        seen.push(method);
        if seen.iter().any(|m| m == "notifications/roots/list_changed") {
            return;
        }
    }
    panic!("no notifications/roots/list_changed; saw {seen:?}");
}

// ---- inbound ordering, overload and cancellation ------------------------------------

/// Records progress values in the order the handler sees them; slow on
/// purpose, so a burst piles up behind it.
#[derive(Clone, Default)]
struct ProgressSpy {
    seen: Arc<Mutex<Vec<i64>>>,
}

#[async_trait]
impl NotificationHandler for ProgressSpy {
    async fn on_notification(&self, _method: String, params: Option<Value>) {
        tokio::time::sleep(Duration::from_millis(1)).await;
        if let Some(p) = params.and_then(|p| p["progress"].as_i64()) {
            self.seen.lock().unwrap().push(p);
        }
    }
}

/// Each notification used to be its own task, so progress could reach the
/// handler out of order, and a burst of more than 128 outstanding closed the
/// connection. Now one task delivers them, in order, and a burst is just a
/// queue.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn notifications_arrive_in_order_and_a_burst_does_not_disconnect() {
    let (client_io, server_io) = tokio::io::duplex(1024 * 1024);
    tokio::spawn(async move {
        let (rd, mut wr) = split(server_io);
        let mut lines = BufReader::new(rd).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let frame: Value = serde_json::from_str(&line).unwrap();
            match frame.get("method").and_then(Value::as_str) {
                Some("server/discover") => {
                    let mut reply = json!({ "jsonrpc": "2.0", "id": frame["id"] });
                    reply
                        .as_object_mut()
                        .unwrap()
                        .extend(discover_ok().as_object().unwrap().clone());
                    let mut out = format!("{reply}\n");
                    for p in 0..300 {
                        let note = json!({
                            "jsonrpc": "2.0",
                            "method": "notifications/progress",
                            "params": { "progressToken": "t", "progress": p },
                        });
                        out.push_str(&format!("{note}\n"));
                    }
                    wr.write_all(out.as_bytes()).await.unwrap();
                }
                Some("tools/list") => {
                    let reply = json!({ "jsonrpc": "2.0", "id": frame["id"], "result": {
                        "resultType": "complete", "ttlMs": 0, "cacheScope": "private", "tools": []
                    }});
                    wr.write_all(format!("{reply}\n").as_bytes()).await.unwrap();
                }
                _ => {}
            }
        }
    });
    let spy = ProgressSpy::default();
    let client = ClientBuilder::new("bursty", "1.0.0")
        .with_connect_mode(ConnectMode::Modern)
        .with_notifications(spy.clone())
        .connect(transport_for(client_io))
        .await
        .unwrap();
    // A deadline, not a poll count: the handler sleeps 1ms per notification,
    // and Windows timers round that up to ~15ms, so 300 take seconds there.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    while spy.seen.lock().unwrap().len() < 300 && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let seen = spy.seen.lock().unwrap().clone();
    assert_eq!(seen, (0..300).collect::<Vec<_>>(), "every one, in order");
    client.list_tools(None).await.expect("still connected");
}

/// Holds its elicitation open until cancelled.
#[derive(Clone, Default)]
struct Hangs {
    started: Arc<AtomicUsize>,
    finished: Arc<AtomicUsize>,
}

#[async_trait]
impl ElicitationHandler for Hangs {
    async fn elicit(&self, _request: neutral::ElicitParams) -> neutral::ElicitOutcome {
        self.started.fetch_add(1, SeqCst);
        tokio::time::sleep(Duration::from_secs(30)).await;
        self.finished.fetch_add(1, SeqCst);
        neutral::ElicitOutcome::new(neutral::ElicitAction::Decline, Map::new())
    }
}

/// "Receivers of cancellation notifications SHOULD: Stop processing the
/// cancelled request … Not send a response for the cancelled request." A
/// server that gave up on its elicitation used to get the answer anyway, once
/// the user finally submitted.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_request_the_server_cancels_is_stopped_and_never_answered() {
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    let answered = Arc::new(Mutex::new(Vec::<Value>::new()));
    {
        let answered = Arc::clone(&answered);
        tokio::spawn(async move {
            let (rd, mut wr) = split(server_io);
            let mut lines = BufReader::new(rd).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let frame: Value = serde_json::from_str(&line).unwrap();
                match frame.get("method").and_then(Value::as_str) {
                    Some("initialize") => {
                        let reply = json!({ "jsonrpc": "2.0", "id": frame["id"], "result": {
                            "protocolVersion": "2025-11-25", "capabilities": {},
                            "serverInfo": { "name": "s", "version": "1" }
                        }});
                        wr.write_all(format!("{reply}\n").as_bytes()).await.unwrap();
                    }
                    Some("notifications/initialized") => {
                        let ask = json!({ "jsonrpc": "2.0", "id": "srv-1", "method": "elicitation/create",
                            "params": { "mode": "form", "message": "?",
                                "requestedSchema": { "type": "object", "properties": {} } } });
                        wr.write_all(format!("{ask}\n").as_bytes()).await.unwrap();
                        tokio::time::sleep(Duration::from_millis(100)).await;
                        let cancel = json!({ "jsonrpc": "2.0", "method": "notifications/cancelled",
                            "params": { "requestId": "srv-1", "reason": "timed out" } });
                        wr.write_all(format!("{cancel}\n").as_bytes())
                            .await
                            .unwrap();
                    }
                    None if frame.get("id").is_some() => answered.lock().unwrap().push(frame),
                    _ => {}
                }
            }
        });
    }
    let hangs = Hangs::default();
    let _client = ClientBuilder::new("cancellable", "1.0.0")
        .with_connect_mode(ConnectMode::Legacy)
        .with_elicitation(hangs.clone())
        .connect(transport_for(client_io))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(hangs.started.load(SeqCst), 1, "the handler ran");
    assert_eq!(hangs.finished.load(SeqCst), 0, "and was stopped");
    assert!(
        answered.lock().unwrap().is_empty(),
        "no reply to a cancelled request"
    );
}
