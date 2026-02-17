//! Session management for Streamable HTTP transport.
//!
//! This module provides types for managing stateful MCP connections:
//!
//! - `SessionId`: Unique identifier for a session
//! - `Session`: Session state including metadata and event history
//! - `SessionStore`: Trait for pluggable session storage backends
//! - `StoredEvent`: Persisted event for replay support

#[cfg(not(feature = "std"))]
use alloc::{string::String, vec::Vec};
use core::fmt;
#[cfg(feature = "std")]
use std::{string::String, vec::Vec};

use serde::{Deserialize, Serialize};

use crate::marker::MaybeSend;

/// Maximum allowed session ID length (256 characters).
///
/// This prevents DoS attacks via extremely long session IDs and ensures
/// reasonable memory usage for session storage backends.
pub const MAX_SESSION_ID_LEN: usize = 256;

/// Unique identifier for an MCP session.
///
/// Session IDs are used to:
/// - Track stateful connections across requests
/// - Enable server-initiated messages via SSE GET endpoint
/// - Support message replay via `Last-Event-ID`
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(String);

impl SessionId {
    /// Create a new cryptographically secure random session ID.
    ///
    /// Uses 16 bytes (128 bits) of cryptographic randomness from `getrandom`,
    /// which is sufficient to prevent session enumeration and guessing attacks.
    /// The ID is formatted as `mcp-{hex}` for easy identification.
    ///
    /// # Panics
    ///
    /// Panics if the cryptographic random number generator is unavailable.
    /// This should never happen in practice:
    /// - On WASM: Uses Web Crypto API (always available in browsers/Workers)
    /// - On native: Uses OS-provided CSPRNG
    ///
    /// If you need fallible session ID generation, use [`SessionId::try_new()`].
    ///
    /// # Security
    ///
    /// This function uses fail-closed semantics: it will panic rather than
    /// generate a weak or predictable session ID. This prevents session
    /// hijacking attacks that could occur with weak session IDs.
    pub fn new() -> Self {
        Self::try_new().expect(
            "Cryptographic random number generator unavailable. \
             Cannot create secure session ID. This indicates a serious \
             platform configuration issue.",
        )
    }

    /// Try to create a new cryptographically secure random session ID.
    ///
    /// Returns `None` if the cryptographic random number generator is unavailable.
    /// Prefer [`SessionId::new()`] unless you need to handle RNG failures gracefully.
    pub fn try_new() -> Option<Self> {
        let mut bytes = [0u8; 16]; // 128 bits of entropy

        // getrandom works on all platforms including WASM (via js feature)
        if getrandom::getrandom(&mut bytes).is_err() {
            return None;
        }

        // Format as hex for human-readable session IDs
        let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        Some(Self(format!("mcp-{hex}")))
    }

    /// Create a session ID from a string.
    ///
    /// # Panics
    ///
    /// Panics if the session ID exceeds `MAX_SESSION_ID_LEN` (256 characters).
    /// Use `try_from_string` for non-panicking validation.
    pub fn from_string(s: impl Into<String>) -> Self {
        let string = s.into();
        assert!(
            string.len() <= MAX_SESSION_ID_LEN,
            "Session ID length {} exceeds maximum allowed length {}",
            string.len(),
            MAX_SESSION_ID_LEN
        );
        Self(string)
    }

    /// Try to create a session ID from a string with validation.
    ///
    /// Returns `None` if the session ID exceeds `MAX_SESSION_ID_LEN` (256 characters).
    pub fn try_from_string(s: impl Into<String>) -> Option<Self> {
        let string = s.into();
        if string.len() <= MAX_SESSION_ID_LEN {
            Some(Self(string))
        } else {
            None
        }
    }

    /// Get the session ID as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consume the session ID and return the inner string.
    pub fn into_string(self) -> String {
        self.0
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for SessionId {
    fn from(s: String) -> Self {
        Self::from_string(s)
    }
}

impl From<&str> for SessionId {
    fn from(s: &str) -> Self {
        Self::from_string(s)
    }
}

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

/// State of an MCP session.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionState {
    /// Session is active and accepting requests
    Active,
    /// Session is initialized but waiting for client confirmation
    #[default]
    Pending,
    /// Session has been terminated
    Terminated,
    /// Session has expired due to inactivity
    Expired,
}

/// An MCP session with metadata and optional event history.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Session {
    /// Unique session identifier
    pub id: SessionId,

    /// Current session state
    pub state: SessionState,

    /// Session creation timestamp (Unix milliseconds)
    pub created_at: u64,

    /// Last activity timestamp (Unix milliseconds)
    pub last_activity: u64,

    /// Client information (e.g., client name/version)
    pub client_info: Option<String>,

