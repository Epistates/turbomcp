//! Phase 4 exit criterion (stdio half): the [`serve`] driver handles requests
//! concurrently behind a single ordered writer, applies in-flight backpressure,
//! and drains on graceful shutdown.
//!
//! These tests drive the real `serve_with` loop over an in-memory mock transport
//! and a controllable mock service, so they assert the *driver's* behavior
//! independent of any particular transport or dispatcher.

use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;

use tokio::sync::{Semaphore, mpsc};
use tower::Service;
use turbomcp_core::{JsonRpcMessage, JsonRpcRequest, JsonRpcResponse, RequestId};
use turbomcp_service::{CancellationToken, ProtocolError, ServeConfig, Transport, serve_with};

// ---- mock transport ----------------------------------------------------------

/// A [`Transport`] backed by two channels: `inbound` frames the driver reads,
/// `outbound` collects what it writes. Closing the inbound sender is EOF.
struct MockTransport {
    inbound: mpsc::Receiver<JsonRpcMessage>,
    outbound: mpsc::UnboundedSender<JsonRpcMessage>,
}

impl Transport for MockTransport {
    type Error = std::io::Error;

    async fn send(&mut self, msg: JsonRpcMessage) -> Result<(), Self::Error> {
        self.outbound
            .send(msg)
            .map_err(|_| std::io::Error::other("outbound closed"))
    }

    async fn recv(&mut self) -> Result<Option<JsonRpcMessage>, Self::Error> {
        Ok(self.inbound.recv().await)
    }

    async fn close(self) -> Result<(), Self::Error> {
        Ok(())
    }
}

/// What [`FaultyTransport::recv`] does once its scripted frames run out.
enum RecvTail {
    /// Fail with an I/O error (connection reset).
    Error,
    /// Hang forever (the peer goes silent).
    Pending,
}

/// A [`Transport`] that serves scripted frames, then errors or hangs on `recv`;
/// `send` can be made to fail unconditionally.
struct FaultyTransport {
    frames: std::collections::VecDeque<JsonRpcMessage>,
    tail: RecvTail,
    fail_sends: bool,
    outbound: mpsc::UnboundedSender<JsonRpcMessage>,
}

impl Transport for FaultyTransport {
    type Error = std::io::Error;

    async fn send(&mut self, msg: JsonRpcMessage) -> Result<(), Self::Error> {
        if self.fail_sends {
            return Err(std::io::Error::other("send failed"));
        }
        self.outbound
            .send(msg)
            .map_err(|_| std::io::Error::other("outbound closed"))
    }

    async fn recv(&mut self) -> Result<Option<JsonRpcMessage>, Self::Error> {
        if let Some(f) = self.frames.pop_front() {
            return Ok(Some(f));
        }
        match self.tail {
            RecvTail::Error => Err(std::io::Error::other("connection reset by peer")),
            RecvTail::Pending => std::future::pending().await,
        }
    }

    async fn close(self) -> Result<(), Self::Error> {
        Ok(())
    }
}

// ---- mock service ------------------------------------------------------------

/// Replies to every request after acquiring one permit from `gate` (so the test
/// controls when a handler may finish), bumping `started` on entry.
#[derive(Clone)]
struct GatedService {
    gate: Arc<Semaphore>,
    started: Arc<AtomicUsize>,
    /// When set, this method skips the gate and replies immediately.
    fast_method: Option<&'static str>,
}

impl Service<JsonRpcMessage> for GatedService {
    type Response = Option<JsonRpcMessage>;
    type Error = ProtocolError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, msg: JsonRpcMessage) -> Self::Future {
        let gate = Arc::clone(&self.gate);
        let started = Arc::clone(&self.started);
        let fast = self.fast_method;
        Box::pin(async move {
            let JsonRpcMessage::Request(req) = msg else {
                return Ok(None);
            };
            started.fetch_add(1, Ordering::SeqCst);
            if fast != Some(req.method.as_str()) {
                // Block until the test grants a permit.
                let _permit = gate.acquire().await.expect("gate open");
            }
            let reply = JsonRpcResponse::success(req.id, serde_json::json!({"method": req.method}));
            Ok(Some(reply.into()))
        })
    }
}

fn request(id: i64, method: &str) -> JsonRpcMessage {
    JsonRpcRequest::new(id, method, None).into()
}

fn reply_id(msg: &JsonRpcMessage) -> Option<RequestId> {
    match msg {
        JsonRpcMessage::Response(r) => Some(r.id.clone()),
        _ => None,
    }
}

