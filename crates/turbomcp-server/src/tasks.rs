//! The task registry behind both Tasks front-ends: core `tasks/*` on
//! `2025-11-25`, and the `2026-07-28` Tasks extension (`turbomcp-ext-tasks`).
//!
//! The store is wire-agnostic. Each front-end renders a [`TaskSnapshot`] in
//! its own shape and decides what a [`TaskError`] means on its wire (a full
//! registry is `-32603` on `2025-11-25`, and a synchronous run on
//! `2026-07-28`). One [`TaskBackend`] can serve both: a task's [`TaskOwner`]
//! keeps a session's tasks and a principal's apart.
//!
//! Lifecycle, the same on both wires:
//! - a task begins `working`, may move between `working` and `input_required`,
//!   and reaches `completed`, `failed` or `cancelled` once, never to move
//!   again;
//! - `createdAt`/`lastUpdatedAt` are RFC 3339, and the TTL reported is the one
//!   in force (`None` is unlimited);
//! - a task still running when its TTL runs out is cancelled and marked
//!   `failed` with a status message, so its poller learns what happened; a
//!   finished task is kept for one more TTL after finishing, then deleted
//!   ("servers MAY mark a task as `failed` at any point after the TTL
//!   elapses, and subsequently delete it");
//! - a full registry makes room by dropping the oldest finished task, never a
//!   running one, and one owner can hold only so many running tasks.
//!
//! Every read and write but [`complete`](TaskBackend::complete) and
//! [`request_input`](TaskBackend::request_input), which come from the task's
//! own work, names the owner, and a task another owner holds answers exactly
//! as an unknown one does.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use futures::FutureExt;
use futures::future::BoxFuture;
use serde_json::{Map, Value};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use tokio::sync::{oneshot, watch};
use turbomcp_core::{CancellationToken, JsonRpcError};

/// A task's lifecycle status (rendered to the wire by each front-end).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskStatus {
    /// Created and not yet terminal; the underlying request is executing.
    Working,
    /// The underlying request is waiting on input from the client.
    InputRequired,
    /// The underlying request produced a result.
    Completed,
    /// The underlying request produced an error (or a failing result).
    Failed,
    /// `tasks/cancel` ended it before it finished. (A task that runs out its
    /// TTL is `Failed`.)
    Cancelled,
}

impl TaskStatus {
    /// Whether this status never transitions again.
    #[must_use]
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

/// Whose a task is. Only its owner can read, list, cancel or answer it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TaskOwner {
    /// A `2025-11-25` session. Its tasks end with it
    /// ([`end_session`](TaskBackend::end_session)).
    Session(String),
    /// An authenticated `2026-07-28` caller, by
    /// [`Identity::principal_key`](turbomcp_core::Identity::principal_key).
    Principal(String),
    /// An unauthenticated `2026-07-28` caller. With nothing to tell them
    /// apart, anonymous callers share one owner.
    Anonymous,
}

impl TaskOwner {
    /// The owner for a `2026-07-28` caller with this principal key.
    #[must_use]
    pub fn principal(key: Option<String>) -> Self {
        key.map_or(Self::Anonymous, Self::Principal)
    }
}

/// What a front-end asks for when it creates a task.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct NewTask {
    /// The TTL in milliseconds; `None` is unlimited. The front-end has already
    /// applied its own default and bounds.
    pub ttl_ms: Option<i64>,
    /// The polling interval to suggest, in milliseconds; `None` leaves it to
    /// the backend.
    pub poll_interval_ms: Option<i64>,
}

impl NewTask {
    /// A task with this TTL (`None`: unlimited) and the backend's polling
    /// interval.
    #[must_use]
    pub fn new(ttl_ms: Option<i64>) -> Self {
        Self {
            ttl_ms,
            poll_interval_ms: None,
        }
    }

    /// Suggest this polling interval instead of the backend's.
    #[must_use]
    pub fn with_poll_interval_ms(mut self, poll_interval_ms: Option<i64>) -> Self {
        self.poll_interval_ms = poll_interval_ms;
        self
    }
}

/// A point-in-time copy of one task's state.
#[derive(Clone, Debug, PartialEq)]
pub struct TaskSnapshot {
    /// The task's unique id.
    pub task_id: String,
    /// Current lifecycle status.
    pub status: TaskStatus,
    /// Optional human-readable status detail.
    pub status_message: Option<String>,
    /// RFC 3339.
    pub created_at: String,
    /// RFC 3339.
    pub last_updated_at: String,
    /// The TTL in force, in milliseconds; `None` is unlimited.
    pub ttl_ms: Option<i64>,
    /// The polling interval to suggest, in milliseconds.
    pub poll_interval_ms: Option<i64>,
    /// The input requests the task is waiting on, by key: empty unless
    /// `input_required`.
    pub input_requests: Map<String, Value>,
    /// How the underlying request ended, once terminal: its result, or the
    /// JSON-RPC error it answered with. A cancelled task's is the
    /// cancellation error.
    pub outcome: Option<Result<Value, JsonRpcError>>,
}

