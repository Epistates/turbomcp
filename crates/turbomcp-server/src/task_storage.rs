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

use crate::error::{ServerError, ServerResult};
use turbomcp_protocol::Error as ProtocolError;

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
        let created_at = Utc::now().to_rfc3339();

        // Use explicit TTL, default TTL, or None
        let ttl = metadata.ttl.or(self.default_ttl);

        let task = Task {
            task_id: task_id.clone(),
            status: TaskStatus::Working,
            status_message: None,
            created_at,
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
            .map_err(|_| ServerError::Lifecycle("Lock poisoned".to_string()))?;

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
            .map_err(|_| ServerError::Lifecycle("Lock poisoned".to_string()))?;

        let stored_task = tasks.get(task_id).ok_or_else(|| {
            ServerError::Protocol(ProtocolError::invalid_params(format!(
                "Task not found: {}",
                task_id
            )))
        })?;

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
            .map_err(|_| ServerError::Lifecycle("Lock poisoned".to_string()))?;

        let stored_task = tasks.get_mut(task_id).ok_or_else(|| {
            ServerError::Protocol(ProtocolError::invalid_params(format!(
                "Task not found: {}",
                task_id
            )))
        })?;

        // Validate auth context
        self.validate_auth_context(stored_task, auth_context)?;

        // Validate state transition
        if !stored_task.task.status.can_transition_to(&new_status) {
            return Err(ServerError::Protocol(ProtocolError::invalid_params(
                format!(
                    "Invalid state transition from {:?} to {:?}",
                    stored_task.task.status, new_status
                ),
            )));
        }

        stored_task.task.status = new_status;
        stored_task.task.status_message = status_message;

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
            .map_err(|_| ServerError::Lifecycle("Lock poisoned".to_string()))?;

        let stored_task = tasks.get_mut(task_id).ok_or_else(|| {
            ServerError::Protocol(ProtocolError::invalid_params(format!(
                "Task not found: {}",
                task_id
            )))
        })?;

        // Validate auth context
        self.validate_auth_context(stored_task, auth_context)?;

        // Validate state transition
        if !stored_task
            .task
            .status
            .can_transition_to(&TaskStatus::Completed)
        {
            return Err(ServerError::Protocol(ProtocolError::invalid_params(
                format!(
                    "Cannot complete task in state {:?}",
                    stored_task.task.status
                ),
            )));
        }

        stored_task.task.status = TaskStatus::Completed;
        stored_task.task.status_message = Some("Task completed successfully".to_string());
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
            .map_err(|_| ServerError::Lifecycle("Lock poisoned".to_string()))?;

        let stored_task = tasks.get_mut(task_id).ok_or_else(|| {
            ServerError::Protocol(ProtocolError::invalid_params(format!(
                "Task not found: {}",
                task_id
            )))
        })?;

        // Validate auth context
        self.validate_auth_context(stored_task, auth_context)?;

        // Validate state transition
        if !stored_task
            .task
            .status
            .can_transition_to(&TaskStatus::Failed)
        {
            return Err(ServerError::Protocol(ProtocolError::invalid_params(
                format!("Cannot fail task in state {:?}", stored_task.task.status),
            )));
        }

        stored_task.task.status = TaskStatus::Failed;
        stored_task.task.status_message = Some(error_message.clone());
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
            .map_err(|_| ServerError::Lifecycle("Lock poisoned".to_string()))?;

        let stored_task = tasks.get_mut(task_id).ok_or_else(|| {
            ServerError::Protocol(ProtocolError::invalid_params(format!(
                "Task not found: {}",
                task_id
            )))
        })?;

        // Validate auth context
        self.validate_auth_context(stored_task, auth_context)?;

        // Validate state transition
        if !stored_task
            .task
            .status
            .can_transition_to(&TaskStatus::Cancelled)
        {
            return Err(ServerError::Protocol(ProtocolError::invalid_params(
                format!("Cannot cancel task in state {:?}", stored_task.task.status),
            )));
        }

        stored_task.task.status = TaskStatus::Cancelled;
        stored_task.task.status_message = reason.clone();
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
                .map_err(|_| ServerError::Lifecycle("Lock poisoned".to_string()))?;

            let stored_task = tasks.get(task_id).ok_or_else(|| {
                ServerError::Protocol(ProtocolError::invalid_params(format!(
                    "Task not found: {}",
                    task_id
                )))
            })?;

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
            .map_err(|_| ServerError::Lifecycle("Lock poisoned".to_string()))?;

        Ok(receiver.borrow().clone())
    }

    /// List all tasks (optionally filtered by auth context)
    pub fn list_tasks(&self, auth_context: Option<&str>) -> ServerResult<Vec<Task>> {
        let tasks = self
            .tasks
            .read()
            .map_err(|_| ServerError::Lifecycle("Lock poisoned".to_string()))?;

        let filtered_tasks: Vec<Task> = tasks
            .values()
            .filter(|stored_task| {
                // Filter by auth context if provided
                match (auth_context, &stored_task.auth_context) {
                    (Some(ctx), Some(task_ctx)) => ctx == task_ctx,
                    (None, _) => true,        // No auth context = see all
                    (Some(_), None) => false, // Auth provided but task has none
                }
            })
            .map(|stored_task| stored_task.task.clone())
            .collect();

        Ok(filtered_tasks)
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
                    return Err(ServerError::Protocol(ProtocolError::invalid_params(
                        "Unauthorized: task belongs to different context".to_string(),
                    )));
                }
            }
            (Some(_), None) => {
                return Err(ServerError::Protocol(ProtocolError::invalid_params(
                    "Unauthorized: task requires authentication".to_string(),
                )));
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
        let all_tasks = storage.list_tasks(None).unwrap();
        assert_eq!(all_tasks.len(), 3);

        // List tasks for user1
        let user1_tasks = storage.list_tasks(Some("user1")).unwrap();
        assert_eq!(user1_tasks.len(), 1);

        // List tasks for user2
        let user2_tasks = storage.list_tasks(Some("user2")).unwrap();
        assert_eq!(user2_tasks.len(), 1);
    }
}
