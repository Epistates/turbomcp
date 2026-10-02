//! Task-status notification subscriptions (SEP-2663 §Task Status Notifications).
//!
//! A client opens a `subscriptions/listen` stream with a `taskIds` filter; the
//! server records `(taskId → subscriber)` here and pushes `notifications/tasks`
//! (the full [`DetailedTask`](crate::wire::DetailedTask), minus `resultType`)
//! on each status change through the listen stream's
//! [`Peer`](turbomcp_service::Peer). Every pushed notification carries the
//! originating subscription's id verbatim in
//! `_meta["io.modelcontextprotocol/subscriptionId"]` (a spec MUST for
//! notifications delivered via a `subscriptions/listen` stream). A connection
//! that has closed is pruned on the spot, both when a push finds it closed and
//! when a new subscription is recorded, since a task that never changes status
//! is never pushed to and so never revisited.
//!
//! Task-status notifications are spec-**optional** — clients MUST be able to
//! poll `tasks/get` regardless — so a missing subscription simply means no push.

use std::collections::HashMap;
use std::sync::Mutex;

use serde_json::{Value, json};
use turbomcp_core::{ConnectionId, JsonRpcNotification, RequestId};
use turbomcp_server::{TaskBackend, TaskOwner};
use turbomcp_service::{Delivery, Peer};

use crate::render;
use crate::wire::DetailedTask;

/// `notifications/tasks` — pushed to subscribers on a task's status change.
pub const NOTIFICATIONS_TASKS: &str = "notifications/tasks";

/// The reserved `_meta` key correlating a stream notification with its
/// subscription.
const SUBSCRIPTION_ID_KEY: &str = "io.modelcontextprotocol/subscriptionId";

/// One listening endpoint: the connection to deliver on and the listen
/// request's id (stamped into each notification's `_meta`).
#[derive(Clone)]
struct Subscriber {
    peer: Peer,
    subscription_id: RequestId,
}

impl Subscriber {
    fn same(&self, other: &Self) -> bool {
        self.peer.id() == other.peer.id() && self.subscription_id == other.subscription_id
    }
}

/// One subscribed task: whose it is (only its owner may subscribe, so this is
/// every subscriber's owner too) and who is listening.
struct Topic {
    owner: TaskOwner,
    subscribers: Vec<Subscriber>,
}

/// Maps each subscribed task id to the subscribers listening for its status.
#[derive(Default)]
pub(crate) struct TaskSubscriptions {
    inner: Mutex<HashMap<String, Topic>>,
}

impl TaskSubscriptions {
    /// Record that the listen request `subscription_id` on `peer`, whose
    /// caller is `owner` and owns each of `task_ids`, wants status
    /// notifications for them.
    pub(crate) fn subscribe(
        &self,
        peer: &Peer,
        subscription_id: &RequestId,
        task_ids: &[String],
        owner: &TaskOwner,
    ) {
        let subscriber = Subscriber {
            peer: peer.clone(),
            subscription_id: subscription_id.clone(),
        };
        let mut map = self.lock();
        // Reclaim subscribers whose connection has since closed. `push_status`
        // does this too, but only for tasks that actually change: a subscriber
        // to a task that then goes quiet is never revisited, so without this a
        // server accumulates one entry per client that ever subscribed and went
        // away. A closed peer is what dead means, so a live subscriber is
        // never disturbed.
        map.retain(|_, topic| {
            topic.subscribers.retain(|s| s.peer.is_open());
            !topic.subscribers.is_empty()
        });
        for task_id in task_ids {
            let topic = map.entry(task_id.clone()).or_insert_with(|| Topic {
                owner: owner.clone(),
                subscribers: Vec::new(),
            });
            if !topic.subscribers.iter().any(|s| s.same(&subscriber)) {
                topic.subscribers.push(subscriber.clone());
            }
        }
    }

    /// Forget every subscribed task the backend no longer has: an expired
    /// task is never pushed again, so its subscribers would otherwise stay
    /// for as long as their connection does.
    pub(crate) async fn forget_gone(&self, store: &dyn TaskBackend) {
        let topics: Vec<(String, TaskOwner)> = self
            .lock()
            .iter()
            .map(|(id, topic)| (id.clone(), topic.owner.clone()))
            .collect();
        let mut gone = Vec::new();
        for (id, owner) in topics {
            if store.get(&owner, &id).await.is_err() {
                gone.push(id);
            }
        }
        if !gone.is_empty() {
            let mut map = self.lock();
            for id in gone {
                map.remove(&id);
            }
        }
    }

    /// Forget listen request `subscription_id` on `connection` (the client
    /// cancelled it). Empties are reclaimed.
    pub(crate) fn unsubscribe(&self, connection: &ConnectionId, subscription_id: &RequestId) {
        let mut map = self.lock();
        map.retain(|_, topic| {
            topic
                .subscribers
                .retain(|s| !(s.peer.id() == connection && &s.subscription_id == subscription_id));
            !topic.subscribers.is_empty()
        });
    }

    /// The task's owner and the subscribers listening to it.
    fn topic(&self, task_id: &str) -> Option<(TaskOwner, Vec<Subscriber>)> {
        self.lock()
            .get(task_id)
            .map(|topic| (topic.owner.clone(), topic.subscribers.clone()))
    }

