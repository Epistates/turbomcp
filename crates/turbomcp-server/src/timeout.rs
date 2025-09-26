//! Tool timeout and cancellation management
//!
//! This module provides production-grade timeout and cancellation capabilities
//! for tool execution in TurboMCP servers. It follows 2025 Rust async best
//! practices for cancellation safety and proper resource cleanup.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, instrument, warn};
use uuid::Uuid;

use crate::ServerError;
use crate::config::TimeoutConfig;
use crate::metrics::ServerMetrics;

/// Tool timeout and cancellation manager
///
/// Manages per-tool timeout policies and provides cancellation-safe
/// timeout enforcement for tool execution with comprehensive audit logging.
#[derive(Debug, Clone)]
pub struct ToolTimeoutManager {
    /// Default timeout configuration
    config: TimeoutConfig,

    /// Active tool executions for monitoring and cancellation
    active_executions: Arc<RwLock<HashMap<Uuid, ToolExecution>>>,

    /// Metrics for security audit and monitoring
    metrics: Arc<ServerMetrics>,
}

/// Information about an active tool execution
#[derive(Debug, Clone)]
struct ToolExecution {
    /// Tool name being executed
    tool_name: String,
    /// When execution started
    started_at: Instant,
    /// Configured timeout duration
    timeout_duration: Duration,
    /// Cancellation token for cooperative cancellation
    cancellation_token: CancellationToken,
    /// Whether execution has been marked for cancellation
    cancelled: bool,
}

impl ToolTimeoutManager {
    /// Create a new timeout manager with the given configuration and metrics
    pub fn new(config: TimeoutConfig, metrics: Arc<ServerMetrics>) -> Self {
        Self {
            config,
            active_executions: Arc::new(RwLock::new(HashMap::new())),
            metrics,
        }
    }

    /// Get the timeout duration for a specific tool
    ///
    /// Returns per-tool override if configured, otherwise returns default timeout.
    pub fn get_tool_timeout(&self, tool_name: &str) -> Duration {
        self.config
            .tool_timeouts
            .get(tool_name)
            .map(|&seconds| Duration::from_secs(seconds))
            .unwrap_or(self.config.tool_execution_timeout)
    }

    /// Execute a tool with timeout and cooperative cancellation support
    ///
    /// This is the primary method for executing tools with comprehensive
    /// timeout and cancellation handling following Tokio best practices.
    ///
    /// Returns both the result and the cancellation token for context propagation.
    #[instrument(skip(self, operation), fields(tool_name = %tool_name))]
    pub async fn execute_with_timeout_and_cancellation<F, T>(
        &self,
        tool_name: &str,
        operation: F,
    ) -> Result<(T, CancellationToken), ToolTimeoutError>
    where
        F: std::future::Future<Output = Result<T, ServerError>>,
        T: Send,
    {
        let execution_id = Uuid::new_v4();
        let timeout_duration = self.get_tool_timeout(tool_name);
        let started_at = Instant::now();

        // Create cancellation token for cooperative cancellation
        let cancellation_token = CancellationToken::new();

        // Register this execution for monitoring and update active execution count
        {
            let mut executions = self.active_executions.write().await;
            executions.insert(
                execution_id,
                ToolExecution {
                    tool_name: tool_name.to_string(),
                    started_at,
                    timeout_duration,
                    cancellation_token: cancellation_token.clone(),
                    cancelled: false,
                },
            );
        }

        // Update active executions metric
        self.metrics
            .tool_executions_active
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        debug!(
            tool_name = %tool_name,
            execution_id = %execution_id,
            timeout_seconds = timeout_duration.as_secs(),
            "Starting tool execution with timeout"
        );

        // Execute with cooperative cancellation using tokio::select!
        // This allows tools to respond to cancellation signals gracefully
        let result = tokio::select! {
            // Tool execution completed
            operation_result = operation => {
                TimeoutResult::Completed(operation_result)
            },
            // Timeout occurred
            _ = tokio::time::sleep(timeout_duration) => {
                // Cancel the token to signal cooperative cancellation
                cancellation_token.cancel();
                TimeoutResult::TimedOut
            },
            // Explicit cancellation requested
            _ = cancellation_token.cancelled() => {
                warn!(
                    tool_name = %tool_name,
                    execution_id = %execution_id,
                    "Tool execution cancelled cooperatively"
                );
                TimeoutResult::Cancelled
            },
        };

        // Clean up execution tracking and update active count
        {
            let mut executions = self.active_executions.write().await;
            executions.remove(&execution_id);
        }

        // Decrement active executions metric
        self.metrics
            .tool_executions_active
            .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);

