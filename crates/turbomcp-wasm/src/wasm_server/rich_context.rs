//! Rich context extension traits for enhanced WASM handler capabilities.
//!
//! This module provides extension traits that add advanced capabilities to
//! `RequestContext`, including:
//!
//! - Session state management (`get_state`, `set_state`)
//! - Console logging (`info`, `debug`, `warning`, `error`)
//! - Progress reporting (via callback or Streamable HTTP)
//!
//! # Memory Management
//!
//! Session state is stored in a worker-level map keyed by session ID.
//! **IMPORTANT**: You must ensure cleanup happens when sessions end to prevent
//! memory leaks. Use one of these approaches:
//!
//! 1. **Recommended**: Use [`SessionStateGuard`] which automatically cleans up on drop
//! 2. **Manual**: Call [`cleanup_session_state`] when a session disconnects
//!
//! # Example
//!
//! ```rust,ignore
//! use turbomcp_wasm::wasm_server::{RequestContext, RichContextExt, SessionStateGuard};
//!
//! async fn handle_session(session_id: String) {
//!     // Guard ensures cleanup when it goes out of scope
//!     let _guard = SessionStateGuard::new(&session_id);
//!
//!     let ctx = RequestContext::new().with_session_id(&session_id);
//!     ctx.set_state("counter", &0i32);
//!
//!     // Console logging (output to browser console or Worker logs)
//!     ctx.log_info("Starting processing...");
//!
//!     // Progress reporting
//!     for i in 0..100 {
//!         ctx.report_progress(i, 100, Some(&format!("Step {}", i)));
//!     }
//!
//!     ctx.log_info("Processing complete!");
//!
//! } // Guard dropped here, session state automatically cleaned up
//! ```

use std::collections::HashMap;

use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

use super::context::RequestContext;

// ============================================================================
// State Storage - WASM-compatible (single-threaded with RefCell)
// ============================================================================

#[cfg(target_arch = "wasm32")]
use std::cell::RefCell;

/// Type alias for session state storage (WASM version - single-threaded).
#[cfg(target_arch = "wasm32")]
type SessionStateMap = std::collections::HashMap<String, HashMap<String, Value>>;

/// Thread-local session state storage for WASM.
#[cfg(target_arch = "wasm32")]
thread_local! {
    static SESSION_STATE: RefCell<SessionStateMap> = RefCell::new(HashMap::new());
}

// ============================================================================
// State Storage - Native version (thread-safe for tests)
// ============================================================================

#[cfg(not(target_arch = "wasm32"))]
use std::sync::{LazyLock, RwLock};

/// Session state storage (native version - thread-safe).
#[cfg(not(target_arch = "wasm32"))]
static SESSION_STATE: LazyLock<RwLock<HashMap<String, HashMap<String, Value>>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

// ============================================================================
// Progress Callback
// ============================================================================

/// Progress callback type for custom progress handling.
///
/// Parameters: (progress_token, current, total, message)
pub type ProgressCallback = Box<dyn Fn(&str, u64, Option<u64>, Option<&str>) + Send + Sync>;

// ============================================================================
// Session State Guard
// ============================================================================

/// RAII guard that automatically cleans up session state when dropped.
///
/// This is the recommended way to manage session state lifetime. Create a guard
/// at the start of a session and let it clean up automatically when the session
/// ends.
///
/// # Example
///
/// ```rust,ignore
/// use turbomcp_wasm::wasm_server::SessionStateGuard;
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

// ============================================================================
// State Error
// ============================================================================

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

// ============================================================================
// Log Level
// ============================================================================

/// Log level for console logging.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    /// Debug level
    Debug,
    /// Info level
    Info,
    /// Warning level
    Warning,
    /// Error level
    Error,
}

// ============================================================================
// RichContextExt Trait
// ============================================================================

