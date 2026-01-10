//! Task storage and lifecycle management for MCP Tasks API (SEP-1686)
//!
//! This module provides thread-safe task storage with TTL management, state
//! transitions, and blocking result retrieval.
//!
//! ## Architecture
//!
//! - **Thread-Safety**: `Arc<RwLock<HashMap>>` for concurrent reads/writes
//! - **Task IDs**: UUID v4 for unpredictable, secure identifiers
//! - **TTL Management**: Background cleanup task with configurable interval
//! - **Blocking Behavior**: `tokio::sync::watch` for `tasks/result` blocking
//! - **Auth Binding**: Optional auth context per task for security
//!
//! ## Example
//!
//! ```rust,ignore
//! let storage = TaskStorage::new(Duration::from_secs(60));
//! storage.start_cleanup();
//!
//! // Create task
//! let task_id = storage.create_task(
//!     TaskMetadata { ttl: Some(3600) },
//!     None, // auth_context
//! )?;
//!
//! // Update task status
//! storage.update_status(&task_id, TaskStatus::Working, Some("Processing...")))?;
//!
//! // Complete task
//! storage.complete_task(&task_id, result_value)?;
//!
//! // Retrieve result (blocks until terminal state)
//! let result = storage.get_task_result(&task_id).await?;
//! ```

use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::sync::watch;
use turbomcp_protocol::types::{RelatedTaskMetadata, Task, TaskMetadata, TaskStatus};
use uuid::Uuid;

use crate::error::{McpError, ServerErrorExt, ServerResult};

/// Task storage with thread-safe concurrent access and TTL management
#[derive(Clone, Debug)]
pub struct TaskStorage {
    /// Core task storage (thread-safe)
    tasks: Arc<RwLock<HashMap<String, StoredTask>>>,
    /// Default TTL for tasks without explicit TTL (None = no default)
    default_ttl: Option<u64>,
    /// Cleanup interval for TTL expiry
    cleanup_interval: Duration,
}

/// Internal task storage with result state and metadata
#[derive(Debug)]
struct StoredTask {
    /// Protocol task
    task: Task,
    /// Result state
    result: TaskResultState,
    /// Related messages for input_required state (reserved for future use)
    _related_messages: Vec<RelatedTaskMetadata>,
    /// Auth context binding for security
    auth_context: Option<String>,
    /// Notification channel for blocking result retrieval
    notify: Arc<watch::Sender<TaskResultState>>,
}

/// Task result state (publicly exposed through get_task_result)
#[derive(Debug, Clone)]
pub enum TaskResultState {
    /// Task is pending (working or input_required)
    Pending,
    /// Task completed successfully
    Completed(serde_json::Value),
    /// Task failed with error
    Failed(String),
    /// Task was cancelled
    Cancelled,
}

impl TaskStorage {
    /// Create a new task storage instance
    ///
    /// ## Parameters
    ///
    /// - `cleanup_interval`: How often to scan for expired tasks (recommended: 60s)
    ///
    /// ## Example
    ///
    /// ```rust,ignore
    /// let storage = TaskStorage::new(Duration::from_secs(60));
    /// ```
    pub fn new(cleanup_interval: Duration) -> Self {
        Self {
            tasks: Arc::new(RwLock::new(HashMap::new())),
            default_ttl: None,
            cleanup_interval,
        }
    }

    /// Create a new task storage with default TTL
    ///
    /// Tasks without explicit TTL will use this default.
    pub fn with_default_ttl(cleanup_interval: Duration, default_ttl: u64) -> Self {
        Self {
            tasks: Arc::new(RwLock::new(HashMap::new())),
            default_ttl: Some(default_ttl),
            cleanup_interval,
        }
    }

