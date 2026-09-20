//! The session-termination seam.
//!
//! The `2025-11-25` Streamable HTTP transport lets a client end a session with
//! an HTTP `DELETE` (spec §Session Management). The session table lives in the
//! server layer (`turbomcp-server`), which the HTTP transport doesn't depend
//! on — so, like [auth](crate::HttpAuthenticator), termination crosses the
//! boundary through a small `service`-level trait the server implements and the
//! transport holds behind `Arc<dyn …>`.

use std::future::Future;
use std::pin::Pin;

use turbomcp_core::ProtocolVersion;

/// Boxed future returned by [`SessionTerminator::terminate`] (keeps the trait
/// dyn-compatible).
pub type TerminateFuture<'a> = Pin<Box<dyn Future<Output = bool> + Send + 'a>>;

/// Boxed future returned by [`SessionTerminator::negotiated_version`].
pub type SessionVersionFuture<'a> =
    Pin<Box<dyn Future<Output = Option<ProtocolVersion>> + Send + 'a>>;

/// Terminates a server session by id (backs HTTP `DELETE`). Implemented by the
/// dispatcher (it drops the session state *and* its subscription routes);
/// obtained from `VersionDispatcher::session_terminator`.
pub trait SessionTerminator: Send + Sync {
    /// Terminate the session `session_id`. Returns whether it existed (the
    /// transport answers `204` vs `404` accordingly). Async because the
    /// session state may live in an external backend.
    fn terminate<'a>(&'a self, session_id: &'a str, owner: Option<&'a str>) -> TerminateFuture<'a>;

    /// Check session ownership without changing it. Implementations must match
    /// anonymous sessions only to anonymous callers.
    fn owns<'a>(&'a self, session_id: &'a str, owner: Option<&'a str>) -> TerminateFuture<'a>;

    /// The revision this session negotiated at `initialize`, if it is known.
    ///
    /// The transports spec has the server fall back to assuming a version only
    /// when it "has no other way to identify the version — for example, by
    /// relying on the protocol version negotiated during initialization". A
    /// live session *is* that other way, and it outranks the header: the
    /// handshake is what the two ends agreed on.
    ///
    /// Defaults to `None`, which leaves the header as the only signal.
    fn negotiated_version<'a>(&'a self, session_id: &'a str) -> SessionVersionFuture<'a> {
        let _ = session_id;
        Box::pin(async { None })
    }
}
