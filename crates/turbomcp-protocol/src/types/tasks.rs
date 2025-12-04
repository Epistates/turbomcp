//! Tasks API for durable long-running operations
//!
//! The Tasks API (MCP 2025-11-25, SEP-1686) provides durable state machines for
//! long-running operations, enabling requestor polling and deferred result retrieval.
//!
//! ## Overview
//!
//! Tasks enable:
//! - **Durable state machines** - Long-running operations that outlive individual connections
//! - **Requestor polling** - Clients can poll for completion status
//! - **Deferred results** - Results available after task completion
//! - **Input requests** - Tasks can request additional input during execution
//! - **Bidirectional support** - Works for both client→server and server→client requests
//!
//! ## Key Concepts
//!
//! ### Task Lifecycle
//!
//! ```text
//! [*] → working
//!     ↓
//!     ├─→ input_required ──┬─→ working ──→ terminal
//!     │                    └─→ terminal
//!     │
//!     └─→ terminal
//!
//! Terminal states: completed, failed, cancelled
//! ```
//!
//! ### Supported Requests
//!
//! **Client → Server** (Server as receiver):
//! - `tools/call` - Long-running tool execution
//!
//! **Server → Client** (Client as receiver):
//! - `sampling/createMessage` - LLM inference operations
//! - `elicitation/create` - User input collection
//!
//! ## Usage Example
//!
//! ```rust,no_run
//! use turbomcp_protocol::types::tasks::{Task, TaskStatus, TaskMetadata, CreateTaskResult};
//! use turbomcp_protocol::types::CallToolRequest;
//! use serde_json::json;
//!
//! // Client requests task-augmented tool call
//! let request = CallToolRequest {
//!     name: "long_running_analysis".to_string(),
//!     arguments: Some(json!({"data": "large_dataset"})),
//!     task: Some(TaskMetadata {
//!         ttl: Some(300_000), // 5 minute lifetime
//!     }),
//!     _meta: None,
//! };
//!
//! // Server responds immediately with task
//! let response = CreateTaskResult {
//!     task: Task {
//!         task_id: "task-123".to_string(),
//!         status: TaskStatus::Working,
//!         status_message: None,
//!         created_at: "2025-11-25T10:30:00Z".to_string(),
//!         ttl: Some(300_000),
//!         poll_interval: Some(5_000), // Poll every 5s
//!     },
//!     _meta: None,
//! };
//!
//! // Client polls for status
//! // ... tasks/get request ...
//!
//! // When completed, retrieve results
//! // ... tasks/result request ...
//! ```
//!
//! ## Security Considerations
//!
//! ### Task ID Access Control
//!
//! Task IDs are the **primary access control mechanism**. Implementations MUST:
//!
//! 1. **Bind to authorization context** - Reject operations from different contexts
//! 2. **Use cryptographic entropy** - Task IDs must be unpredictable (use UUID v4)
//! 3. **Enforce TTL limits** - Shorter TTLs reduce exposure windows
//! 4. **Audit access** - Log all task operations for security monitoring
//!
//! ### Resource Management
//!
//! Implementations SHOULD:
//! - Enforce concurrent task limits per requestor
//! - Enforce maximum TTL durations
//! - Clean up expired tasks promptly
//! - Implement rate limiting on task operations

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Task status representing the current state of a long-running operation
///
/// ## State Transitions
///
/// Valid transitions:
/// - `Working` → `InputRequired`, `Completed`, `Failed`, `Cancelled`
/// - `InputRequired` → `Working`, `Completed`, `Failed`, `Cancelled`
/// - Terminal states (`Completed`, `Failed`, `Cancelled`) → **NO TRANSITIONS**
///
/// ## Examples
///
/// ```rust
/// use turbomcp_protocol::types::tasks::TaskStatus;
///
/// let status = TaskStatus::Working;
/// assert!(!status.is_terminal());
///
/// let status = TaskStatus::Completed;
/// assert!(status.is_terminal());
/// ```
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
#[cfg(feature = "mcp-tasks")]
pub enum TaskStatus {
    /// Request is currently being processed
    Working,

    /// Task requires additional input from requestor (e.g., user confirmation)
    ///
    /// When in this state:
    /// - Requestor should call `tasks/result` which will receive input requests
    /// - All input requests MUST include `io.modelcontextprotocol/related-task` metadata
    /// - After providing input, task transitions back to `Working`
    #[serde(rename = "input_required")]
    InputRequired,

