//! Child Process Transport Example - Spawn and communicate with MCP servers as child processes
//!
//! This example demonstrates TurboMCP's child process transport capabilities, allowing you to:
//! - Spawn external MCP servers as child processes
//! - Communicate with them over STDIO using JSON-RPC
//! - Manage process lifecycle (startup, shutdown, error handling)
//! - Handle process failures gracefully with automatic restart
//!
//! # Use Cases
//!
//! Child process transport is perfect for:
//! - **Language bridges**: Call Python/Node.js/Go MCP servers from Rust
//! - **Sandboxing**: Isolate potentially unsafe operations in separate processes
//! - **Resource isolation**: Prevent memory leaks or crashes from affecting the main process
//! - **Dynamic loading**: Spawn different servers based on runtime configuration
//! - **Legacy integration**: Communicate with existing command-line tools
//!
//! # Usage
//!
//! ```bash
//! # Run with a simple command (like cat for echo behavior)
//! cargo run --example 11_child_process -- cat
//!
//! # Run with Python server
//! cargo run --example 11_child_process -- python3 my_mcp_server.py
//!
//! # Run with Node.js server  
//! cargo run --example 11_child_process -- node my_mcp_server.js
//!
//! # Run with custom arguments
//! cargo run --example 11_child_process -- my-server --config config.json --verbose
//! ```
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────┐    STDIO     ┌─────────────────┐
//! │   Rust Parent   │◄──────────► │  Child Process  │
//! │   (This Code)   │   JSON-RPC   │   (Any Lang)    │
//! └─────────────────┘              └─────────────────┘
//! ```
//!
//! The parent process manages:
//! - Process spawning and lifecycle
//! - STDIO communication channels  
//! - Error handling and recovery
//! - Message routing and validation
//! - Graceful shutdown procedures

use std::env;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::time::{sleep, timeout};
use tracing::{debug, error, info, warn};
use turbomcp::prelude::*;

/// Our parent MCP server that manages child processes
#[derive(Clone)]
struct ChildProcessManager {
    /// Command to execute for the child process
    command: String,
    /// Arguments to pass to the child process
    args: Vec<String>,
}

#[server(
    name = "ChildProcessManager",
    version = "1.0.0",
    description = "Manages child processes for MCP communication"
)]
impl ChildProcessManager {
    #[tool("Start a child process with the configured command")]
    async fn start_child(&self) -> McpResult<String> {
        match self.spawn_child_process().await {
            Ok(child_id) => Ok(format!(
                "✅ Successfully started child process (PID: {}): {} {}",
                child_id,
                self.command,
                self.args.join(" ")
            )),
            Err(e) => Err(McpError::Internal(format!(
                "Failed to start child process: {}",
                e
            ))),
        }
    }

    #[tool("Send a message to the child process and get response")]
    async fn send_message(&self, message: String) -> McpResult<String> {
        if message.trim().is_empty() {
            return Err(McpError::InvalidInput(
                "Message cannot be empty".to_string(),
            ));
        }

        // For this demo, we'll create a temporary child process
        // For long-running tools, maintain persistent processes
        match self.send_to_child(&message).await {
            Ok(response) => Ok(format!("Child response: {}", response)),
            Err(e) => Err(McpError::Internal(format!(
                "Failed to communicate with child: {}",
                e
            ))),
        }
    }

    #[tool("Get information about the configured child process")]
    async fn get_child_info(&self) -> McpResult<String> {
        Ok(format!(
            "Child process configuration:\n• Command: {}\n• Arguments: [{}]\n• Full command: {} {}",
            self.command,
            self.args.join(", "),
            self.command,
            self.args.join(" ")
        ))
    }

    #[tool("Test child process with echo behavior")]
    async fn test_echo(&self, text: String) -> McpResult<String> {
        // Use 'cat' command for reliable echo behavior across platforms
        let echo_manager = ChildProcessManager {
            command: "cat".to_string(),
            args: vec![],
        };

        match echo_manager.send_to_child(&text).await {
            Ok(response) => Ok(format!("Echo test successful: '{}'", response.trim())),
            Err(e) => Err(McpError::Internal(format!("Echo test failed: {}", e))),
        }
    }
}

