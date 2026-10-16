//! Draft Tasks extension wire types (SEP-2663).
//!
//! The core `2026-07-28` schema defines none of these — the extension owns
//! its wire types (PLAN §12). They are deliberately serde-(de)serializable on
//! both ends: the server serializes them into responses/notifications, and a
//! client deserializes them while polling. Field renames follow the spec's
//! `camelCase`; status strings are the spec's lowercase/`snake_case` set.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// `resultType: "task"` — marks a [`CreateTaskResult`] (SEP-2663 §Polymorphic
/// Results).
pub const RESULT_TYPE_TASK: &str = "task";
/// `resultType: "complete"` — the standard result shape for `tasks/get`,
/// `tasks/update`, and `tasks/cancel`.
pub const RESULT_TYPE_COMPLETE: &str = "complete";

/// A task's lifecycle status (SEP-2663 §Task Status). Terminal statuses
/// (`completed`/`failed`/`cancelled`) never transition again.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    /// The request is being processed.
    Working,
    /// The server needs client input before it can proceed; outstanding
    /// requests are surfaced via `inputRequests` and fulfilled via
    /// `tasks/update`.
    InputRequired,
    /// The request completed; its result is inlined in `result`.
    Completed,
    /// The request failed with a JSON-RPC error during execution (inlined in
    /// `error`). Not used for tool-level `isError: true` — that is `completed`.
    Failed,
    /// The request was cancelled before completion.
    Cancelled,
}

impl TaskStatus {
    /// Whether this is a terminal status.
    #[must_use]
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

/// Operational metadata about ongoing work (SEP-2663 §Tasks). The base shape
/// shared by [`CreateTaskResult`] and [`DetailedTask`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    /// Stable identifier for this task.
    #[serde(rename = "taskId")]
    pub task_id: String,
    /// Current status.
    pub status: TaskStatus,
    /// Optional human-facing message describing the current state.
    #[serde(
        rename = "statusMessage",
        skip_serializing_if = "Option::is_none",
        default
    )]
    pub status_message: Option<String>,
    /// ISO 8601 creation timestamp.
    #[serde(rename = "createdAt")]
    pub created_at: String,
    /// ISO 8601 last-update timestamp.
    #[serde(rename = "lastUpdatedAt")]
    pub last_updated_at: String,
    /// Time-to-live from creation in integer milliseconds; `null` for
    /// unlimited. Always present (possibly null), per spec.
    #[serde(rename = "ttlMs", default)]
    pub ttl_ms: Option<i64>,
    /// Suggested polling interval in integer milliseconds.
    #[serde(
        rename = "pollIntervalMs",
        skip_serializing_if = "Option::is_none",
        default
    )]
    pub poll_interval_ms: Option<i64>,
}

impl Task {
    /// A fresh `working` task with the given id and timestamps.
    #[must_use]
    pub fn working(task_id: impl Into<String>, created_at: impl Into<String>) -> Self {
        let created_at = created_at.into();
        Self {
            task_id: task_id.into(),
            status: TaskStatus::Working,
            status_message: None,
            last_updated_at: created_at.clone(),
            created_at,
            ttl_ms: None,
            poll_interval_ms: None,
        }
    }
}

/// `CreateTaskResult` (`resultType: "task"`): returned in lieu of the standard
/// result to indicate the request will be processed asynchronously (SEP-2663
/// §Task Creation).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateTaskResult {
    /// Always [`RESULT_TYPE_TASK`].
    #[serde(rename = "resultType")]
    pub result_type: String,
    /// The seed task state (typically `working`).
    #[serde(flatten)]
    pub task: Task,
}

impl CreateTaskResult {
    /// Wrap `task` as a `resultType: "task"` create result.
    #[must_use]
    pub fn new(task: Task) -> Self {
        Self {
            result_type: RESULT_TYPE_TASK.to_owned(),
            task,
        }
    }
}