    /// Create a new task and return its ID
    ///
    /// ## Parameters
    ///
    /// - `metadata`: Task metadata (TTL, etc.)
    /// - `auth_context`: Optional auth context for security binding
    ///
    /// ## Returns
    ///
    /// Unique task ID (UUID v4)
    pub fn create_task(
        &self,
        metadata: TaskMetadata,
        auth_context: Option<String>,
    ) -> ServerResult<String> {
        let task_id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();

        // Use explicit TTL, default TTL, or None
        let ttl = metadata.ttl.or(self.default_ttl);

        let task = Task {
            task_id: task_id.clone(),
            status: TaskStatus::Working,
            status_message: None,
            created_at: now.clone(),
            last_updated_at: now,
            ttl,
            poll_interval: None, // Server doesn't set poll interval
        };

        let (tx, _rx) = watch::channel(TaskResultState::Pending);

        let stored_task = StoredTask {
            task,
            result: TaskResultState::Pending,
            _related_messages: Vec::new(),
            auth_context,
            notify: Arc::new(tx),
        };

        let mut tasks = self
            .tasks
            .write()
            .map_err(|_| McpError::lifecycle("Lock poisoned"))?;

        tasks.insert(task_id.clone(), stored_task);

        Ok(task_id)
    }

    /// Get task by ID (read-only)
    ///
    /// ## Security
    ///
    /// Validates auth context if task has auth binding.
    pub fn get_task(&self, task_id: &str, auth_context: Option<&str>) -> ServerResult<Task> {
        let tasks = self
            .tasks
            .read()
            .map_err(|_| McpError::lifecycle("Lock poisoned"))?;

        let stored_task = tasks
            .get(task_id)
            .ok_or_else(|| McpError::invalid_params(format!("Task not found: {}", task_id)))?;

        // Validate auth context
        self.validate_auth_context(stored_task, auth_context)?;

        Ok(stored_task.task.clone())
    }

    /// Update task status
    ///
    /// Validates state transitions per SEP-1686.
    pub fn update_status(
        &self,
        task_id: &str,
        new_status: TaskStatus,
        status_message: Option<String>,
        auth_context: Option<&str>,
    ) -> ServerResult<()> {
        let mut tasks = self
            .tasks
            .write()
            .map_err(|_| McpError::lifecycle("Lock poisoned"))?;

        let stored_task = tasks
            .get_mut(task_id)
            .ok_or_else(|| McpError::invalid_params(format!("Task not found: {}", task_id)))?;

        // Validate auth context
        self.validate_auth_context(stored_task, auth_context)?;

        // Validate state transition
        if !stored_task.task.status.can_transition_to(&new_status) {
            return Err(McpError::invalid_params(format!(
                "Invalid state transition from {:?} to {:?}",
                stored_task.task.status, new_status
            )));
        }

        stored_task.task.status = new_status;
        stored_task.task.status_message = status_message;
        stored_task.task.last_updated_at = Utc::now().to_rfc3339();

        Ok(())
    }

    /// Complete task with result
    ///
    /// Marks task as Completed and stores result value.
    pub fn complete_task(
        &self,
        task_id: &str,
        result: serde_json::Value,
        auth_context: Option<&str>,
    ) -> ServerResult<()> {
        let mut tasks = self
            .tasks
            .write()
            .map_err(|_| McpError::lifecycle("Lock poisoned"))?;

        let stored_task = tasks
            .get_mut(task_id)
            .ok_or_else(|| McpError::invalid_params(format!("Task not found: {}", task_id)))?;

        // Validate auth context
        self.validate_auth_context(stored_task, auth_context)?;

        // Validate state transition
        if !stored_task
            .task
            .status
            .can_transition_to(&TaskStatus::Completed)
        {
            return Err(McpError::invalid_params(format!(
                "Cannot complete task in state {:?}",
                stored_task.task.status
            )));
        }

        stored_task.task.status = TaskStatus::Completed;
        stored_task.task.status_message = Some("Task completed successfully".to_string());
        stored_task.task.last_updated_at = Utc::now().to_rfc3339();
        stored_task.result = TaskResultState::Completed(result.clone());

        // Notify waiters
        let _ = stored_task.notify.send(TaskResultState::Completed(result));

        Ok(())
    }

