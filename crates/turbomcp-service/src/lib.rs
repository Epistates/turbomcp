//! # turbomcp-service
//!
//! The service layer: the `tower`-shaped protocol seam, the transport trait,
//! and the shared RPC middleware that sits between them.
//!
//! - [`McpService`] — the protocol seam. Every server, every middleware layer,
//!   reduces to `tower::Service<JsonRpcMessage, Response = Option<JsonRpcMessage>,
//!   Error = ProtocolError>`. Notifications produce `None`.
//! - [`Transport`] — a bidirectional `JsonRpcMessage` channel (stdio, HTTP, WS).
//! - [`ProtocolError`] — the service/transport boundary error, with the
//!   canonical [`mcp_to_jsonrpc_error`] mapping for user errors.
//! - [`TracingLayer`] — the first shared RPC middleware.
//!
//! User errors (`McpError`) are *not* `ProtocolError`s: they become JSON-RPC
//! error responses inside the `Ok` arm. `ProtocolError` is for parse failures,
//! version mismatches, and dead transports.
#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod auth;
mod error;
pub mod mcp_headers;
mod middleware;
pub mod outbound;
mod ratelimit;
mod serve;
mod session;
mod transport;

pub use auth::{AuthDecision, AuthFuture, HttpAuthenticator};
pub use error::{ProtocolError, mcp_to_jsonrpc_error, mcp_to_jsonrpc_error_for};
pub use middleware::{Tracing, TracingLayer};
pub use ratelimit::{GovernorRateLimiter, RateKey, RateLimiter};
pub use serve::{ServeConfig, serve, serve_with};
pub use session::{SessionTerminator, TerminateFuture};
pub use transport::Transport;

pub use tokio_util::sync::CancellationToken;

use turbomcp_core::JsonRpcMessage;

/// The protocol seam, as a marker trait over the canonical `tower::Service`
/// shape. Blanket-implemented: anything with the right `Service` signature *is*
/// an `McpService`, so users never implement this directly.
pub trait McpService:
    tower::Service<JsonRpcMessage, Response = Option<JsonRpcMessage>, Error = ProtocolError>
    + Send
    + 'static
{
}

impl<T> McpService for T where
    T: tower::Service<JsonRpcMessage, Response = Option<JsonRpcMessage>, Error = ProtocolError>
        + Send
        + 'static
{
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::task::{Context, Poll};
    use tower::{Service, ServiceExt};
    use turbomcp_core::{JsonRpcRequest, JsonRpcResponse};

    /// A trivial echo service: replies to requests, drops notifications.
    #[derive(Clone)]
    struct Echo;

    impl Service<JsonRpcMessage> for Echo {
        type Response = Option<JsonRpcMessage>;
        type Error = ProtocolError;
        type Future = std::future::Ready<Result<Self::Response, Self::Error>>;

        fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, req: JsonRpcMessage) -> Self::Future {
            let reply = match req {
                JsonRpcMessage::Request(r) => Some(
                    JsonRpcResponse::success(r.id, serde_json::json!({"echo": r.method})).into(),
                ),
                _ => None,
            };
            std::future::ready(Ok(reply))
        }
    }

    fn assert_mcp_service<S: McpService>(_: &S) {}

    #[tokio::test]
    async fn blanket_impl_and_tracing_layer_compose() {
        use tower::Layer;
        let svc = TracingLayer.layer(Echo);
        assert_mcp_service(&svc);

        let mut svc = svc;
        let req: JsonRpcMessage = JsonRpcRequest::new(7, "tools/list", None).into();
        let resp = svc.ready().await.unwrap().call(req).await.unwrap();
        match resp {
            Some(JsonRpcMessage::Response(r)) => {
                assert_eq!(r.result.unwrap()["echo"], "tools/list");
            }
            other => panic!("expected response, got {other:?}"),
        }
    }

    #[test]
    fn protocol_error_codes() {
        assert_eq!(ProtocolError::Parse("x".into()).jsonrpc_code(), -32700);
        assert_eq!(
            ProtocolError::UnsupportedVersion {
                requested: None,
                supported: vec![]
            }
            .jsonrpc_code(),
            -32004
        );
    }
}
