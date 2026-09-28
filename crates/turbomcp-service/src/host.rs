//! One way to serve a server, whatever the transport.
//!
//! A transport runner needs more from a server than a `Service` to call: the
//! revisions it serves (so HTTP can refuse an `MCP-Protocol-Version` it
//! doesn't), a way to end a stateful session (`DELETE`), a way to close live
//! `subscriptions/listen` streams at shutdown, and, for a connection that is
//! its own session (stdio, a WebSocket), a service that remembers that
//! connection's `initialize` handshake. [`ServerHandle`] is that bundle, and
//! [`Serve`] is a transport that knows how to run one.
//!
//! Before this, each entry point wired its own subset, and adding one layer
//! of middleware meant dropping to a lower-level call that silently lost the
//! rest: `DELETE` answered `405`, an unsupported version header was accepted,
//! listen streams were cut off at shutdown instead of closed.

use std::future::Future;
use std::sync::Arc;

use futures::future::BoxFuture;
use turbomcp_core::ProtocolVersion;

use crate::{
    CancellationToken, McpService, ProtocolError, ServeConfig, SessionTerminator, Transport,
    serve_with,
};

/// A server, as the transports that run it see it.
///
/// `turbomcp-server`'s `Server` implements this; build one with
/// `ServerBuilder::layer` or serve a builder directly with
/// `ServerBuilder::serve`.
pub trait ServerHandle: Clone + Send + Sync + 'static {
    /// The request path for transports that route sessions themselves
    /// (Streamable HTTP reads `Mcp-Session-Id`).
    type Service: McpService<Future: Send + 'static> + Clone + Sync;

    /// The request path for one connection that is its own session (stdio, a
    /// WebSocket): it tracks that connection's `initialize` handshake.
    type Connection: McpService<Future: Send + 'static> + Clone;

    /// A service for header-routed requests.
    fn service(&self) -> Self::Service;

    /// A service for one new connection. Call once per connection: the
    /// connection's stateful session ends when the last clone is dropped.
    fn connection(&self) -> Self::Connection;

    /// The revisions this server serves.
    fn supported_versions(&self) -> Vec<ProtocolVersion>;

    /// Ends a stateful session on request (HTTP `DELETE`), if the server
    /// supports that.
    fn session_terminator(&self) -> Option<Arc<dyn SessionTerminator>>;

    /// Answer every live `subscriptions/listen` with its closing response.
    /// Called at shutdown *before* transports start draining, while the
    /// streams can still carry it.
    fn close_subscriptions(&self) -> BoxFuture<'static, ()>;
}

/// A bare service is a server with nothing to wire: every revision, no
/// `DELETE`, nothing to close, and no per-connection session (so on a
/// connection that is its own session, stateful clients fail after
/// `initialize`). For tests and embeddings that bring their own; a real server
/// is `turbomcp-server`'s `Server`.
impl<S> ServerHandle for S
where
    S: McpService<Future: Send + 'static> + Clone + Sync,
{
    type Service = S;
    type Connection = S;

    fn service(&self) -> S {
        self.clone()
    }

    fn connection(&self) -> S {
        self.clone()
    }

    fn supported_versions(&self) -> Vec<ProtocolVersion> {
        ProtocolVersion::SUPPORTED.to_vec()
    }

    fn session_terminator(&self) -> Option<Arc<dyn SessionTerminator>> {
        None
    }

    fn close_subscriptions(&self) -> BoxFuture<'static, ()> {
        Box::pin(async {})
    }
}

/// A transport that can run a [`ServerHandle`]: any [`Transport`] (one
/// connection), [`Pipe`] (one connection with a [`ServeConfig`]), or a
/// network listener such as `turbomcp-transport-http`'s `Http`.
pub trait Serve {
    /// Serve `server` until the transport ends or its shutdown token fires.
    fn serve<H: ServerHandle>(
        self,
        server: H,
    ) -> impl Future<Output = Result<(), ProtocolError>> + Send;
}

/// One long-lived connection (stdio, a socket, an in-memory duplex) with the
/// [`ServeConfig`] to drive it by. A bare [`Transport`] serves with the default
/// config; wrap it in a `Pipe` to set a shutdown token, drain timeout or
/// concurrency bound.
#[derive(Debug)]
pub struct Pipe<T> {
    transport: T,
    config: ServeConfig,
}

impl<T: Transport> Pipe<T> {
    /// `transport` with the default [`ServeConfig`].
    pub fn new(transport: T) -> Self {
        Self {
            transport,
            config: ServeConfig::default(),
        }
    }

    /// Drive the connection by `config`.
    #[must_use]
    pub fn config(mut self, config: ServeConfig) -> Self {
        self.config = config;
        self
    }
}

impl<T: Transport> Serve for Pipe<T> {
    async fn serve<H: ServerHandle>(self, server: H) -> Result<(), ProtocolError> {
        let Self { transport, config } = self;
        let requested = config.shutdown.clone();
        let (driver, closing) = close_then_shut_down(&server, requested, config.drain_timeout);
        let serving = serve_with(
            transport,
            server.connection(),
            ServeConfig {
                shutdown: driver,
                ..config
            },
        );
        tokio::pin!(serving);
        tokio::select! {
            result = &mut serving => result,
            // `closing` never finishes on its own; it only cancels the driver.
            () = closing => unreachable!("the close task never completes"),
        }
    }
}

impl<T: Transport> Serve for T {
    fn serve<H: ServerHandle>(
        self,
        server: H,
    ) -> impl Future<Output = Result<(), ProtocolError>> + Send {
        Pipe::new(self).serve(server)
    }
}

/// The shutdown a transport should drive by, and the task that fires it: when
/// `requested` fires, close the server's listen streams first (bounded by
/// `budget`), then cancel the returned token. Poll the task alongside the
/// transport; it never completes.
///
/// Cancelling the transport on the same token raced the closing responses
/// against the drain, which unregisters the writer they go out on.
pub fn close_then_shut_down<H: ServerHandle>(
    server: &H,
    requested: CancellationToken,
    budget: std::time::Duration,
) -> (CancellationToken, impl Future<Output = ()> + Send + 'static) {
    let driver = CancellationToken::new();
    let fire = driver.clone();
    let server = server.clone();
    let task = async move {
        requested.cancelled().await;
        let _ = tokio::time::timeout(budget, server.close_subscriptions()).await;
        fire.cancel();
        std::future::pending::<()>().await;
    };
    (driver, task)
}