    /// Fail task with error message
    pub fn fail_task(
        &self,
        task_id: &str,
        error_message: String,
        auth_context: Option<&str>,
    ) -> ServerResult<()> {
        let mut tasks = self
            .tasks
            .write()
            .map_err(|_| McpError::lifecycle("Lock poisoned"))?;

        let stored_task = tasks
            .get_mut(task_id)
            .ok_or_else(|| McpError::invalid_params(format!("Task not found: {}", task_id)))?;

        // Validate auth context
        self.validate_auth_context(stored_task, auth_context)?;

        // Validate state transition
        if !stored_task
            .task
            .status
            .can_transition_to(&TaskStatus::Failed)
        {
            return Err(McpError::invalid_params(format!(
                "Cannot fail task in state {:?}",
                stored_task.task.status
            )));
        }

        stored_task.task.status = TaskStatus::Failed;
        stored_task.task.status_message = Some(error_message.clone());
        stored_task.task.last_updated_at = Utc::now().to_rfc3339();
        stored_task.result = TaskResultState::Failed(error_message.clone());

        // Notify waiters
        let _ = stored_task
            .notify
            .send(TaskResultState::Failed(error_message));

        Ok(())
    }

    /// Cancel task
    pub fn cancel_task(
        &self,
        task_id: &str,
        reason: Option<String>,
        auth_context: Option<&str>,
    ) -> ServerResult<()> {
        let mut tasks = self
            .tasks
            .write()
            .map_err(|_| McpError::lifecycle("Lock poisoned"))?;

        let stored_task = tasks
            .get_mut(task_id)
            .ok_or_else(|| McpError::invalid_params(format!("Task not found: {}", task_id)))?;

        // Validate auth context
        self.validate_auth_context(stored_task, auth_context)?;

        // Validate state transition
        if !stored_task
            .task
            .status
            .can_transition_to(&TaskStatus::Cancelled)
        {
            return Err(McpError::invalid_params(format!(
                "Cannot cancel task in state {:?}",
                stored_task.task.status
            )));
        }

        stored_task.task.status = TaskStatus::Cancelled;
        stored_task.task.status_message = reason.clone();
        stored_task.task.last_updated_at = Utc::now().to_rfc3339();
        stored_task.result = TaskResultState::Cancelled;

        // Notify waiters
        let _ = stored_task.notify.send(TaskResultState::Cancelled);

        Ok(())
    }

    /// Get task result (blocks until terminal state)
    ///
    /// ## Behavior
    ///
    /// - Returns immediately if task is in terminal state (Completed/Failed/Cancelled)
    /// - Blocks if task is Working or InputRequired
    ///
    /// Per MCP SEP-1686 spec: "The server MUST block... until the task reaches
    /// a terminal state"
    pub async fn get_task_result(
        &self,
        task_id: &str,
        auth_context: Option<&str>,
    ) -> ServerResult<TaskResultState> {
        // Clone receiver under read lock
        let mut receiver = {
            let tasks = self
                .tasks
                .read()
                .map_err(|_| McpError::lifecycle("Lock poisoned"))?;

            let stored_task = tasks
                .get(task_id)
                .ok_or_else(|| McpError::invalid_params(format!("Task not found: {}", task_id)))?;

            // Validate auth context
            self.validate_auth_context(stored_task, auth_context)?;

            // If already terminal, return immediately
            if stored_task.task.status.is_terminal() {
                return Ok(stored_task.result.clone());
            }

            stored_task.notify.subscribe()
        };

        // Wait for state change
        receiver
            .changed()
            .await
            .map_err(|_| McpError::lifecycle("Lock poisoned"))?;

        Ok(receiver.borrow().clone())
    }

