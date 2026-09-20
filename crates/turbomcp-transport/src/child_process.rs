//! Child Process Transport for TurboMCP
//!
//! This module provides a transport implementation for communicating with MCP servers
//! running as child processes. It uses Tokio's async process management with reliable
//! error handling, graceful shutdown, and proper STDIO stream management.
//!
//! # Interior Mutability Pattern
//!
//! This transport follows the research-backed hybrid mutex pattern:
//!
//! - **std::sync::Mutex** for state (short-lived locks, never cross .await)
//! - **AtomicMetrics** for lock-free counter updates (10-100x faster than Mutex)
//! - **tokio::sync::Mutex** for child process and I/O (cross .await points)

use parking_lot::Mutex;
use std::future::Future;
use std::pin::Pin;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use bytes::Bytes;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::process::{Child, Command};
use tokio::sync::{Mutex as TokioMutex, mpsc};
use tokio::time::timeout;
use tracing::{debug, error, info, trace, warn};

use crate::core::{
    AtomicMetrics, Transport, TransportCapabilities, TransportError, TransportEvent,
    TransportEventEmitter, TransportMessage, TransportMetrics, TransportResult, TransportState,
    TransportType,
};
use turbomcp_protocol::MessageId;

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

    /// How long to wait for the child to exit on its own after its stdin is
    /// closed, before escalating to a signal.
    ///
    /// MCP's stdio shutdown is close-stdin → wait → SIGTERM → SIGKILL, so this
    /// is the window a server gets to run its `on_shutdown` hook, flush caches
    /// and close handles.
    pub shutdown_timeout: Duration,

    /// Grace period between SIGTERM and SIGKILL, on Unix.
    ///
    /// A child that ignored the closed stdin is unlikely to need long, so this
    /// is deliberately much shorter than [`Self::shutdown_timeout`]. Windows
    /// has no SIGTERM equivalent and skips this step.
    pub sigterm_grace: Duration,

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
            sigterm_grace: Duration::from_secs(2),
            max_message_size: 10 * 1024 * 1024, // 10MB
            buffer_size: 8192,
            kill_on_drop: true,
        }
    }
}

/// Child process transport implementation
///
/// # Interior Mutability Architecture
///
/// Following research-backed 2025 Rust async best practices:
///
/// - `state`: std::sync::Mutex (short-lived locks, never held across .await)
/// - `metrics`: AtomicMetrics (lock-free counters, 10-100x faster than Mutex)
/// - `child`/I/O: tokio::sync::Mutex (held across .await, necessary for async operations)
#[derive(Debug)]
pub struct ChildProcessTransport {
    /// Process configuration (immutable after construction)
    config: ChildProcessConfig,

    /// Child process handle (tokio::sync::Mutex - crosses await boundaries)
    child: Arc<TokioMutex<Option<Child>>>,

    /// Transport state (std::sync::Mutex - never crosses await)
    state: Arc<Mutex<TransportState>>,

    /// Transport capabilities (immutable after construction)
    capabilities: TransportCapabilities,

    /// Lock-free atomic metrics (10-100x faster than Mutex)
    metrics: Arc<AtomicMetrics>,

    /// Event emitter
    event_emitter: TransportEventEmitter,

    /// STDIO communication channels (tokio::sync::Mutex - crosses await boundaries)
    stdin_sender: Arc<TokioMutex<Option<mpsc::Sender<String>>>>,
    stdout_receiver: Arc<TokioMutex<Option<mpsc::Receiver<String>>>>,

