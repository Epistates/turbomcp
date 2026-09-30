//! `CallOptions`: per-call timeouts, progress routed to the call that asked
//! for it, `_meta`, log level, cancellation, and the timeout pausing while
//! the client answers a server request.
#![cfg(feature = "client")]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Map, json};
use tokio::io::{BufReader, split};
use turbomcp::client::{
    CallOptions, Client, ClientBuilder, ClientError, ConnectMode, ElicitationHandler, async_trait,
};
use turbomcp::prelude::*;
use turbomcp::{CancellationToken, LineTransport, SerdeJsonCodec};

static SLOW_CANCELLED: AtomicBool = AtomicBool::new(false);

#[derive(Clone)]
struct Worker;

#[server(name = "worker", version = "1.0.0")]
impl Worker {
    /// Report `steps` progress updates, `every_ms` apart.
    #[tool]
    async fn steps(&self, ctx: &CallToolContext, steps: u32, every_ms: u64) -> String {
        for i in 1..=steps {
            tokio::time::sleep(Duration::from_millis(every_ms)).await;
            ctx.progress
                .report(f64::from(i), Some(f64::from(steps)), Some("working"))
                .await;
        }
        "done".into()
    }

    /// Sleep. The server cancels by dropping the handler, which the guard
    /// records.
    #[tool]
    async fn slow(&self, ms: u64) -> String {
        struct Unfinished;
        impl Drop for Unfinished {
            fn drop(&mut self) {
                SLOW_CANCELLED.store(true, Ordering::SeqCst);
            }
        }
        let guard = Unfinished;
        tokio::time::sleep(Duration::from_millis(ms)).await;
        std::mem::forget(guard);
        "slept".into()
    }

    /// What arrived in `_meta`.
    #[tool]
    async fn meta(&self, ctx: &CallToolContext) -> String {
        format!(
            "trace={} app={}",
            ctx.base.trace_context.is_some(),
            ctx.base
                .propagated_meta
                .get("app/tenant")
                .cloned()
                .unwrap_or_default()
        )
    }

    /// Ask the user to confirm (inline on a stateful session).
    #[tool]
    async fn confirm(&self, ctx: &CallToolContext) -> McpResult<String> {
        let outcome = ctx
            .client
            .elicit(
                "confirm",
                neutral::ElicitParams::new(
                    "Proceed?",
                    json!({ "type": "object", "properties": { "ok": { "type": "boolean" } } }),
                ),
            )
            .await?;
        Ok(format!("{:?}", outcome.action))
    }
}

/// Answers every elicitation, slowly: the user taking their time.
struct SlowUser;

#[async_trait]
impl ElicitationHandler for SlowUser {
    async fn elicit(&self, _request: neutral::ElicitParams) -> neutral::ElicitOutcome {
        tokio::time::sleep(Duration::from_millis(400)).await;
        neutral::ElicitOutcome::new(neutral::ElicitAction::Accept, args(json!({ "ok": true })))
    }
}

async fn connect(mode: ConnectMode, timeout: Duration) -> Client {
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    let (s_rd, s_wr) = split(server_io);
    tokio::spawn(Worker.into_server().serve(LineTransport::new(
        BufReader::new(s_rd),
        s_wr,
        SerdeJsonCodec,
    )));
    let (c_rd, c_wr) = split(client_io);
    ClientBuilder::new("options", "1.0.0")
        .with_connect_mode(mode)
        .with_timeout(timeout)
        .with_elicitation(SlowUser)
        .connect(LineTransport::new(
            BufReader::new(c_rd),
            c_wr,
            SerdeJsonCodec,
        ))
        .await
        .expect("handshake")
}

fn args(value: serde_json::Value) -> Map<String, serde_json::Value> {
    value.as_object().cloned().unwrap_or_default()
}

fn text(result: &neutral::CallToolResult) -> &str {
    match &result.content[0] {
        neutral::Content::Text { text, .. } => text,
        other => panic!("expected text, got {other:?}"),
    }
}

