//! # 12: Cooperative Cancellation - Graceful Task Termination
//!
//! **Learning Goals (15 minutes):**
//! - Understand cooperative cancellation vs hard timeouts
//! - Learn cancellation token patterns for graceful shutdown
//! - Master cleanup strategies when operations are cancelled
//! - See real-world examples of cancellation-safe operations
//!
//! **What this example demonstrates:**
//! - Basic cancellation checking in short-running tools
//! - Periodic cancellation checks in long-running operations
//! - Proper resource cleanup on cancellation
//! - Integration with async operations and cancellation tokens
//! - Error handling for cancelled operations
//!
//! **Key Security Concepts:**
//! - Prevents resource leaks through proper cleanup
//! - Enables graceful shutdown of long-running operations
//! - Works alongside hard timeouts as a defense-in-depth strategy
//! - Allows tools to save intermediate state before termination
//!
//! **Run with:** `cargo run --example 12_cooperative_cancellation`

use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::time::sleep;
use turbomcp::prelude::*;

/// Server demonstrating cooperative cancellation patterns
#[derive(Debug, Clone)]
struct CancellationDemoServer;

/// Parameters for long-running processing operations
#[derive(Debug, Deserialize, Serialize)]
struct ProcessingParams {
    /// Number of items to process
    item_count: u32,
    /// Processing delay per item (milliseconds)
    delay_ms: u64,
    /// Whether to simulate work that can be interrupted
    interruptible: Option<bool>,
}

/// Parameters for file operations with cleanup
#[derive(Debug, Deserialize, Serialize)]
struct FileOperationParams {
    /// File path to operate on
    file_path: String,
    /// Operation size (simulated bytes to process)
    operation_size: u64,
    /// Whether to save progress on cancellation
    save_progress: Option<bool>,
}

#[turbomcp::server(name = "CancellationDemo", version = "1.0.0")]
impl CancellationDemoServer {
    /// Quick operation with basic cancellation check
    ///
    /// Demonstrates the simplest form of cooperative cancellation -
    /// checking the token before starting work
    #[tool("Perform a quick operation with cancellation support")]
    async fn quick_operation(&self, ctx: Context, message: String) -> McpResult<String> {
        // Starting quick operation

        // Basic cancellation check before starting work
        if let Some(token) = &ctx.request.cancellation_token
            && token.is_cancelled()
        {
            // Operation cancelled before starting
            return Err(McpError::Tool(
                "Operation was cancelled before starting".to_string(),
            ));
        }

        // Simulate some quick work
        sleep(Duration::from_millis(100)).await;

        // Final cancellation check before returning results
        if let Some(token) = &ctx.request.cancellation_token
            && token.is_cancelled()
        {
            // Operation cancelled before completion
            return Err(McpError::Tool(
                "Operation was cancelled during processing".to_string(),
            ));
        }

        let result = format!("Completed: {}", message);
        // Quick operation successful
        Ok(result)
    }

    /// Long-running operation with periodic cancellation checks
    ///
    /// Demonstrates periodic cancellation checking in loops and
    /// how to provide progress feedback before cancellation
    #[tool("Process items with cooperative cancellation support")]
    async fn long_running_process(
        &self,
        ctx: Context,
        params: ProcessingParams,
    ) -> McpResult<String> {
        // Starting long-running process

        let interruptible = params.interruptible.unwrap_or(true);
        let mut processed_count = 0u32;
        let delay = Duration::from_millis(params.delay_ms);

        for _i in 0..params.item_count {
            // Periodic cancellation check - critical for long-running operations
            if let Some(token) = &ctx.request.cancellation_token
                && token.is_cancelled()
            {
                // Operation cancelled gracefully

                if interruptible {
                    // Graceful cancellation with progress report
                    return Ok(format!(
                        "Operation cancelled gracefully. Processed {} of {} items. Progress saved.",
                        processed_count, params.item_count
                    ));
                } else {
                    // Non-interruptible work - still respect cancellation but indicate constraint
                    return Err(McpError::Tool(format!(
                        "Operation cancelled during non-interruptible phase. Processed {} of {} items.",
                        processed_count, params.item_count
                    )));
                }
            }

            // Simulate processing work
            // Processing item
            sleep(delay).await;
            processed_count += 1;

            // Progress logging every 10 items
            if processed_count % 10 == 0 {
                // Progress update
            }
        }

        // Long-running process completed successfully
        Ok(format!(
            "Successfully processed all {} items",
            params.item_count
        ))
    }

