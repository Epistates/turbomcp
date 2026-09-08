//! In-flight request registry: `notifications/cancelled` → token.
//!
//! Each request arriving on an identified connection (see
//! [`turbomcp_core::meta::internal::CONNECTION_ID`]) is registered here for
//! the duration of its dispatch. A later `notifications/cancelled` *from the
//! same connection* fires the request's [`CancellationToken`]; the dispatcher
//! then drops the handler future and suppresses the response (cancellation
//! spec: "stop processing … not send a response").
//!
//! Keys are `(connection, request id)`, so one client can never cancel
//! another's work.
//!
//! On Streamable HTTP that scoping makes the notification unreachable by
//! construction: every POST is its own connection, so a `notifications/cancelled`
//! arriving on a POST of its own can never name a request registered under a
//! different one. That is by design rather than a gap — HTTP's cancellation
//! signal is the client closing the request's response stream, which drops the
//! call future directly (transports spec: "the server MUST treat a client
//! disconnect as cancellation of that request"), and our HTTP *client*
//! transport turns an abandoned request into exactly that disconnect. Entries
//! are still registered on HTTP, and still deregistered when dispatch ends.
//!
//! Size is bounded by the serve driver's `max_in_flight` semaphore per
//! connection (entries live exactly as long as their dispatch), so the
//! registry needs no cap of its own.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use turbomcp_core::{CancellationToken, RequestId};

type Key = (String, RequestId);

/// Shared map of currently-dispatching requests. One per dispatcher; clones of
/// the dispatcher share it via `Arc`.
#[derive(Default)]
pub(crate) struct InFlightRegistry {
    map: Mutex<HashMap<Key, CancellationToken>>,
}

impl InFlightRegistry {
    /// Track a request and hand back a guard that deregisters on drop (normal
    /// completion, cancellation, or panic alike).
    pub(crate) fn register(
        self: &Arc<Self>,
        connection: &str,
        id: &RequestId,
        token: CancellationToken,
    ) -> InFlightGuard {
        let key = (connection.to_owned(), id.clone());
        self.map
            .lock()
            .expect("inflight lock poisoned")
            .insert(key.clone(), token);
        InFlightGuard {
            registry: Arc::clone(self),
            key,
        }
    }

    /// Fire the token for `(connection, id)` if it is still in flight.
    /// Unknown ids are ignored per spec ("fire and forget").
    pub(crate) fn cancel(&self, connection: &str, id: &RequestId) -> bool {
        let key = (connection.to_owned(), id.clone());
        let found = self
            .map
            .lock()
            .expect("inflight lock poisoned")
            .get(&key)
            .cloned();
        match found {
            Some(token) => {
                token.cancel();
                true
            }
            None => false,
        }
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.map.lock().expect("inflight lock poisoned").len()
    }
}

/// Removes its registration when dropped.
pub(crate) struct InFlightGuard {
    registry: Arc<InFlightRegistry>,
    key: Key,
}

impl Drop for InFlightGuard {
    fn drop(&mut self) {
        self.registry
            .map
            .lock()
            .expect("inflight lock poisoned")
            .remove(&self.key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_is_scoped_to_the_connection() {
        let reg = Arc::new(InFlightRegistry::default());
        let token_a = CancellationToken::new();
        let token_b = CancellationToken::new();
        let id = RequestId::from(7i64);
        let _guard_a = reg.register("conn-a", &id, token_a.clone());
        let _guard_b = reg.register("conn-b", &id, token_b.clone());

        // Same request id, different connection: only conn-a's token fires.
        assert!(reg.cancel("conn-a", &id));
        assert!(token_a.is_cancelled());
        assert!(!token_b.is_cancelled());
    }

    #[test]
    fn guard_drop_deregisters_and_late_cancel_is_a_noop() {
        let reg = Arc::new(InFlightRegistry::default());
        let token = CancellationToken::new();
        let id = RequestId::from("r-1");
        let guard = reg.register("conn-1", &id, token.clone());
        assert_eq!(reg.len(), 1);
        drop(guard);
        assert_eq!(reg.len(), 0);
        assert!(!reg.cancel("conn-1", &id), "late cancel is ignored");
        assert!(!token.is_cancelled());
    }
}
