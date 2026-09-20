//! Per-connection ordered writers — the server-initiated-message push seam.
//!
//! The `serve` driver's writer actor is the only thing allowed to write a
//! connection's transport (invariant 13). For server-initiated messages
//! (subscription notifications, bidirectional requests) the rest of the stack
//! needs a way to reach that writer: this table maps the driver-minted
//! connection id (see [`turbomcp_core::meta::internal::CONNECTION_ID`]) to a
//! clone of the writer's `mpsc::Sender`.
//!
//! - The `serve` driver [`register`]s its sender for the connection's lifetime
//!   (the guard unregisters before the drain phase, so the channel can close).
//! - The HTTP transport registers a per-stream sender for SSE responses under
//!   its own minted id.
//! - The dispatcher resolves [`writer`] lazily at publish time: a missing
//!   entry means the connection is gone and the subscription with it.
//!
//! Entries are process-local bookkeeping, never wire data; ids are unique per
//! process, so two connections can never collide.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use tokio::sync::mpsc;
use turbomcp_core::JsonRpcMessage;

static WRITERS: OnceLock<Mutex<HashMap<String, mpsc::Sender<JsonRpcMessage>>>> = OnceLock::new();

fn table() -> &'static Mutex<HashMap<String, mpsc::Sender<JsonRpcMessage>>> {
    WRITERS.get_or_init(Mutex::default)
}

/// Unregisters its connection's writer when dropped — and only its own.
#[must_use = "dropping the guard immediately unregisters the writer"]
#[derive(Debug)]
pub struct WriterGuard {
    connection_id: String,
    /// The sender this guard installed, so drop can tell whether the table
    /// still holds it.
    tx: mpsc::Sender<JsonRpcMessage>,
}

impl Drop for WriterGuard {
    fn drop(&mut self) {
        let mut table = table().lock().expect("outbound writer table poisoned");
        // Retract only our own registration. Not every key is unique:
        // [`session_stream_id`] is deterministic, so a reconnecting legacy GET
        // stream deliberately *replaces* the previous writer — and the old
        // guard, dropping a moment later, used to remove the new one. The
        // session then had a live SSE stream the server could no longer write
        // to, silently, for as long as the client kept it open.
        if table
            .get(&self.connection_id)
            .is_some_and(|current| current.same_channel(&self.tx))
        {
            table.remove(&self.connection_id);
        }
    }
}

/// Make `tx` the ordered outbound writer for `connection_id`, for as long as
/// the returned guard lives. Replaces any previous writer under the same id
/// (which [`session_stream_id`] relies on: one stream per session).
pub fn register(connection_id: impl Into<String>, tx: mpsc::Sender<JsonRpcMessage>) -> WriterGuard {
    let connection_id = connection_id.into();
    table()
        .lock()
        .expect("outbound writer table poisoned")
        .insert(connection_id.clone(), tx.clone());
    WriterGuard { connection_id, tx }
}

/// The ordered outbound writer for `connection_id`, if that connection is
/// still open.
#[must_use]
pub fn writer(connection_id: &str) -> Option<mpsc::Sender<JsonRpcMessage>> {
    table()
        .lock()
        .expect("outbound writer table poisoned")
        .get(connection_id)
        .cloned()
}

/// The table key for a legacy (`2025-11-25`) session's server→client stream.
///
/// The HTTP transport registers its `GET`-opened SSE stream under this key;
/// the dispatcher's subscription registry resolves legacy deliveries through
/// it first (falling back to the session's byte-pipe connection on stdio).
/// One key per session also enforces the spec's "MUST NOT broadcast the same
/// message across multiple streams": a newer GET stream replaces the older
/// registration.
#[must_use]
pub fn session_stream_id(session_id: &str) -> String {
    format!("http-get-{session_id}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_lookup_and_guard_drop() {
        let (tx, _rx) = mpsc::channel(1);
        let guard = register("test-conn-outbound", tx);
        assert!(writer("test-conn-outbound").is_some());
        assert!(writer("test-conn-other").is_none());
        drop(guard);
        assert!(writer("test-conn-outbound").is_none());
    }

    /// A reconnecting legacy GET stream registers under the *same* key, and the
    /// old guard dropping afterwards must not take the new stream with it.
    ///
    /// It did: the session kept a live SSE stream the server could no longer
    /// write to, silently, for as long as the client held it open.
    #[test]
    fn a_replaced_registration_survives_the_old_guards_drop() {
        let key = session_stream_id("sess-replace");
        let (first_tx, _first_rx) = mpsc::channel(1);
        let (second_tx, mut second_rx) = mpsc::channel(1);

        let first = register(&key, first_tx);
        let _second = register(&key, second_tx);
        drop(first);

        let still_there = writer(&key).expect("the reconnecting stream is still registered");
        still_there
            .try_send(turbomcp_core::JsonRpcNotification::new("ping", None).into())
            .expect("and it is the second stream's channel");
        assert!(second_rx.try_recv().is_ok());
    }
}