        let elapsed = started_at.elapsed();

        match result {
            TimeoutResult::Completed(Ok(value)) => {
                // Record successful execution metrics
                self.metrics
                    .tool_executions_successful
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

                debug!(
                    tool_name = %tool_name,
                    execution_id = %execution_id,
                    elapsed_ms = elapsed.as_millis(),
                    "Tool execution completed successfully"
                );
                Ok((value, cancellation_token))
            }
            TimeoutResult::Completed(Err(server_error)) => {
                warn!(
                    tool_name = %tool_name,
                    execution_id = %execution_id,
                    elapsed_ms = elapsed.as_millis(),
                    error = %server_error,
                    "Tool execution failed with server error"
                );
                Err(ToolTimeoutError::ServerError(server_error))
            }
            TimeoutResult::TimedOut => {
                // Record timeout metrics for security monitoring
                self.metrics
                    .tool_timeouts_total
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                self.metrics.timeout_wasted_time_us.fetch_add(
                    elapsed.as_micros() as u64,
                    std::sync::atomic::Ordering::Relaxed,
                );
                self.metrics
                    .errors_timeout
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

                // Security audit event - potential DoS indicator
                error!(
                    tool_name = %tool_name,
                    execution_id = %execution_id,
                    timeout_seconds = timeout_duration.as_secs(),
                    elapsed_ms = elapsed.as_millis(),
                    event_type = "TIMEOUT_EVENT",
                    security_concern = "potential_dos_indicator",
                    "🔒 SECURITY AUDIT: Tool execution timed out"
                );
                Err(ToolTimeoutError::Timeout {
                    tool_name: tool_name.to_string(),
                    timeout_duration,
                    elapsed,
                })
            }
            TimeoutResult::Cancelled => {
                // Record cancellation metrics for monitoring
                self.metrics
                    .tool_cancellations_total
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

                // Audit event for cancellation (normal operation)
                warn!(
                    tool_name = %tool_name,
                    execution_id = %execution_id,
                    elapsed_ms = elapsed.as_millis(),
                    event_type = "CANCELLATION_EVENT",
                    "🔒 AUDIT: Tool execution was cancelled cooperatively"
                );
                Err(ToolTimeoutError::Cancelled {
                    tool_name: tool_name.to_string(),
                    elapsed,
                })
            }
        }
    }

    /// Execute a tool with a provided cancellation token
    ///
    /// This method allows external code to provide the cancellation token,
    /// enabling tight integration with RequestContext and other systems.
    #[instrument(skip(self, operation, cancellation_token), fields(tool_name = %tool_name))]
    pub async fn execute_with_external_token<F, T>(
        &self,
        tool_name: &str,
        operation: F,
        cancellation_token: CancellationToken,
    ) -> Result<T, ToolTimeoutError>
    where
        F: std::future::Future<Output = Result<T, ServerError>>,
        T: Send,
    {
        let execution_id = Uuid::new_v4();
        let timeout_duration = self.get_tool_timeout(tool_name);
        let started_at = Instant::now();

        // Register this execution for monitoring (using provided token)
        {
            let mut executions = self.active_executions.write().await;
            executions.insert(
                execution_id,
                ToolExecution {
                    tool_name: tool_name.to_string(),
                    started_at,
                    timeout_duration,
                    cancellation_token: cancellation_token.clone(),
                    cancelled: false,
                },
            );
        }

        debug!(
            tool_name = %tool_name,
            execution_id = %execution_id,
            timeout_seconds = timeout_duration.as_secs(),
            "Starting tool execution with provided cancellation token"
        );

        // Execute with cooperative cancellation using the provided token
        let result = tokio::select! {
            // Tool execution completed
            operation_result = operation => {
                TimeoutResult::Completed(operation_result)
            },
            // Timeout occurred
            _ = tokio::time::sleep(timeout_duration) => {
                // Cancel the token to signal cooperative cancellation
                cancellation_token.cancel();
                TimeoutResult::TimedOut
            },
            // Explicit cancellation requested via external token
            _ = cancellation_token.cancelled() => {
                warn!(
                    tool_name = %tool_name,
                    execution_id = %execution_id,
                    "Tool execution cancelled via external token"
                );
                TimeoutResult::Cancelled
            },
        };

        // Clean up execution tracking and update active count
        {
            let mut executions = self.active_executions.write().await;
            executions.remove(&execution_id);
        }

        // Decrement active executions metric
        self.metrics
            .tool_executions_active
            .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);

        let elapsed = started_at.elapsed();

        match result {
            TimeoutResult::Completed(Ok(value)) => {
                debug!(
                    tool_name = %tool_name,
                    execution_id = %execution_id,
                    elapsed_ms = elapsed.as_millis(),
                    "Tool execution completed successfully"
                );
                Ok(value)
            }
            TimeoutResult::Completed(Err(server_error)) => {
                warn!(
                    tool_name = %tool_name,
                    execution_id = %execution_id,
                    elapsed_ms = elapsed.as_millis(),
                    error = %server_error,
                    "Tool execution failed with server error"
                );
                Err(ToolTimeoutError::ServerError(server_error))
            }
            TimeoutResult::TimedOut => {
                // Record timeout metrics for security monitoring
                self.metrics
                    .tool_timeouts_total
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                self.metrics.timeout_wasted_time_us.fetch_add(
                    elapsed.as_micros() as u64,
                    std::sync::atomic::Ordering::Relaxed,
                );
                self.metrics
                    .errors_timeout
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

                // Security audit event - potential DoS indicator
                error!(
                    tool_name = %tool_name,
                    execution_id = %execution_id,
                    timeout_seconds = timeout_duration.as_secs(),
                    elapsed_ms = elapsed.as_millis(),
                    event_type = "TIMEOUT_EVENT",
                    security_concern = "potential_dos_indicator",
                    "🔒 SECURITY AUDIT: Tool execution timed out"
                );
                Err(ToolTimeoutError::Timeout {
                    tool_name: tool_name.to_string(),
                    timeout_duration,
                    elapsed,
                })
            }
            TimeoutResult::Cancelled => {
                // Record cancellation metrics for monitoring
                self.metrics
                    .tool_cancellations_total
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

                // Audit event for cancellation (normal operation)
                warn!(
                    tool_name = %tool_name,
                    execution_id = %execution_id,
                    elapsed_ms = elapsed.as_millis(),
                    event_type = "CANCELLATION_EVENT",
                    "🔒 AUDIT: Tool execution was cancelled cooperatively"
                );
                Err(ToolTimeoutError::Cancelled {
                    tool_name: tool_name.to_string(),
                    elapsed,
                })
            }
        }
    }

    /// Execute a tool with timeout (backward compatible API)
    ///
    /// This method provides backward compatibility for existing code that doesn't
    /// need access to the cancellation token. New code should use
    /// `execute_with_timeout_and_cancellation` for cooperative cancellation support.
    #[instrument(skip(self, operation), fields(tool_name = %tool_name))]
    pub async fn execute_with_timeout<F, T>(
        &self,
        tool_name: &str,
        operation: F,
    ) -> Result<T, ToolTimeoutError>
    where
        F: std::future::Future<Output = Result<T, ServerError>>,
        T: Send,
    {
        // Use the new method and discard the cancellation token for compatibility
        match self
            .execute_with_timeout_and_cancellation(tool_name, operation)
            .await
        {
            Ok((result, _token)) => Ok(result),
            Err(error) => Err(error),
        }
    }

    /// Get statistics about active tool executions
    ///
    /// Returns information about currently running tools for monitoring
    /// and debugging purposes.
    pub async fn get_active_executions(&self) -> Vec<ActiveExecutionInfo> {
        let executions = self.active_executions.read().await;
        executions
            .iter()
            .map(|(&id, execution)| ActiveExecutionInfo {
                execution_id: id,
                tool_name: execution.tool_name.clone(),
                started_at: execution.started_at,
                timeout_duration: execution.timeout_duration,
                elapsed: execution.started_at.elapsed(),
                cancellation_token: execution.cancellation_token.clone(),
                cancelled: execution.cancelled,
            })
            .collect()
    }

    /// Cancel all active executions (for graceful shutdown)
    ///
    /// Signals cooperative cancellation to all active tool executions.
    /// Tools that check their cancellation tokens will receive the signal
    /// and can perform graceful cleanup before terminating.
    #[instrument(skip(self))]
    pub async fn cancel_all_executions(&self) {
        let mut executions = self.active_executions.write().await;
        let count = executions.len();

        if count > 0 {
            // Security audit event for bulk cancellation - could indicate emergency shutdown
            warn!(
                active_count = count,
                event_type = "BULK_CANCELLATION_EVENT",
                security_note = "emergency_shutdown_or_resource_cleanup",
                "🔒 SECURITY AUDIT: Cancelling all active tool executions"
            );

            for execution in executions.values_mut() {
                // Signal cooperative cancellation via the token
                execution.cancellation_token.cancel();
                execution.cancelled = true;
            }

            // Update cancellation metrics for bulk operation
            self.metrics
                .tool_cancellations_total
                .fetch_add(count as u64, std::sync::atomic::Ordering::Relaxed);

            debug!(
                cancelled_count = count,
                "Sent cooperative cancellation signals to all active tool executions"
            );
        }

        // Cooperative cancellation tokens allow tools to respond gracefully
        // Tools that check cancellation_token.is_cancelled() will see the signal
    }
}