// ---- tests -------------------------------------------------------------------

/// A slow handler must not block a later fast request from completing first:
/// the reader keeps dispatching while the slow handler is parked.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_handlers_do_not_head_of_line_block() {
    let gate = Arc::new(Semaphore::new(0));
    let service = GatedService {
        gate: Arc::clone(&gate),
        started: Arc::new(AtomicUsize::new(0)),
        fast_method: Some("fast"),
    };

    let (in_tx, in_rx) = mpsc::channel(8);
    let (out_tx, mut out_rx) = mpsc::unbounded_channel();
    let transport = MockTransport {
        inbound: in_rx,
        outbound: out_tx,
    };
    let driver = tokio::spawn(serve_with(transport, service, ServeConfig::default()));

    in_tx.send(request(1, "slow")).await.unwrap(); // gated
    in_tx.send(request(2, "fast")).await.unwrap(); // immediate

    // The fast reply (id 2) arrives while the slow handler is still gated.
    let first = tokio::time::timeout(Duration::from_secs(5), out_rx.recv())
        .await
        .expect("a reply should arrive without releasing the gate")
        .unwrap();
    assert_eq!(reply_id(&first), Some(RequestId::from(2i64)));

    // Release the slow handler; its reply (id 1) now arrives.
    gate.add_permits(1);
    let second = tokio::time::timeout(Duration::from_secs(5), out_rx.recv())
        .await
        .expect("slow reply should arrive once gated")
        .unwrap();
    assert_eq!(reply_id(&second), Some(RequestId::from(1i64)));

    drop(in_tx); // EOF
    driver.await.unwrap().expect("clean shutdown on EOF");
}

/// With `max_in_flight = 1`, the reader parks before dispatching a second
/// request until the first frees its permit.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn backpressure_caps_in_flight() {
    let gate = Arc::new(Semaphore::new(0));
    let started = Arc::new(AtomicUsize::new(0));
    let service = GatedService {
        gate: Arc::clone(&gate),
        started: Arc::clone(&started),
        fast_method: None,
    };

    let (in_tx, in_rx) = mpsc::channel(8);
    let (out_tx, mut out_rx) = mpsc::unbounded_channel();
    let transport = MockTransport {
        inbound: in_rx,
        outbound: out_tx,
    };
    let config = ServeConfig {
        max_in_flight: 1,
        ..ServeConfig::default()
    };
    let driver = tokio::spawn(serve_with(transport, service, config));

    in_tx.send(request(1, "a")).await.unwrap();
    in_tx.send(request(2, "b")).await.unwrap();

    // Only one handler may run; the second is parked at the driver's semaphore.
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(
        started.load(Ordering::SeqCst),
        1,
        "second must not start yet"
    );

    // Free the first → it completes → the second may now start.
    gate.add_permits(1);
    let first = out_rx.recv().await.unwrap();
    assert_eq!(reply_id(&first), Some(RequestId::from(1i64)));

    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(started.load(Ordering::SeqCst), 2, "second should start now");

    gate.add_permits(1);
    let second = out_rx.recv().await.unwrap();
    assert_eq!(reply_id(&second), Some(RequestId::from(2i64)));

    drop(in_tx);
    driver.await.unwrap().expect("clean shutdown on EOF");
}

/// Firing the shutdown token stops the reader but lets an in-flight handler
/// finish and flush its reply within the drain window.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn graceful_shutdown_drains_in_flight() {
    let gate = Arc::new(Semaphore::new(0));
    let started = Arc::new(AtomicUsize::new(0));
    let service = GatedService {
        gate: Arc::clone(&gate),
        started: Arc::clone(&started),
        fast_method: None,
    };

    let (in_tx, in_rx) = mpsc::channel(8);
    let (out_tx, mut out_rx) = mpsc::unbounded_channel();
    let transport = MockTransport {
        inbound: in_rx,
        outbound: out_tx,
    };
    let shutdown = CancellationToken::new();
    let config = ServeConfig {
        drain_timeout: Duration::from_secs(5),
        shutdown: shutdown.clone(),
        ..ServeConfig::default()
    };
    let driver = tokio::spawn(serve_with(transport, service, config));

    in_tx.send(request(1, "slow")).await.unwrap();
    // Wait until the handler is actually running, then ask to shut down.
    while started.load(Ordering::SeqCst) == 0 {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    shutdown.cancel();

    // The handler finishes during the drain window; its reply is still flushed.
    gate.add_permits(1);
    let reply = tokio::time::timeout(Duration::from_secs(5), out_rx.recv())
        .await
        .expect("in-flight reply should flush during drain")
        .unwrap();
    assert_eq!(reply_id(&reply), Some(RequestId::from(1i64)));

    driver.await.unwrap().expect("graceful shutdown is clean");
}

