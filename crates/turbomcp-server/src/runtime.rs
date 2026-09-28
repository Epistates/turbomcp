//! [`Server`]: a built server plus its RPC middleware, served on any transport.
//!
//! ```no_run
//! # use turbomcp_server::{IntoServerBuilder, McpServerCore};
//! # use turbomcp_core::Implementation;
//! # use turbomcp_service::{TracingLayer, io::stdio};
//! # #[derive(Clone)] struct MyServer;
//! # impl McpServerCore for MyServer {
//! #     fn server_info(&self) -> Implementation { Implementation::new("s", "1.0") }
//! # }
//! # async fn run() -> Result<(), turbomcp_service::ProtocolError> {
//! MyServer.into_server().layer(TracingLayer).serve(stdio()).await
//! # }
//! ```
//!
//! The runtime is the only thing that wires a server to its transport, so a
//! layer of middleware no longer costs anything else: every connection that is
//! its own session (stdio, a WebSocket) gets a session adapter outside the
//! layers, HTTP gets the server's supported revisions and its `DELETE`
//! handler, and every transport closes `subscriptions/listen` streams before
//! it drains.

use std::sync::Arc;

use futures::future::BoxFuture;
use tower::ServiceBuilder;
use tower::layer::Layer;
use tower::layer::util::{Identity, Stack};
use turbomcp_core::ProtocolVersion;
use turbomcp_service::{McpService, ProtocolError, Serve, ServerHandle, SessionTerminator};

use crate::adapter::LegacySessionAdapter;
use crate::dispatcher::VersionDispatcher;
use crate::subscriptions::ServerNotifier;
use crate::traits::McpServerCore;

/// A built server and the RPC middleware around it, ready to
/// [`serve`](Self::serve) on any transport.
///
/// Get one from [`ServerBuilder::layer`](crate::ServerBuilder::layer), or
/// serve a builder directly with
/// [`ServerBuilder::serve`](crate::ServerBuilder::serve).
pub struct Server<S, L = Identity> {
    dispatcher: VersionDispatcher<S>,
    layers: ServiceBuilder<L>,
}

impl<S, L> core::fmt::Debug for Server<S, L> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Server").finish_non_exhaustive()
    }
}

impl<S: Clone, L: Clone> Clone for Server<S, L> {
    fn clone(&self) -> Self {
        Self {
            dispatcher: self.dispatcher.clone(),
            layers: self.layers.clone(),
        }
    }
}

impl<S: McpServerCore> Server<S> {
    /// Serve `dispatcher` with no middleware.
    #[must_use]
    pub fn new(dispatcher: VersionDispatcher<S>) -> Self {
        Self {
            dispatcher,
            layers: ServiceBuilder::new(),
        }
    }
}

impl<S: McpServerCore, L> Server<S, L> {
    /// Wrap the server in `layer` (a [`tower::Layer`] over the
    /// `Service<McpRequest>` seam). The first layer added is the outermost:
    /// it sees each request first and each response last.
    #[must_use]
    pub fn layer<T>(self, layer: T) -> Server<S, Stack<T, L>> {
        Server {
            dispatcher: self.dispatcher,
            layers: self.layers.layer(layer),
        }
    }

    /// The dispatcher inside, for what it hands out
    /// ([`session_terminator`](VersionDispatcher::session_terminator), …).
    #[must_use]
    pub fn dispatcher(&self) -> &VersionDispatcher<S> {
        &self.dispatcher
    }

    /// A handle for publishing change notifications (`*/list_changed`,
    /// `resources/updated`) to every live subscription.
    #[must_use]
    pub fn notifier(&self) -> ServerNotifier {
        self.dispatcher.notifier()
    }

    /// Serve on `target` until it ends or its shutdown token fires: a
    /// [`Transport`](turbomcp_service::Transport) such as `stdio()` for one
    /// connection, a [`Pipe`](turbomcp_service::Pipe) for one connection with
    /// a [`ServeConfig`](turbomcp_service::ServeConfig), or a network listener
    /// such as `turbomcp-transport-http`'s `Http`.
    ///
    /// # Errors
    /// Whatever the transport fails with.
    pub async fn serve<T: Serve>(self, target: T) -> Result<(), ProtocolError>
    where
        Self: ServerHandle,
    {
        target.serve(self).await
    }
}

impl<S, L> ServerHandle for Server<S, L>
where
    S: McpServerCore + Clone + Send + Sync + 'static,
    L: Layer<VersionDispatcher<S>> + Clone + Send + Sync + 'static,
    L::Service: McpService<Future: Send + 'static> + Clone + Sync,
{
    type Service = L::Service;
    type Connection = LegacySessionAdapter<L::Service>;

    fn service(&self) -> Self::Service {
        self.layers.service(self.dispatcher.clone())
    }

    /// The adapter goes outside the layers, so middleware sees each request
    /// with its session and negotiated version already attached.
    fn connection(&self) -> Self::Connection {
        LegacySessionAdapter::ending_with(self.service(), self.dispatcher.session_end())
    }

    fn supported_versions(&self) -> Vec<ProtocolVersion> {
        self.dispatcher.supported_versions().to_vec()
    }

    fn session_terminator(&self) -> Option<Arc<dyn SessionTerminator>> {
        Some(Arc::new(self.dispatcher.session_terminator()))
    }

    fn close_subscriptions(&self) -> BoxFuture<'static, ()> {
        let dispatcher = self.dispatcher.clone();
        Box::pin(async move { dispatcher.close_subscriptions().await })
    }
}
