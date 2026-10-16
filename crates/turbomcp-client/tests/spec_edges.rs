//! `2026-07-28` rules the happy path never exercises, each against a scripted
//! server: an unrecognized `resultType` on any result, an `input_required`
//! where no input can be asked for, a multi-round `resources/read` kept out
//! of the response cache, and a server request that belongs to no call.

use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
use std::sync::{Arc, Mutex};
use turbomcp_wire_check::CheckedTransport;

use async_trait::async_trait;
use serde_json::{Map, Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, split};
use turbomcp_client::{Client, ClientBuilder, ClientError, ElicitationHandler, SamplingHandler};
use turbomcp_core::codec::SerdeJsonCodec;
use turbomcp_protocol::neutral;
use turbomcp_service::io::LineTransport;

/// Frames the client sent that were not requests or notifications: its
/// answers to the server's own requests.
type Answers = Arc<Mutex<Vec<Value>>>;

/// A scripted `2026-07-28` server writing raw frames, so a reply can be a
/// server request as well as a response. Discovery is answered here.
async fn connect<F>(builder: ClientBuilder, mut frames_for: F) -> (Client, Answers)
where
    F: FnMut(&str, &Value) -> Vec<Value> + Send + 'static,
{
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    let answers = Answers::default();
    let seen = Arc::clone(&answers);
    tokio::spawn(async move {
        let (rd, mut wr) = split(server_io);
        let mut lines = BufReader::new(rd).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let frame: Value = serde_json::from_str(&line).expect("client sends valid json");
            let Some(method) = frame.get("method").and_then(Value::as_str) else {
                seen.lock().unwrap().push(frame);
                continue;
            };
            let out = match method {
                "server/discover" => vec![result_for(
                    &frame,
                    json!({
                        "capabilities": { "tools": {}, "resources": {} },
                        "supportedVersions": ["2026-07-28"],
                        "resultType": "complete", "cacheScope": "private", "ttlMs": 0
                    }),
                )],
                other => frames_for(other, &frame),
            };
            for out in out {
                wr.write_all(format!("{out}\n").as_bytes()).await.unwrap();
            }
        }
    });
    let (rd, wr) = split(client_io);
    let client = builder
        .connect(CheckedTransport::new(LineTransport::new(
            BufReader::new(rd),
            wr,
            SerdeJsonCodec,
        )))
        .await
        .expect("handshake");
    (client, answers)
}

fn result_for(frame: &Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": frame.get("id").cloned().unwrap_or(Value::Null), "result": result })
}

struct Accept;

#[async_trait]
impl ElicitationHandler for Accept {
    async fn elicit(&self, _request: neutral::ElicitParams) -> neutral::ElicitOutcome {
        neutral::ElicitOutcome::new(neutral::ElicitAction::Accept, Map::new())
    }
}

/// Counts the sampling requests it is asked to run.
#[derive(Clone, Default)]
struct CountingSampler(Arc<AtomicUsize>);

#[async_trait]
impl SamplingHandler for CountingSampler {
    async fn create_message(
        &self,
        _request: neutral::CreateMessageParams,
    ) -> turbomcp_client::ClientResult<neutral::CreateMessageResult> {
        self.0.fetch_add(1, SeqCst);
        Err(ClientError::Protocol("not in this test".into()))
    }
}

/// "A `resultType` of any value unrecognized by the client MUST be considered
/// invalid" — on a list result as much as on a call.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unrecognized_result_type_is_invalid_on_any_result() {
    let (client, _) = connect(ClientBuilder::new("edges", "1.0.0"), |method, frame| {
        match method {
            "tools/list" => vec![result_for(
                frame,
                json!({ "tools": [], "resultType": "streaming", "cacheScope": "private", "ttlMs": 0 }),
            )],
            other => panic!("unexpected method {other}"),
        }
    })
    .await;
    let err = client.list_tools(None).await.expect_err("invalid result");
    assert!(matches!(err, ClientError::Protocol(_)), "{err:?}");
}

/// Only `tools/call`, `resources/read` and `prompts/get` can ask for input.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn input_required_is_invalid_where_no_input_can_be_asked() {
    let (client, _) = connect(
        ClientBuilder::new("edges", "1.0.0"),
        |method, frame| match method {
            "tools/list" => vec![result_for(
                frame,
                json!({ "resultType": "input_required", "requestState": "x" }),
            )],
            other => panic!("unexpected method {other}"),
        },
    )
    .await;
    let err = client.list_tools(None).await.expect_err("invalid result");
    assert!(matches!(err, ClientError::Protocol(_)), "{err:?}");
}

/// "Results from requests that include `inputResponses` or `requestState`
/// MUST NOT be cached": the next read of the same URI asks again.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_read_that_asked_for_input_is_not_cached() {
    let reads = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&reads);
    let (client, _) = connect(
        ClientBuilder::new("edges", "1.0.0")
            .with_response_cache(true)
            .with_elicitation(Accept),
        move |method, frame| match method {
            "resources/read" => {
                counted.fetch_add(1, SeqCst);
                let answered = frame["params"].get("inputResponses").is_some();
                if answered {
                    vec![result_for(
                        frame,
                        json!({
                            "contents": [{ "uri": "vault://pin", "text": "secret" }],
                            "resultType": "complete", "cacheScope": "private", "ttlMs": 60000
                        }),
                    )]
                } else {
                    vec![result_for(
                        frame,
                        json!({
                            "resultType": "input_required",
                            "inputRequests": { "pin": {
                                "method": "elicitation/create",
                                "params": {
                                    "mode": "form", "message": "PIN?",
                                    "requestedSchema": { "type": "object", "properties": {} }
                                }
                            }}
                        }),
                    )]
                }
            }
            other => panic!("unexpected method {other}"),
        },
    )
    .await;
    for _ in 0..2 {
        let read = client.read_resource("vault://pin").await.expect("read");
        assert_eq!(read.contents.len(), 1);
    }
    assert_eq!(
        reads.load(SeqCst),
        4,
        "both reads asked, and both were answered"
    );
}

/// SEP-2260: "Clients receiving server-to-client requests with no associated
/// outbound request SHOULD respond with a `-32602`." The handler never runs.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unassociated_server_request_is_refused() {
    let sampler = CountingSampler::default();
    let runs = Arc::clone(&sampler.0);
    let (client, answers) = connect(
        ClientBuilder::new("edges", "1.0.0").with_sampling(sampler),
        |method, frame| match method {
            "tools/list" => vec![
                json!({
                    "jsonrpc": "2.0", "id": "srv-1", "method": "sampling/createMessage",
                    "params": { "messages": [], "maxTokens": 1 }
                }),
                result_for(
                    frame,
                    json!({ "tools": [], "resultType": "complete", "cacheScope": "private", "ttlMs": 0 }),
                ),
            ],
            other => panic!("unexpected method {other}"),
        },
    )
    .await;
    client.list_tools(None).await.expect("list");
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    let answer = loop {
        if let Some(answer) = answers.lock().unwrap().first().cloned() {
            break answer;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the client answered"
        );
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    };
    assert_eq!(answer["id"], "srv-1");
    assert_eq!(answer["error"]["code"], -32602);
    assert_eq!(runs.load(SeqCst), 0, "the sampling handler never ran");
}
