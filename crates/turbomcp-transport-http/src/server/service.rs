//! [`HttpService`]: the endpoint as a `tower` service over `http` types, for
//! mounting in an app of your own.
//!
//! Its signature names only `http`, `http-body` and `tower` types, all at 1.x
//! or tower's long-lived 0.3 service trait, so it mounts in whichever axum
//! (or hyper, or other `http`-based server) the application uses, rather than
//! only the axum this crate is built on.

use std::convert::Infallible;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{Context, Poll};

use axum::Router;
use axum::extract::ConnectInfo;
use bytes::Bytes;

/// The error a request or response body may fail with.
pub type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// The MCP endpoint as a [`tower::Service`] over [`http::Request`]s, from
/// [`service`](crate::service). Mount it in an axum app with
/// `Router::fallback_service`, or under a path prefix with
/// `Router::nest_service`.
///
/// Per-client rate limits and trusted-proxy resolution need the client's
/// address: insert it into each request's extensions as a
/// [`SocketAddr`](std::net::SocketAddr) (a one-line middleware), or serve the
/// app with axum's `into_make_service_with_connect_info::<SocketAddr>()` on
/// the same axum version this crate uses.
#[derive(Clone)]
pub struct HttpService {
    router: Router,
}

impl HttpService {
    pub(super) fn new(router: Router) -> Self {
        Self { router }
    }
}

impl core::fmt::Debug for HttpService {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("HttpService").finish_non_exhaustive()
    }
}

impl<B> tower::Service<http::Request<B>> for HttpService
where
    B: http_body::Body<Data = Bytes> + Send + 'static,
    B::Error: Into<BoxError>,
{
    type Response = http::Response<HttpBody>;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Infallible>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
        // The router does its own admission; it is always ready to take a
        // request.
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: http::Request<B>) -> Self::Future {
        let mut request = request.map(axum::body::Body::new);
        if request
            .extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .is_none()
            && let Some(peer) = request.extensions().get::<SocketAddr>().copied()
        {
            request.extensions_mut().insert(ConnectInfo(peer));
        }
        let call = tower::Service::call(&mut self.router.clone(), request);
        Box::pin(async move {
            let response = call.await?;
            Ok(response.map(HttpBody))
        })
    }
}

/// The body of an [`HttpService`] response: JSON, or a stream of
/// server-sent events.
pub struct HttpBody(axum::body::Body);

impl core::fmt::Debug for HttpBody {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("HttpBody").finish_non_exhaustive()
    }
}

impl http_body::Body for HttpBody {
    type Data = Bytes;
    type Error = BoxError;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body::Frame<Bytes>, BoxError>>> {
        Pin::new(&mut self.0)
            .poll_frame(cx)
            .map(|frame| frame.map(|frame| frame.map_err(BoxError::from)))
    }

    fn is_end_stream(&self) -> bool {
        self.0.is_end_stream()
    }

    fn size_hint(&self) -> http_body::SizeHint {
        self.0.size_hint()
    }
}
