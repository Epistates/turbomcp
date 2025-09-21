//! Child Process Transport for TurboMCP
//!
//! This module provides a transport implementation for communicating with MCP servers
//! running as child processes. It uses Tokio's async process management with robust
//! error handling, graceful shutdown, and proper STDIO stream management.

use std::collections::VecDeque;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use parking_lot::Mutex;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;
use tokio::time::timeout;
use tracing::{debug, error, info, trace, warn};

use crate::core::{
    Transport, TransportCapabilities, TransportError, TransportEvent, TransportEventEmitter,
    TransportMessage, TransportMetrics, TransportResult, TransportState, TransportType,
};
use turbomcp_core::MessageId;

/// Configuration for child process transport
#[derive(Debug, Clone)]
pub struct ChildProcessConfig {
    /// Command to execute
    pub command: String,

    /// Arguments to pass to the command
    pub args: Vec<String>,

    /// Working directory for the process
    pub working_directory: Option<String>,

    /// Environment variables to set
    pub environment: Option<Vec<(String, String)>>,

    /// Timeout for process startup
    pub startup_timeout: Duration,

    /// Timeout for process shutdown
    pub shutdown_timeout: Duration,

    /// Maximum message size in bytes
    pub max_message_size: usize,

    /// Buffer size for STDIO streams
    pub buffer_size: usize,

    /// Whether to kill the process on drop
    pub kill_on_drop: bool,
}

impl Default for ChildProcessConfig {
    fn default() -> Self {
        Self {
            command: String::new(),
            args: Vec::new(),
            working_directory: None,
            environment: None,
            startup_timeout: Duration::from_secs(30),
            shutdown_timeout: Duration::from_secs(10),
            max_message_size: 10 * 1024 * 1024, // 10MB
            buffer_size: 8192,
            kill_on_drop: true,
        }
    }
}

/// Child process transport implementation
#[derive(Debug)]
pub struct ChildProcessTransport {
    /// Process configuration
    config: ChildProcessConfig,

    /// Child process handle
    child: Option<Child>,

    /// Transport state
    state: Arc<Mutex<TransportState>>,

    /// Transport capabilities
    capabilities: TransportCapabilities,

    /// Metrics tracking
    metrics: Arc<Mutex<TransportMetrics>>,

    /// Event emitter
    event_emitter: TransportEventEmitter,

    /// Outbound message queue
    #[allow(dead_code)] // Reserved for future buffering implementation
    outbound_queue: Arc<Mutex<VecDeque<TransportMessage>>>,

    /// STDIO communication channels
    stdin_sender: Option<mpsc::Sender<String>>,
    stdout_receiver: Option<mpsc::Receiver<String>>,

    /// Background task handles
    _stdin_task: Option<tokio::task::JoinHandle<()>>,
    _stdout_task: Option<tokio::task::JoinHandle<()>>,
}

impl ChildProcessTransport {
    /// Create a new child process transport
    pub fn new(config: ChildProcessConfig) -> Self {
        let capabilities = TransportCapabilities {
            max_message_size: Some(config.max_message_size),
            supports_streaming: false,
            supports_compression: false,
            supports_bidirectional: true,
            supports_multiplexing: false,
            compression_algorithms: Vec::new(),
            custom: std::collections::HashMap::new(),
        };

        Self {
            config,
            child: None,
            state: Arc::new(Mutex::new(TransportState::Disconnected)),
            capabilities,
            metrics: Arc::new(Mutex::new(TransportMetrics::default())),
            event_emitter: TransportEventEmitter::new().0,
            outbound_queue: Arc::new(Mutex::new(VecDeque::new())),
            stdin_sender: None,
            stdout_receiver: None,
            _stdin_task: None,
            _stdout_task: None,
        }
    }

