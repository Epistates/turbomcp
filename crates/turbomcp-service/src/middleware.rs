//! Shared RPC middleware — `tower::Layer`s that wrap any
//! [`McpService`](crate::McpService) and compose identically under every
//! transport (stdio, HTTP, WebSocket).
//!
//! # Writing a layer
//!
//! An MCP middleware is an ordinary [`tower::Layer`] over
//! `Service<McpRequest, Response = Option<JsonRpcMessage>, Error =
//! ProtocolError>`, where an [`McpRequest`](turbomcp_core::McpRequest) is the
//! message plus the typed facts its transport attached. There is no MCP-specific middleware trait to learn and no
//! per-method hook list to keep in sync with the protocol: one `call`, every
//! method, every transport. [`TracingLayer`] below is the whole shape in 30
//! lines.
//!
//! Three things the frame-level seam implies:
//!
//! - **`Option<JsonRpcMessage>` responses.** `None` means "notification, no
//!   reply" — a layer that fabricates a response must only do so for
//!   [`JsonRpcMessage::Request`], which is the only variant carrying an id.
//! - **Handler errors are not `Err`.** A `#[tool]` returning `McpError` arrives
//!   as `Ok(Some(Response { error }))`. `Err(ProtocolError)` means the
//!   connection-level machinery failed. A layer that logs failures usually wants
//!   the former (see [`JsonRpcResponse::is_error`](turbomcp_core::JsonRpcResponse::is_error)).
//! - **Services are cloned per request.** The driver clones the stack for every
//!   inbound frame, so layer state must be shared (`Arc<…>`), not owned.
//!
//! To short-circuit — reject before the inner service runs — build the response
//! yourself and skip `inner.call`, with
//! `err.to_jsonrpc_error(&version)` for the version
//! [`McpRequest::protocol_version`](turbomcp_core::McpRequest::protocol_version)
//! reads, so the code matches what the dispatcher would have answered on that
//! revision rather than a hand-picked one.
//!
//! # Where a layer sits
//!
//! Add layers with `ServerBuilder::layer`; the first added is the outermost.
//! On a connection that is its own session (stdio, a WebSocket), the runtime
//! puts the session adapter outside every layer, so a layer sees a stateful
//! request with its negotiated protocol version in `_meta` and its
//! [`SessionId`](turbomcp_core::SessionId) attached. On HTTP the endpoint
//! attaches the same facts before dispatch. Either way the facts in
//! `request.extensions` (identity, connection, session) come from the
//! transport and the runtime; a client has no way to put them there.

use std::task::{Context, Poll};

use tower::{Layer, Service};
use tracing::Instrument;
use turbomcp_core::{JsonRpcMessage, McpRequest};

use crate::ProtocolError;

/// A [`tower::Layer`] that wraps each RPC in a `tracing` span carrying the
/// method name. Cheap, allocation-free (`Instrumented<S::Future>` is named, not
/// boxed), and the first link in the shared RPC stack.
#[derive(Debug, Clone, Copy, Default)]
pub struct TracingLayer;

impl<S> Layer<S> for TracingLayer {
    type Service = Tracing<S>;

    fn layer(&self, inner: S) -> Self::Service {
        Tracing { inner }
    }
}

/// The service produced by [`TracingLayer`]. See that type for details.
#[derive(Debug, Clone)]
pub struct Tracing<S> {
    inner: S,
}

impl<S> Service<McpRequest> for Tracing<S>
where
    S: Service<McpRequest, Response = Option<JsonRpcMessage>, Error = ProtocolError>,
{
    type Response = Option<JsonRpcMessage>;
    type Error = ProtocolError;
    type Future = tracing::instrument::Instrumented<S::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: McpRequest) -> Self::Future {
        let method = req.message.method().unwrap_or("(response)").to_owned();
        let span = tracing::debug_span!("mcp.rpc", method = %method);
        self.inner.call(req).instrument(span)
    }
}

/// A [`tower::Layer`] that bounds each request: one the inner service hasn't
/// answered within the deadline gets a JSON-RPC timeout error (the code its
/// caller's revision uses for [`McpError::Timeout`](turbomcp_core::McpError::Timeout)),
/// and the handler is dropped, which ends it as a client's cancellation
/// does.
///
/// `tower`'s own `Timeout` doesn't compose onto an MCP server: its error is
/// `BoxError`, where the protocol seam's is [`ProtocolError`], and a
/// timed-out request still owes its caller a response. This one keeps the
/// seam's types and answers. Notifications and responses pass through
/// unbounded: nothing waits on them.
#[derive(Debug, Clone, Copy)]
pub struct TimeoutLayer {
    deadline: std::time::Duration,
}

impl TimeoutLayer {
    /// Bound each request to `deadline`.
    #[must_use]
    pub fn new(deadline: std::time::Duration) -> Self {
        Self { deadline }
    }
}

impl<S> Layer<S> for TimeoutLayer {
    type Service = Timeout<S>;

    fn layer(&self, inner: S) -> Self::Service {
        Timeout {
            inner,
            deadline: self.deadline,
        }
    }
}

/// The service produced by [`TimeoutLayer`]. See that type for details.
#[derive(Debug, Clone)]
pub struct Timeout<S> {
    inner: S,
    deadline: std::time::Duration,
}

impl<S> Service<McpRequest> for Timeout<S>
where
    S: Service<McpRequest, Response = Option<JsonRpcMessage>, Error = ProtocolError>,
    S::Future: Send + 'static,
{
    type Response = Option<JsonRpcMessage>;
    type Error = ProtocolError;
    type Future = std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>,
    >;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: McpRequest) -> Self::Future {
        let JsonRpcMessage::Request(request) = &req.message else {
            return Box::pin(self.inner.call(req));
        };
        let id = request.id.clone();
        let version = req
            .protocol_version()
            .unwrap_or(turbomcp_core::ProtocolVersion::LATEST);
        let deadline = self.deadline;
        let call = self.inner.call(req);
        Box::pin(async move {
            match tokio::time::timeout(deadline, call).await {
                Ok(answered) => answered,
                Err(_) => {
                    let error = turbomcp_core::McpError::timeout(format!(
                        "the request took longer than {deadline:?}"
                    ));
                    Ok(Some(JsonRpcMessage::Response(
                        turbomcp_core::JsonRpcResponse::error(id, error.to_jsonrpc_error(&version)),
                    )))
                }
            }
        })
    }
}
