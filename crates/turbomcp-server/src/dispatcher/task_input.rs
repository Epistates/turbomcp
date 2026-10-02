//! Mid-task client input on `2025-11-25` (tasks.mdx §Input Required Status).
//!
//! A task's handler asks for input the same way on both revisions: through
//! its [`TaskLink`](crate::TaskLink), which publishes the request to the task
//! so it reads `input_required`. What differs is how the request reaches the
//! client. `2025-11-25` has no `tasks/update`: the server sends the request
//! itself, carrying `io.modelcontextprotocol/related-task`, and the client's
//! JSON-RPC response is the answer. This relay does the sending, on the best
//! stream there is:
//!
//! 1. a `tasks/result` call for the task in progress, the channel the spec
//!    points at ("the requestor SHOULD preemptively call `tasks/result`");
//! 2. the stream of the call that created the task, while it lives (a stdio
//!    or WebSocket connection);
//! 3. the session's `GET` stream ("clients SHOULD expect messages to be
//!    delivered on any SSE stream, including the HTTP GET stream").
//!
//! With none open the request waits, outstanding, for the client to come
//! asking, as `input_required` tells it to. Each request is out on at most
//! one live stream at a time; one sent on a stream that closed unanswered is
//! sent again on the next.
//!
//! The relay is in-process: on a multi-replica deployment a task's
//! `tasks/result` and `GET` streams must reach the replica running it, which
//! a stateful `2025-11-25` session's requests already must.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde_json::{Map, Value};
use turbomcp_core::{CancellationToken, JsonRpcMessage, JsonRpcRequest, RequestId};
use turbomcp_service::Peer;

use crate::mrtr::{PendingGuard, PendingRequests};
use crate::subscriptions::Route;
use crate::tasks::{TaskBackend, TaskOwner};

use super::legacy_tasks::with_related_task;

/// Relays the input requests of the `2025-11-25` tasks running here.
pub(super) struct TaskInputRelay {
    /// Where the client's responses land, shared with inline requests.
    pending: Arc<PendingRequests>,
    tasks: Mutex<HashMap<String, Relayed>>,
    /// Names `tasks/result` streams, so each can remove itself.
    next_stream: std::sync::atomic::AtomicU64,
}

/// One task whose work is running.
struct Relayed {
    store: Arc<dyn TaskBackend>,
    owner: TaskOwner,
    /// The creating call's stream, then the session's `GET` stream.
    route: Route,
    /// The `tasks/result` calls in progress for the task, oldest first.
    result_streams: Vec<(u64, Peer)>,
    /// The requests out with the client, by key, and the stream each went on.
    in_flight: HashMap<String, Peer>,
    /// Fired when the work ends, so nothing waits on an answer after it.
    done: CancellationToken,
}

impl TaskInputRelay {
    pub(super) fn new(pending: Arc<PendingRequests>) -> Self {
        Self {
            pending,
            tasks: Mutex::new(HashMap::new()),
            next_stream: std::sync::atomic::AtomicU64::new(0),
        }
    }