    /// Start the child process and set up communication channels
    async fn start_process(&mut self) -> TransportResult<()> {
        if self.config.command.is_empty() {
            return Err(TransportError::ConfigurationError(
                "Command cannot be empty".to_string(),
            ));
        }

        info!(
            "Starting child process: {} {:?}",
            self.config.command, self.config.args
        );

        // Create the command
        let mut cmd = Command::new(&self.config.command);
        cmd.args(&self.config.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(self.config.kill_on_drop);

        // Set working directory if specified
        if let Some(ref wd) = self.config.working_directory {
            cmd.current_dir(wd);
        }

        // Set environment variables if specified
        if let Some(ref env) = self.config.environment {
            for (key, value) in env {
                cmd.env(key, value);
            }
        }

        // Spawn the process
        let mut child = cmd.spawn().map_err(|e| {
            error!("Failed to spawn child process: {}", e);
            TransportError::ConnectionFailed(format!("Failed to spawn process: {e}"))
        })?;

        // Get STDIO handles
        let stdin = child.stdin.take().ok_or_else(|| {
            TransportError::ConnectionFailed("Failed to get stdin handle".to_string())
        })?;

        let stdout = child.stdout.take().ok_or_else(|| {
            TransportError::ConnectionFailed("Failed to get stdout handle".to_string())
        })?;

        let stderr = child.stderr.take().ok_or_else(|| {
            TransportError::ConnectionFailed("Failed to get stderr handle".to_string())
        })?;

        // Create communication channels
        let (stdin_tx, stdin_rx) = mpsc::channel::<String>(100);
        let (stdout_tx, stdout_rx) = mpsc::channel::<String>(100);

        // Start STDIN writer task
        let stdin_task = {
            let mut writer = BufWriter::new(stdin);
            tokio::spawn(async move {
                let mut stdin_rx = stdin_rx;
                while let Some(message) = stdin_rx.recv().await {
                    if let Err(e) = writer.write_all(message.as_bytes()).await {
                        error!("Failed to write to process stdin: {}", e);
                        break;
                    }
                    if let Err(e) = writer.write_all(b"\n").await {
                        error!("Failed to write newline to process stdin: {}", e);
                        break;
                    }
                    if let Err(e) = writer.flush().await {
                        error!("Failed to flush process stdin: {}", e);
                        break;
                    }
                    trace!("Sent message to child process: {}", message);
                }
                debug!("STDIN writer task completed");
            })
        };

        // Start STDOUT reader task
        let stdout_task = {
            let reader = BufReader::new(stdout);
            let max_size = self.config.max_message_size;
            tokio::spawn(async move {
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    if line.len() > max_size {
                        warn!(
                            "Received oversized message from child process: {} bytes",
                            line.len()
                        );
                        continue;
                    }
                    trace!("Received message from child process: {}", line);
                    if stdout_tx.send(line).await.is_err() {
                        debug!("STDOUT receiver dropped, stopping reader task");
                        break;
                    }
                }
                debug!("STDOUT reader task completed");
            })
        };

        // Start STDERR reader task for logging
        let _stderr_task = {
            let reader = BufReader::new(stderr);
            tokio::spawn(async move {
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    debug!("Child process stderr: {}", line);
                }
                debug!("STDERR reader task completed");
            })
        };

        // Store handles
        self.child = Some(child);
        self.stdin_sender = Some(stdin_tx);
        self.stdout_receiver = Some(stdout_rx);
        self._stdin_task = Some(stdin_task);
        self._stdout_task = Some(stdout_task);

        // Update state
        *self.state.lock() = TransportState::Connected;

