//! The draft Tasks registry — task-id-keyed, **session-less** (SEP-2663 §2.3:
//! the draft removed sessions, so the unguessable task id is the handle).
//!
//! A task begins `working`; the spawned underlying call drives it to a terminal
//! status (`completed`/`failed`/`cancelled`) exactly once. `tasks/cancel` fires
//! the task's cancellation token (cooperative — the handler decides when to
//! stop) and transitions it to `cancelled`; a late `complete` is then a no-op.
//! Expired tasks (finite `ttlMs`) are purged lazily, cancelling any in-flight
//! work, after which a `tasks/get` answers `-32602` (compliant per spec).

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::{Map, Value};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use tokio::sync::oneshot;
use turbomcp_core::{CancellationToken, JsonRpcError};

use crate::wire::{DetailedTask, Task, TaskStatus};

/// The terminal outcome of a task's underlying request.
pub(crate) enum TaskOutcome {
    /// The call completed (a tool-level `isError: true` is still a completion).
    Completed(Value),
    /// The call failed with a JSON-RPC protocol error.
    Failed(JsonRpcError),
}

struct Entry {
    owner: Option<String>,
    status: TaskStatus,
    status_message: Option<String>,
    created_wall: OffsetDateTime,
    updated_wall: OffsetDateTime,
    created: Instant,
    ttl_ms: Option<i64>,
    poll_interval_ms: Option<i64>,
    /// The terminal result/error, present once terminal.
    outcome: Option<TaskOutcome>,
    cancel: CancellationToken,
    /// In-execution client input (SEP-2663 §Task Update Requests).
    inputs: Inputs,
}

/// A task's in-execution input state: what the handler is waiting on.
#[derive(Default)]
struct Inputs {
    /// Outstanding wire `InputRequest`s, keyed by their task-unique key —
    /// exactly what `tasks/get` surfaces as `inputRequests`.
    outstanding: Map<String, Value>,
    /// The handler futures awaiting each outstanding key.
    waiters: HashMap<String, oneshot::Sender<Value>>,
    /// Every key ever issued for this task (keys MUST be unique over the
    /// task's lifetime and never reused — spec).
    used: HashSet<String>,
}

impl Entry {
    fn expired(&self, now: Instant) -> bool {
        match self.ttl_ms {
            None | Some(0) => false, // null/0 ⇒ unlimited here (never auto-purged)
            Some(ms) => {
                let ttl = u64::try_from(ms).unwrap_or(0);
                now.duration_since(self.created) > Duration::from_millis(ttl)
            }
        }
    }

    fn base(&self, id: &str) -> Task {
        Task {
            task_id: id.to_owned(),
            status: self.status,
            status_message: self.status_message.clone(),
            created_at: rfc3339(self.created_wall),
            last_updated_at: rfc3339(self.updated_wall),
            ttl_ms: self.ttl_ms,
            poll_interval_ms: self.poll_interval_ms,
        }
    }

    fn detailed(&self, id: &str) -> DetailedTask {
        let mut detailed = DetailedTask::new(self.base(id));
        if self.status == TaskStatus::InputRequired {
            // tasks/get MUST surface ALL outstanding requests (spec).
            detailed.input_requests = Some(self.inputs.outstanding.clone());
        }
        match &self.outcome {
            Some(TaskOutcome::Completed(result)) => detailed.result = Some(result.clone()),
            Some(TaskOutcome::Failed(error)) => {
                detailed.error = serde_json::to_value(error).ok();
            }
            None => {}
        }
        detailed
    }
}

fn rfc3339(t: OffsetDateTime) -> String {
    t.format(&Rfc3339)
        .unwrap_or_else(|_| String::from("1970-01-01T00:00:00Z"))
}

/// Bounded, session-less, in-memory draft task registry.
pub struct DraftTaskStore {
    inner: Mutex<HashMap<String, Entry>>,
    capacity: usize,
}

impl DraftTaskStore {
    const DEFAULT_CAPACITY: usize = 1024;

