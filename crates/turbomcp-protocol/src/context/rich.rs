//! Rich context extension traits for enhanced tool capabilities.
//!
//! This module provides extension traits that add advanced capabilities to
//! `RequestContext`, including:
//!
//! - Session state management (`get_state`, `set_state`)
//! - Client logging (`info`, `debug`, `warning`, `error`)
//! - Progress reporting (`report_progress`)
//!
//! # Memory Management
//!
//! Session state is stored in a process-level map keyed by session ID.
//! **IMPORTANT**: You must ensure cleanup happens when sessions end to prevent
//! memory leaks. Use one of these approaches:
//!
//! 1. **Recommended**: Use [`SessionStateGuard`] which automatically cleans up on drop
//! 2. **Manual**: Call [`cleanup_session_state`] when a session disconnects
//!
//! # Client Logging and Progress
//!
//! The logging and progress methods require bidirectional transport support.
//! They will silently succeed (no-op) if the transport doesn't support
//! server-to-client notifications.
//!
//! # Example
//!
//! ```rust,ignore
//! use turbomcp_protocol::context::{RichContextExt, SessionStateGuard};
//!
//! async fn handle_session(session_id: String) {
//!     // Guard ensures cleanup when it goes out of scope
//!     let _guard = SessionStateGuard::new(&session_id);
//!
//!     let ctx = RequestContext::new().with_session_id(&session_id);
//!     ctx.set_state("counter", &0i32);
//!
//!     // Client logging
//!     ctx.info("Starting processing...").await;
//!
//!     // Progress reporting
//!     for i in 0..100 {
//!         ctx.report_progress(i, 100, Some(&format!("Step {}", i))).await;
//!     }
//!
//!     ctx.info("Processing complete!").await;
//!
//! } // Guard dropped here, session state automatically cleaned up
//! ```

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

use crate::McpError;
use crate::types::{LogLevel, LoggingNotification, ProgressNotification, ServerNotification};

use super::request::RequestContext;

/// Type alias for session state storage to reduce complexity.
type SessionStateMap = dashmap::DashMap<String, Arc<RwLock<HashMap<String, Value>>>>;

/// Session state storage (keyed by session_id).
///
/// **Warning**: This is a process-level singleton. Entries must be manually
/// cleaned up via [`cleanup_session_state`] or [`SessionStateGuard`] to
/// prevent unbounded memory growth.
///
/// # Multi-Server Considerations
///
/// If you run multiple MCP servers in the same process (e.g., in tests or
/// composite server scenarios), be aware that session IDs may collide.
/// To avoid this:
///
/// 1. Use unique session ID prefixes per server: `"{server_name}:{session_id}"`
/// 2. Or ensure each server uses globally unique session IDs (e.g., UUIDs)
///
/// This global singleton design enables session state to be shared across
/// handler invocations without threading server references through the
/// entire call chain.
static SESSION_STATE: once_cell::sync::Lazy<SessionStateMap> =
    once_cell::sync::Lazy::new(SessionStateMap::new);

/// RAII guard that automatically cleans up session state when dropped.
///
/// This is the recommended way to manage session state lifetime. Create a guard
/// at the start of a session and let it clean up automatically when the session
/// ends.
///
/// # Example
///
/// ```rust,ignore
/// use turbomcp_protocol::context::SessionStateGuard;
///
/// async fn handle_connection(session_id: String) {
///     let _guard = SessionStateGuard::new(&session_id);
///
///     // Session state is available for this session_id
///     // ...
///
/// } // State automatically cleaned up here
/// ```
#[derive(Debug)]
pub struct SessionStateGuard {
    session_id: String,
}

impl SessionStateGuard {
    /// Create a new session state guard.
    ///
    /// The session's state will be automatically cleaned up when this guard
    /// is dropped.
    pub fn new(session_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
        }
    }

    /// Get the session ID this guard is managing.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }
}

impl Drop for SessionStateGuard {
    fn drop(&mut self) {
        cleanup_session_state(&self.session_id);
    }
}

/// Error type for state operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateError {
    /// No session ID is set on the context.
    NoSessionId,
    /// Failed to serialize the value.
    SerializationFailed(String),
    /// Failed to deserialize the value.
    DeserializationFailed(String),
}

impl std::fmt::Display for StateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoSessionId => write!(f, "no session ID set on context"),
            Self::SerializationFailed(e) => write!(f, "serialization failed: {}", e),
            Self::DeserializationFailed(e) => write!(f, "deserialization failed: {}", e),
        }
    }
}

impl std::error::Error for StateError {}