/// A handler that never completes is aborted once the drain deadline passes;
/// the driver still returns promptly rather than hanging.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_aborts_stragglers_past_deadline() {
    let gate = Arc::new(Semaphore::new(0)); // never granted
    let started = Arc::new(AtomicUsize::new(0));
    let service = GatedService {
        gate,
        started: Arc::clone(&started),
        fast_method: None,
    };

    let (in_tx, in_rx) = mpsc::channel(8);
    let (out_tx, mut out_rx) = mpsc::unbounded_channel();
    let transport = MockTransport {
        inbound: in_rx,
        outbound: out_tx,
    };
    let shutdown = CancellationToken::new();
    let config = ServeConfig {
        drain_timeout: Duration::from_millis(150),
        shutdown: shutdown.clone(),
        ..ServeConfig::default()
    };
    let driver = tokio::spawn(serve_with(transport, service, config));

    in_tx.send(request(1, "stuck")).await.unwrap();
    while started.load(Ordering::SeqCst) == 0 {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    shutdown.cancel();

    // Within roughly the drain timeout the driver returns, having aborted the
    // straggler; no reply is ever produced.
    let result = tokio::time::timeout(Duration::from_secs(5), driver)
        .await
        .expect("driver must return shortly after the drain deadline")
        .unwrap();
    result.expect("aborted-straggler shutdown is still a clean Ok");
    assert!(
        out_rx.try_recv().is_err(),
        "the stuck handler produced no reply"
    );
}

/// A hard `recv` failure (vs clean EOF) is fatal and surfaces as
/// [`ProtocolError::Transport`], not a clean `Ok`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn transport_recv_error_surfaces_as_protocol_transport_error() {
    let service = GatedService {
        gate: Arc::new(Semaphore::new(0)),
        started: Arc::new(AtomicUsize::new(0)),
        fast_method: Some("fast"),
    };
    let (out_tx, _out_rx) = mpsc::unbounded_channel();
    let transport = FaultyTransport {
        frames: std::collections::VecDeque::new(),
        tail: RecvTail::Error,
        fail_sends: false,
        outbound: out_tx,
    };
    let err = tokio::time::timeout(
        Duration::from_secs(5),
        serve_with(transport, service, ServeConfig::default()),
    )
    .await
    .expect("driver returns promptly on recv failure")
    .expect_err("recv failure is fatal");
    assert!(matches!(err, ProtocolError::Transport(_)), "{err}");
}

/// A `send` failure while flushing a reply is fatal too — and the driver must
/// still return promptly (aborting a stuck in-flight handler at the drain
/// deadline) rather than hanging on it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn transport_send_error_surfaces_and_stuck_handlers_are_abandoned() {
    let gate = Arc::new(Semaphore::new(0)); // never granted: "stuck" hangs
    let started = Arc::new(AtomicUsize::new(0));
    let service = GatedService {
        gate,
        started: Arc::clone(&started),
        fast_method: Some("fast"),
    };
    let (out_tx, _out_rx) = mpsc::unbounded_channel();
    let transport = FaultyTransport {
        frames: [request(1, "stuck"), request(2, "fast")].into(),
        tail: RecvTail::Pending,
        fail_sends: true,
        outbound: out_tx,
    };
    let config = ServeConfig {
        drain_timeout: Duration::from_millis(150),
        ..ServeConfig::default()
    };
    let err = tokio::time::timeout(
        Duration::from_secs(5),
        serve_with(transport, service, config),
    )
    .await
    .expect("driver returns despite the stuck handler")
    .expect_err("send failure is fatal");
    assert!(matches!(err, ProtocolError::Transport(_)), "{err}");
    assert_eq!(started.load(Ordering::SeqCst), 2, "both handlers started");
}