    /// Request completed successfully
    ///
    /// This is a terminal state - no further transitions allowed.
    Completed,

    /// Request did not complete successfully
    ///
    /// This is a terminal state. The `status_message` field typically contains
    /// diagnostic information about the failure.
    Failed,

    /// Request was cancelled before completion
    ///
    /// This is a terminal state. The `status_message` field may contain the
    /// reason for cancellation.
    Cancelled,
}

impl TaskStatus {
    /// Check if this status is terminal (no further transitions allowed)
    ///
    /// Terminal states: `Completed`, `Failed`, `Cancelled`
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Cancelled
        )
    }

    /// Check if this status indicates the task is still active
    ///
    /// Active states: `Working`, `InputRequired`
    pub fn is_active(&self) -> bool {
        !self.is_terminal()
    }

    /// Check if task can transition to the given status
    ///
    /// # Examples
    ///
    /// ```rust
    /// use turbomcp_protocol::types::tasks::TaskStatus;
    ///
    /// let working = TaskStatus::Working;
    /// assert!(working.can_transition_to(&TaskStatus::Completed));
    /// assert!(working.can_transition_to(&TaskStatus::InputRequired));
    ///
    /// let completed = TaskStatus::Completed;
    /// assert!(!completed.can_transition_to(&TaskStatus::Working)); // Terminal
    /// ```
    pub fn can_transition_to(&self, _next: &TaskStatus) -> bool {
        match self {
            TaskStatus::Working => true,       // Can transition to any state
            TaskStatus::InputRequired => true, // Can transition to any state
            TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Cancelled => false, // Terminal
        }
    }
}

/// Core task type representing a long-running operation
///
/// ## Fields
///
/// - `task_id`: Unique identifier (MUST be cryptographically secure)
/// - `status`: Current task state
/// - `status_message`: Optional human-readable status (any state)
/// - `created_at`: ISO 8601 timestamp of creation
/// - `last_updated_at`: ISO 8601 timestamp when task was last updated
/// - `ttl`: Time-to-live in milliseconds from creation (null = unlimited)
/// - `poll_interval`: Suggested polling interval in milliseconds
///
/// ## TTL Behavior
///
/// TTL is measured from `created_at`, not from last update:
///
/// ```text
/// Creation: 10:00:00, TTL: 60000ms (60s)
/// Expiry:   10:01:00 (regardless of updates)
/// ```
///
/// After TTL expiry, the receiver MAY delete the task and its results.
///
/// ## Examples
///
/// ```rust
/// use turbomcp_protocol::types::tasks::{Task, TaskStatus};
///
/// let task = Task {
///     task_id: "task-123".to_string(),
///     status: TaskStatus::Working,
///     status_message: Some("Processing data...".to_string()),
///     created_at: "2025-11-25T10:30:00Z".to_string(),
///     last_updated_at: "2025-11-25T10:30:00Z".to_string(),
///     ttl: Some(300_000), // 5 minutes
///     poll_interval: Some(5_000), // Poll every 5s
/// };
///
/// assert!(!task.status.is_terminal());
/// assert_eq!(task.ttl, Some(300_000));
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg(feature = "mcp-tasks")]
pub struct Task {
    /// Unique identifier for this task
    ///
    /// MUST be generated by receiver with cryptographic entropy (e.g., UUID v4).
    /// Task IDs are the primary access control mechanism.
    #[serde(rename = "taskId")]
    pub task_id: String,

    /// Current task status
    pub status: TaskStatus,

    /// Optional human-readable status message
    ///
    /// Usage by status:
    /// - `Cancelled`: Reason for cancellation
    /// - `Completed`: Summary of results
    /// - `Failed`: Diagnostic info, error details
    /// - `Working`/`InputRequired`: Progress updates
    #[serde(rename = "statusMessage", skip_serializing_if = "Option::is_none")]
    pub status_message: Option<String>,

    /// ISO 8601 timestamp when task was created
    ///
    /// Format: `YYYY-MM-DDTHH:MM:SSZ` (UTC)
    /// TTL is measured from this timestamp.
    #[serde(rename = "createdAt")]
    pub created_at: String,

    /// ISO 8601 timestamp when task was last updated
    ///
    /// Format: `YYYY-MM-DDTHH:MM:SSZ` (UTC)
    /// Updated whenever task status or other fields change.
    #[serde(rename = "lastUpdatedAt")]
    pub last_updated_at: String,