    /// A registry bounded at `capacity` live (unexpired) tasks.
    pub(crate) fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
            capacity,
        }
    }

    /// Register a fresh `working` task driven by `cancel`. Returns the seed
    /// [`Task`] to render as a `CreateTaskResult`. `None` if the registry is at
    /// capacity (the caller should answer `-32603` and run the call normally).
    pub(crate) fn create_owned(
        &self,
        ttl_ms: Option<i64>,
        poll_interval_ms: Option<i64>,
        cancel: CancellationToken,
        owner: Option<String>,
    ) -> Option<Task> {
        let mut map = self.lock();
        Self::purge_expired(&mut map);
        if map.len() >= self.capacity {
            return None;
        }
        let id = uuid::Uuid::new_v4().to_string();
        let now_wall = OffsetDateTime::now_utc();
        let entry = Entry {
            owner,
            status: TaskStatus::Working,
            status_message: None,
            created_wall: now_wall,
            updated_wall: now_wall,
            created: Instant::now(),
            ttl_ms,
            poll_interval_ms,
            outcome: None,
            cancel,
            inputs: Inputs::default(),
        };
        let task = entry.base(&id);
        map.insert(id, entry);
        Some(task)
    }

    pub(crate) fn owns(&self, id: &str, owner: Option<&str>) -> bool {
        let mut entries = self.lock();
        Self::purge_expired(&mut entries);
        entries
            .get(id)
            .is_some_and(|entry| entry.owner.as_deref() == owner)
    }

    #[cfg(test)]
    fn create(
        &self,
        ttl_ms: Option<i64>,
        poll_interval_ms: Option<i64>,
        cancel: CancellationToken,
    ) -> Option<Task> {
        self.create_owned(ttl_ms, poll_interval_ms, cancel, None)
    }

    /// Record a task's terminal outcome (`Completed` ⇒ `completed`, `Failed` ⇒
    /// `failed`). No-op if the task is already terminal (a cancel won the race)
    /// or was purged.
    pub(crate) fn complete(&self, id: &str, outcome: TaskOutcome) {
        let mut map = self.lock();
        let Some(entry) = map.get_mut(id) else {
            return;
        };
        if entry.status.is_terminal() {
            return;
        }
        entry.status = match &outcome {
            TaskOutcome::Completed(_) => TaskStatus::Completed,
            TaskOutcome::Failed(err) => {
                entry.status_message = Some(err.message.clone());
                TaskStatus::Failed
            }
        };
        entry.outcome = Some(outcome);
        entry.updated_wall = OffsetDateTime::now_utc();
    }

    /// `tasks/get`: the task's current detailed state, or `None` if unknown.
    pub(crate) fn get(&self, id: &str) -> Option<DetailedTask> {
        let mut map = self.lock();
        Self::purge_expired(&mut map);
        map.get(id).map(|e| e.detailed(id))
    }

    /// `tasks/cancel`: fire the task's token and transition it to `cancelled`
    /// (cooperative). Returns whether a live task matched. Already-terminal
    /// tasks are reported as matched (the ack is unconditional per spec) but not
    /// transitioned.
    pub(crate) fn cancel(&self, id: &str) -> bool {
        let mut map = self.lock();
        Self::purge_expired(&mut map);
        let Some(entry) = map.get_mut(id) else {
            return false;
        };
        if !entry.status.is_terminal() {
            entry.cancel.cancel();
            entry.status = TaskStatus::Cancelled;
            entry.status_message = Some("the task was cancelled by request".to_owned());
            entry.updated_wall = OffsetDateTime::now_utc();
            // Unblock any handler awaiting client input (dropping the senders
            // errors the receivers) and stop advertising the requests.
            entry.inputs.waiters.clear();
            entry.inputs.outstanding.clear();
        }
        true
    }

    /// Publish an in-execution input request for a live task (SEP-2663
    /// §Task Update Requests): record `request` as outstanding under a
    /// **task-unique** variant of `key` (keys are never reused over a task's
    /// lifetime — spec MUST), flip a `working` task to `input_required`, and
    /// return the receiver that [`deliver_inputs`](Self::deliver_inputs)
    /// resolves. `Err(())` if the task is unknown, terminal, or expired — the
    /// awaiting handler unwinds.
    pub(crate) fn publish_input(
        &self,
        id: &str,
        key: &str,
        request: Value,
    ) -> Result<oneshot::Receiver<Value>, ()> {
        let mut map = self.lock();
        Self::purge_expired(&mut map);
        let entry = map.get_mut(id).ok_or(())?;
        if entry.status.is_terminal() {
            return Err(());
        }
        // Uniquify: the handler's key as-is when fresh, else `key#2`, `key#3`…
        let mut unique = key.to_owned();
        let mut n = 1;
        while entry.inputs.used.contains(&unique) {
            n += 1;
            unique = format!("{key}#{n}");
        }
        entry.inputs.used.insert(unique.clone());
        let (tx, rx) = oneshot::channel();
        entry.inputs.outstanding.insert(unique.clone(), request);
        entry.inputs.waiters.insert(unique, tx);
        if entry.status == TaskStatus::Working {
            entry.status = TaskStatus::InputRequired;
            entry.updated_wall = OffsetDateTime::now_utc();
        }
        Ok(rx)
    }

    /// Deliver `tasks/update` `inputResponses`: resolve each response whose
    /// key is currently outstanding (unknown, already-answered, and
    /// superseded keys are ignored — spec SHOULD; partial sets are accepted).
    /// A task drained of outstanding requests returns to `working`. `None`
    /// for an unknown task (the caller answers `-32602`); `Some(changed)`
    /// says whether the observable status transitioned (drives the
    /// `notifications/tasks` push).
    pub(crate) fn deliver_inputs(&self, id: &str, responses: &Map<String, Value>) -> Option<bool> {
        let mut map = self.lock();
        Self::purge_expired(&mut map);
        let entry = map.get_mut(id)?;
        for (key, value) in responses {
            if entry.inputs.outstanding.contains_key(key)
                && let Some(waiter) = entry.inputs.waiters.remove(key)
            {
                entry.inputs.outstanding.remove(key);
                // A closed receiver means the handler already unwound
                // (cancel/TTL) — the response is simply dropped.
                let _ = waiter.send(value.clone());
            }
        }
        let mut changed = false;
        if entry.status == TaskStatus::InputRequired && entry.inputs.outstanding.is_empty() {
            entry.status = TaskStatus::Working;
            entry.updated_wall = OffsetDateTime::now_utc();
            changed = true;
        }
        Some(changed)
    }

    fn purge_expired(map: &mut HashMap<String, Entry>) {
        let now = Instant::now();
        map.retain(|_, e| {
            let keep = !e.expired(now);
            if !keep && !e.status.is_terminal() {
                e.cancel.cancel();
            }
            keep
        });
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Entry>> {
        self.inner.lock().expect("draft task store poisoned")
    }
}