/// Why a task operation failed (each front-end maps it to its wire).
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum TaskError {
    /// No live task with that id belongs to this owner.
    NotFound,
    /// The task is already terminal (`tasks/cancel`, or input for it).
    AlreadyTerminal,
    /// The registry is full of running tasks.
    CapacityExhausted,
    /// This owner already runs as many tasks as it may: the per-requestor
    /// limit, so one caller cannot starve every other.
    OwnerLimitReached,
    /// A `tasks/list` cursor this backend did not issue.
    InvalidCursor,
    /// The backend could not do it (an out-of-process store that is down, a
    /// task id that is already taken). Treated as an internal error.
    Unavailable(String),
}

/// How a task's underlying request ended.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum TaskOutcome {
    /// It succeeded with this result.
    Completed(Value),
    /// It answered, but the answer is a failure: a `2025-11-25` tool result
    /// with `isError: true`. The task is `failed`, and its outcome is still
    /// exactly this result, as the request itself would have answered.
    FailedResult {
        /// The result.
        result: Value,
        /// Why, for `statusMessage`.
        message: Option<String>,
    },
    /// It failed with this JSON-RPC error; the task is `failed`.
    Error(JsonRpcError),
}

/// Resolves to the client's answer to one input request, or `None` when the
/// task ends (or is forgotten) first.
pub type InputWaiter = BoxFuture<'static, Option<Value>>;

/// Pluggable task storage, fronted by both wires.
///
/// The bundled [`TaskStore`] is the in-memory default; a shared store (a
/// database, a cache) lets tasks outlive a process or be read from any
/// replica. All methods are async so a backend may live out of process.
///
/// Contract notes for implementors:
/// - a task begins `working`, moves between `working` and `input_required`,
///   and reaches a terminal status once; terminal statuses never transition
///   again, and a late [`complete`](Self::complete) is a no-op;
/// - a task still running at its TTL fails, and a finished one may be deleted
///   a TTL after it finished;
/// - every owner-taking method treats another owner's task as unknown
///   ([`TaskError::NotFound`]);
/// - [`create`](Self::create) returns only once the task is durably stored
///   ("A server MUST NOT return `CreateTaskResult` until the task is durably
///   created");
/// - the `cancel` token handed to `create` stops the task's work, and the
///   work, like the token and every [`InputWaiter`], is process-local. A
///   shared backend reaches work on another replica its own way (pub/sub),
///   or routes each task's requests to the replica running it.
#[async_trait]
pub trait TaskBackend: Send + Sync {
    /// Create a `working` task for `owner`, driven by `cancel`.
    async fn create(
        &self,
        owner: &TaskOwner,
        task: NewTask,
        cancel: CancellationToken,
    ) -> Result<TaskSnapshot, TaskError>;

    /// Record how the task's underlying request ended. A no-op if the task is
    /// already terminal or gone.
    async fn complete(&self, task_id: &str, outcome: TaskOutcome);

    /// The task's current state.
    async fn get(&self, owner: &TaskOwner, task_id: &str) -> Result<TaskSnapshot, TaskError>;

    /// The owner's tasks, oldest first, paginated. The cursor is
    /// backend-defined and opaque to clients.
    async fn list(
        &self,
        owner: &TaskOwner,
        cursor: Option<&str>,
        page_size: usize,
    ) -> Result<(Vec<TaskSnapshot>, Option<String>), TaskError>;

    /// Fire the task's token and mark it `cancelled`.
    /// [`TaskError::AlreadyTerminal`] if it had already finished.
    async fn cancel(&self, owner: &TaskOwner, task_id: &str) -> Result<TaskSnapshot, TaskError>;

    /// Wait until the task is terminal, then return its outcome.
    async fn wait_result(
        &self,
        owner: &TaskOwner,
        task_id: &str,
    ) -> Result<Result<Value, JsonRpcError>, TaskError>;

    /// The task's work needs input: record `request` as outstanding under a
    /// key derived from `key` and unique over the task's lifetime ("keys MUST
    /// be unique ... and never reused"), mark a `working` task
    /// `input_required`, and return what resolves with the answer.
    async fn request_input(
        &self,
        task_id: &str,
        key: &str,
        request: Value,
    ) -> Result<InputWaiter, TaskError>;

    /// The client's answers to outstanding input requests, by key. Answers
    /// for keys that aren't outstanding are ignored, and a partial set is
    /// accepted; a task left waiting on nothing goes back to `working`.
    /// Returns whether the status changed.
    async fn provide_input(
        &self,
        owner: &TaskOwner,
        task_id: &str,
        responses: &Map<String, Value>,
    ) -> Result<bool, TaskError>;

