//! Simple Elicitation Server (stdio) - Server Side
//!
//! Run this server to accept elicitation requests via stdio.
//! Pair with elicitation_stdio_client for a complete demo.
//!
//! Usage:
//! ```bash
//! # Terminal 1 - Start the server
//! cargo run --example elicitation_stdio_server
//!
//! # Terminal 2 - Connect a client (see elicitation_stdio_client.rs)
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use turbomcp::elicitation_api::{ElicitationResult, boolean, string};
use turbomcp::{Context, McpResult, elicit, server, tool};

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
    description = "Interactive task management with elicitation"
)]
impl TaskManager {
    fn new() -> Self {
        Self {
            tasks: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a new task with interactive prompts
    #[tool("Create a new task interactively")]
    async fn create_task(&self, ctx: Context) -> McpResult<String> {
        // Use elicitation to gather task details
        let result = elicit("Create a new task")
            .field(
                "title",
                string()
                    .min_length(1)
                    .max_length(100)
                    .description("Task title")
                    .build(),
            )
            .field(
                "description",
                string().description("Task description").build(),
            )
            .field(
                "priority",
                string()
                    .enum_values(vec![
                        "low".to_string(),
                        "medium".to_string(),
                        "high".to_string(),
                        "critical".to_string(),
                    ])
                    .description("Task priority")
                    .build(),
            )
            .field(
                "assign_to_someone",
                boolean().description("Assign to someone?").build(),
            )
            .field(
                "assigned_to",
                string()
                    .description("Who to assign to (if assigning)")
                    .build(),
            )
            .require(vec!["title", "priority"])
            .send(&ctx.request)
            .await?;

        match result {
            ElicitationResult::Accept(data) => {
                let task_id = format!("task-{}", uuid::Uuid::new_v4());
                let title = data.get::<String>("title")?;
                let description = data
                    .get::<String>("description")
                    .unwrap_or_else(|_| "No description".to_string());
                let priority = data.get::<String>("priority")?;

                let assigned_to = if data.get::<bool>("assign_to_someone").unwrap_or(false) {
                    data.get::<String>("assigned_to").ok()
                } else {
                    None
                };

                let task = Task {
                    id: task_id.clone(),
                    title: title.clone(),
                    description,
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
            ElicitationResult::Decline(reason) => Ok(format!(
                "Task creation cancelled: {}",
                reason.unwrap_or_else(|| "User declined".to_string())
            )),
            ElicitationResult::Cancel => Ok("Task creation cancelled.".to_string()),
        }
    }

    /// Update task priority interactively
    #[tool("Update a task's priority")]
    async fn update_priority(&self, ctx: Context, task_id: String) -> McpResult<String> {
        // Check if task exists
        let tasks = self.tasks.read().await;
        let task = tasks
            .get(&task_id)
            .ok_or_else(|| turbomcp::McpError::Tool(format!("Task {} not found", task_id)))?;
        let current_priority = task.priority.clone();
        let task_title = task.title.clone();
        drop(tasks);

        // Ask for new priority
        let result = elicit(format!("Update priority for: {}", task_title))
            .field(
                "new_priority",
                string()
                    .enum_values(vec![
                        "low".to_string(),
                        "medium".to_string(),
                        "high".to_string(),
                        "critical".to_string(),
                    ])
                    .description(format!("Current priority: {}", current_priority))
                    .build(),
            )
            .require(vec!["new_priority"])
            .send(&ctx.request)
            .await?;

        match result {
            ElicitationResult::Accept(data) => {
                let new_priority = data.get::<String>("new_priority")?;

                let mut tasks = self.tasks.write().await;
                if let Some(task) = tasks.get_mut(&task_id) {
                    task.priority = new_priority.clone();
                    Ok(format!(
                        "✅ Priority updated from {} to {}",
                        current_priority, new_priority
                    ))
                } else {
                    Ok("Task not found".to_string())
                }
            }
            _ => Ok("Priority update cancelled.".to_string()),
        }
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
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    eprintln!("🚀 Task Manager Server (Elicitation Demo)");
    eprintln!("==========================================");
    eprintln!("This server uses elicitation to interactively create tasks.");
    eprintln!("Connect a client that supports elicitation to test it!");
    eprintln!();
    eprintln!("Available tools:");
    eprintln!("  • create_task - Create task with interactive prompts");
    eprintln!("  • update_priority - Update task priority interactively");
    eprintln!("  • list_tasks - List all tasks");
    eprintln!();
    eprintln!("Listening on stdio...");

    let server = TaskManager::new();
    server.run_stdio().await?;

    Ok(())
}