/// Extension trait providing rich context capabilities.
///
/// This trait extends `RequestContext` with session state management,
/// client logging, and progress reporting.
pub trait RichContextExt {
    // ===== State Management =====

    /// Get a value from session state.
    ///
    /// Returns `None` if the key doesn't exist or if there's no session.
    fn get_state<T: DeserializeOwned>(&self, key: &str) -> Option<T>;

    /// Try to get a value from session state with detailed error information.
    ///
    /// Returns `Err` if there's no session ID or deserialization fails.
    /// Returns `Ok(None)` if the key doesn't exist.
    fn try_get_state<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>, StateError>;

    /// Set a value in session state.
    ///
    /// Returns `false` if there's no session ID to store state against.
    fn set_state<T: Serialize>(&self, key: &str, value: &T) -> bool;

    /// Try to set a value in session state with detailed error information.
    fn try_set_state<T: Serialize>(&self, key: &str, value: &T) -> Result<(), StateError>;

    /// Remove a value from session state.
    fn remove_state(&self, key: &str) -> bool;

    /// Clear all session state.
    fn clear_state(&self);

    /// Check if a state key exists.
    fn has_state(&self, key: &str) -> bool;

    // ===== Client Logging =====

    /// Send a debug-level log message to the client.
    ///
    /// Returns `Ok(())` if the notification was sent or if bidirectional
    /// transport is not available (no-op in that case).
    fn debug(
        &self,
        message: impl Into<String> + Send,
    ) -> impl std::future::Future<Output = Result<(), McpError>> + Send;

    /// Send an info-level log message to the client.
    ///
    /// Returns `Ok(())` if the notification was sent or if bidirectional
    /// transport is not available (no-op in that case).
    fn info(
        &self,
        message: impl Into<String> + Send,
    ) -> impl std::future::Future<Output = Result<(), McpError>> + Send;

    /// Send a warning-level log message to the client.
    ///
    /// Returns `Ok(())` if the notification was sent or if bidirectional
    /// transport is not available (no-op in that case).
    fn warning(
        &self,
        message: impl Into<String> + Send,
    ) -> impl std::future::Future<Output = Result<(), McpError>> + Send;

    /// Send an error-level log message to the client.
    ///
    /// Returns `Ok(())` if the notification was sent or if bidirectional
    /// transport is not available (no-op in that case).
    fn error(
        &self,
        message: impl Into<String> + Send,
    ) -> impl std::future::Future<Output = Result<(), McpError>> + Send;

    /// Send a log message to the client with a specific level.
    ///
    /// This is the low-level method that `debug`, `info`, `warning`, and `error`
    /// delegate to. Use this when you need fine-grained control over the log level.
    ///
    /// Returns `Ok(())` if the notification was sent or if bidirectional
    /// transport is not available (no-op in that case).
    fn log(
        &self,
        level: LogLevel,
        message: impl Into<String> + Send,
        logger: Option<String>,
    ) -> impl std::future::Future<Output = Result<(), McpError>> + Send;

    // ===== Progress Reporting =====

    /// Report progress on a long-running operation.
    ///
    /// # Arguments
    ///
    /// * `current` - Current progress value
    /// * `total` - Total value (for percentage: current/total * 100)
    /// * `message` - Optional status message
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// for i in 0..100 {
    ///     ctx.report_progress(i, 100, Some(&format!("Processing item {}", i))).await?;
    /// }
    /// ```
    fn report_progress(
        &self,
        current: u64,
        total: u64,
        message: Option<&str>,
    ) -> impl std::future::Future<Output = Result<(), McpError>> + Send;

    /// Report progress with custom progress token.
    ///
    /// Use this when you need to track multiple concurrent operations
    /// with different progress tokens.
    fn report_progress_with_token(
        &self,
        token: impl Into<String> + Send,
        current: u64,
        total: Option<u64>,
        message: Option<&str>,
    ) -> impl std::future::Future<Output = Result<(), McpError>> + Send;
}