    /// Time-to-live in milliseconds from creation
    ///
    /// - `Some(ms)`: Task expires after this duration from `created_at`
    /// - `None`: Unlimited retention (use with caution)
    ///
    /// After expiry, receiver MAY delete task and results.
    /// Shorter TTLs improve security by reducing task ID exposure.
    pub ttl: Option<u64>,

    /// Suggested polling interval in milliseconds
    ///
    /// Requestors SHOULD respect this value to avoid excessive polling.
    /// Receivers MAY adjust based on task complexity and load.
    #[serde(rename = "pollInterval", skip_serializing_if = "Option::is_none")]
    pub poll_interval: Option<u64>,
}

/// Metadata for requesting task augmentation on a request
///
/// Include this in request parameters to augment the request with task support:
///
/// ```rust
/// use turbomcp_protocol::types::tasks::TaskMetadata;
/// use turbomcp_protocol::types::CallToolRequest;
/// use serde_json::json;
///
/// let request = CallToolRequest {
///     name: "long_tool".to_string(),
///     arguments: Some(json!({"data": "value"})),
///     task: Some(TaskMetadata {
///         ttl: Some(300_000), // Request 5 minute lifetime
///     }),
///     _meta: None,
/// };
/// ```
///
/// ## TTL Negotiation
///
/// The receiver MAY override the requested TTL. Check the actual `ttl` value
/// in the returned `Task` object.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg(feature = "mcp-tasks")]
pub struct TaskMetadata {
    /// Requested time-to-live in milliseconds from creation
    ///
    /// - Receiver MAY override this value
    /// - Omit for server default TTL
    /// - Use `null` (or omit) for unlimited (if server supports)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl: Option<u64>,
}

/// Metadata for associating messages with a task
///
/// Used in `_meta` field to link messages to a specific task during `input_required` state.
///
/// ## Usage
///
/// All messages during input_required MUST include this metadata:
///
/// ```json
/// {
///   "_meta": {
///     "io.modelcontextprotocol/related-task": {
///       "taskId": "task-123"
///     }
///   }
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg(feature = "mcp-tasks")]
pub struct RelatedTaskMetadata {
    /// Task ID this message is associated with
    ///
    /// MUST match the task ID across all related messages.
    #[serde(rename = "taskId")]
    pub task_id: String,
}

/// Result type for task creation (immediate response to task-augmented requests)
///
/// When a request is augmented with `task` metadata, the receiver responds immediately
/// with this result containing the task object. The actual operation result is available
/// later via `tasks/result`.
///
/// ## Two-Phase Response Pattern
///
/// ```text
/// Phase 1 (Immediate):
///   Client → tools/call (task: {...})
///   Server → CreateTaskResult (task with status: working)
///
/// Phase 2 (Deferred):
///   Client → tasks/result (taskId)
///   Server → CallToolResult (actual tool response)
/// ```
///
/// ## Examples
///
/// ```rust
/// use turbomcp_protocol::types::tasks::{CreateTaskResult, Task, TaskStatus};
///
/// let response = CreateTaskResult {
///     task: Task {
///         task_id: "task-abc123".to_string(),
///         status: TaskStatus::Working,
///         status_message: None,
///         created_at: "2025-11-25T10:30:00Z".to_string(),
///         ttl: Some(60_000),
///         poll_interval: Some(5_000),
///     },
///     _meta: None,
/// };
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg(feature = "mcp-tasks")]
pub struct CreateTaskResult {
    /// The created task with initial state (typically `Working`)
    pub task: Task,

    /// Optional metadata
    ///
    /// Host applications can use `io.modelcontextprotocol/model-immediate-response`
    /// to provide immediate feedback to the model before task completion.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _meta: Option<HashMap<String, serde_json::Value>>,
}

// ========== Task Method Request/Response Types ==========

/// Request to retrieve task status
///
/// Poll for task completion and status updates.
///
/// ## Usage
///
/// ```rust
/// use turbomcp_protocol::types::tasks::GetTaskRequest;
///
/// let request = GetTaskRequest {
///     task_id: "task-123".to_string(),
/// };
/// ```
///
/// ## Errors
///
/// - Invalid taskId: JSON-RPC error -32602 (Invalid params)
/// - Task expired: JSON-RPC error -32602
/// - Unauthorized: JSON-RPC error -32602 (if different auth context)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg(feature = "mcp-tasks")]
pub struct GetTaskRequest {
    /// Task identifier to query
    #[serde(rename = "taskId")]
    pub task_id: String,
}

