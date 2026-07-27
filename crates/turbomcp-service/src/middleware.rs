//! Shared RPC middleware — `tower::Layer`s that wrap any
//! [`McpService`](crate::McpService) and compose identically under every
//! transport (stdio, HTTP, WebSocket).
//!
//! # Writing a layer
//!
//! An MCP middleware is an ordinary [`tower::Layer`] over
//! `Service<JsonRpcMessage, Response = Option<JsonRpcMessage>, Error =
//! ProtocolError>`. There is no MCP-specific middleware trait to learn and no
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
//! yourself and skip `inner.call`. Use
//! [`mcp_to_jsonrpc_error`](crate::mcp_to_jsonrpc_error) so the code matches the
//! rest of the SDK rather than hand-picking one.
//!
//! # Where a layer sits
//!
//! Both of these are valid and they see different things:
//!
//! ```text
//! serve_stdio(MyLayer.layer(LegacySessionAdapter::new(dispatcher)))   // outside
//! serve_stdio(LegacySessionAdapter::new(MyLayer.layer(dispatcher)))   // inside
//! ```
//!
//! Outside the adapter a layer sees the frame as the client sent it. Inside, it
//! sees the negotiated protocol version and session id that the adapter stamped
//! into `_meta` — which is what a layer keyed on protocol version needs. Either
//! way the layer is already behind the wire trust boundary: [`serve`](crate::serve)
//! (and the HTTP endpoint) sanitize forged internal `_meta` keys and assert the
//! connection's own identity *before* the service is called, so
//! `io.turbomcp.internal/*` keys are trustworthy at every layer.
//!
//! Stack several with `tower::ServiceBuilder`; the first layer added is the
//! outermost.

use std::task::{Context, Poll};

use tower::{Layer, Service};
use tracing::Instrument;
use turbomcp_core::JsonRpcMessage;

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

impl<S> Service<JsonRpcMessage> for Tracing<S>
where
    S: Service<JsonRpcMessage, Response = Option<JsonRpcMessage>, Error = ProtocolError>,
{
    type Response = Option<JsonRpcMessage>;
    type Error = ProtocolError;
    type Future = tracing::instrument::Instrumented<S::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: JsonRpcMessage) -> Self::Future {
        let method = req.method().unwrap_or("(response)").to_owned();
        let span = tracing::debug_span!("mcp.rpc", method = %method);
        self.inner.call(req).instrument(span)
    }
}