impl RichContextExt for RequestContext {
    fn get_state<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        self.try_get_state(key).ok().flatten()
    }

    fn try_get_state<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>, StateError> {
        let session_id = self.session_id.as_ref().ok_or(StateError::NoSessionId)?;

        let Some(state) = SESSION_STATE.get(session_id) else {
            return Ok(None);
        };

        let state_read = state.read();
        let Some(value) = state_read.get(key) else {
            return Ok(None);
        };

        serde_json::from_value(value.clone())
            .map(Some)
            .map_err(|e| StateError::DeserializationFailed(e.to_string()))
    }

    fn set_state<T: Serialize>(&self, key: &str, value: &T) -> bool {
        self.try_set_state(key, value).is_ok()
    }

    fn try_set_state<T: Serialize>(&self, key: &str, value: &T) -> Result<(), StateError> {
        let session_id = self.session_id.as_ref().ok_or(StateError::NoSessionId)?;

        let json_value = serde_json::to_value(value)
            .map_err(|e| StateError::SerializationFailed(e.to_string()))?;

        let state = SESSION_STATE
            .entry(session_id.clone())
            .or_insert_with(|| Arc::new(RwLock::new(HashMap::new())));

        state.write().insert(key.to_string(), json_value);
        Ok(())
    }

    fn remove_state(&self, key: &str) -> bool {
        let Some(ref session_id) = self.session_id else {
            return false;
        };

        if let Some(state) = SESSION_STATE.get(session_id) {
            state.write().remove(key);
            return true;
        }
        false
    }

    fn clear_state(&self) {
        if let Some(ref session_id) = self.session_id
            && let Some(state) = SESSION_STATE.get(session_id)
        {
            state.write().clear();
        }
    }

    fn has_state(&self, key: &str) -> bool {
        if let Some(ref session_id) = self.session_id
            && let Some(state) = SESSION_STATE.get(session_id)
        {
            return state.read().contains_key(key);
        }
        false
    }

    // ===== Client Logging =====

    async fn debug(&self, message: impl Into<String> + Send) -> Result<(), McpError> {
        self.log(LogLevel::Debug, message, None).await
    }

    async fn info(&self, message: impl Into<String> + Send) -> Result<(), McpError> {
        self.log(LogLevel::Info, message, None).await
    }

    async fn warning(&self, message: impl Into<String> + Send) -> Result<(), McpError> {
        self.log(LogLevel::Warning, message, None).await
    }

    async fn error(&self, message: impl Into<String> + Send) -> Result<(), McpError> {
        self.log(LogLevel::Error, message, None).await
    }

    async fn log(
        &self,
        level: LogLevel,
        message: impl Into<String> + Send,
        logger: Option<String>,
    ) -> Result<(), McpError> {
        // If no server_to_client available, this is a no-op (not an error)
        let Some(s2c) = self.server_to_client() else {
            return Ok(());
        };

        let notification = ServerNotification::Message(LoggingNotification {
            level,
            data: serde_json::Value::String(message.into()),
            logger,
        });

        s2c.send_notification(notification).await
    }

    // ===== Progress Reporting =====

    async fn report_progress(
        &self,
        current: u64,
        total: u64,
        message: Option<&str>,
    ) -> Result<(), McpError> {
        // Use request_id as the progress token by default
        self.report_progress_with_token(&self.request_id, current, Some(total), message)
            .await
    }

    async fn report_progress_with_token(
        &self,
        token: impl Into<String> + Send,
        current: u64,
        total: Option<u64>,
        message: Option<&str>,
    ) -> Result<(), McpError> {
        // If no server_to_client available, this is a no-op (not an error)
        let Some(s2c) = self.server_to_client() else {
            return Ok(());
        };

        let notification = ServerNotification::Progress(ProgressNotification {
            progress_token: token.into(),
            progress: current,
            total,
            message: message.map(ToString::to_string),
        });

        s2c.send_notification(notification).await
    }
}

/// Clean up session state when a session ends.
///
/// **Important**: Call this when a session disconnects to free memory.
/// Alternatively, use [`SessionStateGuard`] for automatic cleanup.
///
/// # Example
///
/// ```rust,ignore
/// use turbomcp_protocol::context::cleanup_session_state;
///
/// fn on_session_disconnect(session_id: &str) {
///     cleanup_session_state(session_id);
/// }
/// ```
pub fn cleanup_session_state(session_id: &str) {
    SESSION_STATE.remove(session_id);
}

/// Get the number of active sessions with state.
///
/// This is useful for monitoring memory usage.
pub fn active_sessions_count() -> usize {
    SESSION_STATE.len()
}

