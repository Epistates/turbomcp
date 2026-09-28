//! The draft Tasks extension (`io.modelcontextprotocol/tasks`, SEP-2663):
//! answer a `tools/call` with an asynchronous task handle instead of blocking.
//!
//! A `#[server]` exposes one slow tool, `generate_report`, which the
//! [`TasksExtension`] is configured to run as a *task*. A client that declares
//! the `io.modelcontextprotocol/tasks` capability gets a `CreateTaskResult`
//! (`resultType: "task"`) immediately and polls `tasks/get` for the outcome; a
//! client that doesn't runs the tool synchronously. (Run with
//! `--features ext-tasks`; speak the modern `2026-07-28` path.)
//!
//! ```text
//! cargo run -p turbomcp --example tasks --features ext-tasks
//! ```

use std::sync::Arc;
use std::time::Duration;

use turbomcp::ProtocolError;
use turbomcp::ext_tasks::TasksExtension;
use turbomcp::prelude::*;

#[derive(Clone)]
struct Reports;

#[server(name = "reports", version = "1.0.0")]
impl Reports {
    /// Generate a report on a topic. Slow enough to be worth deferring — the
    /// Tasks extension runs it as a background task.
    #[tool]
    async fn generate_report(
        &self,
        #[description("What to report on")] topic: String,
    ) -> McpResult<String> {
        // Stand-in for expensive work (a real server would do I/O here).
        tokio::time::sleep(Duration::from_millis(250)).await;
        Ok(format!("# Report: {topic}\n\nAll systems nominal."))
    }
}

#[tokio::main]
async fn main() -> Result<(), ProtocolError> {
    // Register the Tasks extension, opting the slow tool into task execution,
    // and serve over stdio (dual-stack, like `run_stdio()`).
    Reports
        .into_server()
        .with_extension(Arc::new(
            TasksExtension::new().task_tools(["generate_report"]),
        ))
        .serve(stdio())
        .await
}