    /// Negotiated protocol version
    pub protocol_version: Option<String>,

    /// Last event ID sent to this session (for replay)
    pub last_event_id: Option<String>,

    /// Number of events stored for this session
    pub event_count: u64,
}

impl Session {
    /// Create a new session with the given ID.
    #[cfg(feature = "std")]
    pub fn new(id: SessionId) -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        Self {
            id,
            state: SessionState::Pending,
            created_at: now,
            last_activity: now,
            client_info: None,
            protocol_version: None,
            last_event_id: None,
            event_count: 0,
        }
    }

    /// Create a new session with explicit timestamps (for no_std environments).
    pub fn new_with_timestamp(id: SessionId, timestamp_ms: u64) -> Self {
        Self {
            id,
            state: SessionState::Pending,
            created_at: timestamp_ms,
            last_activity: timestamp_ms,
            client_info: None,
            protocol_version: None,
            last_event_id: None,
            event_count: 0,
        }
    }

    /// Check if the session is active.
    pub fn is_active(&self) -> bool {
        matches!(self.state, SessionState::Active)
    }

    /// Check if the session can accept requests.
    pub fn can_accept_requests(&self) -> bool {
        matches!(self.state, SessionState::Active | SessionState::Pending)
    }

    /// Mark the session as active.
    pub fn activate(&mut self) {
        self.state = SessionState::Active;
    }

    /// Mark the session as terminated.
    pub fn terminate(&mut self) {
        self.state = SessionState::Terminated;
    }

    /// Update the last activity timestamp.
    #[cfg(feature = "std")]
    pub fn touch(&mut self) {
        use std::time::{SystemTime, UNIX_EPOCH};

        self.last_activity = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(self.last_activity);
    }

    /// Update the last activity timestamp with explicit value.
    pub fn touch_with_timestamp(&mut self, timestamp_ms: u64) {
        self.last_activity = timestamp_ms;
    }

    /// Check if the session has expired based on timeout.
    pub fn is_expired(&self, current_time_ms: u64, timeout_ms: u64) -> bool {
        current_time_ms.saturating_sub(self.last_activity) > timeout_ms
    }
}

/// A stored event for replay support.
///
/// When clients reconnect with `Last-Event-ID`, the server replays
/// events that occurred after that ID.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StoredEvent {
    /// Unique event ID for resumption
    pub id: String,

    /// Event type (e.g., "message", "notification")
    pub event_type: Option<String>,

    /// Event data (typically JSON-RPC message)
    pub data: String,

    /// Timestamp when the event was created (Unix milliseconds)
    pub timestamp: u64,
}

impl StoredEvent {
    /// Create a new stored event.
    #[cfg(feature = "std")]
    pub fn new(id: impl Into<String>, data: impl Into<String>) -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        Self {
            id: id.into(),
            event_type: None,
            data: data.into(),
            timestamp,
        }
    }

    /// Create a new stored event with explicit timestamp.
    pub fn new_with_timestamp(
        id: impl Into<String>,
        data: impl Into<String>,
        timestamp: u64,
    ) -> Self {
        Self {
            id: id.into(),
            event_type: None,
            data: data.into(),
            timestamp,
        }
    }

    /// Set the event type.
    pub fn with_event_type(mut self, event_type: impl Into<String>) -> Self {
        self.event_type = Some(event_type.into());
        self
    }
}

/// Trait for pluggable session storage backends.
///
/// Implementations can store sessions in:
/// - Memory (single Worker instance)
/// - Cloudflare KV (cross-request persistence)
/// - Cloudflare Durable Objects (strong consistency)
/// - Redis, DynamoDB, etc.
///
/// # Example
///
/// ```rust,ignore
/// use turbomcp_transport_streamable::{SessionId, Session, SessionStore, StoredEvent};
/// use std::collections::HashMap;
///
/// struct MemorySessionStore {
///     sessions: HashMap<String, Session>,
/// }
///
/// impl SessionStore for MemorySessionStore {
///     type Error = std::convert::Infallible;
///
///     async fn create(&self) -> Result<SessionId, Self::Error> {
///         let id = SessionId::new();
///         // Store session...
///         Ok(id)
///     }
///
///     // ... implement other methods
/// }
/// ```
pub trait SessionStore {
    /// Error type for storage operations
    type Error: core::fmt::Debug;

    /// Create a new session and return its ID.
    fn create(
        &self,
    ) -> impl core::future::Future<Output = Result<SessionId, Self::Error>> + MaybeSend;

    /// Get a session by ID.
    fn get(
        &self,
        id: &SessionId,
    ) -> impl core::future::Future<Output = Result<Option<Session>, Self::Error>> + MaybeSend;

