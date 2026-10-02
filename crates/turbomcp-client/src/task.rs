//! Tasks from the client's side: a typed view of a task ([`TaskInfo`]), and
//! a task a tool call became that the caller drives, inspects or resumes
//! ([`ToolTask`]).
//!
//! [`Client::call_tool`](crate::Client::call_tool) drives a task to its end
//! without the caller seeing it. To keep the task instead (to come back to
//! it after a restart, as "Clients SHOULD persist task IDs to durable storage
//! so that polling can resume after a crash or restart" asks), call
//! [`Client::call_tool_detached`](crate::Client::call_tool_detached), store
//! [`ToolTask::id`], and later [`Client::resume_task`](crate::Client::resume_task).

use std::time::Duration;

use serde_json::{Map, Value};
use turbomcp_core::JsonRpcError;
use turbomcp_protocol::neutral;

use crate::{Client, ClientError, ClientResult};

/// A task's lifecycle status. Terminal statuses (`Completed`, `Failed`,
/// `Cancelled`) never change again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum TaskStatus {
    /// The request is being processed.
    Working,
    /// The server needs input from the client before it can go on.
    InputRequired,
    /// The request finished.
    Completed,
    /// The request failed.
    Failed,
    /// The task was cancelled before it finished.
    Cancelled,
}

impl TaskStatus {
    /// Whether this status never changes again.
    #[must_use]
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }

    fn parse(status: &str) -> Option<Self> {
        Some(match status {
            "working" => Self::Working,
            "input_required" => Self::InputRequired,
            "completed" => Self::Completed,
            "failed" => Self::Failed,
            "cancelled" => Self::Cancelled,
            _ => return None,
        })
    }
}

/// One task as the server last described it, on either revision.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct TaskInfo {
    /// The task's id: the handle to it, worth persisting.
    pub task_id: String,
    /// Where it is in its lifecycle.
    pub status: TaskStatus,
    /// What the server says it is doing, or how it ended.
    pub status_message: Option<String>,
    /// When it was created (RFC 3339, as the server wrote it).
    pub created_at: String,
    /// When it last changed (RFC 3339, as the server wrote it).
    pub last_updated_at: String,
    /// How long the server keeps it from creation; `None` is unlimited. It
    /// may change over the task's lifetime.
    pub ttl: Option<Duration>,
    /// How often the server suggests polling.
    pub poll_interval: Option<Duration>,
    /// The input requests it is waiting on, by key (`2026-07-28`; on
    /// `2025-11-25` the server sends them as ordinary requests instead).
    pub input_requests: Map<String, Value>,
    /// The result, once `completed` (`2026-07-28` inlines it; on
    /// `2025-11-25` it comes from `tasks/result`, which
    /// [`ToolTask::wait`] fetches).
    pub result: Option<Value>,
    /// The JSON-RPC error, once `failed` (`2026-07-28`).
    pub error: Option<JsonRpcError>,
}

impl TaskInfo {
    /// Read a task from the wire: a `tasks/get` result, a `tasks/list`
    /// entry, or a `CreateTaskResult`'s task, on either revision
    /// (`ttl`/`pollInterval` on `2025-11-25`, `ttlMs`/`pollIntervalMs` on
    /// `2026-07-28`).
    pub(crate) fn from_wire(value: &Value) -> ClientResult<Self> {
        let text = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_owned);
        let required = |key: &str| {
            text(key).ok_or_else(|| ClientError::Decode(format!("a task without `{key}`")))
        };
        let millis = |keys: [&str; 2]| {
            keys.iter()
                .find_map(|key| value.get(*key).and_then(Value::as_u64))
                .map(Duration::from_millis)
        };
        let status = required("status")?;
        Ok(Self {
            task_id: required("taskId")?,
            status: TaskStatus::parse(&status)
                .ok_or_else(|| ClientError::Decode(format!("unknown task status `{status}`")))?,
            status_message: text("statusMessage"),
            created_at: required("createdAt")?,
            last_updated_at: required("lastUpdatedAt")?,
            ttl: millis(["ttlMs", "ttl"]),
            poll_interval: millis(["pollIntervalMs", "pollInterval"]),
            input_requests: value
                .get("inputRequests")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default(),
            result: value.get("result").cloned(),
            error: value
                .get("error")
                .map(|e| serde_json::from_value(e.clone()))
                .transpose()
                .map_err(|e| ClientError::Decode(format!("a task's error: {e}")))?,
        })
    }
}

/// One page of `tasks/list`.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct TaskPage {
    /// The tasks on this page.
    pub tasks: Vec<TaskInfo>,
    /// Where the next page starts, if there is one.
    pub next_cursor: Option<String>,
}

/// How a [`Client::call_tool_detached`](crate::Client::call_tool_detached)
/// call came back.
#[derive(Debug)]
pub enum Detached {
    /// The server answered inline: there is no task.
    Done(neutral::CallToolResult),
    /// The call became a task, still running. (Boxed: a task holds a
    /// [`Client`].)
    Task(Box<ToolTask>),
}

/// A task a tool call became, to inspect, wait on, or cancel. Dropping it
/// leaves the task running: persist [`id`](Self::id) to come back to it with
/// [`Client::resume_task`](crate::Client::resume_task).
#[derive(Clone)]
pub struct ToolTask {
    client: Client,
    task_id: String,
    /// The tool it runs, when known, so the result can be held to the tool's
    /// `outputSchema`.
    tool: Option<String>,
}

impl core::fmt::Debug for ToolTask {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ToolTask")
            .field("task_id", &self.task_id)
            .field("tool", &self.tool)
            .finish_non_exhaustive()
    }
}

impl ToolTask {
    pub(crate) fn new(client: Client, task_id: String, tool: Option<String>) -> Self {
        Self {
            client,
            task_id,
            tool,
        }
    }

    /// The task's id.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.task_id
    }

    /// The task as the server describes it now (`tasks/get`).
    ///
    /// # Errors
    /// Propagates RPC failures (`-32602` for a task the server no longer
    /// has).
    pub async fn get(&self) -> ClientResult<TaskInfo> {
        self.client.task_get(&self.task_id).await
    }

    /// Ask the server to stop it (`tasks/cancel`). Cooperative: the task may
    /// still finish.
    ///
    /// # Errors
    /// Propagates RPC failures.
    pub async fn cancel(&self) -> ClientResult<()> {
        self.client.task_cancel(&self.task_id).await
    }

    /// Drive the task to its end, as
    /// [`Client::call_tool`](crate::Client::call_tool) would have: poll at the
    /// server's suggested interval, answer its input requests through the
    /// client's handlers, and return the tool's result. Dropping this future
    /// stops waiting and leaves the task running; [`cancel`](Self::cancel)
    /// stops it. The result is checked against the tool's `outputSchema`
    /// when the tool is known (a task from
    /// [`call_tool_detached`](crate::Client::call_tool_detached), not one from
    /// [`resume_task`](crate::Client::resume_task)).
    ///
    /// # Errors
    /// As [`Client::call_tool`](crate::Client::call_tool): a `failed` task
    /// surfaces the call's JSON-RPC error, a `cancelled` one is a
    /// [`ClientError::Protocol`], and one still unfinished at its TTL is a
    /// [`ClientError::Timeout`].
    pub async fn wait(&self) -> ClientResult<neutral::CallToolResult> {
        self.client
            .wait_task(&self.task_id, self.tool.as_deref())
            .await
    }
}