    /// A `2025-11-25` session ended: cancel its running tasks and forget all
    /// of them. The default does nothing, which suits a backend whose tasks
    /// outlive sessions by design.
    async fn end_session(&self, _session_id: &str) {}
}

/// A task's in-execution input state: what its work is waiting on.
#[derive(Default)]
struct Inputs {
    /// Outstanding input requests, keyed by their task-unique key.
    outstanding: Map<String, Value>,
    /// What resolves each outstanding key's waiter.
    waiters: HashMap<String, oneshot::Sender<Value>>,
    /// Every key ever issued for this task, so none is reused.
    used: HashSet<String>,
}

impl Inputs {
    /// Stop waiting: dropping the senders resolves every waiter with `None`.
    fn clear(&mut self) {
        self.waiters.clear();
        self.outstanding.clear();
    }
}

struct Entry {
    owner: TaskOwner,
    status: TaskStatus,
    status_message: Option<String>,
    created_wall: OffsetDateTime,
    updated_wall: OffsetDateTime,
    created: Instant,
    /// When it reached a terminal status.
    finished: Option<Instant>,
    ttl_ms: Option<i64>,
    poll_interval_ms: Option<i64>,
    outcome: Option<Result<Value, JsonRpcError>>,
    cancel: CancellationToken,
    /// Wakes `wait_result` on every transition; dropped with the entry.
    notify: watch::Sender<()>,
    inputs: Inputs,
}

impl Entry {
    fn ttl(&self) -> Option<Duration> {
        self.ttl_ms
            .map(|ms| Duration::from_millis(u64::try_from(ms).unwrap_or(0)))
    }

    /// When a running task runs out its TTL.
    fn deadline(&self) -> Option<Instant> {
        self.ttl().map(|ttl| self.created + ttl)
    }

    /// Past its TTL while still running.
    fn overdue(&self, now: Instant) -> bool {
        !self.status.is_terminal() && self.deadline().is_some_and(|deadline| now > deadline)
    }

    /// Finished, and kept for a full TTL since.
    fn collectable(&self, now: Instant) -> bool {
        match (self.finished, self.ttl()) {
            (Some(finished), Some(ttl)) => now.duration_since(finished) > ttl,
            _ => false,
        }
    }

    fn touch(&mut self) {
        self.updated_wall = OffsetDateTime::now_utc();
        let _ = self.notify.send(());
    }

    fn finish(
        &mut self,
        status: TaskStatus,
        message: Option<String>,
        outcome: Result<Value, JsonRpcError>,
    ) {
        self.status = status;
        self.status_message = message;
        self.outcome = Some(outcome);
        self.finished = Some(Instant::now());
        self.inputs.clear();
        self.touch();
    }

    fn snapshot(&self, id: &str) -> TaskSnapshot {
        TaskSnapshot {
            task_id: id.to_owned(),
            status: self.status,
            status_message: self.status_message.clone(),
            created_at: rfc3339(self.created_wall),
            last_updated_at: rfc3339(self.updated_wall),
            ttl_ms: self.ttl_ms,
            poll_interval_ms: self.poll_interval_ms,
            input_requests: self.inputs.outstanding.clone(),
            outcome: self.outcome.clone(),
        }
    }
}

fn rfc3339(t: OffsetDateTime) -> String {
    t.format(&Rfc3339)
        .unwrap_or_else(|_| String::from("1970-01-01T00:00:00Z"))
}

type IdGenerator = Arc<dyn Fn() -> String + Send + Sync>;

/// Bounded, in-memory task registry: the default [`TaskBackend`] for both
/// wires.
pub struct TaskStore {
    inner: Mutex<HashMap<String, Entry>>,
    capacity: usize,
    owner_limit: usize,
    poll_interval_ms: Option<i64>,
    ids: IdGenerator,
}

impl core::fmt::Debug for TaskStore {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TaskStore")
            .field("capacity", &self.capacity)
            .field("owner_limit", &self.owner_limit)
            .field("poll_interval_ms", &self.poll_interval_ms)
            .field("live", &self.inner.lock().map(|m| m.len()).unwrap_or(0))
            .finish_non_exhaustive()
    }
}

impl Default for TaskStore {
    fn default() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
            capacity: Self::DEFAULT_CAPACITY,
            owner_limit: Self::DEFAULT_OWNER_LIMIT,
            poll_interval_ms: Some(Self::DEFAULT_POLL_INTERVAL_MS),
            ids: Arc::new(|| uuid::Uuid::new_v4().to_string()),
        }
    }
}

impl TaskStore {
    /// How many tasks the store holds, across every owner, by default.
    pub const DEFAULT_CAPACITY: usize = 1024;
    /// How many tasks one owner may have running at once, by default.
    pub const DEFAULT_OWNER_LIMIT: usize = 64;
    /// The polling interval suggested when the front-end names none, in
    /// milliseconds.
    pub const DEFAULT_POLL_INTERVAL_MS: i64 = 500;