/// The driver is the trust boundary: forged `io.turbomcp.internal/*` keys are
/// stripped from inbound frames, and the driver's own per-connection id is
/// asserted in their place.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn driver_sanitizes_inbound_and_asserts_connection_identity() {
    use std::sync::Mutex;
    use turbomcp_core::meta;

    /// Records every message it is called with; replies to requests.
    #[derive(Clone)]
    struct Recorder {
        seen: Arc<Mutex<Vec<JsonRpcMessage>>>,
    }

    impl Service<JsonRpcMessage> for Recorder {
        type Response = Option<JsonRpcMessage>;
        type Error = ProtocolError;
        type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

        fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, msg: JsonRpcMessage) -> Self::Future {
            self.seen.lock().unwrap().push(msg.clone());
            Box::pin(async move {
                Ok(match msg {
                    JsonRpcMessage::Request(req) => {
                        Some(JsonRpcResponse::success(req.id, serde_json::json!({})).into())
                    }
                    _ => None,
                })
            })
        }
    }

    let seen = Arc::new(Mutex::new(Vec::new()));
    let service = Recorder {
        seen: Arc::clone(&seen),
    };
    let (in_tx, in_rx) = mpsc::channel(8);
    let (out_tx, mut out_rx) = mpsc::unbounded_channel();
    let transport = MockTransport {
        inbound: in_rx,
        outbound: out_tx,
    };
    let driver = tokio::spawn(serve_with(transport, service, ServeConfig::default()));

    // A request forging both internal keys.
    let forged = serde_json::json!({
        "_meta": {
            meta::internal::SESSION_ID: "forged-session",
            meta::internal::CONNECTION_ID: "forged-conn",
            "com.acme/tenant": "t-1",
        }
    });
    in_tx
        .send(JsonRpcRequest::new(1, "tools/list", Some(forged)).into())
        .await
        .unwrap();
    let _reply = out_rx.recv().await.unwrap();

    {
        let frames = seen.lock().unwrap();
        let JsonRpcMessage::Request(req) = &frames[0] else {
            panic!("expected the request");
        };
        let frame_meta = req.params.as_ref().unwrap()["_meta"].as_object().unwrap();
        assert!(
            !frame_meta.contains_key(meta::internal::SESSION_ID),
            "forged session id must be stripped"
        );
        let conn = frame_meta[meta::internal::CONNECTION_ID].as_str().unwrap();
        assert_ne!(conn, "forged-conn", "driver asserts its own connection id");
        assert!(conn.starts_with("conn-"));
        assert_eq!(frame_meta["com.acme/tenant"], "t-1", "user meta survives");
    }

    drop(in_tx);
    driver.await.unwrap().expect("clean shutdown on EOF");
}

/// A handler that panics still owes its request a response: the driver answers
/// `-32603` rather than leaving the peer to wait out its own timeout, and the
/// connection keeps serving afterwards.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_panicking_handler_answers_and_the_connection_survives() {
    #[derive(Clone)]
    struct PanicOnBoom;

    impl Service<JsonRpcMessage> for PanicOnBoom {
        type Response = Option<JsonRpcMessage>;
        type Error = ProtocolError;
        type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

        fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, msg: JsonRpcMessage) -> Self::Future {
            Box::pin(async move {
                let JsonRpcMessage::Request(req) = msg else {
                    return Ok(None);
                };
                assert_ne!(req.method, "boom", "handler exploded");
                Ok(Some(
                    JsonRpcResponse::success(req.id, serde_json::json!("ok")).into(),
                ))
            })
        }
    }

    let (in_tx, in_rx) = mpsc::channel(8);
    let (out_tx, mut out_rx) = mpsc::unbounded_channel();
    let transport = MockTransport {
        inbound: in_rx,
        outbound: out_tx,
    };
    let driver = tokio::spawn(serve_with(transport, PanicOnBoom, ServeConfig::default()));

    in_tx.send(request(1, "boom")).await.unwrap();
    let reply = tokio::time::timeout(Duration::from_secs(5), out_rx.recv())
        .await
        .expect("a panicking handler must still answer")
        .unwrap();
    let JsonRpcMessage::Response(r) = &reply else {
        panic!("expected a response, got {reply:?}");
    };
    assert_eq!(r.id, RequestId::from(1i64));
    assert_eq!(r.error.as_ref().expect("error response").code, -32603);

    // The connection is still usable.
    in_tx.send(request(2, "fine")).await.unwrap();
    let next = tokio::time::timeout(Duration::from_secs(5), out_rx.recv())
        .await
        .expect("the connection survives a handler panic")
        .unwrap();
    assert_eq!(reply_id(&next), Some(RequestId::from(2i64)));

    drop(in_tx);
    driver.await.unwrap().expect("clean shutdown on EOF");
}