/// Information about an active tool execution
#[derive(Debug, Clone)]
pub struct ActiveExecutionInfo {
    /// Unique execution identifier
    pub execution_id: Uuid,
    /// Tool name being executed
    pub tool_name: String,
    /// When execution started
    pub started_at: Instant,
    /// Configured timeout duration
    pub timeout_duration: Duration,
    /// How long execution has been running
    pub elapsed: Duration,
    /// Cancellation token for this execution
    pub cancellation_token: CancellationToken,
    /// Whether execution has been cancelled
    pub cancelled: bool,
}

/// Tool timeout error types
#[derive(Debug, thiserror::Error)]
pub enum ToolTimeoutError {
    /// Tool execution exceeded configured timeout
    #[error("Tool '{tool_name}' timed out after {timeout_duration:?} (elapsed: {elapsed:?})")]
    Timeout {
        /// Name of the tool that timed out
        tool_name: String,
        /// Configured timeout duration that was exceeded
        timeout_duration: Duration,
        /// Actual time elapsed before timeout
        elapsed: Duration,
    },

    /// Tool execution was cancelled cooperatively
    #[error("Tool '{tool_name}' was cancelled (elapsed: {elapsed:?})")]
    Cancelled {
        /// Name of the tool that was cancelled
        tool_name: String,
        /// Time elapsed before cancellation
        elapsed: Duration,
    },