    /// List tasks with pagination and optional filtering by auth context.
    ///
    /// ## Parameters
    ///
    /// - `auth_context`: Optional authentication context to filter tasks by.
    /// - `cursor`: An optional opaque cursor (task_id) to start pagination from.
    /// - `limit`: Optional maximum number of tasks to return. Defaults to 100.
    ///
    /// ## Returns
    ///
    /// A `ServerResult` containing a tuple:
    /// - `Vec<Task>`: The list of tasks for the current page.
    /// - `Option<String>`: An opaque cursor for the next page, if more results are available.
    pub fn list_tasks(
        &self,
        auth_context: Option<&str>,
        cursor: Option<&str>,
        limit: Option<usize>,
    ) -> ServerResult<(Vec<Task>, Option<String>)> {
        let tasks_guard = self
            .tasks
            .read()
            .map_err(|_| McpError::lifecycle("Lock poisoned"))?;

        // Convert HashMap values to a Vec for sorting and slicing
        let mut all_tasks: Vec<&StoredTask> = tasks_guard.values().collect();

        // Sort by task_id to ensure consistent pagination order
        all_tasks.sort_by(|a, b| a.task.task_id.cmp(&b.task.task_id));

        let actual_limit = limit.unwrap_or(100); // Default limit to 100

        // Handle limit=0 edge case: return empty results with no cursor
        if actual_limit == 0 {
            return Ok((Vec::new(), None));
        }

        // Filter tasks by auth context
        let filtered_tasks: Vec<&StoredTask> = all_tasks
            .into_iter()
            .filter(
                |stored_task| match (auth_context, &stored_task.auth_context) {
                    (Some(ctx), Some(task_ctx)) => ctx == task_ctx,
                    (None, _) => true,
                    (Some(_), None) => false,
                },
            )
            .collect();

        // Find starting index based on cursor
        // If cursor is not found or not provided, start from index 0
        let start_index = cursor
            .and_then(|c| filtered_tasks.iter().position(|t| t.task.task_id == c))
            .unwrap_or(0);

        // Get the page of tasks
        let paginated_tasks: Vec<Task> = filtered_tasks
            .iter()
            .skip(start_index)
            .take(actual_limit)
            .map(|t| t.task.clone())
            .collect();

        // Determine next_cursor: the ID of the first item NOT included in this page
        let next_index = start_index + actual_limit;
        let next_cursor = if next_index < filtered_tasks.len() {
            Some(filtered_tasks[next_index].task.task_id.clone())
        } else {
            None
        };

        Ok((paginated_tasks, next_cursor))
    }

    /// Start background cleanup task for TTL expiry
    ///
    /// Returns JoinHandle for graceful shutdown.
    pub fn start_cleanup(&self) -> tokio::task::JoinHandle<()> {
        let tasks = self.tasks.clone();
        let interval = self.cleanup_interval;

        tokio::spawn(async move {
            let mut interval_timer = tokio::time::interval(interval);
            loop {
                interval_timer.tick().await;
                Self::cleanup_expired_tasks_impl(&tasks);
            }
        })
    }

    /// Trigger manual cleanup of expired tasks (exposed for testing)
    #[doc(hidden)]
    pub fn trigger_cleanup(&self) {
        Self::cleanup_expired_tasks_impl(&self.tasks);
    }

    /// Cleanup expired tasks (internal implementation)
    fn cleanup_expired_tasks_impl(tasks: &Arc<RwLock<HashMap<String, StoredTask>>>) {
        let now = Utc::now();

        if let Ok(mut tasks_guard) = tasks.write() {
            tasks_guard.retain(|_task_id, stored_task| {
                if let Some(ttl) = stored_task.task.ttl
                    && let Ok(created_at) =
                        DateTime::parse_from_rfc3339(&stored_task.task.created_at)
                {
                    let expiry = created_at + chrono::Duration::seconds(ttl as i64);
                    // Keep if not yet expired
                    return now < expiry;
                }
                // No TTL or parse error = keep forever
                true
            });
        }
    }