    /// Drop `peer`'s connection from every task's subscriber set (it has
    /// closed). Empties are reclaimed.
    fn drop_connection(&self, peer: &Peer) {
        let mut map = self.lock();
        map.retain(|_, topic| {
            topic.subscribers.retain(|s| s.peer.id() != peer.id());
            !topic.subscribers.is_empty()
        });
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Topic>> {
        self.inner.lock().expect("task subscriptions poisoned")
    }
}

/// Push `notifications/tasks` for `task_id` to every subscriber, pruning
/// connections whose writer has closed. No-op if nobody is subscribed or the
/// task is gone.
pub(crate) async fn push_status(subs: &TaskSubscriptions, store: &dyn TaskBackend, task_id: &str) {
    let Some((owner, subscribers)) = subs.topic(task_id) else {
        return;
    };
    let Ok(snapshot) = store.get(&owner, task_id).await else {
        return;
    };
    let base = notification_params(&render::detailed(snapshot));
    for subscriber in subscribers {
        let params = stamp_subscription_id(base.clone(), &subscriber.subscription_id);
        let note = JsonRpcNotification::new(NOTIFICATIONS_TASKS, Some(params));
        // Never waits on one subscriber: see `Peer::offer`.
        if subscriber.peer.offer(note.into()) == Delivery::Closed {
            subs.drop_connection(&subscriber.peer);
        }
    }
}

/// The `notifications/tasks` params: the full `DetailedTask` carries a
/// `resultType` (it doubles as the `tasks/get` result), but a notification
/// isn't a result — strip it (SEP-2663 §Task Status Notifications example).
fn notification_params(detailed: &DetailedTask) -> Value {
    let mut value = serde_json::to_value(detailed).unwrap_or(Value::Null);
    if let Some(obj) = value.as_object_mut() {
        obj.remove("resultType");
    }
    value
}

/// Stamp the subscription's id — verbatim, string or number — into the
/// notification's `_meta` (subscriptions spec: the server MUST include it on
/// every notification delivered via a listen stream).
fn stamp_subscription_id(mut params: Value, id: &RequestId) -> Value {
    let id_value = serde_json::to_value(id).unwrap_or(Value::Null);
    if let Some(obj) = params.as_object_mut() {
        let meta = obj
            .entry("_meta")
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
        if let Some(meta_obj) = meta.as_object_mut() {
            meta_obj.insert(SUBSCRIPTION_ID_KEY.to_owned(), id_value);
        }
    } else {
        params = json!({ "_meta": { SUBSCRIPTION_ID_KEY: id_value } });
    }
    params
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task_ids(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("task-{i}")).collect()
    }

    /// Pruning only when a task's status changes leaves subscribers to a task
    /// that then goes quiet with nothing to reclaim on. Subscribing is where
    /// the map grows, so it is where the departed are cleared.
    #[test]
    fn subscribing_reclaims_the_connections_that_have_since_gone() {
        let subs = TaskSubscriptions::default();
        for i in 0..25 {
            let (tx, _rx) = tokio::sync::mpsc::channel(1);
            let gone = Peer::new(format!("gone-{i}"), &tx);
            subs.subscribe(
                &gone,
                &RequestId::from(i64::from(i)),
                &task_ids(2),
                &TaskOwner::Anonymous,
            );
        }

        let (tx, _rx) = tokio::sync::mpsc::channel(8);
        let here = Peer::new("tasks-still-here", &tx);
        subs.subscribe(
            &here,
            &RequestId::from(99i64),
            &task_ids(1),
            &TaskOwner::Anonymous,
        );

        let map = subs.lock();
        assert_eq!(
            map.values()
                .map(|topic| topic.subscribers.len())
                .sum::<usize>(),
            1,
            "subscribers whose connections are gone must not outlive them"
        );
        assert_eq!(
            map["task-0"].subscribers[0].peer.id().as_str(),
            "tasks-still-here"
        );
    }

    /// The same subscriber listening twice is recorded once, so a client that
    /// re-subscribes cannot inflate the fan-out for a task.
    #[test]
    fn subscribing_twice_records_one_subscriber() {
        let subs = TaskSubscriptions::default();
        let (tx, _rx) = tokio::sync::mpsc::channel(8);
        let peer = Peer::new("tasks-dup", &tx);

        subs.subscribe(
            &peer,
            &RequestId::from(1i64),
            &task_ids(1),
            &TaskOwner::Anonymous,
        );
        subs.subscribe(
            &peer,
            &RequestId::from(1i64),
            &task_ids(1),
            &TaskOwner::Anonymous,
        );

        assert_eq!(subs.topic("task-0").unwrap().1.len(), 1);
    }

    #[test]
    fn dropping_a_connection_reclaims_its_empty_tasks() {
        let subs = TaskSubscriptions::default();
        let (tx, _rx) = tokio::sync::mpsc::channel(8);
        let peer = Peer::new("tasks-drop", &tx);

        subs.subscribe(
            &peer,
            &RequestId::from(1i64),
            &task_ids(3),
            &TaskOwner::Anonymous,
        );
        assert_eq!(subs.lock().len(), 3);

        subs.drop_connection(&peer);
        assert!(
            subs.lock().is_empty(),
            "a task with no subscribers left keeps no entry"
        );
    }
}
