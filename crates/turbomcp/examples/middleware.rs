//! # Typed Middleware Example
//!
//! Demonstrates using McpMiddleware for cross-cutting concerns like:
//! - Logging all requests
//! - Metrics collection
//! - Access control
//! - Rate limiting
//!
//! Run with: `cargo run --example middleware`

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use serde_json::Value;
use turbomcp::__macro_support::turbomcp_core::handler::McpHandler;
use turbomcp::prelude::*;
use turbomcp_server::middleware::typed::{McpMiddleware, MiddlewareStack, Next};

// ============================================================================
// Logging Middleware
// ============================================================================

/// Logs all MCP operations with timing information.
#[derive(Clone)]
struct LoggingMiddleware;

impl McpMiddleware for LoggingMiddleware {
    fn on_list_tools<'a>(
        &'a self,
        next: Next<'a>,
    ) -> Pin<Box<dyn Future<Output = Vec<Tool>> + Send + 'a>> {
        Box::pin(async move {
            let start = Instant::now();
            let tools = next.list_tools();
            println!(
                "[LOG] list_tools: {} tools in {:?}",
                tools.len(),
                start.elapsed()
            );
            tools
        })
    }

    fn on_call_tool<'a>(
        &'a self,
        name: &'a str,
        args: Value,
        ctx: &'a RequestContext,
        next: Next<'a>,
    ) -> Pin<Box<dyn Future<Output = McpResult<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            let start = Instant::now();
            let name_owned = name.to_string();
            let result = next.call_tool(name, args, ctx).await;
            let status = if result.is_ok() { "OK" } else { "ERROR" };
            println!(
                "[LOG] call_tool '{}': {} in {:?}",
                name_owned,
                status,
                start.elapsed()
            );
            result
        })
    }

    fn on_read_resource<'a>(
        &'a self,
        uri: &'a str,
        ctx: &'a RequestContext,
        next: Next<'a>,
    ) -> Pin<Box<dyn Future<Output = McpResult<ResourceResult>> + Send + 'a>> {
        Box::pin(async move {
            let start = Instant::now();
            let uri_owned = uri.to_string();
            let result = next.read_resource(uri, ctx).await;
            let status = if result.is_ok() { "OK" } else { "ERROR" };
            println!(
                "[LOG] read_resource '{}': {} in {:?}",
                uri_owned,
                status,
                start.elapsed()
            );
            result
        })
    }

    fn on_initialize<'a>(
        &'a self,
        next: Next<'a>,
    ) -> Pin<Box<dyn Future<Output = McpResult<()>> + Send + 'a>> {
        Box::pin(async move {
            println!("[LOG] Server initializing...");
            let result = next.initialize().await;
            println!(
                "[LOG] Server initialized: {}",
                if result.is_ok() { "OK" } else { "ERROR" }
            );
            result
        })
    }

    fn on_shutdown<'a>(
        &'a self,
        next: Next<'a>,
    ) -> Pin<Box<dyn Future<Output = McpResult<()>> + Send + 'a>> {
        Box::pin(async move {
            println!("[LOG] Server shutting down...");
            let result = next.shutdown().await;
            println!("[LOG] Server shutdown complete");
            result
        })
    }
}

// ============================================================================
// Metrics Middleware
// ============================================================================

/// Collects metrics on tool calls.
#[derive(Clone)]
struct MetricsMiddleware {
    tool_calls: Arc<AtomicU64>,
    resource_reads: Arc<AtomicU64>,
    errors: Arc<AtomicU64>,
}

impl MetricsMiddleware {
    fn new() -> Self {
        Self {
            tool_calls: Arc::new(AtomicU64::new(0)),
            resource_reads: Arc::new(AtomicU64::new(0)),
            errors: Arc::new(AtomicU64::new(0)),
        }
    }

    fn print_stats(&self) {
        println!("\n[METRICS] Statistics:");
        println!("  Tool calls: {}", self.tool_calls.load(Ordering::Relaxed));
        println!(
            "  Resource reads: {}",
            self.resource_reads.load(Ordering::Relaxed)
        );
        println!("  Errors: {}", self.errors.load(Ordering::Relaxed));
    }
}

impl McpMiddleware for MetricsMiddleware {
    fn on_call_tool<'a>(
        &'a self,
        name: &'a str,
        args: Value,
        ctx: &'a RequestContext,
        next: Next<'a>,
    ) -> Pin<Box<dyn Future<Output = McpResult<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            self.tool_calls.fetch_add(1, Ordering::Relaxed);
            let result = next.call_tool(name, args, ctx).await;
            if result.is_err() {
                self.errors.fetch_add(1, Ordering::Relaxed);
            }
            result
        })
    }

    fn on_read_resource<'a>(
        &'a self,
        uri: &'a str,
        ctx: &'a RequestContext,
        next: Next<'a>,
    ) -> Pin<Box<dyn Future<Output = McpResult<ResourceResult>> + Send + 'a>> {
        Box::pin(async move {
            self.resource_reads.fetch_add(1, Ordering::Relaxed);
            let result = next.read_resource(uri, ctx).await;
            if result.is_err() {
                self.errors.fetch_add(1, Ordering::Relaxed);
            }
            result
        })
    }
}