/// Each call's progress goes to that call, in the order it was sent.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn progress_goes_to_the_call_that_asked_in_order() {
    let client = connect(ConnectMode::Modern, Duration::from_secs(10)).await;
    let seen_a = Arc::new(Mutex::new(Vec::new()));
    let seen_b = Arc::new(Mutex::new(Vec::new()));
    let options = |seen: &Arc<Mutex<Vec<f64>>>| {
        let seen = Arc::clone(seen);
        CallOptions::new().on_progress(move |p| seen.lock().unwrap().push(p.progress))
    };
    let (options_a, options_b) = (options(&seen_a), options(&seen_b));
    let (a, b) = tokio::join!(
        client.call_tool_with(
            "steps",
            args(json!({ "steps": 20, "every_ms": 1 })),
            &options_a
        ),
        client.call_tool_with(
            "steps",
            args(json!({ "steps": 5, "every_ms": 3 })),
            &options_b
        ),
    );
    assert_eq!(text(&a.unwrap()), "done");
    assert_eq!(text(&b.unwrap()), "done");
    let expect = |n: u32| (1..=n).map(f64::from).collect::<Vec<_>>();
    assert_eq!(*seen_a.lock().unwrap(), expect(20));
    assert_eq!(*seen_b.lock().unwrap(), expect(5));
}

#[tokio::test]
async fn a_per_call_timeout_overrides_the_clients() {
    let client = connect(ConnectMode::Modern, Duration::from_secs(10)).await;
    let err = client
        .call_tool_with(
            "slow",
            args(json!({ "ms": 2000 })),
            &CallOptions::new().timeout(Duration::from_millis(100)),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, ClientError::Timeout), "{err}");
}

/// A long operation that keeps reporting outlives a short timeout when asked
/// to, but not the overall ceiling.
#[tokio::test]
async fn progress_restarts_the_timeout_up_to_the_ceiling() {
    let client = connect(ConnectMode::Modern, Duration::from_secs(10)).await;
    let long = || args(json!({ "steps": 8, "every_ms": 100 }));
    let short = CallOptions::new().timeout(Duration::from_millis(250));

    let err = client
        .call_tool_with("steps", long(), &short)
        .await
        .unwrap_err();
    assert!(matches!(err, ClientError::Timeout), "without resets: {err}");

    let resetting = short.clone().reset_timeout_on_progress(true);
    let ok = client
        .call_tool_with("steps", long(), &resetting)
        .await
        .unwrap();
    assert_eq!(text(&ok), "done");

    let capped = resetting.max_total_timeout(Duration::from_millis(400));
    let err = client
        .call_tool_with("steps", long(), &capped)
        .await
        .unwrap_err();
    assert!(
        matches!(err, ClientError::Timeout),
        "past the ceiling: {err}"
    );
}

#[tokio::test]
async fn cancellation_ends_the_call_and_tells_the_server() {
    let client = connect(ConnectMode::Modern, Duration::from_secs(10)).await;
    let token = CancellationToken::new();
    let trigger = token.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        trigger.cancel();
    });
    let err = client
        .call_tool_with(
            "slow",
            args(json!({ "ms": 5000 })),
            &CallOptions::new().cancel_on(token),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, ClientError::Cancelled), "{err}");
    for _ in 0..100 {
        if SLOW_CANCELLED.load(Ordering::SeqCst) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the server never heard the call was cancelled");
}

#[tokio::test]
async fn meta_reaches_the_server_and_reserved_keys_are_refused() {
    let client = connect(ConnectMode::Modern, Duration::from_secs(10)).await;
    let options = CallOptions::new()
        .meta(
            "traceparent",
            json!("00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01"),
        )
        .meta("app/tenant", json!("acme"));
    let result = client
        .call_tool_with("meta", Map::new(), &options)
        .await
        .unwrap();
    assert_eq!(text(&result), "trace=true app=\"acme\"");

    let forged = CallOptions::new().meta("io.modelcontextprotocol/protocolVersion", json!("x"));
    let err = client
        .call_tool_with("meta", Map::new(), &forged)
        .await
        .unwrap_err();
    assert!(matches!(err, ClientError::Protocol(_)), "{err}");
}

/// A stateful session sets its log level for the whole session.
#[tokio::test]
async fn a_per_call_log_level_is_refused_on_a_stateful_session() {
    let client = connect(ConnectMode::Legacy, Duration::from_secs(10)).await;
    let options = CallOptions::new().log_level(LogLevel::Debug);
    let err = client
        .call_tool_with("meta", Map::new(), &options)
        .await
        .unwrap_err();
    assert!(matches!(err, ClientError::Protocol(_)), "{err}");
}

/// On a stateful session the server asks the user in the middle of the call.
/// The call's clock used to keep running meanwhile, so a user slower than the
/// timeout lost their answer and the call.
#[tokio::test]
async fn the_timeout_waits_while_the_user_answers() {
    let client = connect(ConnectMode::Legacy, Duration::from_millis(200)).await;
    let result = client.call_tool("confirm", Map::new()).await.unwrap();
    assert_eq!(text(&result), "Accept");
}
