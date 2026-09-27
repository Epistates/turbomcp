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
use turbomcp_core::{JsonRpcMessage, JsonRpcRequest, JsonRpcResponse, McpRequest, RequestId};
use turbomcp_service::{CancellationToken, ProtocolError, ServeConfig, Transport, serve_with};

// ---- mock transport ----------------------------------------------------------

/// A [`Transport`] backed by two channels: `inbound` frames the driver reads,
/// `outbound` collects what it writes. Closing the inbound sender is EOF.
struct MockTransport {
    inbound: mpsc::Receiver<JsonRpcMessage>,
    outbound: mpsc::UnboundedSender<JsonRpcMessage>,
    /// When set, every `send` waits for a permit first, so a test can hold the
    /// driver inside a write and control when it completes.
    write_gate: Option<Arc<Semaphore>>,
}

impl MockTransport {
    fn new(
        inbound: mpsc::Receiver<JsonRpcMessage>,
        outbound: mpsc::UnboundedSender<JsonRpcMessage>,
    ) -> Self {
        Self {
            inbound,
            outbound,
            write_gate: None,
        }
    }

    /// Make every write wait for a permit from `gate`.
    fn gated_writes(mut self, gate: Arc<Semaphore>) -> Self {
        self.write_gate = Some(gate);
        self
    }
}

impl Transport for MockTransport {
    type Error = std::io::Error;