impl ChildProcessManager {
    /// Spawn a child process and return its PID
    async fn spawn_child_process(&self) -> Result<u32, Box<dyn std::error::Error + Send + Sync>> {
        info!("Spawning child process: {} {:?}", self.command, self.args);

        let mut cmd = Command::new(&self.command);
        cmd.args(&self.args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);

        let mut child = cmd.spawn()?;
        let pid = child.id().unwrap_or(0);

        info!("✅ Child process started with PID: {}", pid);

        // Let the child process initialize
        sleep(Duration::from_millis(100)).await;

        // Check if process is still running
        if let Ok(Some(status)) = child.try_wait() {
            return Err(format!("Child process exited early with status: {}", status).into());
        }

        Ok(pid)
    }

    /// Send a message to a child process and get the response
    async fn send_to_child(
        &self,
        message: &str,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        debug!("Sending message to child process: '{}'", message);

        let mut cmd = Command::new(&self.command);
        cmd.args(&self.args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);

        let mut child = cmd.spawn()?;

        // Get handles to stdin/stdout
        let stdin = child.stdin.take().ok_or("Failed to get stdin handle")?;
        let stdout = child.stdout.take().ok_or("Failed to get stdout handle")?;
        let stderr = child.stderr.take().ok_or("Failed to get stderr handle")?;

        // Send message to child process
        let mut stdin_writer = stdin;
        stdin_writer.write_all(message.as_bytes()).await?;
        stdin_writer.write_all(b"\n").await?;
        stdin_writer.shutdown().await?;

        // Read response with timeout
        let response_future = async {
            let mut stdout_reader = BufReader::new(stdout);
            let mut stderr_reader = BufReader::new(stderr);
            let mut response = String::new();
            let mut error_output = String::new();

            // Try to read from stdout first
            if (stdout_reader.read_line(&mut response).await).is_ok() && !response.trim().is_empty()
            {
                return Ok::<String, Box<dyn std::error::Error + Send + Sync>>(
                    response.trim().to_string(),
                );
            }

            // If no stdout, check stderr for error messages
            if (stderr_reader.read_line(&mut error_output).await).is_ok()
                && !error_output.trim().is_empty()
            {
                return Err(format!("Child process error: {}", error_output.trim()).into());
            }

            // If no output from either, return timeout error
            Err::<String, Box<dyn std::error::Error + Send + Sync>>(
                "No response from child process".into(),
            )
        };

        let response = timeout(Duration::from_secs(5), response_future).await??;

        // Wait for child process to complete
        match timeout(Duration::from_secs(2), child.wait()).await {
            Ok(Ok(status)) => {
                debug!("Child process completed with status: {}", status);
            }
            Ok(Err(e)) => {
                warn!("Error waiting for child process: {}", e);
            }
            Err(_) => {
                warn!("Child process did not complete within timeout");
                let _ = child.kill().await;
            }
        }

        Ok(response)
    }