    /// Hold at most `capacity` tasks. When full, the oldest finished task makes
    /// room; a store full of running tasks refuses new ones.
    #[must_use]
    pub fn with_capacity(mut self, capacity: usize) -> Self {
        self.capacity = capacity.max(1);
        self
    }

    /// Let one owner have at most `limit` tasks running at once ("Enforce
    /// limits on concurrent tasks per requestor").
    #[must_use]
    pub fn with_owner_limit(mut self, limit: usize) -> Self {
        self.owner_limit = limit.max(1);
        self
    }

    /// The polling interval to suggest when the front-end names none
    /// (`None`: suggest nothing).
    #[must_use]
    pub fn with_poll_interval_ms(mut self, poll_interval_ms: Option<i64>) -> Self {
        self.poll_interval_ms = poll_interval_ms;
        self
    }

    /// Mint task ids with `ids` instead of random UUIDs: to encode the
    /// replica in each id, say, so a load balancer can route a task's
    /// requests to the replica running it. Each id must be new, and should
    /// be unguessable; a duplicate fails the create.
    #[must_use]
    pub fn with_id_generator<F>(mut self, ids: F) -> Self
    where
        F: Fn() -> String + Send + Sync + 'static,
    {
        self.ids = Arc::new(ids);
        self
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Entry>> {
        self.inner.lock().expect("task store lock poisoned")
    }

    /// The store, with every task that ran out its TTL failed and every
    /// finished one past retention gone.
    fn live(&self) -> std::sync::MutexGuard<'_, HashMap<String, Entry>> {
        let mut map = self.lock();
        let now = Instant::now();
        for entry in map.values_mut() {
            if entry.overdue(now) {
                entry.cancel.cancel();
                let ttl = entry.ttl_ms.unwrap_or_default();
                let message = format!("the task did not finish within its TTL of {ttl} ms");
                let error = JsonRpcError {
                    code: turbomcp_core::codes::INTERNAL_ERROR,
                    message: message.clone(),
                    data: None,
                };
                entry.finish(TaskStatus::Failed, Some(message), Err(error));
            }
        }
        map.retain(|_, entry| !entry.collectable(now));
        map
    }
}

/// `id`'s entry, if `owner` holds it.
fn owned<'a>(
    map: &'a mut HashMap<String, Entry>,
    owner: &TaskOwner,
    id: &str,
) -> Result<&'a mut Entry, TaskError> {
    match map.get_mut(id) {
        Some(entry) if &entry.owner == owner => Ok(entry),
        _ => Err(TaskError::NotFound),
    }
}

#[async_trait]
impl TaskBackend for TaskStore {
    async fn create(
        &self,
        owner: &TaskOwner,
        task: NewTask,
        cancel: CancellationToken,
    ) -> Result<TaskSnapshot, TaskError> {
        let mut map = self.live();
        let running = map
            .values()
            .filter(|e| &e.owner == owner && !e.status.is_terminal())
            .count();
        if running >= self.owner_limit {
            return Err(TaskError::OwnerLimitReached);
        }
        if map.len() >= self.capacity {
            // A finished task is only waiting to be collected; the oldest one
            // makes room. Running tasks are never evicted.
            let oldest_finished = map
                .iter()
                .filter_map(|(id, e)| e.finished.map(|at| (at, id.clone())))
                .min()
                .map(|(_, id)| id);
            match oldest_finished {
                Some(id) => {
                    map.remove(&id);
                }
                None => return Err(TaskError::CapacityExhausted),
            }
        }
        let id = (self.ids)();
        if map.contains_key(&id) {
            return Err(TaskError::Unavailable(
                "the task id generator returned an id already in use".to_owned(),
            ));
        }
        let now_wall = OffsetDateTime::now_utc();
        let entry = Entry {
            owner: owner.clone(),
            status: TaskStatus::Working,
            status_message: None,
            created_wall: now_wall,
            updated_wall: now_wall,
            created: Instant::now(),
            finished: None,
            ttl_ms: task.ttl_ms,
            poll_interval_ms: task.poll_interval_ms.or(self.poll_interval_ms),
            outcome: None,
            cancel,
            notify: watch::channel(()).0,
            inputs: Inputs::default(),
        };
        let snapshot = entry.snapshot(&id);
        map.insert(id, entry);
        Ok(snapshot)
    }

    async fn complete(&self, task_id: &str, outcome: TaskOutcome) {
        let mut map = self.lock();
        let Some(entry) = map.get_mut(task_id) else {
            return;
        };
        if entry.status.is_terminal() {
            return;
        }
        let (status, message, outcome) = match outcome {
            TaskOutcome::Completed(v) => (TaskStatus::Completed, None, Ok(v)),
            TaskOutcome::FailedResult { result, message } => {
                (TaskStatus::Failed, message, Ok(result))
            }
            TaskOutcome::Error(e) => (TaskStatus::Failed, Some(e.message.clone()), Err(e)),
        };
        entry.finish(status, message, outcome);
    }

