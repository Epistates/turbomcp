//! # Tasks API Demo
//!
//! Demonstrates the MCP Tasks API (SEP-1686) for handling long-running operations.
//!
//! This server implements a `long_running_analysis` tool that simulates heavy work.
//! When called with `task: { ... }` metadata, it returns immediately with a task ID,
//! runs in the background, and results can be retrieved via `tasks/get` and `tasks/result`.
//!
//! ## Run the Demo
//!
//! ```bash
//! cargo run --example tasks_demo --features "stdio experimental-tasks"
//! ```
//!
//! ## Client Interaction Example (JSON-RPC)
//!
//! 1. Call the tool with task metadata:
//! ```json
//! {
//!   "jsonrpc": "2.0",
//!   "id": 1,
//!   "method": "tools/call",
//!   "params": {
//!     "name": "analyze_data",
//!     "arguments": { "dataset_size": 100 },
//!     "task": { "ttl": 60000 }
//!   }
//! }
//! ```
//!
//! 2. Server responds immediately:
//! ```json
//! {
//!   "jsonrpc": "2.0",
//!   "id": 1,
//!   "result": {
//!     "task": { "taskId": "...", "status": "working", ... }
//!   }
//! }
//! ```
//!
//! 3. Poll status:
//! ```json
//! {
//!   "jsonrpc": "2.0",
//!   "id": 2,
//!   "method": "tasks/get",
//!   "params": { "taskId": "..." }
//! }
//! ```
//!
//! 4. Get result (blocks until done):
//! ```json
//! {
//!   "jsonrpc": "2.0",
//!   "id": 3,
//!   "method": "tasks/result",
//!   "params": { "taskId": "..." }
//! }
//! ```

use tokio::time::{Duration, sleep};
use turbomcp::prelude::*;

#[derive(Clone)]
struct TasksServer;

#[turbomcp::server(name = "tasks-demo", version = "1.0.0", transports = ["stdio"])]
impl TasksServer {
    /// A simulated long-running analysis tool.
    ///
    /// When invoked as a task, this runs asynchronously.
    #[tool(
        name = "analyze_data",
        description = "Analyzes a dataset (simulated long operation). Use task augmentation to run asynchronously."
    )]
    async fn analyze_data(&self, ctx: Context, dataset_size: u64) -> McpResult<String> {
        ctx.info(&format!("Starting analysis of {} items...", dataset_size))
            .await?;

        // Simulate progress (in a real app, you could update task status/progress here if exposed)
        let steps = 5;
        let step_time = Duration::from_secs(1);

        for i in 1..=steps {
            // Simulate work
            sleep(step_time).await;
            ctx.info(&format!("Analysis progress: {}/{}", i, steps))
                .await?;
        }

        Ok(format!(
            "Analysis complete for {} items. Found 42 anomalies.",
            dataset_size
        ))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Starting Tasks Demo Server...");
    println!("Call 'analyze_data' with 'task' metadata to test async execution.");
    TasksServer.run_stdio().await?;
    Ok(())
}
