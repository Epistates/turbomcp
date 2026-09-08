//! Task-status notification subscriptions (SEP-2663 §Task Status Notifications).
//!
//! A client opens a `subscriptions/listen` stream with a `taskIds` filter; the
//! server records `(taskId → subscriber)` here and pushes `notifications/tasks`
//! (the full [`DetailedTask`](crate::wire::DetailedTask), minus `resultType`)
//! on each status change over the connection's ordered writer
//! ([`turbomcp_service::outbound`]). Every pushed notification carries the
//! originating subscription's id verbatim in
//! `_meta["io.modelcontextprotocol/subscriptionId"]` (a spec MUST for
//! notifications delivered via a `subscriptions/listen` stream). A connection
//! whose writer is gone is pruned on the spot — both when a push finds it
//! missing and when a new subscription is recorded, since a task that never
//! changes status is never pushed to and so never revisited.
//!
//! Task-status notifications are spec-**optional** — clients MUST be able to
//! poll `tasks/get` regardless — so a missing subscription simply means no push.

use std::collections::HashMap;
use std::sync::Mutex;

use serde_json::{Value, json};
use turbomcp_core::{JsonRpcNotification, RequestId};
use turbomcp_service::outbound;

use crate::store::DraftTaskStore;
use crate::wire::DetailedTask;

/// `notifications/tasks` — pushed to subscribers on a task's status change.
pub const NOTIFICATIONS_TASKS: &str = "notifications/tasks";

/// The reserved `_meta` key correlating a stream notification with its
/// subscription.
const SUBSCRIPTION_ID_KEY: &str = "io.modelcontextprotocol/subscriptionId";

/// One listening endpoint: the connection to deliver on and the listen
/// request's id (stamped into each notification's `_meta`).
#[derive(Clone, PartialEq, Eq, Hash)]
struct Subscriber {
    connection: String,
    subscription_id: RequestId,
}

/// Maps each subscribed task id to the subscribers listening for its status.
#[derive(Default)]
pub(crate) struct TaskSubscriptions {
    inner: Mutex<HashMap<String, Vec<Subscriber>>>,
}

impl TaskSubscriptions {
    /// Record that `connection_id`'s listen request `subscription_id` wants
    /// status notifications for `task_ids`.
    pub(crate) fn subscribe(
        &self,
        connection_id: &str,
        subscription_id: &RequestId,
        task_ids: &[String],
    ) {
        let subscriber = Subscriber {
            connection: connection_id.to_owned(),
            subscription_id: subscription_id.clone(),
        };
        let mut map = self.lock();
        // Reclaim subscribers whose connection has since closed. `push_status`
        // does this too, but only for tasks that actually change: a subscriber
        // to a task that then goes quiet is never revisited, so without this a
        // server accumulates one entry per client that ever subscribed and went
        // away. A missing writer is what dead means, so a live subscriber is
        // never disturbed.
        map.retain(|_, subs| {
            subs.retain(|s| outbound::writer(&s.connection).is_some());
            !subs.is_empty()
        });
        for task_id in task_ids {
            let subs = map.entry(task_id.clone()).or_default();
            if !subs.contains(&subscriber) {
                subs.push(subscriber.clone());
            }
        }
    }

    /// The subscribers listening to `task_id`.
    fn subscribers(&self, task_id: &str) -> Vec<Subscriber> {
        self.lock().get(task_id).cloned().unwrap_or_default()
    }

    /// Drop `connection_id` from every task's subscriber set (its writer is
    /// gone). Empties are reclaimed.
    fn drop_connection(&self, connection_id: &str) {
        let mut map = self.lock();
        map.retain(|_, subs| {
            subs.retain(|s| s.connection != connection_id);
            !subs.is_empty()
        });
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Vec<Subscriber>>> {
        self.inner.lock().expect("task subscriptions poisoned")
    }
}

/// Push `notifications/tasks` for `task_id` to every subscriber, pruning
/// connections whose writer has closed. No-op if nobody is subscribed or the
/// task is gone.
pub(crate) async fn push_status(subs: &TaskSubscriptions, store: &DraftTaskStore, task_id: &str) {
    let subscribers = subs.subscribers(task_id);
    if subscribers.is_empty() {
        return;
    }
    let Some(detailed) = store.get(task_id) else {
        return;
    };
    let base = notification_params(&detailed);
    for subscriber in subscribers {
        match outbound::writer(&subscriber.connection) {
            Some(writer) => {
                let params = stamp_subscription_id(base.clone(), &subscriber.subscription_id);
                let note = JsonRpcNotification::new(NOTIFICATIONS_TASKS, Some(params));
                if writer.send(note.into()).await.is_err() {
                    subs.drop_connection(&subscriber.connection);
                }
            }
            None => subs.drop_connection(&subscriber.connection),
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
            subs.subscribe(
                &format!("gone-{i}"),
                &RequestId::from(i64::from(i)),
                &task_ids(2),
            );
        }

        let (tx, _rx) = tokio::sync::mpsc::channel(8);
        let _guard = outbound::register("tasks-still-here", tx);
        subs.subscribe("tasks-still-here", &RequestId::from(99i64), &task_ids(1));

        let map = subs.lock();
        assert_eq!(
            map.values().flatten().count(),
            1,
            "subscribers whose writers are gone must not outlive them"
        );
        assert_eq!(map["task-0"][0].connection, "tasks-still-here");
    }

    /// The same subscriber listening twice is recorded once, so a client that
    /// re-subscribes cannot inflate the fan-out for a task.
    #[test]
    fn subscribing_twice_records_one_subscriber() {
        let subs = TaskSubscriptions::default();
        let (tx, _rx) = tokio::sync::mpsc::channel(8);
        let _guard = outbound::register("tasks-dup", tx);

        subs.subscribe("tasks-dup", &RequestId::from(1i64), &task_ids(1));
        subs.subscribe("tasks-dup", &RequestId::from(1i64), &task_ids(1));

        assert_eq!(subs.subscribers("task-0").len(), 1);
    }

    #[test]
    fn dropping_a_connection_reclaims_its_empty_tasks() {
        let subs = TaskSubscriptions::default();
        let (tx, _rx) = tokio::sync::mpsc::channel(8);
        let _guard = outbound::register("tasks-drop", tx);

        subs.subscribe("tasks-drop", &RequestId::from(1i64), &task_ids(3));
        assert_eq!(subs.lock().len(), 3);

        subs.drop_connection("tasks-drop");
        assert!(
            subs.lock().is_empty(),
            "a task with no subscribers left keeps no entry"
        );
    }
}