impl Default for DraftTaskStore {
    fn default() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
            capacity: Self::DEFAULT_CAPACITY,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn lifecycle_working_to_completed() {
        let store = DraftTaskStore::default();
        let task = store
            .create(Some(60_000), Some(500), CancellationToken::new())
            .unwrap();
        assert_eq!(task.status, TaskStatus::Working);

        store.complete(
            &task.task_id,
            TaskOutcome::Completed(json!({"isError": false})),
        );
        let got = store.get(&task.task_id).unwrap();
        assert_eq!(got.task.status, TaskStatus::Completed);
        assert_eq!(got.result.unwrap()["isError"], false);
        assert!(got.error.is_none());
    }

    #[test]
    fn expired_tasks_are_purged_and_their_work_cancelled() {
        let store = DraftTaskStore::default();
        let cancel = CancellationToken::new();
        let short = store.create(Some(20), None, cancel.clone()).unwrap();
        // ttl 0 means "unlimited here" — never auto-purged (spec: null/0).
        let unlimited = store
            .create(Some(0), None, CancellationToken::new())
            .unwrap();
        assert!(store.get(&short.task_id).is_some(), "fresh: visible");

        std::thread::sleep(Duration::from_millis(50));
        assert!(
            store.get(&short.task_id).is_none(),
            "past createdAt + ttlMs the task is gone (SEP-2663) → tasks/get -32602"
        );
        assert!(
            cancel.is_cancelled(),
            "purging a live task fires its cancel token so in-flight work stops"
        );
        assert!(
            store.get(&unlimited.task_id).is_some(),
            "ttl 0/null tasks are never auto-purged"
        );
    }

    #[test]
    fn at_capacity_create_declines_until_an_expiry_frees_a_slot() {
        let store = DraftTaskStore {
            inner: Mutex::new(HashMap::new()),
            capacity: 1,
        };
        let first = store.create(Some(20), None, CancellationToken::new());
        assert!(first.is_some());
        // Full: the caller must fall back to running the call synchronously.
        assert!(
            store
                .create(Some(20), None, CancellationToken::new())
                .is_none()
        );
        // An expired entry frees the slot on the next create's purge.
        std::thread::sleep(Duration::from_millis(50));
        assert!(
            store
                .create(Some(20), None, CancellationToken::new())
                .is_some()
        );
    }