/// Response from tasks/get containing current task status
///
/// This is a type alias - the response is a `Task` object with all current information.
#[cfg(feature = "mcp-tasks")]
pub type GetTaskResult = Task;

/// Request to retrieve task results (or receive input requests during input_required)
///
/// ## Blocking Behavior
///
/// - **Terminal states** (`Completed`, `Failed`, `Cancelled`): Returns immediately
/// - **Non-terminal states** (`Working`, `InputRequired`): **BLOCKS** until terminal
///
/// During `InputRequired` state, this request may receive input requests from the receiver
/// (e.g., elicitation/create) before finally returning the result.
///
/// ## Usage
///
/// ```rust
/// use turbomcp_protocol::types::tasks::GetTaskPayloadRequest;
///
/// let request = GetTaskPayloadRequest {
///     task_id: "task-123".to_string(),
/// };
/// ```
///
/// ## Errors
///
/// Same as GetTaskRequest
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg(feature = "mcp-tasks")]
pub struct GetTaskPayloadRequest {
    /// Task identifier to retrieve results for
    #[serde(rename = "taskId")]
    pub task_id: String,
}

/// Response from tasks/result containing the actual operation result
///
/// The structure matches the original request type:
/// - For `tools/call` task: `CallToolResult`
/// - For `sampling/createMessage` task: `CreateMessageResult`
/// - For `elicitation/create` task: `ElicitResult`
///
/// The `_meta` field SHOULD include `io.modelcontextprotocol/related-task` metadata.
///
/// ## Examples
///
/// ```json
/// {
///   "content": [{"type": "text", "text": "Result data"}],
///   "isError": false,
///   "_meta": {
///     "io.modelcontextprotocol/related-task": {
///       "taskId": "task-123"
///     }
///   }
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg(feature = "mcp-tasks")]
pub struct GetTaskPayloadResult {
    /// Dynamic result content (structure depends on original request type)
    #[serde(flatten)]
    pub result: serde_json::Value,

    /// Optional metadata (SHOULD include related-task)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _meta: Option<HashMap<String, serde_json::Value>>,
}

/// Request to list all tasks (with pagination)
///
/// Returns a paginated list of tasks. Use `cursor` for pagination.
///
/// ## Usage
///
/// ```rust
/// use turbomcp_protocol::types::tasks::ListTasksRequest;
///
/// // First page
/// let request = ListTasksRequest {
///     cursor: None,
/// };
///
/// // Subsequent pages
/// let request = ListTasksRequest {
///     cursor: Some("next-page-cursor".to_string()),
/// };
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[cfg(feature = "mcp-tasks")]
pub struct ListTasksRequest {
    /// Opaque pagination cursor
    ///
    /// - Omit for first page
    /// - Use `nextCursor` from previous response for subsequent pages
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    /// Maximum number of tasks to return
    ///
    /// - Omit for server default (typically 100)
    /// - Values > 1000 may be truncated by server
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

/// Response from tasks/list containing paginated task list
///
/// ## Pagination
///
/// If `next_cursor` is present, more tasks are available:
///
/// ```rust
/// use turbomcp_protocol::types::tasks::ListTasksResult;
///
/// let response = ListTasksResult {
///     tasks: vec![/* tasks */],
///     next_cursor: Some("next-page".to_string()),
///     _meta: None,
/// };
///
/// if response.next_cursor.is_some() {
///     // More pages available
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg(feature = "mcp-tasks")]
pub struct ListTasksResult {
    /// Array of tasks (may be empty)
    pub tasks: Vec<Task>,

    /// Opaque cursor for next page (if more results available)
    #[serde(rename = "nextCursor", skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,

    /// Optional metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _meta: Option<HashMap<String, serde_json::Value>>,
}

/// Request to cancel a task
///
/// Attempt to cancel a running task. This is a **best-effort** operation.
///
/// ## Behavior
///
/// - Receiver MAY ignore cancellation for tasks that cannot be interrupted
/// - Terminal tasks cannot be cancelled (returns error -32602)
/// - Successful cancellation transitions task to `Cancelled` status
///
/// ## Usage
///
/// ```rust
/// use turbomcp_protocol::types::tasks::CancelTaskRequest;
///
/// let request = CancelTaskRequest {
///     task_id: "task-123".to_string(),
/// };
/// ```
///
/// ## Errors
///
/// - Invalid taskId: -32602
/// - Already terminal: -32602 ("Cannot cancel task: already in terminal status")
/// - Unauthorized: -32602
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg(feature = "mcp-tasks")]
pub struct CancelTaskRequest {
    /// Task identifier to cancel
    #[serde(rename = "taskId")]
    pub task_id: String,
}

