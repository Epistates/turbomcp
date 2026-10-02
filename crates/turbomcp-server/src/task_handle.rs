//! A handler's way to the task it runs as: [`TaskHandle`] on its context, and
//! the [`TaskLink`] a Tasks front-end binds it to.
//!
//! Whether a call becomes a task is decided after its context is built (on
//! `2026-07-28` the Tasks extension decides once it sees the prepared call),
//! so the handle is late-bound: the dispatcher shares one [`TaskSlot`]
//! between the context and the [`CallRunner`](crate::CallRunner), and the
//! front-end that creates the task fills it before the work starts. A call
//! that never becomes a task keeps an empty slot, and its handle stays inert.

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use futures::future::BoxFuture;
use serde_json::Value;
use turbomcp_core::{CancellationToken, McpError, McpResult};

use crate::tasks::{TaskBackend, TaskUpdate};

/// Called with a task's id whenever its work changed it, so a front-end can
/// tell subscribers.
type ChangeHook = Arc<dyn Fn(String) -> BoxFuture<'static, ()> + Send + Sync>;

/// What a task's work reaches its task through: the store, the task's id
/// and cancellation, and who to tell when the work changes it.
#[derive(Clone)]
pub struct TaskLink {
    store: Arc<dyn TaskBackend>,
    task_id: String,
    cancel: CancellationToken,
    on_change: Option<ChangeHook>,
}

impl core::fmt::Debug for TaskLink {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TaskLink")
            .field("task_id", &self.task_id)
            .finish_non_exhaustive()
    }
}

impl TaskLink {
    /// The link to task `task_id` in `store`, whose work stops on `cancel`.
    #[must_use]
    pub fn new(
        store: Arc<dyn TaskBackend>,
        task_id: impl Into<String>,
        cancel: CancellationToken,
    ) -> Self {
        Self {
            store,
            task_id: task_id.into(),
            cancel,
            on_change: None,
        }
    }

    /// Run `hook` with the task's id each time the work changes the task
    /// (its status message, its polling interval, a request for input). The
    /// Tasks extension pushes `notifications/tasks` from here.
    #[must_use]
    pub fn on_change<F, Fut>(mut self, hook: F) -> Self
    where
        F: Fn(String) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.on_change = Some(Arc::new(move |task_id| Box::pin(hook(task_id))));
        self
    }

    /// The task's id.
    #[must_use]
    pub fn task_id(&self) -> &str {
        &self.task_id
    }

    async fn changed(&self) {
        if let Some(hook) = &self.on_change {
            hook(self.task_id.clone()).await;
        }
    }

    pub(crate) async fn update(&self, update: TaskUpdate) {
        match self.store.update(&self.task_id, update).await {
            Ok(true) => self.changed().await,
            Ok(false) => {}
            Err(e) => tracing::debug!(error = ?e, task = %self.task_id, "task update dropped"),
        }
    }

    /// Ask the client for input through the task: publish the request, so
    /// the task reads `input_required` with it outstanding, and wait for
    /// `tasks/update` to answer it (SEP-2663 §Task Update Requests).
    pub(crate) async fn request_input(&self, key: &str, request: Value) -> McpResult<Value> {
        let answer = self
            .store
            .request_input(&self.task_id, key, request)
            .await
            .map_err(|_| {
                McpError::internal("the task is no longer live; client input is unavailable")
            })?;
        self.changed().await;
        tokio::select! {
            () = self.cancel.cancelled() => Err(McpError::internal(
                "the task was cancelled while awaiting client input",
            )),
            answer = answer => answer.ok_or_else(|| {
                McpError::internal("the task ended before the client answered")
            }),
        }
    }
}

/// The late-bound [`TaskLink`] a call's context shares with its
/// [`CallRunner`](crate::CallRunner); filled once, before the work starts,
/// if the call becomes a task.
pub type TaskSlot = Arc<OnceLock<TaskLink>>;

/// The task a handler runs as, if it runs as one: `ctx.task`.
///
/// Inert outside a task, so a handler can report unconditionally:
///
/// ```ignore
/// ctx.task.set_status_message("indexing shard 3 of 8").await;
/// ```
///
/// Inside a task, [`ctx.progress`](crate::ProgressReporter) reports land in
/// the task's status message too, which on `2026-07-28` is the only progress
/// channel a task has ("`notifications/progress` ... are not supported on
/// tasks").
#[derive(Clone, Debug, Default)]
pub struct TaskHandle {
    slot: Option<TaskSlot>,
}

impl TaskHandle {
    /// A handle for a call that cannot become a task.
    #[must_use]
    pub(crate) fn disabled() -> Self {
        Self::default()
    }

    /// A handle that becomes live once `slot` is filled.
    #[must_use]
    pub(crate) fn bound(slot: TaskSlot) -> Self {
        Self { slot: Some(slot) }
    }

    pub(crate) fn link(&self) -> Option<&TaskLink> {
        self.slot.as_ref().and_then(|slot| slot.get())
    }

    /// Whether this call is running as a task.
    #[must_use]
    pub fn is_task(&self) -> bool {
        self.link().is_some()
    }

    /// The task's id, when this call runs as one.
    #[must_use]
    pub fn id(&self) -> Option<&str> {
        self.link().map(TaskLink::task_id)
    }

    /// Say what the work is doing, as the task's `statusMessage`. A no-op
    /// outside a task.
    pub async fn set_status_message(&self, message: impl Into<String>) {
        if let Some(link) = self.link() {
            link.update(TaskUpdate::status_message(message)).await;
        }
    }

    /// Suggest how often the client should poll from now on. A no-op outside
    /// a task.
    pub async fn set_poll_interval(&self, interval: Duration) {
        if let Some(link) = self.link() {
            let ms = i64::try_from(interval.as_millis()).unwrap_or(i64::MAX);
            link.update(TaskUpdate::poll_interval_ms(ms)).await;
        }
    }
}