// ============================================================================
// Access Control Middleware
// ============================================================================

/// Blocks access to tools with "dangerous" in their name.
#[derive(Clone)]
struct AccessControlMiddleware;

impl McpMiddleware for AccessControlMiddleware {
    fn on_list_tools<'a>(
        &'a self,
        next: Next<'a>,
    ) -> Pin<Box<dyn Future<Output = Vec<Tool>> + Send + 'a>> {
        Box::pin(async move {
            // Filter out dangerous tools from the list
            next.list_tools()
                .into_iter()
                .filter(|tool| !tool.name.contains("dangerous"))
                .collect()
        })
    }

    fn on_call_tool<'a>(
        &'a self,
        name: &'a str,
        args: Value,
        ctx: &'a RequestContext,
        next: Next<'a>,
    ) -> Pin<Box<dyn Future<Output = McpResult<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            // Block calls to dangerous tools
            if name.contains("dangerous") {
                return Err(McpError::invalid_request("Access denied: dangerous tool"));
            }
            next.call_tool(name, args, ctx).await
        })
    }
}

// ============================================================================
// Sample Server
// ============================================================================

#[derive(Clone)]
struct SampleServer;

#[turbomcp::server(name = "sample", version = "1.0.0")]
impl SampleServer {
    #[tool(description = "A safe operation")]
    async fn safe_operation(&self) -> McpResult<String> {
        Ok("Safe operation completed".into())
    }

    #[tool(description = "A dangerous operation")]
    async fn dangerous_delete(&self) -> McpResult<String> {
        Ok("Deleted everything!".into())
    }

    #[tool(description = "Add two numbers")]
    async fn add(&self, a: i32, b: i32) -> McpResult<i32> {
        Ok(a + b)
    }

    #[resource("data://config")]
    async fn get_config(&self, _uri: String, _ctx: &RequestContext) -> McpResult<String> {
        Ok(r#"{"version": "1.0"}"#.into())
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Typed Middleware Demo ===\n");

    let server = SampleServer;
    let metrics = MetricsMiddleware::new();

    // Build middleware stack (order matters: first added = outermost)
    // Logging -> Metrics -> AccessControl -> Handler
    let stack = MiddlewareStack::new(server)
        .with_middleware(LoggingMiddleware)
        .with_middleware(metrics.clone())
        .with_middleware(AccessControlMiddleware);

    // Show available tools (filtered by access control)
    println!("Available tools (after access control filtering):");
    println!("-------------------------------------------------");
    for tool in stack.list_tools() {
        println!("  - {}: {:?}", tool.name, tool.description);
    }
    println!();

    // Make some calls
    tokio::runtime::Runtime::new()?.block_on(async {
        // Lifecycle: initialize through middleware chain
        println!("Lifecycle hooks:");
        println!("----------------");
        stack.on_initialize().await?;
        println!();

        let ctx = RequestContext::default();

        println!("Making tool calls:\n");

        // Call safe operation
        let _ = stack
            .call_tool("safe_operation", serde_json::json!({}), &ctx)
            .await;

        // Call add
        let result = stack
            .call_tool("add", serde_json::json!({"a": 5, "b": 3}), &ctx)
            .await?;
        println!("  add(5, 3) = {:?}", result.first_text());

        // Try to call dangerous tool (blocked by access control)
        let result = stack
            .call_tool("dangerous_delete", serde_json::json!({}), &ctx)
            .await;
        println!(
            "  dangerous_delete: {:?}",
            result.err().map(|e| e.to_string())
        );

        // Read resource
        let _ = stack.read_resource("data://config", &ctx).await;

        // Lifecycle: shutdown through middleware chain
        println!();
        stack.on_shutdown().await?;

        Ok::<_, McpError>(())
    })?;

    // Print metrics
    metrics.print_stats();

    println!("\n=== Middleware Chain Order ===\n");
    println!("Request flow:  Client -> Logging -> Metrics -> AccessControl -> Handler");
    println!("Response flow: Handler -> AccessControl -> Metrics -> Logging -> Client");
    println!("\nThis allows:");
    println!("  - Logging sees all requests (even blocked ones)");
    println!("  - Metrics counts all attempts");
    println!("  - Access control filters tools and blocks calls");
    println!("  - Lifecycle hooks (on_initialize/on_shutdown) chain through middlewares");

    Ok(())
}