    fn tasks(&self) -> std::sync::MutexGuard<'_, HashMap<String, Relayed>> {
        self.tasks.lock().expect("task input relay poisoned")
    }

    /// Relay `task_id`'s input requests until the returned guard drops, which
    /// the task's work holds for as long as it runs.
    pub(super) fn track(
        self: &Arc<Self>,
        task_id: &str,
        store: Arc<dyn TaskBackend>,
        owner: TaskOwner,
        route: Route,
    ) -> Tracked {
        self.tasks().insert(
            task_id.to_owned(),
            Relayed {
                store,
                owner,
                route,
                result_streams: Vec::new(),
                in_flight: HashMap::new(),
                done: CancellationToken::new(),
            },
        );
        Tracked {
            relay: Arc::clone(self),
            task_id: task_id.to_owned(),
        }
    }

    /// A `tasks/result` call by `owner` for `task_id` streams on `peer` until
    /// the returned guard drops; requests go there first meanwhile. `None`
    /// when the task's work isn't running here, or isn't `owner`'s.
    pub(super) fn result_stream(
        self: &Arc<Self>,
        task_id: &str,
        owner: &TaskOwner,
        peer: Peer,
    ) -> Option<ResultStream> {
        let mut tasks = self.tasks();
        let entry = tasks.get_mut(task_id).filter(|e| &e.owner == owner)?;
        let n = self
            .next_stream
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        entry.result_streams.push((n, peer));
        Some(ResultStream {
            relay: Arc::clone(self),
            task_id: task_id.to_owned(),
            n,
        })
    }

    /// Send the client every request `task_id` has outstanding that isn't
    /// already out on a live stream. Called whenever the task changes (a new
    /// request among the reasons) and when a `tasks/result` stream opens.
    pub(super) async fn deliver(self: &Arc<Self>, task_id: &str) {
        let Some((store, owner)) = self
            .tasks()
            .get(task_id)
            .map(|e| (Arc::clone(&e.store), e.owner.clone()))
        else {
            return;
        };
        let Ok(snapshot) = store.get(&owner, task_id).await else {
            return;
        };
        if snapshot.input_requests.is_empty() {
            return;
        }
        // Claimed under the lock, so two deliveries never send one request
        // twice.
        let (stream, done, sends) = {
            let mut tasks = self.tasks();
            let Some(entry) = tasks.get_mut(task_id) else {
                return;
            };
            let Some(stream) = entry
                .result_streams
                .iter()
                .rev()
                .map(|(_, peer)| peer)
                .find(|peer| peer.is_open())
                .cloned()
                .or_else(|| entry.route.peer())
            else {
                return;
            };
            let mut sends = Vec::new();
            for (key, request) in snapshot.input_requests {
                if entry.in_flight.get(&key).is_some_and(Peer::is_open) {
                    continue;
                }
                entry.in_flight.insert(key.clone(), stream.clone());
                sends.push((key, request));
            }
            (stream, entry.done.clone(), sends)
        };
        for (key, request) in sends {
            let id = RequestId::from(format!("srv-{}", uuid::Uuid::new_v4()));
            let (answer, guard) = self.pending.register(id.clone());
            if stream
                .send(related_request(id, &request, task_id))
                .await
                .is_err()
            {
                // Gone between the check and the send: the next stream
                // carries it.
                self.release(task_id, &key);
                continue;
            }
            tokio::spawn(Arc::clone(self).await_answer(
                Answer {
                    task_id: task_id.to_owned(),
                    key,
                    store: Arc::clone(&store),
                    owner: owner.clone(),
                    done: done.clone(),
                },
                answer,
                guard,
            ));
        }
    }

    /// Hand the client's answer to the task, which resumes the handler.
    async fn await_answer(
        self: Arc<Self>,
        a: Answer,
        answer: tokio::sync::oneshot::Receiver<turbomcp_core::JsonRpcResponse>,
        _guard: PendingGuard,
    ) {
        let response = tokio::select! {
            () = a.done.cancelled() => return,
            response = answer => response,
        };
        let Ok(response) = response else { return };
        match (response.result, response.error) {
            (Some(result), None) => {
                let answers = Map::from_iter([(a.key.clone(), result)]);
                if let Err(e) = a.store.provide_input(&a.owner, &a.task_id, &answers).await {
                    tracing::debug!(error = ?e, task = %a.task_id, "task input answer dropped");
                }
                self.release(&a.task_id, &a.key);
            }
            // Left marked as out on that stream, so it isn't asked again
            // there; a fresh `tasks/result` stream asks again.
            (_, Some(e)) => tracing::warn!(
                task = %a.task_id,
                key = %a.key,
                code = e.code,
                message = %e.message,
                "the client answered a task's input request with an error",
            ),
            _ => tracing::warn!(
                task = %a.task_id,
                key = %a.key,
                "the client answered a task's input request with an empty response",
            ),
        }
    }

    fn release(&self, task_id: &str, key: &str) {
        if let Some(entry) = self.tasks().get_mut(task_id) {
            entry.in_flight.remove(key);
        }
    }
}

/// What an awaited answer is for.
struct Answer {
    task_id: String,
    key: String,
    store: Arc<dyn TaskBackend>,
    owner: TaskOwner,
    done: CancellationToken,
}

/// Held by a task's work while it runs; dropping it ends the relay.
pub(super) struct Tracked {
    relay: Arc<TaskInputRelay>,
    task_id: String,
}

impl Drop for Tracked {
    fn drop(&mut self) {
        if let Some(entry) = self.relay.tasks().remove(&self.task_id) {
            entry.done.cancel();
        }
    }
}

/// Held by a `tasks/result` call while it streams.
pub(super) struct ResultStream {
    relay: Arc<TaskInputRelay>,
    task_id: String,
    n: u64,
}

impl Drop for ResultStream {
    fn drop(&mut self) {
        if let Some(entry) = self.relay.tasks().get_mut(&self.task_id) {
            entry.result_streams.retain(|(n, _)| *n != self.n);
        }
    }
}

/// `request` (a `{ method, params }` object) as a JSON-RPC request with id
/// `id`, its `_meta` naming the task ("The receiver MUST include the
/// `io.modelcontextprotocol/related-task` metadata in the request").
fn related_request(id: RequestId, request: &Value, task_id: &str) -> JsonRpcMessage {
    let method = request
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let params = request
        .get("params")
        .cloned()
        .unwrap_or_else(|| Value::Object(Map::new()));
    JsonRpcRequest::new(id, method, Some(with_related_task(params, task_id))).into()
}
