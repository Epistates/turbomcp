//! Per-call input handlers: who answers a server's `elicitation/create`,
//! `sampling/createMessage` or `roots/list` that belongs to one call.
//!
//! SEP-2260 associates every server→client request with an originating
//! client request. A call made with its own handlers
//! ([`CallOptions::with_elicitation`](crate::CallOptions::with_elicitation)
//! and friends) gets the requests that belong to it, and the client's own
//! handlers keep answering the rest:
//!
//! - On `2026-07-28` the association is structural: the requests come back
//!   in the call's own `InputRequiredResult`.
//! - On Streamable HTTP, a request arrives on its originating call's POST
//!   stream, which the transport reports ([`RelatedRequest`]).
//! - Where nothing says (stdio, WebSocket), a request is the only in-flight
//!   call's if there is exactly one. With several in flight and per-call
//!   handlers among them, the request is answered by the client's own
//!   handler if it has one, and refused otherwise: guessing would show one
//!   caller's question to another.
//!
//! [`RelatedRequest`]: turbomcp_service::RelatedRequest

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use turbomcp_core::RequestId;

use crate::handler::ClientHandlers;

/// What a server request can be associated with.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum RouteKey {
    /// One of our requests (the stream it arrived on said so).
    Request(RequestId),
    /// A `2025-11-25` task of ours (its `related-task` metadata said so).
    Task(String),
}

/// How long a task's route stands on the connection's word alone, before
/// the call driving the task claims it.
const PROVISIONAL: Duration = Duration::from_secs(30);

/// The per-call handlers of the requests and tasks in flight.
#[derive(Default)]
pub(crate) struct InputRoutes {
    routes: Mutex<HashMap<RouteKey, Route>>,
}

struct Route {
    handlers: ClientHandlers,
    /// Set on a route no call holds yet, and when it lapses.
    provisional_until: Option<Instant>,
}

/// Who answers one server→client request.
pub(crate) enum Answerer {
    /// These handlers (a call's, over the client's own).
    Handlers(ClientHandlers),
    /// Nobody can be named without guessing.
    Ambiguous,
}

impl InputRoutes {
    /// The routes, with lapsed provisional ones gone.
    fn routes(&self) -> std::sync::MutexGuard<'_, HashMap<RouteKey, Route>> {
        let mut routes = self.routes.lock().expect("input routes poisoned");
        let now = Instant::now();
        routes.retain(|_, route| route.provisional_until.is_none_or(|until| until > now));
        routes
    }

    /// Route requests belonging to `key` to `handlers` until the guard drops.
    pub(crate) fn register(
        self: &Arc<Self>,
        key: RouteKey,
        handlers: ClientHandlers,
    ) -> InputGuard {
        self.routes().insert(
            key.clone(),
            Route {
                handlers,
                provisional_until: None,
            },
        );
        InputGuard {
            routes: Arc::clone(self),
            key,
        }
    }

    /// Request `id` became task `task_id`: route the task's requests to the
    /// request's handlers now, before the call gets to (a `2025-11-25`
    /// server may ask on the call's stream the moment the task exists). The
    /// call claims the route by registering it; unclaimed, it lapses.
    pub(crate) fn became_task(&self, id: &RequestId, task_id: &str) {
        let mut routes = self.routes();
        let Some(handlers) = routes
            .get(&RouteKey::Request(id.clone()))
            .map(|route| route.handlers.clone())
        else {
            return;
        };
        routes
            .entry(RouteKey::Task(task_id.to_owned()))
            .or_insert(Route {
                handlers,
                provisional_until: Some(Instant::now() + PROVISIONAL),
            });
    }

    /// Whether any call has its own handlers.
    pub(crate) fn is_empty(&self) -> bool {
        self.routes
            .lock()
            .expect("input routes poisoned")
            .is_empty()
    }

    /// Who answers a request that arrived `related` to one of ours or to
    /// one of our tasks (when the transport or its metadata could tell), with
    /// `in_flight` requests of ours pending.
    pub(crate) fn answerer(
        &self,
        related: &[RouteKey],
        global: &ClientHandlers,
        in_flight: usize,
    ) -> Answerer {
        let routes = self.routes();
        if routes.is_empty() {
            return Answerer::Handlers(global.clone());
        }
        let call = if related.is_empty() {
            if routes.len() == 1 && in_flight == 1 {
                routes.values().next()
            } else {
                return Answerer::Ambiguous;
            }
        } else {
            related.iter().find_map(|key| routes.get(key))
        };
        Answerer::Handlers(match call {
            Some(call) => call.handlers.over(global),
            None => global.clone(),
        })
    }
}

/// Keeps one call's handlers routed while it is in flight.
pub(crate) struct InputGuard {
    routes: Arc<InputRoutes>,
    key: RouteKey,
}

impl Drop for InputGuard {
    fn drop(&mut self) {
        self.routes.routes().remove(&self.key);
    }
}
