//! Bidirectional session and cancellation abstractions.
//!
//! The [`McpSession`] trait represents an open transport channel capable of
//! issuing server-to-client JSON-RPC requests and notifications. It is the
//! minimal plumbing that makes sampling (`sampling/createMessage`) and
//! elicitation (`elicitation/create`) reachable from request handlers.
//!
//! Keeping the trait here (rather than in `turbomcp-server`) lets
//! [`crate::context::RequestContext`] expose `sample()` / `elicit_*()` /
//! `notify_client()` directly, so `#[tool]` / `#[resource]` / `#[prompt]`
//! bodies — which receive `&RequestContext` — can use them.
//!
//! The traits use `Pin<Box<dyn Future>>` returns instead of `async fn` so they
//! stay object-safe and free of the `async-trait` macro (which would drag a
//! tokio dependency into `no_std` builds).

use alloc::boxed::Box;
use core::fmt::Debug;
use core::future::Future;
use core::pin::Pin;

use serde_json::Value;
use turbomcp_types::{ClientCapabilities, ProtocolVersion};

use crate::error::McpResult;
use crate::marker::{MaybeSend, MaybeSync};

/// Future returned by [`McpSession`] methods.
///
/// Boxed so the trait stays object-safe (`Arc<dyn McpSession>` is the intended
/// storage shape). On native targets the future must be `Send`; WASM drops
/// the `Send` bound. (Can't write `+ MaybeSend` on a `dyn` — `MaybeSend` isn't
/// an auto trait — so we branch the type alias.)
#[cfg(not(target_arch = "wasm32"))]
pub type SessionFuture<'a, T> = Pin<Box<dyn Future<Output = McpResult<T>> + Send + 'a>>;

/// Future returned by [`McpSession`] methods (WASM variant, no `Send` bound).
#[cfg(target_arch = "wasm32")]
pub type SessionFuture<'a, T> = Pin<Box<dyn Future<Output = McpResult<T>> + 'a>>;

/// Bidirectional session handle.
///
/// Implementations are provided by the server transports (STDIO, HTTP, WS,
/// TCP, Unix, channel). Handlers obtain an `Arc<dyn McpSession>` via
/// [`crate::context::RequestContext::session`] — populated by the server
/// dispatcher before a request is routed.
///
/// # Example
///
/// ```rust,ignore
/// // Inside a #[tool] body:
/// async fn ask(&self, ctx: &RequestContext) -> McpResult<String> {
///     let approval = ctx.elicit_form("Allow write?", schema).await?;
///     // ...
/// }
/// ```
pub trait McpSession: Debug + MaybeSend + MaybeSync {
    /// Client capabilities captured during MCP initialization, when known.
    ///
    /// Transport-provided sessions should return `Some` after a successful
    /// initialize handshake so handler helpers can enforce server-initiated
    /// request capability requirements.
    fn client_capabilities<'a>(&'a self) -> SessionFuture<'a, Option<ClientCapabilities>> {
        Box::pin(async move { Ok(None) })
    }

    /// Protocol version negotiated for this session, when known.
    ///
    /// Inbound requests are already version-adapted by the router, but a
    /// server-initiated request is built by handler code and goes out
    /// unfiltered — so this is the only way for anything on that path to tell
    /// which wire it is writing to. A multi-version server that cannot ask
    /// will happily send 2025-11-25-only shapes to a 2025-06-18 client.
    fn protocol_version<'a>(&'a self) -> SessionFuture<'a, Option<ProtocolVersion>> {
        Box::pin(async move { Ok(None) })
    }

    /// Send a JSON-RPC request to the client and await its response.
    ///
    /// Used for round-trip operations such as `sampling/createMessage` and
    /// `elicitation/create`.
    fn call<'a>(&'a self, method: &'a str, params: Value) -> SessionFuture<'a, Value>;

    /// Send a JSON-RPC notification to the client (no response).
    ///
    /// Used for server-to-client notifications such as
    /// `notifications/tools/list_changed` or progress updates.
    fn notify<'a>(&'a self, method: &'a str, params: Value) -> SessionFuture<'a, ()>;
}

/// Cooperative-cancellation handle.
///
/// Keeps the context layer free of any specific cancellation crate.
/// When the `std` feature is enabled, `tokio_util::sync::CancellationToken`
/// gets a blanket `impl Cancellable` (see below), which is how the server
/// wires tokio-based cancellation into the unified [`crate::RequestContext`].
pub trait Cancellable: Debug + MaybeSend + MaybeSync {
    /// Returns `true` if cancellation has been requested.
    fn is_cancelled(&self) -> bool;
}

#[cfg(feature = "std")]
impl Cancellable for tokio_util::sync::CancellationToken {
    fn is_cancelled(&self) -> bool {
        tokio_util::sync::CancellationToken::is_cancelled(self)
    }
}