/// Extension trait providing rich context capabilities for WASM handlers.
///
/// This trait extends `RequestContext` with session state management,
/// console logging, and progress reporting.
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

    // ===== Console Logging =====

    /// Log a debug message to the console.
    fn log_debug(&self, message: impl AsRef<str>);

    /// Log an info message to the console.
    fn log_info(&self, message: impl AsRef<str>);

    /// Log a warning message to the console.
    fn log_warning(&self, message: impl AsRef<str>);

    /// Log an error message to the console.
    fn log_error(&self, message: impl AsRef<str>);

    /// Log a message with a specific level.
    fn log(&self, level: LogLevel, message: impl AsRef<str>);

    // ===== Progress Reporting =====

    /// Report progress on a long-running operation.
    ///
    /// In WASM environments, this logs to the console by default.
    /// For SSE-based progress, use `report_progress_with_callback`.
    ///
    /// # Arguments
    ///
    /// * `current` - Current progress value
    /// * `total` - Total value (for percentage: current/total * 100)
    /// * `message` - Optional status message
    fn report_progress(&self, current: u64, total: u64, message: Option<&str>);

    /// Report progress with a custom callback.
    ///
    /// Use this when you need to send progress over SSE or custom transport.
    fn report_progress_with_callback(
        &self,
        current: u64,
        total: Option<u64>,
        message: Option<&str>,
        callback: &ProgressCallback,
    );
}

// ============================================================================
// Implementation for RequestContext
// ============================================================================

impl RichContextExt for RequestContext {
    // ===== State Management =====