    /// Validate auth context against task's auth binding
    fn validate_auth_context(
        &self,
        stored_task: &StoredTask,
        auth_context: Option<&str>,
    ) -> ServerResult<()> {
        match (&stored_task.auth_context, auth_context) {
            (Some(task_ctx), Some(provided_ctx)) => {
                if task_ctx != provided_ctx {
                    return Err(McpError::permission_denied(
                        "Unauthorized: task belongs to different context",
                    ));
                }
            }
            (Some(_), None) => {
                return Err(McpError::permission_denied(
                    "Unauthorized: task requires authentication",
                ));
            }
            _ => {
                // No auth context on task, or both None = allowed
            }
        }
        Ok(())
    }

    /// Get count of active tasks (for metrics)
    pub fn count_tasks(&self) -> usize {
        self.tasks.read().map(|t| t.len()).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_task_lifecycle() {
        let storage = TaskStorage::new(Duration::from_secs(60));

        // Create task
        let task_id = storage
            .create_task(TaskMetadata { ttl: Some(3600) }, None)
            .unwrap();

        // Get task
        let task = storage.get_task(&task_id, None).unwrap();
        assert_eq!(task.status, TaskStatus::Working);

        // Update status
        storage
            .update_status(
                &task_id,
                TaskStatus::InputRequired,
                Some("Need input".to_string()),
                None,
            )
            .unwrap();

        let task = storage.get_task(&task_id, None).unwrap();
        assert_eq!(task.status, TaskStatus::InputRequired);

        // Complete task
        storage
            .complete_task(&task_id, serde_json::json!({"result": "success"}), None)
            .unwrap();

        let task = storage.get_task(&task_id, None).unwrap();
        assert_eq!(task.status, TaskStatus::Completed);
    }

    #[tokio::test]
    async fn test_blocking_result_retrieval() {
        let storage = TaskStorage::new(Duration::from_secs(60));

        let task_id = storage
            .create_task(TaskMetadata { ttl: Some(3600) }, None)
            .unwrap();

        // Spawn task to complete after delay
        let storage_clone = storage.clone();
        let task_id_clone = task_id.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            storage_clone
                .complete_task(&task_id_clone, serde_json::json!({"data": 42}), None)
                .unwrap();
        });