/// Response from tasks/cancel containing updated task with cancelled status
///
/// This is a type alias - the response is a `Task` object with `status: Cancelled`.
#[cfg(feature = "mcp-tasks")]
pub type CancelTaskResult = Task;

/// Task status change notification (optional, not required by spec)
///
/// Receivers MAY send notifications when task status changes, but requestors
/// MUST NOT rely on these - they must continue polling via `tasks/get`.
///
/// ## Usage
///
/// ```json
/// {
///   "jsonrpc": "2.0",
///   "method": "notifications/tasks/status",
///   "params": {
///     "taskId": "task-123",
///     "status": "completed",
///     "createdAt": "2025-11-25T10:30:00Z",
///     "ttl": 60000
///   }
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg(feature = "mcp-tasks")]
pub struct TaskStatusNotification {
    /// Task ID this notification is for
    #[serde(rename = "taskId")]
    pub task_id: String,

    /// New task status
    pub status: TaskStatus,

    /// Optional status message
    #[serde(rename = "statusMessage", skip_serializing_if = "Option::is_none")]
    pub status_message: Option<String>,

    /// Task creation timestamp (ISO 8601)
    #[serde(rename = "createdAt")]
    pub created_at: String,

    /// Time-to-live in milliseconds
    pub ttl: Option<u64>,

    /// Suggested poll interval
    #[serde(rename = "pollInterval", skip_serializing_if = "Option::is_none")]
    pub poll_interval: Option<u64>,

    /// Optional metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _meta: Option<HashMap<String, serde_json::Value>>,
}

#[cfg(all(test, feature = "mcp-tasks"))]
mod tests {
    use super::*;

    #[test]
    fn test_task_status_terminal() {
        assert!(!TaskStatus::Working.is_terminal());
        assert!(!TaskStatus::InputRequired.is_terminal());
        assert!(TaskStatus::Completed.is_terminal());
        assert!(TaskStatus::Failed.is_terminal());
        assert!(TaskStatus::Cancelled.is_terminal());
    }

    #[test]
    fn test_task_status_active() {
        assert!(TaskStatus::Working.is_active());
        assert!(TaskStatus::InputRequired.is_active());
        assert!(!TaskStatus::Completed.is_active());
        assert!(!TaskStatus::Failed.is_active());
        assert!(!TaskStatus::Cancelled.is_active());
    }

    #[test]
    fn test_task_status_transitions() {
        // Working can transition to anything
        assert!(TaskStatus::Working.can_transition_to(&TaskStatus::InputRequired));
        assert!(TaskStatus::Working.can_transition_to(&TaskStatus::Completed));
        assert!(TaskStatus::Working.can_transition_to(&TaskStatus::Failed));
        assert!(TaskStatus::Working.can_transition_to(&TaskStatus::Cancelled));

        // InputRequired can transition to anything
        assert!(TaskStatus::InputRequired.can_transition_to(&TaskStatus::Working));
        assert!(TaskStatus::InputRequired.can_transition_to(&TaskStatus::Completed));

        // Terminal states cannot transition
        assert!(!TaskStatus::Completed.can_transition_to(&TaskStatus::Working));
        assert!(!TaskStatus::Failed.can_transition_to(&TaskStatus::Working));
        assert!(!TaskStatus::Cancelled.can_transition_to(&TaskStatus::Working));
    }

    #[test]
    fn test_task_status_serialization() {
        assert_eq!(
            serde_json::to_string(&TaskStatus::Working).unwrap(),
            "\"working\""
        );
        assert_eq!(
            serde_json::to_string(&TaskStatus::InputRequired).unwrap(),
            "\"input_required\""
        );
        assert_eq!(
            serde_json::to_string(&TaskStatus::Completed).unwrap(),
            "\"completed\""
        );
        assert_eq!(
            serde_json::to_string(&TaskStatus::Failed).unwrap(),
            "\"failed\""
        );
        assert_eq!(
            serde_json::to_string(&TaskStatus::Cancelled).unwrap(),
            "\"cancelled\""
        );
    }