    fn get_state<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        self.try_get_state(key).ok().flatten()
    }

    #[cfg(target_arch = "wasm32")]
    fn try_get_state<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>, StateError> {
        let session_id = self.session_id().ok_or(StateError::NoSessionId)?;

        SESSION_STATE.with(|state| {
            let state = state.borrow();
            let Some(session_state) = state.get(session_id) else {
                return Ok(None);
            };
            let Some(value) = session_state.get(key) else {
                return Ok(None);
            };
            serde_json::from_value(value.clone())
                .map(Some)
                .map_err(|e| StateError::DeserializationFailed(e.to_string()))
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn try_get_state<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>, StateError> {
        let session_id = self.session_id().ok_or(StateError::NoSessionId)?;

        let state = SESSION_STATE.read().unwrap();
        let Some(session_state) = state.get(session_id) else {
            return Ok(None);
        };
        let Some(value) = session_state.get(key) else {
            return Ok(None);
        };
        serde_json::from_value(value.clone())
            .map(Some)
            .map_err(|e| StateError::DeserializationFailed(e.to_string()))
    }

    fn set_state<T: Serialize>(&self, key: &str, value: &T) -> bool {
        self.try_set_state(key, value).is_ok()
    }

    #[cfg(target_arch = "wasm32")]
    fn try_set_state<T: Serialize>(&self, key: &str, value: &T) -> Result<(), StateError> {
        let session_id = self.session_id().ok_or(StateError::NoSessionId)?;

        let json_value = serde_json::to_value(value)
            .map_err(|e| StateError::SerializationFailed(e.to_string()))?;

        SESSION_STATE.with(|state| {
            let mut state = state.borrow_mut();
            let session_state = state.entry(session_id.to_string()).or_default();
            session_state.insert(key.to_string(), json_value);
        });

        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn try_set_state<T: Serialize>(&self, key: &str, value: &T) -> Result<(), StateError> {
        let session_id = self.session_id().ok_or(StateError::NoSessionId)?;

        let json_value = serde_json::to_value(value)
            .map_err(|e| StateError::SerializationFailed(e.to_string()))?;

        let mut state = SESSION_STATE.write().unwrap();
        let session_state = state.entry(session_id.to_string()).or_default();
        session_state.insert(key.to_string(), json_value);

        Ok(())
    }

    #[cfg(target_arch = "wasm32")]
    fn remove_state(&self, key: &str) -> bool {
        let Some(session_id) = self.session_id() else {
            return false;
        };

        SESSION_STATE.with(|state| {
            let mut state = state.borrow_mut();
            if let Some(session_state) = state.get_mut(session_id) {
                session_state.remove(key);
                return true;
            }
            false
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn remove_state(&self, key: &str) -> bool {
        let Some(session_id) = self.session_id() else {
            return false;
        };

        let mut state = SESSION_STATE.write().unwrap();
        if let Some(session_state) = state.get_mut(session_id) {
            session_state.remove(key);
            return true;
        }
        false
    }

    #[cfg(target_arch = "wasm32")]
    fn clear_state(&self) {
        if let Some(session_id) = self.session_id() {
            SESSION_STATE.with(|state| {
                let mut state = state.borrow_mut();
                if let Some(session_state) = state.get_mut(session_id) {
                    session_state.clear();
                }
            });
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn clear_state(&self) {
        if let Some(session_id) = self.session_id() {
            let mut state = SESSION_STATE.write().unwrap();
            if let Some(session_state) = state.get_mut(session_id) {
                session_state.clear();
            }
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn has_state(&self, key: &str) -> bool {
        let Some(session_id) = self.session_id() else {
            return false;
        };

        SESSION_STATE.with(|state| {
            let state = state.borrow();
            state
                .get(session_id)
                .map(|s| s.contains_key(key))
                .unwrap_or(false)
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn has_state(&self, key: &str) -> bool {
        let Some(session_id) = self.session_id() else {
            return false;
        };

        let state = SESSION_STATE.read().unwrap();
        state
            .get(session_id)
            .map(|s| s.contains_key(key))
            .unwrap_or(false)
    }

    // ===== Console Logging =====

    fn log_debug(&self, message: impl AsRef<str>) {
        self.log(LogLevel::Debug, message);
    }

    fn log_info(&self, message: impl AsRef<str>) {
        self.log(LogLevel::Info, message);
    }

    fn log_warning(&self, message: impl AsRef<str>) {
        self.log(LogLevel::Warning, message);
    }

    fn log_error(&self, message: impl AsRef<str>) {
        self.log(LogLevel::Error, message);
    }

    #[cfg(target_arch = "wasm32")]
    fn log(&self, level: LogLevel, message: impl AsRef<str>) {
        let msg = message.as_ref();
        let prefix = format!("[{}] ", self.request_id());
        let full_msg = format!("{}{}", prefix, msg);

        match level {
            LogLevel::Debug => web_sys::console::debug_1(&full_msg.into()),
            LogLevel::Info => web_sys::console::info_1(&full_msg.into()),
            LogLevel::Warning => web_sys::console::warn_1(&full_msg.into()),
            LogLevel::Error => web_sys::console::error_1(&full_msg.into()),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn log(&self, level: LogLevel, message: impl AsRef<str>) {
        let msg = message.as_ref();
        let prefix = format!("[{}] ", self.request_id());
        let level_str = match level {
            LogLevel::Debug => "DEBUG",
            LogLevel::Info => "INFO",
            LogLevel::Warning => "WARN",
            LogLevel::Error => "ERROR",
        };
        println!("{} {}{}", level_str, prefix, msg);
    }

    // ===== Progress Reporting =====

    fn report_progress(&self, current: u64, total: u64, message: Option<&str>) {
        let percentage = if total > 0 {
            (current as f64 / total as f64 * 100.0) as u32
        } else {
            0
        };

        let msg = match message {
            Some(m) => format!("Progress: {}% ({}/{}) - {}", percentage, current, total, m),
            None => format!("Progress: {}% ({}/{})", percentage, current, total),
        };

        self.log_info(msg);
    }

    fn report_progress_with_callback(
        &self,
        current: u64,
        total: Option<u64>,
        message: Option<&str>,
        callback: &ProgressCallback,
    ) {
        callback(self.request_id(), current, total, message);
    }
}

// ============================================================================
// Module-level functions
// ============================================================================

/// Clean up session state when a session ends.
///
/// **Important**: Call this when a session disconnects to free memory.
/// Alternatively, use [`SessionStateGuard`] for automatic cleanup.
///
/// # Example
///
/// ```rust,ignore
/// use turbomcp_wasm::wasm_server::cleanup_session_state;
///
/// fn on_session_disconnect(session_id: &str) {
///     cleanup_session_state(session_id);
/// }
/// ```
///
/// # Platform Support
///
/// - **WASM32**: Uses thread-local storage
/// - **Native**: Uses global static with RwLock
#[cfg(target_arch = "wasm32")]
pub fn cleanup_session_state(session_id: &str) {
    SESSION_STATE.with(|state| {
        state.borrow_mut().remove(session_id);
    });
}

/// Clean up session state when a session ends (native version).
///
/// See the WASM32 version's documentation for details.
#[cfg(not(target_arch = "wasm32"))]
pub fn cleanup_session_state(session_id: &str) {
    SESSION_STATE.write().unwrap().remove(session_id);
}

/// Get the number of active sessions with state.
///
/// This is useful for monitoring memory usage.
///
/// # Platform Support
///
/// - **WASM32**: Counts entries in thread-local storage
/// - **Native**: Counts entries in global static
#[cfg(target_arch = "wasm32")]
pub fn active_sessions_count() -> usize {
    SESSION_STATE.with(|state| state.borrow().len())
}

/// Get the number of active sessions with state (native version).
///
/// See the WASM32 version's documentation for details.
#[cfg(not(target_arch = "wasm32"))]
pub fn active_sessions_count() -> usize {
    SESSION_STATE.read().unwrap().len()
}

/// Clear all session state.
///
/// **Warning**: This removes state for ALL sessions. Use with caution.
/// Primarily intended for testing.
#[cfg(test)]
#[allow(dead_code)]
fn clear_all_session_state() {
    #[cfg(not(target_arch = "wasm32"))]
    {
        SESSION_STATE.write().unwrap().clear();
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn cleanup_test_sessions() {
        cleanup_session_state("test-session-1");
        cleanup_session_state("test-session-2");
        cleanup_session_state("session-iso-1");
        cleanup_session_state("session-iso-2");
        cleanup_session_state("complex-session-1");
        cleanup_session_state("guard-test-session");
        cleanup_session_state("error-test-session");
        cleanup_session_state("logging-test");
        cleanup_session_state("progress-test");
    }

    #[test]
    fn test_get_set_state() {
        cleanup_test_sessions();

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
        cleanup_test_sessions();

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
        cleanup_test_sessions();

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
        cleanup_test_sessions();

        let session_id = "guard-test-session";

        {
            let _guard = SessionStateGuard::new(session_id);
            let ctx = RequestContext::new().with_session_id(session_id);

            ctx.set_state("key", &"value");
            assert_eq!(ctx.get_state::<String>("key"), Some("value".to_string()));

            // State exists while guard is alive
            assert!(active_sessions_count() > 0 || ctx.has_state("key"));
        }

        // After guard drops, state should be cleaned up
        let ctx = RequestContext::new().with_session_id(session_id);
        assert_eq!(ctx.get_state::<String>("key"), None);
    }

    #[test]
    fn test_try_get_state_errors() {
        cleanup_test_sessions();

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

    #[test]
    fn test_logging() {
        let ctx = RequestContext::new().with_session_id("logging-test");

        // These should not panic
        ctx.log_debug("debug message");
        ctx.log_info("info message");
        ctx.log_warning("warning message");
        ctx.log_error("error message");
        ctx.log(LogLevel::Info, "custom level message");
    }

    #[test]
    fn test_progress_reporting() {
        let ctx = RequestContext::new().with_session_id("progress-test");

        // Basic progress reporting (logs to console)
        ctx.report_progress(50, 100, Some("halfway"));
        ctx.report_progress(100, 100, None);

        // Progress with callback
        let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let calls_clone = calls.clone();

        let callback: ProgressCallback = Box::new(move |token, current, total, message| {
            calls_clone.lock().unwrap().push((
                token.to_string(),
                current,
                total,
                message.map(String::from),
            ));
        });

        ctx.report_progress_with_callback(25, Some(100), Some("processing"), &callback);

        let recorded = calls.lock().unwrap();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].1, 25);
        assert_eq!(recorded[0].2, Some(100));
        assert_eq!(recorded[0].3, Some("processing".to_string()));
    }

    #[test]
    fn test_log_level_ordering() {
        assert!(LogLevel::Debug < LogLevel::Info);
        assert!(LogLevel::Info < LogLevel::Warning);
        assert!(LogLevel::Warning < LogLevel::Error);
    }
}