        // Should block until complete
        let result = storage.get_task_result(&task_id, None).await.unwrap();
        match result {
            TaskResultState::Completed(value) => {
                assert_eq!(value["data"], 42);
            }
            _ => panic!("Expected completed state"),
        }
    }

    #[tokio::test]
    async fn test_invalid_state_transition() {
        let storage = TaskStorage::new(Duration::from_secs(60));

        let task_id = storage
            .create_task(TaskMetadata { ttl: Some(3600) }, None)
            .unwrap();

        // Complete task
        storage
            .complete_task(&task_id, serde_json::json!({}), None)
            .unwrap();

        // Try to transition from Completed (should fail)
        let result = storage.update_status(&task_id, TaskStatus::Working, None, None);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_auth_context_binding() {
        let storage = TaskStorage::new(Duration::from_secs(60));

        // Create task with auth context
        let task_id = storage
            .create_task(
                TaskMetadata { ttl: Some(3600) },
                Some("user123".to_string()),
            )
            .unwrap();

        // Access with correct context
        let result = storage.get_task(&task_id, Some("user123"));
        assert!(result.is_ok());

        // Access with wrong context
        let result = storage.get_task(&task_id, Some("user456"));
        assert!(result.is_err());

        // Access without context
        let result = storage.get_task(&task_id, None);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_list_tasks_filtering() {
        let storage = TaskStorage::new(Duration::from_secs(60));

        // Create tasks with different auth contexts
        let _task1 = storage
            .create_task(TaskMetadata { ttl: Some(3600) }, Some("user1".to_string()))
            .unwrap();
        let _task2 = storage
            .create_task(TaskMetadata { ttl: Some(3600) }, Some("user2".to_string()))
            .unwrap();
        let _task3 = storage
            .create_task(TaskMetadata { ttl: Some(3600) }, None)
            .unwrap();

        // List all tasks (no filter)
        let (all_tasks, _) = storage.list_tasks(None, None, None).unwrap();
        assert_eq!(all_tasks.len(), 3);

        // List tasks for user1
        let (user1_tasks, _) = storage.list_tasks(Some("user1"), None, None).unwrap();
        assert_eq!(user1_tasks.len(), 1);

        // List tasks for user2
        let (user2_tasks, _) = storage.list_tasks(Some("user2"), None, None).unwrap();
        assert_eq!(user2_tasks.len(), 1);
    }

    #[tokio::test]
    async fn test_list_tasks_pagination() {
        let storage = TaskStorage::new(Duration::from_secs(60));

        // Create 5 tasks with distinct task_ids (sorted alphabetically for consistent cursor)
        let task_ids = vec![
            storage
                .create_task(TaskMetadata { ttl: Some(3600) }, None)
                .unwrap(), // A
            storage
                .create_task(TaskMetadata { ttl: Some(3600) }, None)
                .unwrap(), // B
            storage
                .create_task(TaskMetadata { ttl: Some(3600) }, None)
                .unwrap(), // C
            storage
                .create_task(TaskMetadata { ttl: Some(3600) }, None)
                .unwrap(), // D
            storage
                .create_task(TaskMetadata { ttl: Some(3600) }, None)
                .unwrap(), // E
        ];
        // Ensure task_ids are truly unique and can be sorted consistently
        let mut sorted_task_ids = task_ids.clone();
        sorted_task_ids.sort();

        // Page 1: Limit 2
        let (page1_tasks, next_cursor1) = storage.list_tasks(None, None, Some(2)).unwrap();
        assert_eq!(page1_tasks.len(), 2);
        assert_eq!(page1_tasks[0].task_id, sorted_task_ids[0]);
        assert_eq!(page1_tasks[1].task_id, sorted_task_ids[1]);
        assert_eq!(next_cursor1, Some(sorted_task_ids[2].clone()));

        // Page 2: Limit 2, using cursor from Page 1
        let (page2_tasks, next_cursor2) = storage
            .list_tasks(None, next_cursor1.as_deref(), Some(2))
            .unwrap();
        assert_eq!(page2_tasks.len(), 2);
        assert_eq!(page2_tasks[0].task_id, sorted_task_ids[2]);
        assert_eq!(page2_tasks[1].task_id, sorted_task_ids[3]);
        assert_eq!(next_cursor2, Some(sorted_task_ids[4].clone()));

        // Page 3: Limit 2, using cursor from Page 2 (should be last task, so no next cursor)
        let (page3_tasks, next_cursor3) = storage
            .list_tasks(None, next_cursor2.as_deref(), Some(2))
            .unwrap();
        assert_eq!(page3_tasks.len(), 1); // Only one task remaining
        assert_eq!(page3_tasks[0].task_id, sorted_task_ids[4]);
        assert_eq!(next_cursor3, None);

        // Test with limit greater than remaining tasks
        let (page_large_limit, next_cursor_large_limit) =
            storage.list_tasks(None, None, Some(10)).unwrap();
        assert_eq!(page_large_limit.len(), 5);
        assert_eq!(next_cursor_large_limit, None);

        // Test with limit 0 (should return no tasks and no cursor)
        let (page_limit_0, next_cursor_limit_0) = storage.list_tasks(None, None, Some(0)).unwrap();
        assert_eq!(page_limit_0.len(), 0);
        assert_eq!(next_cursor_limit_0, None);

        // Test with an invalid cursor (should act like no cursor)
        let (page_invalid_cursor, next_cursor_invalid_cursor) = storage
            .list_tasks(None, Some("non-existent-cursor"), Some(2))
            .unwrap();
        assert_eq!(page_invalid_cursor.len(), 2);
        assert_eq!(page_invalid_cursor[0].task_id, sorted_task_ids[0]);
        assert_eq!(page_invalid_cursor[1].task_id, sorted_task_ids[1]);
        assert_eq!(next_cursor_invalid_cursor, Some(sorted_task_ids[2].clone()));
    }
}