    async fn get(&self, owner: &TaskOwner, task_id: &str) -> Result<TaskSnapshot, TaskError> {
        let mut map = self.live();
        owned(&mut map, owner, task_id).map(|entry| entry.snapshot(task_id))
    }

    async fn list(
        &self,
        owner: &TaskOwner,
        cursor: Option<&str>,
        page_size: usize,
    ) -> Result<(Vec<TaskSnapshot>, Option<String>), TaskError> {
        let offset = match cursor {
            None => 0,
            Some(c) => c.parse::<usize>().map_err(|_| TaskError::InvalidCursor)?,
        };
        let map = self.live();
        let mut all: Vec<(&String, &Entry)> =
            map.iter().filter(|(_, e)| &e.owner == owner).collect();
        all.sort_by_key(|(_, e)| e.created);
        let page: Vec<TaskSnapshot> = all
            .iter()
            .skip(offset)
            .take(page_size)
            .map(|(id, e)| e.snapshot(id))
            .collect();
        let next = (offset + page.len() < all.len()).then(|| (offset + page.len()).to_string());
        Ok((page, next))
    }

    async fn cancel(&self, owner: &TaskOwner, task_id: &str) -> Result<TaskSnapshot, TaskError> {
        let mut map = self.live();
        let entry = owned(&mut map, owner, task_id)?;
        if entry.status.is_terminal() {
            return Err(TaskError::AlreadyTerminal);
        }
        entry.cancel.cancel();
        // The underlying request never finished; its "result" is the
        // cancellation error. The code is implementation-defined: MCP's
        // error-code allocation policy reserves `-32000..-32019` for
        // implementations.
        let error = JsonRpcError {
            code: turbomcp_core::codes::TASK_CANCELLED,
            message: "task cancelled".to_owned(),
            data: None,
        };
        entry.finish(
            TaskStatus::Cancelled,
            Some("the task was cancelled by request".to_owned()),
            Err(error),
        );
        Ok(entry.snapshot(task_id))
    }

    async fn wait_result(
        &self,
        owner: &TaskOwner,
        task_id: &str,
    ) -> Result<Result<Value, JsonRpcError>, TaskError> {
        loop {
            let (mut changed, deadline) = {
                let mut map = self.live();
                let entry = owned(&mut map, owner, task_id)?;
                if entry.status.is_terminal() {
                    return Ok(entry.outcome.clone().unwrap_or_else(|| {
                        Err(JsonRpcError {
                            code: turbomcp_core::codes::INTERNAL_ERROR,
                            message: "task finished without an outcome".to_owned(),
                            data: None,
                        })
                    }));
                }
                (entry.notify.subscribe(), entry.deadline())
            }; // the lock is released before waiting
            let woke = match deadline {
                // Nothing touches the store when a task runs out its TTL, so
                // wake then and look again: the TTL fails it.
                Some(deadline) => {
                    let deadline = tokio::time::Instant::from_std(deadline);
                    tokio::time::timeout_at(deadline, changed.changed())
                        .await
                        .unwrap_or(Ok(()))
                }
                None => changed.changed().await,
            };
            if woke.is_err() {
                // The entry, and its sender, are gone.
                return Err(TaskError::NotFound);
            }
        }
    }