/// `GetTaskResult` / `notifications/tasks` payload (`DetailedTask`): the full
/// task state, with status-specific fields inlined (SEP-2663 §Task Polling).
/// The `resultType` is `"complete"` (the standard `tasks/get` result shape).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetailedTask {
    /// Always [`RESULT_TYPE_COMPLETE`].
    #[serde(rename = "resultType")]
    pub result_type: String,
    /// The base task metadata.
    #[serde(flatten)]
    pub task: Task,
    /// Outstanding server→client requests (present iff `input_required`).
    #[serde(
        rename = "inputRequests",
        skip_serializing_if = "Option::is_none",
        default
    )]
    pub input_requests: Option<Map<String, Value>>,
    /// The final result (present iff `completed`); shape matches the original
    /// request's result type (e.g. `CallToolResult`).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub result: Option<Value>,
    /// The JSON-RPC error that caused failure (present iff `failed`).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub error: Option<Value>,
}

impl DetailedTask {
    /// Build a detailed view of `task` with no status-specific extras.
    #[must_use]
    pub fn new(task: Task) -> Self {
        Self {
            result_type: RESULT_TYPE_COMPLETE.to_owned(),
            task,
            input_requests: None,
            result: None,
            error: None,
        }
    }

    /// Attach a completed task's `result`.
    #[must_use]
    pub fn with_result(mut self, result: Value) -> Self {
        self.result = Some(result);
        self
    }

    /// Attach a failed task's `error`.
    #[must_use]
    pub fn with_error(mut self, error: Value) -> Self {
        self.error = Some(error);
        self
    }

    /// Attach an `input_required` task's outstanding `inputRequests`.
    #[must_use]
    pub fn with_input_requests(mut self, requests: Map<String, Value>) -> Self {
        self.input_requests = Some(requests);
        self
    }
}

/// `tasks/update` params: `inputResponses` for an `input_required` task
/// (SEP-2663 §Task Update Requests).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct UpdateTaskParams {
    /// The task to update.
    #[serde(rename = "taskId")]
    pub task_id: String,
    /// Responses keyed by their outstanding `inputRequests` key. Required by
    /// SEP-2663: an update without it is malformed, not empty.
    #[serde(rename = "inputResponses")]
    pub input_responses: Map<String, Value>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn create_task_result_serializes_with_result_type_task() {
        let result = CreateTaskResult::new(Task {
            ttl_ms: Some(60_000),
            status_message: Some("in progress".into()),
            poll_interval_ms: Some(5_000),
            ..Task::working("abc", "2026-01-01T00:00:00Z")
        });
        let v = serde_json::to_value(&result).unwrap();
        assert_eq!(v["resultType"], "task");
        assert_eq!(v["taskId"], "abc");
        assert_eq!(v["status"], "working");
        assert_eq!(v["ttlMs"], 60_000);
        assert_eq!(v["statusMessage"], "in progress");
        assert_eq!(v["pollIntervalMs"], 5_000);
    }

    #[test]
    fn detailed_task_inlines_status_payload_and_round_trips() {
        let completed = DetailedTask::new(Task {
            status: TaskStatus::Completed,
            ttl_ms: None,
            ..Task::working("t1", "2026-01-01T00:00:00Z")
        })
        .with_result(json!({"content": [{"type": "text", "text": "5"}], "isError": false}));

        let v = serde_json::to_value(&completed).unwrap();
        assert_eq!(v["resultType"], "complete");
        assert_eq!(v["status"], "completed");
        assert_eq!(v["result"]["content"][0]["text"], "5");
        assert!(v["ttlMs"].is_null(), "unlimited ttl serializes as null");
        assert!(v.get("error").is_none());
        assert!(v.get("inputRequests").is_none());

        let back: DetailedTask = serde_json::from_value(v).unwrap();
        assert_eq!(back, completed);
    }

    #[test]
    fn status_strings_match_the_spec() {
        for (status, wire) in [
            (TaskStatus::Working, "working"),
            (TaskStatus::InputRequired, "input_required"),
            (TaskStatus::Completed, "completed"),
            (TaskStatus::Failed, "failed"),
            (TaskStatus::Cancelled, "cancelled"),
        ] {
            assert_eq!(serde_json::to_value(status).unwrap(), json!(wire));
        }
    }
}