    #[test]
    fn failed_outcome_inlines_error_and_status_message() {
        let store = DraftTaskStore::default();
        let task = store.create(None, None, CancellationToken::new()).unwrap();
        store.complete(
            &task.task_id,
            TaskOutcome::Failed(JsonRpcError {
                code: -32603,
                message: "boom".into(),
                data: None,
            }),
        );
        let got = store.get(&task.task_id).unwrap();
        assert_eq!(got.task.status, TaskStatus::Failed);
        assert_eq!(got.task.status_message.as_deref(), Some("boom"));
        assert_eq!(got.error.unwrap()["code"], -32603);
    }

    #[test]
    fn cancel_fires_token_and_blocks_late_completion() {
        let store = DraftTaskStore::default();
        let token = CancellationToken::new();
        let task = store.create(None, None, token.clone()).unwrap();

        assert!(store.cancel(&task.task_id));
        assert!(token.is_cancelled());
        // A late completion is ignored; the task stays cancelled.
        store.complete(&task.task_id, TaskOutcome::Completed(json!("too late")));
        assert_eq!(
            store.get(&task.task_id).unwrap().task.status,
            TaskStatus::Cancelled
        );
    }

    #[test]
    fn unknown_task_is_a_miss() {
        let store = DraftTaskStore::default();
        assert!(store.get("nope").is_none());
        assert!(!store.cancel("nope"));
        assert!(store.deliver_inputs("nope", &Map::new()).is_none());
    }

    #[tokio::test]
    async fn input_flow_publish_get_deliver_resume() {
        let store = DraftTaskStore::default();
        let task = store.create(None, None, CancellationToken::new()).unwrap();

        // Publishing flips a working task to input_required…
        let rx = store
            .publish_input(
                &task.task_id,
                "confirm",
                json!({"method": "elicitation/create"}),
            )
            .unwrap();
        let got = store.get(&task.task_id).unwrap();
        assert_eq!(got.task.status, TaskStatus::InputRequired);
        // …and tasks/get surfaces ALL outstanding requests.
        assert!(got.input_requests.as_ref().unwrap().contains_key("confirm"));

        // Responses for unknown keys are ignored (spec SHOULD); the task
        // stays input_required and the waiter stays pending.
        let mut wrong = Map::new();
        wrong.insert("never-issued".into(), json!({}));
        assert_eq!(store.deliver_inputs(&task.task_id, &wrong), Some(false));

        // The matching response resolves the waiter and drains the task back
        // to working.
        let mut responses = Map::new();
        responses.insert("confirm".into(), json!({"action": "accept"}));
        assert_eq!(store.deliver_inputs(&task.task_id, &responses), Some(true));
        assert_eq!(rx.await.unwrap()["action"], "accept");
        let got = store.get(&task.task_id).unwrap();
        assert_eq!(got.task.status, TaskStatus::Working);
        assert!(got.input_requests.is_none());

        // A second answer for the same key is already-satisfied → ignored.
        let mut again = Map::new();
        again.insert("confirm".into(), json!({"action": "decline"}));
        assert_eq!(store.deliver_inputs(&task.task_id, &again), Some(false));
    }

    #[tokio::test]
    async fn input_keys_are_unique_over_the_task_lifetime() {
        let store = DraftTaskStore::default();
        let task = store.create(None, None, CancellationToken::new()).unwrap();

        let _rx1 = store
            .publish_input(&task.task_id, "confirm", json!({"n": 1}))
            .unwrap();
        // Reusing the handler key mints a distinct wire key (spec MUST:
        // never reuse a key over a task's lifetime).
        let _rx2 = store
            .publish_input(&task.task_id, "confirm", json!({"n": 2}))
            .unwrap();
        let outstanding = store.get(&task.task_id).unwrap().input_requests.unwrap();
        assert_eq!(outstanding.len(), 2);
        assert!(outstanding.contains_key("confirm"));
        assert!(outstanding.contains_key("confirm#2"));
    }

    #[tokio::test]
    async fn cancel_unblocks_an_awaiting_input() {
        let store = DraftTaskStore::default();
        let token = CancellationToken::new();
        let task = store.create(None, None, token.clone()).unwrap();
        let rx = store
            .publish_input(&task.task_id, "confirm", json!({}))
            .unwrap();

        assert!(store.cancel(&task.task_id));
        // The waiter's sender was dropped: the receiver errors, so an
        // awaiting handler unwinds instead of hanging.
        assert!(rx.await.is_err());
        // A terminal task refuses new input requests.
        assert!(
            store
                .publish_input(&task.task_id, "again", json!({}))
                .is_err()
        );
        // And no longer advertises the old ones.
        assert!(store.get(&task.task_id).unwrap().input_requests.is_none());
    }
}