    async fn send(&mut self, msg: JsonRpcMessage) -> Result<(), Self::Error> {
        if let Some(gate) = &self.write_gate {
            let permit = gate
                .acquire()
                .await
                .map_err(|_| std::io::Error::other("write gate closed"))?;
            permit.forget();
        }
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
/// controls when a handler may finish), bumping `started` when called.
#[derive(Clone)]
struct GatedService {
    gate: Arc<Semaphore>,
    started: Arc<AtomicUsize>,
    /// When set, this method skips the gate and replies immediately.
    fast_method: Option<&'static str>,
}

impl Service<McpRequest> for GatedService {
    type Response = Option<JsonRpcMessage>;
    type Error = ProtocolError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: McpRequest) -> Self::Future {
        let msg = request.message;
        let gate = Arc::clone(&self.gate);
        let fast = self.fast_method;
        // Counted when the driver calls, which it does in arrival order on
        // the reader, so the count doesn't depend on when the runtime gets
        // round to polling each handler.
        if matches!(msg, JsonRpcMessage::Request(_)) {
            self.started.fetch_add(1, Ordering::SeqCst);
        }
        Box::pin(async move {
            let JsonRpcMessage::Request(req) = msg else {
                return Ok(None);
            };
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
        JsonRpcMessage::Response(r) => r.id.clone(),
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
    let transport = MockTransport::new(in_rx, out_tx);
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

/// With `max_in_flight = 1`, a second application request is rejected while
/// the first owns capacity; the reader remains available for control traffic.
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
    let transport = MockTransport::new(in_rx, out_tx);
    let config = ServeConfig {
        max_in_flight: 1,
        ..ServeConfig::default()
    };
    let driver = tokio::spawn(serve_with(transport, service, config));

    in_tx.send(request(1, "a")).await.unwrap();
    // Reserving capacity precedes polling the spawned handler. Synchronize on
    // actual entry rather than assuming an overload response implies entry.
    tokio::time::timeout(Duration::from_secs(5), async {
        while started.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("first handler entered");
    in_tx.send(request(2, "b")).await.unwrap();

    // Capacity rejects new work while the first handler is still blocked.
    let rejected = tokio::time::timeout(Duration::from_secs(1), out_rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(reply_id(&rejected), Some(RequestId::from(2i64)));
    assert!(matches!(rejected, JsonRpcMessage::Response(r) if r.error.is_some()));
    assert_eq!(started.load(Ordering::SeqCst), 1);
    gate.add_permits(1);
    let first = out_rx.recv().await.unwrap();
    assert_eq!(reply_id(&first), Some(RequestId::from(1i64)));
    assert_eq!(
        started.load(Ordering::SeqCst),
        1,
        "rejected work never executes"
    );

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
    let transport = MockTransport::new(in_rx, out_tx);
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

/// Shutdown firing while a reply is *already being written* must not throw that
/// reply away.
///
/// The frame has left the outbound channel by then, so nothing else will ever
/// send it, and abandoning it also skipped the drain entirely: the driver
/// treated an unfinished write as a possibly-partial frame, aborted every
/// in-flight handler, and returned. On a busy server that is the ordinary
/// shutdown, not a rare one, because the outbound channel is rarely empty.
///
/// Holding the write open makes the interleaving deterministic; the same window
/// is what `graceful_shutdown_drains_in_flight` hits intermittently.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_reply_being_written_when_shutdown_fires_still_goes_out() {
    let gate = Arc::new(Semaphore::new(0));
    let started = Arc::new(AtomicUsize::new(0));
    let service = GatedService {
        gate: Arc::clone(&gate),
        started: Arc::clone(&started),
        fast_method: None,
    };

    let (in_tx, in_rx) = mpsc::channel(8);
    let (out_tx, mut out_rx) = mpsc::unbounded_channel();
    let writes = Arc::new(Semaphore::new(0));
    let transport = MockTransport::new(in_rx, out_tx).gated_writes(Arc::clone(&writes));
    let shutdown = CancellationToken::new();
    let config = ServeConfig {
        drain_timeout: Duration::from_secs(5),
        shutdown: shutdown.clone(),
        ..ServeConfig::default()
    };
    let driver = tokio::spawn(serve_with(transport, service, config));

    in_tx.send(request(1, "slow")).await.unwrap();
    while started.load(Ordering::SeqCst) == 0 {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    // Let the handler finish. Its reply reaches the writer, which parks on the
    // write gate, so the driver is now inside `transport.send`.
    gate.add_permits(1);
    tokio::time::sleep(Duration::from_millis(50)).await;

    shutdown.cancel();
    writes.add_permits(8);

    let reply = tokio::time::timeout(Duration::from_secs(5), out_rx.recv())
        .await
        .expect("the in-flight write should complete")
        .expect("the reply should not be dropped on the floor");
    assert_eq!(reply_id(&reply), Some(RequestId::from(1i64)));
    driver.await.unwrap().expect("graceful shutdown is clean");
}

/// The drain holds under load, not just for one request: every handler that
/// finishes inside the window gets its reply out, and the driver still returns.
///
/// One in-flight request is the easy case. The defect this guards against only
/// needed the outbound channel to be non-empty when shutdown fired, which is
/// the normal state of a busy server and the hard case to reason about.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_shutdown_under_load_drains_every_reply() {
    const IN_FLIGHT: usize = 32;

    let gate = Arc::new(Semaphore::new(0));
    let started = Arc::new(AtomicUsize::new(0));
    let service = GatedService {
        gate: Arc::clone(&gate),
        started: Arc::clone(&started),
        fast_method: None,
    };

    let (in_tx, in_rx) = mpsc::channel(IN_FLIGHT);
    let (out_tx, mut out_rx) = mpsc::unbounded_channel();
    let writes = Arc::new(Semaphore::new(0));
    let transport = MockTransport::new(in_rx, out_tx).gated_writes(Arc::clone(&writes));
    let shutdown = CancellationToken::new();
    let config = ServeConfig {
        drain_timeout: Duration::from_secs(10),
        shutdown: shutdown.clone(),
        ..ServeConfig::default()
    };
    let driver = tokio::spawn(serve_with(transport, service, config));

    for id in 1..=IN_FLIGHT {
        in_tx.send(request(id as i64, "slow")).await.unwrap();
    }
    while started.load(Ordering::SeqCst) < IN_FLIGHT {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    // Release every handler while writes are still blocked, so one reply is
    // stuck in the writer and the rest queue up behind it. That is the state a
    // busy server is in when shutdown arrives, and cancelling now with an empty
    // outbound channel would test the easy path instead.
    gate.add_permits(IN_FLIGHT);
    tokio::time::sleep(Duration::from_millis(100)).await;
    shutdown.cancel();
    writes.add_permits(IN_FLIGHT);

    let mut seen = Vec::with_capacity(IN_FLIGHT);
    for _ in 0..IN_FLIGHT {
        let reply = tokio::time::timeout(Duration::from_secs(10), out_rx.recv())
            .await
            .expect("the drain should not stall")
            .expect("every in-flight reply should survive the drain");
        seen.push(reply_id(&reply).expect("replies carry an id"));
    }
    for id in 1..=IN_FLIGHT {
        let id = RequestId::from(id as i64);
        assert_eq!(
            seen.iter().filter(|seen| **seen == id).count(),
            1,
            "{id:?} should be answered exactly once"
        );
    }

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
    let transport = MockTransport::new(in_rx, out_tx);
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

/// What the driver knows about a connection travels beside each message:
/// its `ConnectionId` and the `Peer` that reaches it. Nothing in the message
/// can assert them, so a client writing the old internal `_meta` keys changes
/// nothing, and those keys pass through as the opaque user data they now are.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn driver_attaches_connection_facts_beside_the_message() {
    use std::sync::Mutex;
    use turbomcp_core::ConnectionId;
    use turbomcp_service::Peer;

    /// Records every request it is called with; replies to requests.
    #[derive(Clone)]
    struct Recorder {
        seen: Arc<Mutex<Vec<McpRequest>>>,
    }

    impl Service<McpRequest> for Recorder {
        type Response = Option<JsonRpcMessage>;
        type Error = ProtocolError;
        type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

        fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, request: McpRequest) -> Self::Future {
            self.seen.lock().unwrap().push(request.clone());
            Box::pin(async move {
                Ok(match request.message {
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
    let transport = MockTransport::new(in_rx, out_tx);
    let driver = tokio::spawn(serve_with(transport, service, ServeConfig::default()));

    let forged = serde_json::json!({
        "_meta": {
            "io.turbomcp.internal/connectionId": "forged-conn",
            "com.acme/tenant": "t-1",
        }
    });
    in_tx
        .send(JsonRpcRequest::new(1, "tools/list", Some(forged)).into())
        .await
        .unwrap();
    let _reply = out_rx.recv().await.unwrap();

    {
        let requests = seen.lock().unwrap();
        let request = &requests[0];
        let conn = request
            .extensions
            .get::<ConnectionId>()
            .expect("the driver attached its connection");
        assert!(conn.as_str().starts_with("conn-"), "{conn}");
        let peer = request.extensions.get::<Peer>().expect("and its peer");
        assert_eq!(peer.id(), conn);
        assert!(peer.is_open());
        let JsonRpcMessage::Request(req) = &request.message else {
            panic!("expected the request");
        };
        assert_eq!(
            req.params.as_ref().unwrap()["_meta"]["com.acme/tenant"],
            "t-1",
            "user meta survives"
        );
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

    impl Service<McpRequest> for PanicOnBoom {
        type Response = Option<JsonRpcMessage>;
        type Error = ProtocolError;
        type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

        fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, request: McpRequest) -> Self::Future {
            let msg = request.message;
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
    let transport = MockTransport::new(in_rx, out_tx);
    let driver = tokio::spawn(serve_with(transport, PanicOnBoom, ServeConfig::default()));

    in_tx.send(request(1, "boom")).await.unwrap();
    let reply = tokio::time::timeout(Duration::from_secs(5), out_rx.recv())
        .await
        .expect("a panicking handler must still answer")
        .unwrap();
    let JsonRpcMessage::Response(r) = &reply else {
        panic!("expected a response, got {reply:?}");
    };
    assert_eq!(r.id, Some(RequestId::from(1i64)));
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

/// "The Server MUST reply with a Response, except for in the case of
/// Notifications": a service that fails a request still answers it, with the
/// request's id. It used to be logged and dropped, so the peer waited out its
/// own timeout (an evicted legacy session on stdio did exactly this).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failing_service_still_answers_the_request() {
    #[derive(Clone)]
    struct Forgetful;

    impl Service<McpRequest> for Forgetful {
        type Response = Option<JsonRpcMessage>;
        type Error = ProtocolError;
        type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

        fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, request: McpRequest) -> Self::Future {
            let _msg = request.message;
            Box::pin(async { Err(ProtocolError::UnknownSession("evicted".into())) })
        }
    }

    let (in_tx, in_rx) = mpsc::channel(8);
    let (out_tx, mut out_rx) = mpsc::unbounded_channel();
    let driver = tokio::spawn(serve_with(
        MockTransport::new(in_rx, out_tx),
        Forgetful,
        ServeConfig::default(),
    ));

    in_tx.send(request(9, "tools/list")).await.unwrap();
    let reply = tokio::time::timeout(Duration::from_secs(5), out_rx.recv())
        .await
        .expect("a failed request is still answered")
        .unwrap();
    let JsonRpcMessage::Response(r) = &reply else {
        panic!("expected a response, got {reply:?}");
    };
    assert_eq!(r.id, Some(RequestId::from(9i64)));
    assert_eq!(
        r.error.as_ref().expect("an error").code,
        turbomcp_core::codes::NO_ACTIVE_SESSION
    );

    drop(in_tx);
    driver.await.unwrap().expect("clean shutdown on EOF");
}

/// The service sees frames in the order they arrived. Each used to be called
/// from its own spawned task, in whatever order the runtime ran them.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_service_is_called_in_arrival_order() {
    #[derive(Clone)]
    struct Recorder(Arc<std::sync::Mutex<Vec<RequestId>>>);

    impl Service<McpRequest> for Recorder {
        type Response = Option<JsonRpcMessage>;
        type Error = ProtocolError;
        type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

        fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, request: McpRequest) -> Self::Future {
            let msg = request.message;
            if let JsonRpcMessage::Request(req) = &msg {
                self.0.lock().unwrap().push(req.id.clone());
            }
            Box::pin(async { Ok(None) })
        }
    }

    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let (in_tx, in_rx) = mpsc::channel(512);
    let (out_tx, _out_rx) = mpsc::unbounded_channel();
    let driver = tokio::spawn(serve_with(
        MockTransport::new(in_rx, out_tx),
        Recorder(Arc::clone(&seen)),
        ServeConfig::default(),
    ));
    for id in 0..500 {
        in_tx.send(request(id, "x")).await.unwrap();
    }
    drop(in_tx);
    driver.await.unwrap().expect("clean shutdown on EOF");

    let expected: Vec<RequestId> = (0..500).map(RequestId::from).collect();
    assert_eq!(*seen.lock().unwrap(), expected);
}

/// A handler that floods its connection with notifications doesn't starve the
/// reader. When outbound was polled first, a queue that never emptied meant
/// `recv` never ran, so the peer's next frame (in practice, its
/// `notifications/cancelled` for the flooding handler) sat unread.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_notification_flood_does_not_starve_the_reader() {
    use std::sync::atomic::AtomicBool;
    use turbomcp_core::JsonRpcNotification;
    use turbomcp_service::Peer;

    /// `flood` pushes notifications through its peer until `stop`; anything
    /// else is answered at once.
    #[derive(Clone)]
    struct Flooder {
        stop: Arc<AtomicBool>,
    }

    impl Service<McpRequest> for Flooder {
        type Response = Option<JsonRpcMessage>;
        type Error = ProtocolError;
        type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

        fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, request: McpRequest) -> Self::Future {
            let stop = Arc::clone(&self.stop);
            let peer = request.extensions.get::<Peer>().cloned();
            Box::pin(async move {
                let JsonRpcMessage::Request(req) = request.message else {
                    return Ok(None);
                };
                if req.method == "flood" {
                    let peer = peer.expect("the driver attaches a peer");
                    while !stop.load(Ordering::SeqCst) {
                        let note = JsonRpcNotification::new("notifications/message", None);
                        if peer.send(note.into()).await.is_err() {
                            break;
                        }
                    }
                }
                Ok(Some(
                    JsonRpcResponse::success(req.id, serde_json::json!({})).into(),
                ))
            })
        }
    }

    // Writes slower than the handler produces, so the outbound queue never
    // empties: the condition that starved the reader.
    let pace = Arc::new(Semaphore::new(0));
    let pacer = {
        let pace = Arc::clone(&pace);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(1)).await;
                pace.add_permits(1);
            }
        })
    };
    let stop = Arc::new(AtomicBool::new(false));
    let (in_tx, in_rx) = mpsc::channel(8);
    let (out_tx, mut out_rx) = mpsc::unbounded_channel();
    let driver = tokio::spawn(serve_with(
        MockTransport::new(in_rx, out_tx).gated_writes(pace),
        Flooder {
            stop: Arc::clone(&stop),
        },
        ServeConfig {
            max_in_flight: 16,
            ..ServeConfig::default()
        },
    ));

    in_tx.send(request(1, "flood")).await.unwrap();
    // Wait for the flood to be under way before the frame it must not starve.
    assert!(matches!(
        out_rx.recv().await,
        Some(JsonRpcMessage::Notification(_))
    ));
    in_tx.send(request(2, "ping")).await.unwrap();

    let answered = tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(msg) = out_rx.recv().await {
            if reply_id(&msg) == Some(RequestId::from(2)) {
                return;
            }
        }
        panic!("the driver stopped writing");
    })
    .await;
    stop.store(true, Ordering::SeqCst);
    assert!(answered.is_ok(), "the ping was never read during the flood");

    drop(in_tx);
    tokio::spawn(async move { while out_rx.recv().await.is_some() {} });
    driver.await.unwrap().expect("clean shutdown on EOF");
    pacer.abort();
}