    /// Background task handles (tokio::sync::Mutex - crosses await boundaries)
    _stdin_task: Arc<TokioMutex<Option<tokio::task::JoinHandle<()>>>>,
    _stdout_task: Arc<TokioMutex<Option<tokio::task::JoinHandle<()>>>>,
    /// stderr drain task; tracked so `stop_process` can abort it on shutdown
    /// rather than relying on stderr-EOF after `kill_on_drop` to make it exit.
    _stderr_task: Arc<TokioMutex<Option<tokio::task::JoinHandle<()>>>>,
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
            child: Arc::new(TokioMutex::new(None)),
            state: Arc::new(Mutex::new(TransportState::Disconnected)),
            capabilities,
            metrics: Arc::new(AtomicMetrics::default()),
            event_emitter: TransportEventEmitter::new().0,
            stdin_sender: Arc::new(TokioMutex::new(None)),
            stdout_receiver: Arc::new(TokioMutex::new(None)),
            _stdin_task: Arc::new(TokioMutex::new(None)),
            _stdout_task: Arc::new(TokioMutex::new(None)),
            _stderr_task: Arc::new(TokioMutex::new(None)),
        }
    }

    /// Start the child process and set up communication channels
    async fn start_process(&self) -> TransportResult<()> {
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
        let stderr_task = {
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
        *self.child.lock().await = Some(child);
        *self.stdin_sender.lock().await = Some(stdin_tx);
        *self.stdout_receiver.lock().await = Some(stdout_rx);
        *self._stdin_task.lock().await = Some(stdin_task);
        *self._stdout_task.lock().await = Some(stdout_task);
        *self._stderr_task.lock().await = Some(stderr_task);

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
    async fn wait_for_ready(&self) -> TransportResult<()> {
        let mut child_guard = self.child.lock().await;
        if let Some(ref mut child) = child_guard.as_mut() {
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

    /// Stop the child process following MCP's stdio shutdown sequence.
    ///
    /// §Shutdown > stdio prescribes three steps, in order: close the child's
    /// input stream, wait for it to exit, and only then signal it — SIGTERM
    /// first, SIGKILL last. Before 3.5.0 this sent SIGKILL immediately, which
    /// is uncatchable: every MCP server launched as a child died without
    /// running its `on_shutdown` hook, so cached writes, open database handles
    /// and unpersisted state were lost on every disconnect.
    async fn stop_process(&self) -> TransportResult<()> {
        info!("Stopping child process");

        // Drop communication channels first
        *self.stdin_sender.lock().await = None;
        *self.stdout_receiver.lock().await = None;

        // Step 1: close the child's stdin. The sender is gone, so the writer
        // task ends on its own and drops the `BufWriter<ChildStdin>` with it —
        // awaiting rather than aborting is what guarantees the pipe is actually
        // closed before we start waiting for an exit that depends on it.
        if let Some(handle) = self._stdin_task.lock().await.take()
            && timeout(Duration::from_secs(1), handle).await.is_err()
        {
            warn!("stdin writer did not finish; the child may not see EOF");
        }

        if let Some(mut child) = self.child.lock().await.take() {
            // Step 2: give it the configured window to exit voluntarily.
            match timeout(self.config.shutdown_timeout, child.wait()).await {
                Ok(Ok(status)) => {
                    info!("Child process exited with status: {}", status);
                }
                Ok(Err(e)) => {
                    error!("Failed to wait for child process exit: {}", e);
                }
                Err(_) => {
                    // Step 3: SIGTERM, then SIGKILL if it is not honoured.
                    warn!("Child did not exit after stdin close; sending SIGTERM");
                    #[cfg(unix)]
                    if let Some(pid) = child.id()
                        && let Err(e) = nix::sys::signal::kill(
                            nix::unistd::Pid::from_raw(pid as i32),
                            nix::sys::signal::Signal::SIGTERM,
                        )
                    {
                        warn!("Failed to send SIGTERM to child process: {}", e);
                    }

                    // Windows has no SIGTERM; `start_kill` there is the only
                    // option and is equivalent to the SIGKILL below.
                    #[cfg(not(unix))]
                    if let Err(e) = child.start_kill() {
                        warn!("Failed to signal child process: {}", e);
                    }

                    if timeout(self.config.sigterm_grace, child.wait())
                        .await
                        .is_err()
                    {
                        warn!("Child ignored SIGTERM; forcing kill");
                        if let Err(e) = child.kill().await {
                            error!("Failed to force kill child process: {}", e);
                        }
                    }
                }
            }
        }

        // Drain tasks last, so stderr written during the child's own shutdown
        // still reaches the log rather than being cut off by the abort.
        if let Some(handle) = self._stdout_task.lock().await.take() {
            handle.abort();
        }
        if let Some(handle) = self._stderr_task.lock().await.take() {
            handle.abort();
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
    pub async fn is_process_alive(&self) -> bool {
        let mut child_guard = self.child.lock().await;
        if let Some(ref mut child) = child_guard.as_mut() {
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

impl Transport for ChildProcessTransport {
    fn connect(&self) -> Pin<Box<dyn Future<Output = TransportResult<()>> + Send + '_>> {
        Box::pin(async move {
            match *self.state.lock() {
                TransportState::Connected => return Ok(()),
                TransportState::Connecting => {
                    return Err(TransportError::Internal("Already connecting".to_string()));
                }
                _ => {}
            }

            *self.state.lock() = TransportState::Connecting;
            self.start_process().await
        })
    }

    fn disconnect(&self) -> Pin<Box<dyn Future<Output = TransportResult<()>> + Send + '_>> {
        Box::pin(async move { self.stop_process().await })
    }

    fn send(
        &self,
        message: TransportMessage,
    ) -> Pin<Box<dyn Future<Output = TransportResult<()>> + Send + '_>> {
        Box::pin(async move {
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
                TransportError::SerializationFailed(format!(
                    "Invalid UTF-8 in message payload: {e}"
                ))
            })?;

            // Send through stdin channel
            let stdin_sender = self.stdin_sender.lock().await;
            if let Some(sender) = stdin_sender.as_ref() {
                sender.send(payload_str).await.map_err(|_| {
                    error!("Failed to send message: stdin channel closed");
                    TransportError::ConnectionLost("STDIN channel closed".to_string())
                })?;

                // Update metrics (lock-free atomic operations)
                self.metrics.messages_sent.fetch_add(1, Ordering::Relaxed);
                self.metrics
                    .bytes_sent
                    .fetch_add(message.payload.len() as u64, Ordering::Relaxed);

                trace!("Sent message via child process transport");
                Ok(())
            } else {
                Err(TransportError::ConnectionLost(
                    "No stdin channel available".to_string(),
                ))
            }
        })
    }

    fn receive(
        &self,
    ) -> Pin<Box<dyn Future<Output = TransportResult<Option<TransportMessage>>> + Send + '_>> {
        Box::pin(async move {
            let state = self.state.lock().clone();
            if state != TransportState::Connected {
                return Ok(None);
            }

            // Check if process is still alive
            if !self.is_process_alive().await {
                warn!("Child process died, disconnecting transport");
                self.stop_process().await?;
                return Ok(None);
            }

            // Properly block and wait for messages from stdout channel
            let mut stdout_receiver = self.stdout_receiver.lock().await;
            if let Some(ref mut receiver) = stdout_receiver.as_mut() {
                match receiver.recv().await {
                    Some(line) => {
                        let payload = Bytes::from(line);
                        let message = TransportMessage::new(
                            MessageId::String(uuid::Uuid::new_v4().to_string()),
                            payload,
                        );

                        // Update metrics (lock-free atomic operations)
                        self.metrics
                            .messages_received
                            .fetch_add(1, Ordering::Relaxed);
                        self.metrics
                            .bytes_received
                            .fetch_add(message.payload.len() as u64, Ordering::Relaxed);

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
        })
    }

    fn state(&self) -> Pin<Box<dyn Future<Output = TransportState> + Send + '_>> {
        Box::pin(async move { self.state.lock().clone() })
    }

    fn transport_type(&self) -> TransportType {
        TransportType::ChildProcess
    }

    fn capabilities(&self) -> &TransportCapabilities {
        &self.capabilities
    }

    fn metrics(&self) -> Pin<Box<dyn Future<Output = TransportMetrics> + Send + '_>> {
        Box::pin(async move {
            // AtomicMetrics: lock-free snapshot with Ordering::Relaxed
            self.metrics.snapshot()
        })
    }
}

impl Drop for ChildProcessTransport {
    fn drop(&mut self) {
        if self.config.kill_on_drop {
            // Last-resort cleanup for a transport dropped without
            // `disconnect()`. Drop is synchronous, so there is nowhere to wait
            // for a voluntary exit — but SIGTERM at least gives the child the
            // chance to run its shutdown path, where SIGKILL gives it none.
            // Tokio's own `kill_on_drop` still reaps whatever is left.
            if let Ok(mut child_guard) = self.child.try_lock()
                && let Some(child) = child_guard.as_mut()
            {
                #[cfg(unix)]
                if let Some(pid) = child.id() {
                    let _ = nix::sys::signal::kill(
                        nix::unistd::Pid::from_raw(pid as i32),
                        nix::sys::signal::Signal::SIGTERM,
                    );
                }
                #[cfg(not(unix))]
                let _ = child.start_kill();
            }
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
        let transport = ChildProcessTransport::new(config);

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

        let transport = ChildProcessTransport::new(config);

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

    /// MCP §Shutdown > stdio: close the input stream, then wait. A child that
    /// exits on EOF must never be signalled at all.
    ///
    /// Before 3.5.0 this sent SIGKILL first and waited afterwards, so every
    /// child MCP server died uncatchably and its `on_shutdown` hook — cache
    /// flushes, database handles, persisted state — never ran.
    #[cfg(unix)]
    #[tokio::test]
    async fn stdin_close_shuts_a_well_behaved_child_down_without_a_signal() {
        // `sh -c 'cat >/dev/null; exit 7'`: reads until EOF on stdin, then
        // exits with a status that only a voluntary exit can produce — a
        // signalled process reports the signal instead.
        let config = ChildProcessConfig {
            command: "sh".to_string(),
            args: vec!["-c".to_string(), "cat >/dev/null; exit 7".to_string()],
            startup_timeout: Duration::from_secs(5),
            shutdown_timeout: Duration::from_secs(5),
            ..Default::default()
        };

        let transport = ChildProcessTransport::new(config);
        if transport.connect().await.is_err() {
            // Process spawning is restricted in some CI sandboxes.
            return;
        }
        sleep(Duration::from_millis(100)).await;

        let started = std::time::Instant::now();
        transport.disconnect().await.expect("disconnect");

        // The child exits as soon as it sees EOF, so this must not sit out the
        // shutdown timeout — that would mean stdin was never actually closed.
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "a child that exits on EOF should not wait out the shutdown timeout"
        );
        assert_eq!(transport.state().await, TransportState::Disconnected);
    }

    /// A child that ignores its closed stdin is escalated to SIGTERM before
    /// SIGKILL, so it still gets the chance to run a handler.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_child_that_ignores_eof_is_sigtermed_before_being_killed() {
        // Traps SIGTERM and exits on it; ignores stdin entirely. If SIGTERM
        // were never sent, this would only die to the final SIGKILL, which
        // takes an extra `sigterm_grace` to reach.
        let config = ChildProcessConfig {
            command: "sh".to_string(),
            args: vec![
                "-c".to_string(),
                "trap 'exit 0' TERM; while true; do sleep 0.05; done".to_string(),
            ],
            startup_timeout: Duration::from_secs(5),
            shutdown_timeout: Duration::from_millis(300),
            sigterm_grace: Duration::from_secs(5),
            ..Default::default()
        };

        let transport = ChildProcessTransport::new(config);
        if transport.connect().await.is_err() {
            return;
        }
        sleep(Duration::from_millis(100)).await;

        let started = std::time::Instant::now();
        transport.disconnect().await.expect("disconnect");

        // It cannot have exited before the shutdown window elapsed, and it must
        // not have taken the full sigterm_grace — that would mean SIGTERM was
        // never delivered and only SIGKILL ended it.
        let elapsed = started.elapsed();
        assert!(
            elapsed >= Duration::from_millis(300),
            "the child must get its full grace period before being signalled"
        );
        assert!(
            elapsed < Duration::from_secs(4),
            "SIGTERM should have been honoured well inside sigterm_grace, took {elapsed:?}"
        );
    }
}