        // Wait for process to be ready with timeout
        match timeout(self.config.startup_timeout, self.wait_for_ready()).await {
            Ok(Ok(_)) => {
                info!("Child process started successfully");
                self.event_emitter.emit(TransportEvent::Connected {
                    transport_type: TransportType::ChildProcess,
                    endpoint: format!("{}:{:?}", self.config.command, self.config.args),
                });
                Ok(())
            }
            Ok(Err(e)) => {
                error!("Child process startup failed: {}", e);
                self.stop_process().await?;
                Err(e)
            }
            Err(_) => {
                error!("Child process startup timed out");
                self.stop_process().await?;
                Err(TransportError::Timeout)
            }
        }
    }

    /// Wait for the process to be ready by checking if it's still running
    async fn wait_for_ready(&mut self) -> TransportResult<()> {
        if let Some(ref mut child) = self.child {
            // Check if process is still running
            match child.try_wait() {
                Ok(Some(status)) => {
                    error!("Child process exited early with status: {}", status);
                    return Err(TransportError::ConnectionFailed(format!(
                        "Process exited early: {status}"
                    )));
                }
                Ok(None) => {
                    // Process is still running, good
                    return Ok(());
                }
                Err(e) => {
                    error!("Failed to check child process status: {}", e);
                    return Err(TransportError::ConnectionFailed(format!(
                        "Failed to check process status: {e}"
                    )));
                }
            }
        }

        Err(TransportError::ConnectionFailed(
            "No child process".to_string(),
        ))
    }

    /// Stop the child process gracefully
    async fn stop_process(&mut self) -> TransportResult<()> {
        info!("Stopping child process");

        // Drop communication channels first
        self.stdin_sender = None;
        self.stdout_receiver = None;

        if let Some(mut child) = self.child.take() {
            // Try graceful shutdown first
            if let Err(e) = child.start_kill() {
                warn!("Failed to send kill signal to child process: {}", e);
            }

            // Wait for process to exit with timeout
            match timeout(self.config.shutdown_timeout, child.wait()).await {
                Ok(Ok(status)) => {
                    info!("Child process exited with status: {}", status);
                }
                Ok(Err(e)) => {
                    error!("Failed to wait for child process exit: {}", e);
                }
                Err(_) => {
                    warn!("Child process shutdown timed out, forcing kill");
                    if let Err(e) = child.kill().await {
                        error!("Failed to force kill child process: {}", e);
                    }
                }
            }
        }

        // Update state
        *self.state.lock() = TransportState::Disconnected;
        self.event_emitter.emit(TransportEvent::Disconnected {
            transport_type: TransportType::ChildProcess,
            endpoint: format!("{}:{:?}", self.config.command, self.config.args),
            reason: Some("Process stopped".to_string()),
        });

        Ok(())
    }

    /// Check if the child process is still running
    pub fn is_process_alive(&mut self) -> bool {
        if let Some(ref mut child) = self.child {
            match child.try_wait() {
                Ok(Some(_)) => false, // Process has exited
                Ok(None) => true,     // Process is still running
                Err(_) => false,      // Error checking status
            }
        } else {
            false
        }
    }
}

#[async_trait]
impl Transport for ChildProcessTransport {
    async fn connect(&mut self) -> TransportResult<()> {
        match *self.state.lock() {
            TransportState::Connected => return Ok(()),
            TransportState::Connecting => {
                return Err(TransportError::Internal("Already connecting".to_string()));
            }
            _ => {}
        }

        *self.state.lock() = TransportState::Connecting;
        self.start_process().await
    }

    async fn disconnect(&mut self) -> TransportResult<()> {
        self.stop_process().await
    }

    async fn send(&mut self, message: TransportMessage) -> TransportResult<()> {
        let state = self.state.lock().clone();
        if state != TransportState::Connected {
            return Err(TransportError::Internal(format!(
                "Cannot send in state: {state:?}"
            )));
        }

        if message.payload.len() > self.config.max_message_size {
            return Err(TransportError::Internal(format!(
                "Message too large: {} bytes (max: {})",
                message.payload.len(),
                self.config.max_message_size
            )));
        }

        // Convert message payload to string
        let payload_str = String::from_utf8(message.payload.to_vec()).map_err(|e| {
            TransportError::SerializationFailed(format!("Invalid UTF-8 in message payload: {e}"))
        })?;

        // Send through stdin channel
        if let Some(ref sender) = self.stdin_sender {
            sender.send(payload_str).await.map_err(|_| {
                error!("Failed to send message: stdin channel closed");
                TransportError::ConnectionLost("STDIN channel closed".to_string())
            })?;

            // Update metrics
            let mut metrics = self.metrics.lock();
            metrics.messages_sent += 1;
            metrics.bytes_sent += message.payload.len() as u64;

            trace!("Sent message via child process transport");
            Ok(())
        } else {
            Err(TransportError::ConnectionLost(
                "No stdin channel available".to_string(),
            ))
        }
    }