    /// Update a session.
    fn update(
        &self,
        session: &Session,
    ) -> impl core::future::Future<Output = Result<(), Self::Error>> + MaybeSend;

    /// Store an event for replay support.
    fn store_event(
        &self,
        id: &SessionId,
        event: StoredEvent,
    ) -> impl core::future::Future<Output = Result<(), Self::Error>> + MaybeSend;

    /// Replay events from a given event ID.
    ///
    /// Returns events that occurred after `last_event_id`.
    fn replay_from(
        &self,
        id: &SessionId,
        last_event_id: &str,
    ) -> impl core::future::Future<Output = Result<Vec<StoredEvent>, Self::Error>> + MaybeSend;

    /// Destroy a session.
    fn destroy(
        &self,
        id: &SessionId,
    ) -> impl core::future::Future<Output = Result<(), Self::Error>> + MaybeSend;

    /// Clean up expired sessions.
    ///
    /// Default implementation does nothing.
    fn cleanup_expired(
        &self,
        _timeout_ms: u64,
    ) -> impl core::future::Future<Output = Result<u64, Self::Error>> + MaybeSend {
        async { Ok(0) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_id_from_string() {
        let id = SessionId::from_string("test-123");
        assert_eq!(id.as_str(), "test-123");
    }

    #[test]
    fn test_session_id_display() {
        let id = SessionId::from_string("display-test");
        assert_eq!(format!("{id}"), "display-test");
    }

    #[test]
    fn test_session_state_default() {
        assert_eq!(SessionState::default(), SessionState::Pending);
    }

    #[cfg(feature = "std")]
    #[test]
    fn test_session_new() {
        let id = SessionId::new();
        let session = Session::new(id.clone());

        assert_eq!(session.id, id);
        assert_eq!(session.state, SessionState::Pending);
        assert!(session.created_at > 0);
    }

    #[test]
    fn test_session_lifecycle() {
        let id = SessionId::from_string("lifecycle-test");
        let mut session = Session::new_with_timestamp(id, 1000);

        assert!(!session.is_active());
        assert!(session.can_accept_requests());

        session.activate();
        assert!(session.is_active());
        assert!(session.can_accept_requests());

        session.terminate();
        assert!(!session.is_active());
        assert!(!session.can_accept_requests());
    }

    #[test]
    fn test_session_expiration() {
        let id = SessionId::from_string("expiry-test");
        let session = Session::new_with_timestamp(id, 1000);

        // Not expired within timeout
        assert!(!session.is_expired(2000, 5000));

        // Expired after timeout
        assert!(session.is_expired(10000, 5000));
    }

    #[cfg(feature = "std")]
    #[test]
    fn test_stored_event_new() {
        let event = StoredEvent::new("evt-1", r#"{"method": "test"}"#);

        assert_eq!(event.id, "evt-1");
        assert!(event.timestamp > 0);
        assert!(event.event_type.is_none());
    }

    #[test]
    fn test_stored_event_with_type() {
        let event =
            StoredEvent::new_with_timestamp("evt-2", "data", 1000).with_event_type("notification");

        assert_eq!(event.event_type, Some("notification".to_string()));
    }

    #[test]
    fn test_session_id_length_validation() {
        // Valid session ID within limit
        let valid_id = "a".repeat(256);
        let session_id = SessionId::try_from_string(valid_id.clone());
        assert!(session_id.is_some());
        assert_eq!(session_id.unwrap().as_str(), valid_id);

        // Session ID at exact limit should be accepted
        let at_limit = "b".repeat(MAX_SESSION_ID_LEN);
        let session_id = SessionId::try_from_string(at_limit.clone());
        assert!(session_id.is_some());

        // Session ID exceeding limit should be rejected
        let too_long = "c".repeat(MAX_SESSION_ID_LEN + 1);
        let session_id = SessionId::try_from_string(too_long);
        assert!(session_id.is_none());

        // Extremely long session ID should be rejected
        let very_long = "d".repeat(10000);
        let session_id = SessionId::try_from_string(very_long);
        assert!(session_id.is_none());
    }

    #[test]
    #[should_panic(expected = "Session ID length")]
    fn test_session_id_from_string_panics_on_overflow() {
        let too_long = "e".repeat(MAX_SESSION_ID_LEN + 1);
        let _session_id = SessionId::from_string(too_long);
    }

    #[test]
    fn test_session_id_from_trait_validates() {
        // Test From<String> trait
        let valid = "valid-id".to_string();
        let _session_id: SessionId = valid.into();

        // Test From<&str> trait
        let _session_id: SessionId = "another-valid-id".into();
    }
}
