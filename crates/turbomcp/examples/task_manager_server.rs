//! Task Manager Server - Works with Interactive Client
//!
//! This server provides task management capabilities and can work with
//! clients that don't support elicitation by accepting direct parameters.
//!
//! Run this server in one terminal:
//! ```bash
//! cargo run --example task_manager_server
//! ```
//!
//! Then connect the client in another terminal:
//! ```bash
//! cargo run --example task_manager_server 2>/dev/null | \
//!   cargo run --package turbomcp-client --example elicitation_interactive_client
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use turbomcp::{Context, McpResult, server, tool};

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Task {
    id: String,
    title: String,
    description: String,
    priority: String,
    assigned_to: Option<String>,
    completed: bool,
}

#[derive(Clone)]
struct TaskManager {
    tasks: Arc<RwLock<HashMap<String, Task>>>,
}

#[server(
    name = "task-manager",
    version = "1.0.0",
    description = "Task management server with direct parameter support"
)]
impl TaskManager {
    fn new() -> Self {
        Self {
            tasks: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a new task with provided parameters
    #[tool("Create a new task")]
    async fn create_task(
        &self,
        _ctx: Context,
        title: String,
        description: Option<String>,
        priority: String,
        assigned_to: Option<String>,
    ) -> McpResult<String> {
        // Validate priority
        let valid_priorities = ["low", "medium", "high", "critical"];
        if !valid_priorities.contains(&priority.as_str()) {
            return Ok(format!(
                "❌ Invalid priority '{}'. Must be one of: low, medium, high, critical",
                priority
            ));
        }

        let task_id = format!("task-{}", uuid::Uuid::new_v4());
        let task = Task {
            id: task_id.clone(),
            title: title.clone(),
            description: description.unwrap_or_else(|| "No description".to_string()),
            priority: priority.clone(),
            assigned_to: assigned_to.clone(),
            completed: false,
        };

        self.tasks.write().await.insert(task_id.clone(), task);

        Ok(format!(
            "✅ Task created!\n\
             ID: {}\n\
             Title: {}\n\
             Priority: {}\n\
             Assigned: {}",
            task_id,
            title,
            priority,
            assigned_to.unwrap_or_else(|| "Unassigned".to_string())
        ))
    }

    /// List all tasks
    #[tool("List all tasks")]
    async fn list_tasks(&self) -> McpResult<String> {
        let tasks = self.tasks.read().await;

        if tasks.is_empty() {
            return Ok("No tasks yet. Create one with 'create_task'!".to_string());
        }

        let mut result = String::from("📋 Tasks:\n");
        for task in tasks.values() {
            result.push_str(&format!(
                "\n[{}] {} - {} ({})\n  {}\n  Assigned: {}\n",
                if task.completed { "✓" } else { " " },
                task.id,
                task.title,
                task.priority,
                task.description,
                task.assigned_to.as_deref().unwrap_or("Unassigned")
            ));
        }

        Ok(result)
    }

    /// Update a task's priority
    #[tool("Update task priority")]
    async fn update_priority(&self, task_id: String, new_priority: String) -> McpResult<String> {
        // Validate priority
        let valid_priorities = ["low", "medium", "high", "critical"];
        if !valid_priorities.contains(&new_priority.as_str()) {
            return Ok(format!(
                "❌ Invalid priority '{}'. Must be one of: low, medium, high, critical",
                new_priority
            ));
        }

        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.get_mut(&task_id) {
            let old_priority = task.priority.clone();
            task.priority = new_priority.clone();
            Ok(format!(
                "✅ Priority updated from {} to {}",
                old_priority, new_priority
            ))
        } else {
            Ok(format!("Task {} not found", task_id))
        }
    }

    /// Mark a task as completed
    #[tool("Mark task as completed")]
    async fn complete_task(&self, task_id: String) -> McpResult<String> {
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.get_mut(&task_id) {
            task.completed = true;
            Ok(format!("✅ Task '{}' marked as completed!", task.title))
        } else {
            Ok(format!("Task {} not found", task_id))
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    eprintln!("🚀 Task Manager Server");
    eprintln!("======================");
    eprintln!("This server provides task management capabilities.");
    eprintln!();
    eprintln!("Available tools:");
    eprintln!("  • create_task - Create a new task");
    eprintln!("  • list_tasks - List all tasks");
    eprintln!("  • update_priority - Update task priority");
    eprintln!("  • complete_task - Mark task as completed");
    eprintln!();
    eprintln!("Connect a client to start managing tasks!");
    eprintln!("Listening on stdio...");

    let server = TaskManager::new();
    server.run_stdio().await?;

    Ok(())
}