    /// Demonstrate advanced child process management
    #[allow(dead_code)] // Demo method for documentation purposes
    async fn manage_long_running_child(
        &self,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        info!("Starting long-running child process management demo");

        let mut cmd = Command::new(&self.command);
        cmd.args(&self.args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);

        let mut child = cmd.spawn()?;
        let pid = child.id().unwrap_or(0);

        info!("Long-running child started with PID: {}", pid);

        // Simulate some communication
        if let Some(stdin) = child.stdin.as_mut() {
            stdin.write_all(b"test message\n").await?;
        }

        // Check process health
        sleep(Duration::from_millis(500)).await;

        if let Ok(Some(status)) = child.try_wait() {
            warn!("Child process exited with status: {}", status);
            return Ok(format!("Child process completed with status: {}", status));
        }

        // Graceful shutdown
        info!("Shutting down child process gracefully");
        if let Err(e) = child.kill().await {
            warn!("Error killing child process: {}", e);
        }

        match child.wait().await {
            Ok(status) => Ok(format!("Child process shut down successfully: {}", status)),
            Err(e) => Err(format!("Error during child process shutdown: {}", e).into()),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_target(false)
        .with_thread_ids(true)
        .init();

    // Parse command line arguments
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("❌ Usage: {} <command> [args...]", args[0]);
        eprintln!();
        eprintln!("Examples:");
        eprintln!("  {} cat", args[0]);
        eprintln!("  {} python3 my_server.py", args[0]);
        eprintln!("  {} node my_server.js --config config.json", args[0]);
        eprintln!();
        eprintln!("The child process will be spawned and managed by this MCP server.");
        eprintln!("Communication happens over STDIO using JSON-RPC protocol.");
        std::process::exit(1);
    }

    let command = args[1].clone();
    let child_args = if args.len() > 2 {
        args[2..].to_vec()
    } else {
        vec![]
    };

    info!("🚀 TurboMCP Child Process Transport Example");
    info!("============================================");
    info!("Command: {}", command);
    info!("Arguments: {:?}", child_args);

    // Verify the command exists
    match Command::new(&command).arg("--help").output().await {
        Ok(_) => {
            info!("✅ Command '{}' found and accessible", command);
        }
        Err(_) => {
            warn!("⚠️  Command '{}' may not exist or be accessible", command);
            info!("   This may cause child process operations to fail");
        }
    }

    // Create our child process manager
    let manager = ChildProcessManager {
        command,
        args: child_args,
    };

    // Demonstrate child process management
    info!("");
    info!("🔄 Demonstrating child process management...");

    // Test basic child process spawning
    match manager.spawn_child_process().await {
        Ok(pid) => info!("✅ Successfully spawned child process with PID: {}", pid),
        Err(e) => error!("❌ Failed to spawn child process: {}", e),
    }

    // Test communication if using a command that can echo (like cat)
    if manager.command == "cat" {
        info!("");
        info!("🗣️  Testing communication with child process...");

        match manager.send_to_child("Hello from parent process!").await {
            Ok(response) => info!("✅ Child process responded: '{}'", response),
            Err(e) => error!("❌ Communication failed: {}", e),
        }
    }

    // Demonstrate the MCP server capabilities
    info!("");
    info!("🎯 Starting MCP server with child process management tools");
    info!("   Available tools:");
    info!("   • start_child - Start the configured child process");
    info!("   • send_message - Send a message to child process");
    info!("   • get_child_info - Get child process configuration");
    info!("   • test_echo - Test echo behavior with 'cat' command");
    info!("");
    info!("📝 Reading JSON-RPC requests from stdin...");
    info!("   Press Ctrl+C to exit");

    // Run the MCP server
    manager.run_stdio().await?;

    info!("🛑 Child Process Manager shutting down");
    Ok(())
}

/// Example of how to implement a robust child process wrapper
#[allow(dead_code)]
struct RobustChildProcess {
    command: String,
    args: Vec<String>,
    child: Option<Child>,
    restart_count: u32,
    max_restarts: u32,
}

#[allow(dead_code)]
impl RobustChildProcess {
    fn new(command: String, args: Vec<String>) -> Self {
        Self {
            command,
            args,
            child: None,
            restart_count: 0,
            max_restarts: 3,
        }
    }

    async fn ensure_running(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Check if child is still running
        if let Some(child) = &mut self.child
            && let Ok(Some(status)) = child.try_wait()
        {
            warn!("Child process exited with status: {}", status);
            self.child = None;
        }

        // Start child if not running
        if self.child.is_none() {
            if self.restart_count >= self.max_restarts {
                return Err("Maximum restart attempts reached".into());
            }

            info!(
                "Starting child process (attempt {})",
                self.restart_count + 1
            );

            let mut cmd = Command::new(&self.command);
            cmd.args(&self.args)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .kill_on_drop(true);

            self.child = Some(cmd.spawn()?);
            self.restart_count += 1;

            // Give child time to initialize
            sleep(Duration::from_millis(100)).await;
        }

        Ok(())
    }

    async fn shutdown(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(mut child) = self.child.take() {
            info!("Shutting down child process gracefully");

            // Try graceful shutdown first
            let _ = child.kill().await;

            // Wait for process to exit
            match timeout(Duration::from_secs(5), child.wait()).await {
                Ok(Ok(status)) => info!("Child process shut down with status: {}", status),
                Ok(Err(e)) => warn!("Error during child shutdown: {}", e),
                Err(_) => warn!("Child process shutdown timed out"),
            }
        }

        Ok(())
    }
}