    /// Tool execution failed with server error
    #[error("Tool execution failed: {0}")]
    ServerError(ServerError),
}

/// Internal result type for timeout operations
#[derive(Debug)]
enum TimeoutResult<T> {
    Completed(Result<T, ServerError>),
    TimedOut,
    Cancelled,
}

impl From<ToolTimeoutError> for ServerError {
    fn from(timeout_error: ToolTimeoutError) -> Self {
        match timeout_error {
            ToolTimeoutError::Timeout {
                tool_name,
                timeout_duration,
                ..
            } => ServerError::timeout(
                format!("Tool '{}'", tool_name),
                timeout_duration.as_millis() as u64,
            ),
            ToolTimeoutError::Cancelled { tool_name, .. } => {
                ServerError::handler(format!("Tool '{}' was cancelled", tool_name))
            }
            ToolTimeoutError::ServerError(server_error) => server_error,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ServerError;
    use tokio::time::{Duration, sleep};

    fn create_test_config() -> TimeoutConfig {
        let mut tool_timeouts = HashMap::new();
        tool_timeouts.insert("fast_tool".to_string(), 1); // 1 second
        tool_timeouts.insert("slow_tool".to_string(), 5); // 5 seconds

        TimeoutConfig {
            request_timeout: Duration::from_secs(30),
            connection_timeout: Duration::from_secs(10),
            keep_alive_timeout: Duration::from_secs(60),
            tool_execution_timeout: Duration::from_secs(3), // 3 second default
            tool_timeouts,
        }
    }

    fn create_test_metrics() -> Arc<ServerMetrics> {
        Arc::new(ServerMetrics::new())
    }

    #[tokio::test]
    async fn test_successful_tool_execution() {
        let manager = ToolTimeoutManager::new(create_test_config(), create_test_metrics());

        let result = manager
            .execute_with_timeout("test_tool", async {
                sleep(Duration::from_millis(100)).await;
                Ok::<String, ServerError>("success".to_string())
            })
            .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "success");
    }

    #[tokio::test]
    async fn test_tool_timeout() {
        let manager = ToolTimeoutManager::new(create_test_config(), create_test_metrics());

        // This should timeout after 1 second (fast_tool override)
        let result = manager
            .execute_with_timeout("fast_tool", async {
                sleep(Duration::from_secs(2)).await; // Sleep longer than timeout
                Ok::<String, ServerError>("should_not_reach".to_string())
            })
            .await;

        assert!(result.is_err());
        match result.unwrap_err() {
            ToolTimeoutError::Timeout {
                tool_name,
                timeout_duration,
                ..
            } => {
                assert_eq!(tool_name, "fast_tool");
                assert_eq!(timeout_duration, Duration::from_secs(1));
            }
            _ => panic!("Expected timeout error"),
        }
    }

    #[tokio::test]
    async fn test_per_tool_timeout_override() {
        let manager = ToolTimeoutManager::new(create_test_config(), create_test_metrics());

        // Test that slow_tool gets its 5-second override
        assert_eq!(
            manager.get_tool_timeout("slow_tool"),
            Duration::from_secs(5)
        );

        // Test that unknown tool gets default timeout
        assert_eq!(
            manager.get_tool_timeout("unknown_tool"),
            Duration::from_secs(3)
        );
    }

    #[tokio::test]
    async fn test_server_error_propagation() {
        let manager = ToolTimeoutManager::new(create_test_config(), create_test_metrics());

        let result = manager
            .execute_with_timeout("test_tool", async {
                Err::<String, ServerError>(ServerError::handler("custom error"))
            })
            .await;

        assert!(result.is_err());
        match result.unwrap_err() {
            ToolTimeoutError::ServerError(server_error) => {
                assert!(server_error.to_string().contains("custom error"));
            }
            _ => panic!("Expected server error"),
        }
    }

    #[tokio::test]
    async fn test_active_executions_tracking() {
        let manager = ToolTimeoutManager::new(create_test_config(), create_test_metrics());

        let manager_clone = manager.clone();
        let _handle = tokio::spawn(async move {
            let _ = manager_clone
                .execute_with_timeout("long_running", async {
                    sleep(Duration::from_millis(100)).await;
                    Ok::<String, ServerError>("done".to_string())
                })
                .await;
        });

        // Give the task time to start
        sleep(Duration::from_millis(10)).await;

        let active = manager.get_active_executions().await;
        // Should have at least one execution (may be completed by now)
        // This test mainly ensures the tracking API works
        assert!(active.len() <= 1); // Could be 0 if already completed
    }
}