    #[test]
    fn test_task_serialization() {
        let task = Task {
            task_id: "task-123".to_string(),
            status: TaskStatus::Working,
            status_message: Some("Processing...".to_string()),
            created_at: "2025-11-25T10:30:00Z".to_string(),
            last_updated_at: "2025-11-25T10:30:00Z".to_string(),
            ttl: Some(60000),
            poll_interval: Some(5000),
        };

        let json = serde_json::to_string(&task).unwrap();
        assert!(json.contains("\"taskId\":\"task-123\""));
        assert!(json.contains("\"status\":\"working\""));
        assert!(json.contains("\"statusMessage\":\"Processing...\""));
        assert!(json.contains("\"createdAt\":\"2025-11-25T10:30:00Z\""));
        assert!(json.contains("\"lastUpdatedAt\":\"2025-11-25T10:30:00Z\""));
        assert!(json.contains("\"ttl\":60000"));
        assert!(json.contains("\"pollInterval\":5000"));

        // Verify deserialization
        let deserialized: Task = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.task_id, "task-123");
        assert_eq!(deserialized.status, TaskStatus::Working);
    }

    #[test]
    fn test_task_metadata_serialization() {
        let metadata = TaskMetadata { ttl: Some(300000) };

        let json = serde_json::to_string(&metadata).unwrap();
        assert!(json.contains("\"ttl\":300000"));

        // Verify deserialization
        let deserialized: TaskMetadata = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.ttl, Some(300000));

        // Test with no TTL
        let metadata = TaskMetadata { ttl: None };
        let json = serde_json::to_string(&metadata).unwrap();
        assert_eq!(json, "{}"); // Empty object when ttl is None
    }

    #[test]
    fn test_related_task_metadata() {
        let metadata = RelatedTaskMetadata {
            task_id: "task-abc".to_string(),
        };

        let json = serde_json::to_string(&metadata).unwrap();
        assert!(json.contains("\"taskId\":\"task-abc\""));

        let deserialized: RelatedTaskMetadata = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.task_id, "task-abc");
    }

    #[test]
    fn test_create_task_result() {
        let result = CreateTaskResult {
            task: Task {
                task_id: "task-123".to_string(),
                status: TaskStatus::Working,
                status_message: None,
                created_at: "2025-11-25T10:30:00Z".to_string(),
                last_updated_at: "2025-11-25T10:30:00Z".to_string(),
                ttl: Some(60000),
                poll_interval: Some(5000),
            },
            _meta: None,
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"task\""));
        assert!(json.contains("\"taskId\":\"task-123\""));
    }

    #[test]
    fn test_get_task_request() {
        let request = GetTaskRequest {
            task_id: "task-456".to_string(),
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("\"taskId\":\"task-456\""));

        let deserialized: GetTaskRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.task_id, "task-456");
    }

    #[test]
    fn test_list_tasks_result() {
        let result = ListTasksResult {
            tasks: vec![
                Task {
                    task_id: "task-1".to_string(),
                    status: TaskStatus::Working,
                    status_message: None,
                    created_at: "2025-11-25T10:30:00Z".to_string(),
                    last_updated_at: "2025-11-25T10:30:00Z".to_string(),
                    ttl: Some(60000),
                    poll_interval: None,
                },
                Task {
                    task_id: "task-2".to_string(),
                    status: TaskStatus::Completed,
                    status_message: Some("Done".to_string()),
                    created_at: "2025-11-25T09:00:00Z".to_string(),
                    last_updated_at: "2025-11-25T09:30:00Z".to_string(),
                    ttl: Some(30000),
                    poll_interval: None,
                },
            ],
            next_cursor: Some("next-page".to_string()),
            _meta: None,
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"tasks\""));
        assert!(json.contains("\"task-1\""));
        assert!(json.contains("\"task-2\""));
        assert!(json.contains("\"nextCursor\":\"next-page\""));
    }

    #[test]
    fn test_cancel_task_request() {
        let request = CancelTaskRequest {
            task_id: "task-789".to_string(),
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("\"taskId\":\"task-789\""));
    }

    #[test]
    fn test_task_status_notification() {
        let notification = TaskStatusNotification {
            task_id: "task-999".to_string(),
            status: TaskStatus::Completed,
            status_message: Some("Task finished successfully".to_string()),
            created_at: "2025-11-25T10:30:00Z".to_string(),
            ttl: Some(60000),
            poll_interval: None,
            _meta: None,
        };

        let json = serde_json::to_string(&notification).unwrap();
        assert!(json.contains("\"taskId\":\"task-999\""));
        assert!(json.contains("\"status\":\"completed\""));
        assert!(json.contains("\"statusMessage\":\"Task finished successfully\""));
    }
}
