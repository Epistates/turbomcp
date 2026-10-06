//! Watching stateful sessions end: the seam a session-duration metric
//! (`turbomcp-telemetry`'s `mcp.server.session.duration`) plugs into.

use std::time::Duration;

use turbomcp_core::ProtocolVersion;

/// How a stateful session ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SessionEndReason {
    /// The client ended it (Streamable HTTP's `DELETE`).
    Terminated,
    /// Its connection closed (a stdio or WebSocket session).
    Closed,
    /// It sat idle past the store's timeout.
    Expired,
}

/// A stateful session that has ended.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct EndedSession<'a> {
    /// The session's id (`Mcp-Session-Id` on HTTP).
    pub id: &'a str,
    /// From `initialize` to the end.
    pub duration: Duration,
    /// The revision the session spoke.
    pub protocol_version: &'a ProtocolVersion,
    /// How it ended.
    pub reason: SessionEndReason,
}

impl<'a> EndedSession<'a> {
    /// Session `id` on `protocol_version`, which lived `duration` and ended
    /// for `reason`.
    #[must_use]
    pub fn new(
        id: &'a str,
        duration: Duration,
        protocol_version: &'a ProtocolVersion,
        reason: SessionEndReason,
    ) -> Self {
        Self {
            id,
            duration,
            protocol_version,
            reason,
        }
    }
}

/// Told when each stateful (`2025-06-18`/`2025-11-25`) session ends.
/// `2026-07-28` is stateless: it has no sessions to end.
pub trait SessionObserver: Send + Sync + 'static {
    /// `session` ended. Called on the task that ended it: keep it quick.
    fn session_ended(&self, session: &EndedSession<'_>);
}