    async fn request_input(
        &self,
        task_id: &str,
        key: &str,
        request: Value,
    ) -> Result<InputWaiter, TaskError> {
        let mut map = self.live();
        let entry = map.get_mut(task_id).ok_or(TaskError::NotFound)?;
        if entry.status.is_terminal() {
            return Err(TaskError::AlreadyTerminal);
        }
        // The work's key as it is when fresh, else `key#2`, `key#3`, ...
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
        }
        entry.touch();
        Ok(rx.map(Result::ok).boxed())
    }

    async fn provide_input(
        &self,
        owner: &TaskOwner,
        task_id: &str,
        responses: &Map<String, Value>,
    ) -> Result<bool, TaskError> {
        let mut map = self.live();
        let entry = owned(&mut map, owner, task_id)?;
        for (key, value) in responses {
            if entry.inputs.outstanding.remove(key).is_some()
                && let Some(waiter) = entry.inputs.waiters.remove(key)
            {
                // A closed receiver means the work already unwound; the
                // answer is dropped.
                let _ = waiter.send(value.clone());
            }
        }
        if entry.status == TaskStatus::InputRequired && entry.inputs.outstanding.is_empty() {
            entry.status = TaskStatus::Working;
            entry.touch();
            return Ok(true);
        }
        Ok(false)
    }

    async fn end_session(&self, session_id: &str) {
        let mut map = self.lock();
        map.retain(|_, e| {
            if !matches!(&e.owner, TaskOwner::Session(s) if s == session_id) {
                return true;
            }
            if !e.status.is_terminal() {
                e.cancel.cancel();
            }
            false
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn session(id: &str) -> TaskOwner {
        TaskOwner::Session(id.to_owned())
    }

    fn sess() -> TaskOwner {
        session("sess")
    }

    async fn create(s: &TaskStore, owner: &TaskOwner, ttl_ms: Option<i64>) -> TaskSnapshot {
        s.create(owner, NewTask::new(ttl_ms), CancellationToken::new())
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn lifecycle_working_to_completed() {
        let s = TaskStore::default();
        let snap = create(&s, &sess(), Some(60_000)).await;
        assert_eq!(snap.status, TaskStatus::Working);
        assert_eq!(snap.ttl_ms, Some(60_000));
        assert_eq!(
            snap.poll_interval_ms,
            Some(TaskStore::DEFAULT_POLL_INTERVAL_MS)
        );
        // RFC 3339 timestamps render.
        assert!(snap.created_at.contains('T'));

        s.complete(&snap.task_id, TaskOutcome::Completed(json!({"done": true})))
            .await;
        let got = s.get(&sess(), &snap.task_id).await.unwrap();
        assert_eq!(got.status, TaskStatus::Completed);
        assert_eq!(got.outcome.unwrap().unwrap()["done"], true);

        let outcome = s.wait_result(&sess(), &snap.task_id).await.unwrap();
        assert_eq!(outcome.unwrap()["done"], true);
    }

    #[tokio::test]
    async fn wait_result_blocks_until_terminal() {
        let s = Arc::new(TaskStore::default());
        let snap = create(&s, &sess(), None).await;
        let waiter = {
            let s = Arc::clone(&s);
            let id = snap.task_id.clone();
            tokio::spawn(async move { s.wait_result(&sess(), &id).await })
        };
        tokio::task::yield_now().await;
        s.complete(&snap.task_id, TaskOutcome::Completed(json!("late")))
            .await;
        let outcome = waiter.await.unwrap().unwrap();
        assert_eq!(outcome.unwrap(), json!("late"));
    }

    /// Nothing touches the store when a task runs out its TTL, so a blocked
    /// `wait_result` has to wake for it on its own.
    #[tokio::test]
    async fn wait_result_wakes_when_the_ttl_fails_the_task() {
        let s = TaskStore::default();
        let snap = create(&s, &sess(), Some(30)).await;
        let outcome = tokio::time::timeout(
            Duration::from_secs(5),
            s.wait_result(&sess(), &snap.task_id),
        )
        .await
        .expect("woke at the deadline")
        .unwrap();
        assert!(outcome.unwrap_err().message.contains("TTL"));
    }

    #[tokio::test]
    async fn cancel_fires_token_and_rejects_terminal() {
        let s = TaskStore::default();
        let token = CancellationToken::new();
        let snap = s
            .create(&sess(), NewTask::new(Some(1000)), token.clone())
            .await
            .unwrap();

        let cancelled = s.cancel(&sess(), &snap.task_id).await.unwrap();
        assert_eq!(cancelled.status, TaskStatus::Cancelled);
        assert!(token.is_cancelled());

        // Terminal cancel → AlreadyTerminal; completion after cancel is a no-op.
        assert_eq!(
            s.cancel(&sess(), &snap.task_id).await,
            Err(TaskError::AlreadyTerminal)
        );
        s.complete(&snap.task_id, TaskOutcome::Completed(json!("too late")))
            .await;
        assert_eq!(
            s.get(&sess(), &snap.task_id).await.unwrap().status,
            TaskStatus::Cancelled
        );

        let outcome = s.wait_result(&sess(), &snap.task_id).await.unwrap();
        assert_eq!(
            outcome.unwrap_err().code,
            turbomcp_core::codes::TASK_CANCELLED
        );
    }

    #[tokio::test]
    async fn another_owner_sees_nothing() {
        let s = TaskStore::default();
        let snap = create(&s, &session("alice"), None).await;
        for other in [
            session("mallory"),
            TaskOwner::Principal("alice".into()),
            TaskOwner::Anonymous,
        ] {
            assert_eq!(s.get(&other, &snap.task_id).await, Err(TaskError::NotFound));
            assert_eq!(
                s.cancel(&other, &snap.task_id).await,
                Err(TaskError::NotFound)
            );
            assert_eq!(
                s.provide_input(&other, &snap.task_id, &Map::new()).await,
                Err(TaskError::NotFound)
            );
            assert!(s.list(&other, None, 10).await.unwrap().0.is_empty());
        }
        assert_eq!(
            s.list(&session("alice"), None, 10).await.unwrap().0.len(),
            1
        );
    }

    #[tokio::test]
    async fn list_paginates_with_offset_cursor() {
        let s = TaskStore::default();
        for _ in 0..5 {
            create(&s, &sess(), None).await;
        }
        let (first, next) = s.list(&sess(), None, 2).await.unwrap();
        assert_eq!(first.len(), 2);
        let (second, next2) = s.list(&sess(), next.as_deref(), 2).await.unwrap();
        assert_eq!(second.len(), 2);
        let (third, end) = s.list(&sess(), next2.as_deref(), 2).await.unwrap();
        assert_eq!(third.len(), 1);
        assert!(end.is_none());
        assert_eq!(
            s.list(&sess(), Some("bogus"), 2).await,
            Err(TaskError::InvalidCursor)
        );
    }

    /// "When the tool result has `isError` set to `true`, the task should reach
    /// `failed` status", and `tasks/result` still returns that result.
    #[tokio::test]
    async fn a_failed_result_is_a_failed_task_that_still_returns_its_result() {
        let s = TaskStore::default();
        let snap = create(&s, &sess(), None).await;
        s.complete(
            &snap.task_id,
            TaskOutcome::FailedResult {
                result: json!({ "isError": true }),
                message: Some("the tool reported an error".into()),
            },
        )
        .await;
        let got = s.get(&sess(), &snap.task_id).await.unwrap();
        assert_eq!(got.status, TaskStatus::Failed);
        assert_eq!(
            got.status_message.as_deref(),
            Some("the tool reported an error")
        );
        let outcome = s.wait_result(&sess(), &snap.task_id).await.unwrap();
        assert_eq!(outcome.unwrap()["isError"], true);
    }

    #[tokio::test]
    async fn an_error_outcome_carries_its_message() {
        let s = TaskStore::default();
        let snap = create(&s, &TaskOwner::Anonymous, None).await;
        let error = JsonRpcError {
            code: turbomcp_core::codes::INTERNAL_ERROR,
            message: "boom".into(),
            data: None,
        };
        s.complete(&snap.task_id, TaskOutcome::Error(error.clone()))
            .await;
        let got = s.get(&TaskOwner::Anonymous, &snap.task_id).await.unwrap();
        assert_eq!(got.status, TaskStatus::Failed);
        assert_eq!(got.status_message.as_deref(), Some("boom"));
        assert_eq!(got.outcome, Some(Err(error)));
    }

    /// One owner cannot hold more than its share, and a full store makes room
    /// by dropping finished tasks, never running ones.
    #[tokio::test]
    async fn limits_are_per_owner_and_capacity_evicts_finished_tasks() {
        let s = TaskStore::default().with_capacity(3).with_owner_limit(2);
        let a1 = create(&s, &session("a"), None).await;
        create(&s, &session("a"), None).await;
        assert_eq!(
            s.create(&session("a"), NewTask::new(None), CancellationToken::new())
                .await,
            Err(TaskError::OwnerLimitReached)
        );
        // Another owner is unaffected.
        create(&s, &TaskOwner::Principal("b".into()), None).await;
        // Full, and nothing is finished.
        assert_eq!(
            s.create(&session("c"), NewTask::new(None), CancellationToken::new())
                .await,
            Err(TaskError::CapacityExhausted)
        );
        // Once one finishes, it makes room.
        s.complete(&a1.task_id, TaskOutcome::Completed(json!(1)))
            .await;
        create(&s, &session("c"), None).await;
        assert_eq!(
            s.get(&session("a"), &a1.task_id).await,
            Err(TaskError::NotFound)
        );
    }

    /// "servers MAY mark a task as `failed` at any point after the TTL
    /// elapses, and subsequently delete it". Deleting it outright would leave
    /// the poller with an unknown task and no idea what happened.
    #[tokio::test]
    async fn an_overdue_task_fails_visibly_and_is_deleted_a_ttl_later() {
        let s = TaskStore::default();
        let cancel = CancellationToken::new();
        let short = s
            .create(&sess(), NewTask::new(Some(30)), cancel.clone())
            .await
            .unwrap();
        let unlimited = create(&s, &sess(), None).await;

        tokio::time::sleep(Duration::from_millis(45)).await;
        let overdue = s.get(&sess(), &short.task_id).await.unwrap();
        assert_eq!(overdue.status, TaskStatus::Failed);
        assert!(
            overdue
                .status_message
                .as_deref()
                .is_some_and(|m| m.contains("TTL")),
            "{:?}",
            overdue.status_message
        );
        assert!(cancel.is_cancelled(), "its work is told to stop");

        tokio::time::sleep(Duration::from_millis(45)).await;
        assert_eq!(
            s.get(&sess(), &short.task_id).await,
            Err(TaskError::NotFound),
            "collected a TTL after finishing"
        );
        assert!(
            s.get(&sess(), &unlimited.task_id).await.is_ok(),
            "only None is unlimited"
        );
    }

    /// Nothing can ask about a dead session's tasks, so they are cancelled and
    /// released rather than left holding capacity until their TTL.
    #[tokio::test]
    async fn ending_a_session_cancels_and_releases_its_tasks() {
        let s = TaskStore::default();
        let token = CancellationToken::new();
        let snap = s
            .create(&session("gone"), NewTask::new(None), token.clone())
            .await
            .unwrap();
        let kept = create(&s, &session("kept"), None).await;
        let principal = create(&s, &TaskOwner::Principal("gone".into()), None).await;
        s.end_session("gone").await;
        assert!(token.is_cancelled());
        assert_eq!(
            s.get(&session("gone"), &snap.task_id).await,
            Err(TaskError::NotFound)
        );
        assert!(s.get(&session("kept"), &kept.task_id).await.is_ok());
        assert!(
            s.get(&TaskOwner::Principal("gone".into()), &principal.task_id)
                .await
                .is_ok(),
            "a principal's tasks don't end with a session of the same name"
        );
    }

    #[tokio::test]
    async fn the_front_end_poll_interval_wins_over_the_store_default() {
        let s = TaskStore::default().with_poll_interval_ms(Some(10));
        let defaulted = create(&s, &sess(), None).await;
        assert_eq!(defaulted.poll_interval_ms, Some(10));
        let named = s
            .create(
                &sess(),
                NewTask::new(None).with_poll_interval_ms(Some(250)),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(named.poll_interval_ms, Some(250));
    }

    #[tokio::test]
    async fn a_custom_id_generator_names_tasks_and_a_duplicate_fails() {
        let s = TaskStore::default().with_id_generator(|| "replica-a.1".to_owned());
        let first = create(&s, &sess(), None).await;
        assert_eq!(first.task_id, "replica-a.1");
        assert!(matches!(
            s.create(&sess(), NewTask::new(None), CancellationToken::new())
                .await,
            Err(TaskError::Unavailable(_))
        ));
    }

    #[tokio::test]
    async fn input_flow_request_get_provide_resume() {
        let s = TaskStore::default();
        let owner = TaskOwner::Principal("alice".into());
        let task = create(&s, &owner, None).await;

        // Requesting flips a working task to input_required…
        let waiter = s
            .request_input(
                &task.task_id,
                "confirm",
                json!({"method": "elicitation/create"}),
            )
            .await
            .unwrap();
        let got = s.get(&owner, &task.task_id).await.unwrap();
        assert_eq!(got.status, TaskStatus::InputRequired);
        // …and the snapshot carries every outstanding request.
        assert!(got.input_requests.contains_key("confirm"));

        // Answers for keys never issued are ignored; still waiting.
        let mut wrong = Map::new();
        wrong.insert("never-issued".into(), json!({}));
        assert_eq!(
            s.provide_input(&owner, &task.task_id, &wrong).await,
            Ok(false)
        );

        // The matching answer resolves the waiter and the task works again.
        let mut responses = Map::new();
        responses.insert("confirm".into(), json!({"action": "accept"}));
        assert_eq!(
            s.provide_input(&owner, &task.task_id, &responses).await,
            Ok(true)
        );
        assert_eq!(waiter.await.unwrap()["action"], "accept");
        let got = s.get(&owner, &task.task_id).await.unwrap();
        assert_eq!(got.status, TaskStatus::Working);
        assert!(got.input_requests.is_empty());

        // A second answer for the same key is already satisfied: ignored.
        let mut again = Map::new();
        again.insert("confirm".into(), json!({"action": "decline"}));
        assert_eq!(
            s.provide_input(&owner, &task.task_id, &again).await,
            Ok(false)
        );
    }

    #[tokio::test]
    async fn input_keys_are_unique_over_the_task_lifetime() {
        let s = TaskStore::default();
        let task = create(&s, &sess(), None).await;
        let _first = s
            .request_input(&task.task_id, "confirm", json!({"n": 1}))
            .await
            .unwrap();
        let _second = s
            .request_input(&task.task_id, "confirm", json!({"n": 2}))
            .await
            .unwrap();
        let outstanding = s.get(&sess(), &task.task_id).await.unwrap().input_requests;
        assert_eq!(outstanding.len(), 2);
        assert!(outstanding.contains_key("confirm"));
        assert!(outstanding.contains_key("confirm#2"));
    }

    #[tokio::test]
    async fn cancel_unblocks_an_awaiting_input() {
        let s = TaskStore::default();
        let task = create(&s, &sess(), None).await;
        let waiter = s
            .request_input(&task.task_id, "confirm", json!({}))
            .await
            .unwrap();

        s.cancel(&sess(), &task.task_id).await.unwrap();
        // The waiter resolves empty, so awaiting work unwinds instead of
        // hanging.
        assert_eq!(waiter.await, None);
        // A terminal task takes no new input requests…
        assert!(matches!(
            s.request_input(&task.task_id, "again", json!({})).await,
            Err(TaskError::AlreadyTerminal)
        ));
        // …and no longer shows the old ones.
        assert!(
            s.get(&sess(), &task.task_id)
                .await
                .unwrap()
                .input_requests
                .is_empty()
        );
    }
}