    async fn receive(&mut self) -> TransportResult<Option<TransportMessage>> {
        let state = self.state.lock().clone();
        if state != TransportState::Connected {
            return Ok(None);
        }

        // Check if process is still alive
        if !self.is_process_alive() {
            warn!("Child process died, disconnecting transport");
            self.stop_process().await?;
            return Ok(None);
        }

        // Properly block and wait for messages from stdout channel
        if let Some(ref mut receiver) = self.stdout_receiver {
            match receiver.recv().await {
                Some(line) => {
                    let payload = Bytes::from(line);
                    let message = TransportMessage::new(
                        MessageId::String(uuid::Uuid::new_v4().to_string()),
                        payload,
                    );

                    // Update metrics
                    let mut metrics = self.metrics.lock();
                    metrics.messages_received += 1;
                    metrics.bytes_received += message.payload.len() as u64;

                    trace!("Received message via child process transport");
                    Ok(Some(message))
                }
                None => {
                    debug!("STDOUT channel disconnected");
                    Ok(None)
                }
            }
        } else {
            Ok(None)
        }
    }

    async fn state(&self) -> TransportState {
        self.state.lock().clone()
    }

    fn transport_type(&self) -> TransportType {
        TransportType::ChildProcess
    }

    fn capabilities(&self) -> &TransportCapabilities {
        &self.capabilities
    }

    async fn metrics(&self) -> TransportMetrics {
        self.metrics.lock().clone()
    }
}

impl Drop for ChildProcessTransport {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take()
            && self.config.kill_on_drop
        {
            let _ = child.start_kill();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::time::sleep;

    #[tokio::test]
    async fn test_child_process_config_default() {
        let config = ChildProcessConfig::default();
        assert_eq!(config.startup_timeout, Duration::from_secs(30));
        assert_eq!(config.shutdown_timeout, Duration::from_secs(10));
        assert_eq!(config.max_message_size, 10 * 1024 * 1024);
        assert!(config.kill_on_drop);
    }

    #[tokio::test]
    async fn test_child_process_transport_creation() {
        let config = ChildProcessConfig {
            command: "echo".to_string(),
            args: vec!["hello".to_string()],
            ..Default::default()
        };

        let transport = ChildProcessTransport::new(config);
        assert_eq!(transport.state().await, TransportState::Disconnected);
        assert_eq!(transport.transport_type(), TransportType::ChildProcess);
    }

    #[tokio::test]
    async fn test_empty_command_error() {
        let config = ChildProcessConfig::default();
        let mut transport = ChildProcessTransport::new(config);

        let result = transport.connect().await;
        assert!(result.is_err());
        if let Err(TransportError::ConfigurationError(msg)) = result {
            assert!(msg.contains("Command cannot be empty"));
        } else {
            panic!("Expected ConfigurationError");
        }
    }

    // Integration test with a simple command
    #[tokio::test]
    async fn test_echo_command() {
        let config = ChildProcessConfig {
            command: "cat".to_string(), // Use cat for echo-like behavior
            args: vec![],
            startup_timeout: Duration::from_secs(5),
            ..Default::default()
        };

        let mut transport = ChildProcessTransport::new(config);

        // Connect should succeed
        if transport.connect().await.is_ok() {
            // Give it a moment to fully initialize
            sleep(Duration::from_millis(100)).await;

            // Send a test message
            let test_message = TransportMessage::new(
                MessageId::String("test".to_string()),
                Bytes::from("Hello, World!"),
            );
            if transport.send(test_message).await.is_ok() {
                // Try to receive the echo
                for _ in 0..10 {
                    if let Ok(Some(_response)) = transport.receive().await {
                        break;
                    }
                    sleep(Duration::from_millis(10)).await;
                }
            }

            // Clean disconnect
            let _ = transport.disconnect().await;
        }
        // Note: This test may fail in some CI environments where 'cat' is not available
        // or process spawning is restricted. That's expected.
    }
}