/// Clear all session state.
///
/// **Warning**: This removes state for ALL sessions. Use with caution.
/// Primarily intended for testing.
#[cfg(test)]
pub fn clear_all_session_state() {
    SESSION_STATE.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_set_state() {
        let ctx = RequestContext::new().with_session_id("test-session-1");

        // Set state
        assert!(ctx.set_state("counter", &42i32));
        assert!(ctx.set_state("name", &"Alice".to_string()));

        // Get state
        assert_eq!(ctx.get_state::<i32>("counter"), Some(42));
        assert_eq!(ctx.get_state::<String>("name"), Some("Alice".to_string()));
        assert_eq!(ctx.get_state::<i32>("missing"), None);

        // Has state
        assert!(ctx.has_state("counter"));
        assert!(!ctx.has_state("missing"));

        // Remove state
        assert!(ctx.remove_state("counter"));
        assert_eq!(ctx.get_state::<i32>("counter"), None);
        assert!(!ctx.has_state("counter"));

        // Clear state
        ctx.clear_state();
        assert_eq!(ctx.get_state::<String>("name"), None);

        // Cleanup
        cleanup_session_state("test-session-1");
    }

    #[test]
    fn test_state_without_session() {
        let ctx = RequestContext::new();

        // Without session_id, state operations fail
        assert!(!ctx.set_state("key", &"value"));
        assert_eq!(ctx.get_state::<String>("key"), None);
        assert!(!ctx.has_state("key"));

        // try_* methods return proper errors
        assert_eq!(
            ctx.try_set_state("key", &"value"),
            Err(StateError::NoSessionId)
        );
        assert_eq!(
            ctx.try_get_state::<String>("key"),
            Err(StateError::NoSessionId)
        );
    }

    #[test]
    fn test_state_isolation() {
        let ctx1 = RequestContext::new().with_session_id("session-iso-1");
        let ctx2 = RequestContext::new().with_session_id("session-iso-2");

        // Set different values in different sessions
        ctx1.set_state("value", &1i32);
        ctx2.set_state("value", &2i32);

        // Each session sees its own value
        assert_eq!(ctx1.get_state::<i32>("value"), Some(1));
        assert_eq!(ctx2.get_state::<i32>("value"), Some(2));

        // Cleanup
        cleanup_session_state("session-iso-1");
        cleanup_session_state("session-iso-2");
    }

    #[test]
    fn test_complex_types() {
        let ctx = RequestContext::new().with_session_id("complex-session-1");

        #[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug)]
        struct MyData {
            count: i32,
            items: Vec<String>,
        }

        let data = MyData {
            count: 3,
            items: vec!["a".to_string(), "b".to_string(), "c".to_string()],
        };

        ctx.set_state("data", &data);
        let retrieved: Option<MyData> = ctx.get_state("data");
        assert_eq!(retrieved, Some(data));

        cleanup_session_state("complex-session-1");
    }

    #[test]
    fn test_session_state_guard() {
        let session_id = "guard-test-session";

        {
            let _guard = SessionStateGuard::new(session_id);
            let ctx = RequestContext::new().with_session_id(session_id);

            ctx.set_state("key", &"value");
            assert_eq!(ctx.get_state::<String>("key"), Some("value".to_string()));

            // State exists while guard is alive
            assert!(active_sessions_count() > 0);
        }

        // After guard drops, state should be cleaned up
        let ctx = RequestContext::new().with_session_id(session_id);
        assert_eq!(ctx.get_state::<String>("key"), None);
    }

    #[test]
    fn test_try_get_state_errors() {
        let ctx = RequestContext::new().with_session_id("error-test-session");
        ctx.set_state("number", &42i32);

        // Type mismatch returns deserialization error
        let result: Result<Option<String>, StateError> = ctx.try_get_state("number");
        assert!(matches!(result, Err(StateError::DeserializationFailed(_))));

        cleanup_session_state("error-test-session");
    }

    #[test]
    fn test_state_error_display() {
        assert_eq!(
            StateError::NoSessionId.to_string(),
            "no session ID set on context"
        );
        assert!(
            StateError::SerializationFailed("test".into())
                .to_string()
                .contains("serialization failed")
        );
        assert!(
            StateError::DeserializationFailed("test".into())
                .to_string()
                .contains("deserialization failed")
        );
    }

    #[tokio::test]
    async fn test_logging_without_server_to_client() {
        // Without server_to_client configured, logging methods should be no-ops
        let ctx = RequestContext::new().with_session_id("logging-test");

        // These should all succeed (no-op) without server_to_client
        assert!(ctx.debug("debug message").await.is_ok());
        assert!(ctx.info("info message").await.is_ok());
        assert!(ctx.warning("warning message").await.is_ok());
        assert!(ctx.error("error message").await.is_ok());
        assert!(ctx.log(LogLevel::Notice, "notice", None).await.is_ok());
    }

    #[tokio::test]
    async fn test_progress_without_server_to_client() {
        // Without server_to_client configured, progress methods should be no-ops
        let ctx = RequestContext::new().with_session_id("progress-test");

        // These should all succeed (no-op) without server_to_client
        assert!(ctx.report_progress(50, 100, Some("halfway")).await.is_ok());
        assert!(ctx.report_progress(100, 100, None).await.is_ok());
        assert!(
            ctx.report_progress_with_token("custom-token", 25, Some(100), Some("processing"))
                .await
                .is_ok()
        );
    }
}