    /// File operation with resource cleanup on cancellation
    ///
    /// Demonstrates proper resource management and cleanup when
    /// operations are cancelled mid-execution
    #[tool("Perform file operations with proper cancellation cleanup")]
    async fn file_operation_with_cleanup(
        &self,
        ctx: Context,
        params: FileOperationParams,
    ) -> McpResult<String> {
        // Starting file operation

        let save_progress = params.save_progress.unwrap_or(true);
        let mut bytes_processed = 0u64;
        let chunk_size = 1024u64; // Process in 1KB chunks

        // Simulate opening resources (in real code, these would be actual file handles, etc.)
        // Opening file resources

        while bytes_processed < params.operation_size {
            // Check for cancellation before each chunk
            if let Some(token) = &ctx.request.cancellation_token
                && token.is_cancelled()
            {
                // File operation cancelled, performing cleanup

                // Perform cleanup operations
                if save_progress && bytes_processed > 0 {
                    // Saving progress
                    // In real code: save intermediate state, flush buffers, etc.
                    sleep(Duration::from_millis(50)).await; // Simulate cleanup time

                    return Ok(format!(
                        "File operation cancelled gracefully. Processed {} of {} bytes. Progress saved to {}.partial",
                        bytes_processed, params.operation_size, params.file_path
                    ));
                } else {
                    // Quick cleanup without saving
                    // Performing quick cleanup
                    // In real code: close handles, release locks, etc.
                    sleep(Duration::from_millis(10)).await;

                    return Err(McpError::Tool(format!(
                        "File operation cancelled. Processed {} of {} bytes. No progress saved.",
                        bytes_processed, params.operation_size
                    )));
                }
            }

            // Process a chunk of data
            let chunk = std::cmp::min(chunk_size, params.operation_size - bytes_processed);
            sleep(Duration::from_millis(10)).await; // Simulate processing time
            bytes_processed += chunk;

            // Progress reporting
            if bytes_processed % (chunk_size * 10) == 0 {
                // File progress update
            }
        }

        // File operation completed successfully
        Ok(format!(
            "Successfully processed {} bytes from {}",
            params.operation_size, params.file_path
        ))
    }

    /// Timed operation using tokio::select! with cancellation
    ///
    /// Demonstrates integration of cancellation tokens with other async patterns
    /// like timeouts and interval-based operations
    #[tool("Run timed operation that respects cancellation")]
    async fn timed_operation_with_cancellation(
        &self,
        ctx: Context,
        duration_seconds: u64,
    ) -> McpResult<String> {
        // Starting timed operation

        let mut interval = tokio::time::interval(Duration::from_millis(500));
        let mut ticks = 0u32;
        let max_ticks = (duration_seconds * 2) as u32; // 500ms intervals = 2 ticks per second

        loop {
            tokio::select! {
                // Regular interval tick
                _ = interval.tick() => {
                    ticks += 1;
                    // Tick progress

                    if ticks >= max_ticks {
                        // Timed operation completed normally
                        return Ok(format!("Timed operation completed after {} ticks ({} seconds)", ticks, duration_seconds));
                    }
                },

                // Cancellation handling
                _ = async {
                    if let Some(token) = &ctx.request.cancellation_token {
                        token.cancelled().await;
                    } else {
                        // If no cancellation token, wait forever (this branch won't execute)
                        std::future::pending::<()>().await;
                    }
                } => {
                    // Timed operation cancelled
                    let elapsed_seconds = (ticks as f64) / 2.0;
                    return Ok(format!(
                        "Timed operation cancelled gracefully after {:.1} seconds ({} of {} ticks)",
                        elapsed_seconds, ticks, max_ticks
                    ));
                }
            }
        }
    }

    /// Network operation simulation with connection cleanup
    ///
    /// Demonstrates cancellation handling for network operations,
    /// including proper connection cleanup and state management
    #[tool("Simulate network operation with cancellation and cleanup")]
    async fn network_operation(
        &self,
        ctx: Context,
        host: String,
        request_count: u32,
    ) -> McpResult<String> {
        // Starting network operation

        // Simulate connection establishment
        // Establishing connection
        sleep(Duration::from_millis(200)).await;

        let mut completed_requests = 0u32;
        let connection_active = true;

        for _request_id in 1..=request_count {
            // Check cancellation before each request
            if let Some(token) = &ctx.request.cancellation_token
                && token.is_cancelled()
            {
                // Network operation cancelled, closing connection

                // Simulate connection cleanup
                if connection_active {
                    // Gracefully closing connection
                    sleep(Duration::from_millis(100)).await; // Simulate cleanup time
                    // Connection closed
                }

                return Ok(format!(
                    "Network operation cancelled gracefully. Completed {} of {} requests to {}. Connection closed cleanly.",
                    completed_requests, request_count, host
                ));
            }

            // Simulate network request
            // Sending network request
            sleep(Duration::from_millis(150)).await;
            completed_requests += 1;

            // Progress reporting
            if completed_requests % 5 == 0 {
                // Network progress update
            }
        }

        // Simulate connection cleanup on success
        // Closing connection after successful completion
        sleep(Duration::from_millis(50)).await;
        // Connection closed successfully

        // Network operation completed successfully
        Ok(format!(
            "Successfully completed {} requests to {}. Connection closed cleanly.",
            request_count, host
        ))
    }
}

#[tokio::main]
async fn main() -> McpResult<()> {
    // CRITICAL: For MCP STDIO protocol, do NOT initialize any logging
    // stdout is reserved exclusively for JSON-RPC messages
    // stderr should also be avoided as it may interfere with some clients

    let server = CancellationDemoServer;
    server
        .run_stdio()
        .await
        .map_err(|e| McpError::internal(format!("Server error: {e}")))
}
