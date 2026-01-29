//! Request context for WASM MCP handlers.
//!
//! This module provides a WASM-compatible `RequestContext` that can be passed
//! to tool, resource, and prompt handlers for accessing request metadata.
//!
//! ## Example
//!
//! ```ignore
//! use turbomcp_wasm::wasm_server::RequestContext;
//!
//! async fn my_tool(ctx: &RequestContext, args: MyArgs) -> String {
//!     // Access session ID
//!     if let Some(session) = ctx.session_id() {
//!         println!("Session: {}", session);
//!     }
//!
//!     // Access HTTP headers
//!     if let Some(user_agent) = ctx.header("user-agent") {
//!         println!("User-Agent: {}", user_agent);
//!     }
//!
//!     format!("Request ID: {}", ctx.request_id())
//! }
//! ```

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::Value;

/// Request context passed to MCP handlers.
///
/// Contains metadata about the current request including session information,
/// HTTP headers, user identity, and custom metadata.
#[derive(Clone, Debug)]
pub struct RequestContext {
    /// Unique identifier for this request (typically UUID)
    request_id: String,

    /// Session ID for stateful connections
    session_id: Option<String>,

    /// User ID for authenticated requests
    user_id: Option<String>,

    /// Client ID (application identifier)
    client_id: Option<String>,

    /// Transport type (e.g., "http", "websocket", "wasm-worker")
    transport: Option<String>,

    /// HTTP headers from the incoming request
    headers: Option<HashMap<String, String>>,

    /// Custom metadata key-value pairs
    metadata: Arc<HashMap<String, Value>>,

    /// Request timestamp (Unix milliseconds)
    timestamp_ms: u64,
}

impl RequestContext {
    /// Create a new request context with a generated request ID.
    pub fn new() -> Self {
        Self {
            request_id: generate_request_id(),
            session_id: None,
            user_id: None,
            client_id: None,
            transport: Some("wasm-worker".to_string()),
            headers: None,
            metadata: Arc::new(HashMap::new()),
            timestamp_ms: current_timestamp_ms(),
        }
    }

    /// Create a request context with a specific request ID.
    pub fn with_id(request_id: impl Into<String>) -> Self {
        Self {
            request_id: request_id.into(),
            ..Self::new()
        }
    }

    /// Get the request ID.
    pub fn request_id(&self) -> &str {
        &self.request_id
    }

    /// Get the session ID, if set.
    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    /// Set the session ID.
    pub fn with_session_id(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }

    /// Get the user ID, if set.
    pub fn user_id(&self) -> Option<&str> {
        self.user_id.as_deref()
    }

    /// Set the user ID.
    pub fn with_user_id(mut self, user_id: impl Into<String>) -> Self {
        self.user_id = Some(user_id.into());
        self
    }

    /// Get the client ID, if set.
    pub fn client_id(&self) -> Option<&str> {
        self.client_id.as_deref()
    }

    /// Set the client ID.
    pub fn with_client_id(mut self, client_id: impl Into<String>) -> Self {
        self.client_id = Some(client_id.into());
        self
    }

    /// Get the transport type.
    pub fn transport(&self) -> Option<&str> {
        self.transport.as_deref()
    }

    /// Set the transport type.
    pub fn with_transport(mut self, transport: impl Into<String>) -> Self {
        self.transport = Some(transport.into());
        self
    }

    /// Get all HTTP headers.
    pub fn headers(&self) -> Option<&HashMap<String, String>> {
        self.headers.as_ref()
    }

    /// Set HTTP headers.
    pub fn with_headers(mut self, headers: HashMap<String, String>) -> Self {
        self.headers = Some(headers);
        self
    }

    /// Get a specific HTTP header (case-insensitive).
    pub fn header(&self, name: &str) -> Option<&str> {
        let headers = self.headers.as_ref()?;
        let name_lower = name.to_lowercase();

        headers
            .iter()
            .find(|(key, _)| key.to_lowercase() == name_lower)
            .map(|(_, value)| value.as_str())
    }

    /// Get a metadata value by key.
    pub fn get_metadata(&self, key: &str) -> Option<&Value> {
        self.metadata.get(key)
    }

    /// Set a metadata value.
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        Arc::make_mut(&mut self.metadata).insert(key.into(), value.into());
        self
    }

    /// Get the request timestamp in Unix milliseconds.
    pub fn timestamp_ms(&self) -> u64 {
        self.timestamp_ms
    }

    /// Check if the request has a specific role.
    ///
    /// Roles are stored in the "auth.roles" metadata field.
    pub fn has_role(&self, role: &str) -> bool {
        self.get_metadata("auth")
            .and_then(|auth| auth.get("roles"))
            .and_then(|roles| roles.as_array())
            .map(|roles| roles.iter().any(|r| r.as_str() == Some(role)))
            .unwrap_or(false)
    }

    /// Check if the request is authenticated.
    ///
    /// Authentication status is stored in the "authenticated" metadata field.
    pub fn is_authenticated(&self) -> bool {
        self.get_metadata("authenticated")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }

    /// Create a context from an incoming Worker request.
    pub fn from_worker_request(
        request_id: Option<String>,
        session_id: Option<String>,
        headers: HashMap<String, String>,
    ) -> Self {
        let mut ctx = Self::new();
        ctx.request_id = request_id.unwrap_or_else(generate_request_id);
        ctx.session_id = session_id;
        ctx.headers = Some(headers);
        ctx
    }
}

impl Default for RequestContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Generate a unique request ID with cryptographic randomness.
///
/// Uses Web Crypto API on WASM, getrandom on native.
/// Format: `req-{timestamp_hex}-{random_hex}`
fn generate_request_id() -> String {
    let timestamp = current_timestamp_ms();

    // Get 8 bytes of cryptographic randomness
    let random = get_random_u64();

    format!("req-{timestamp:x}-{random:x}")
}

/// Get cryptographically secure random u64.
#[cfg(target_arch = "wasm32")]
fn get_random_u64() -> u64 {
    // Use Web Crypto API for secure randomness
    if let Some(window) = web_sys::window() {
        if let Ok(crypto) = window.crypto() {
            let mut bytes = [0u8; 8];
            if crypto.get_random_values_with_u8_array(&mut bytes).is_ok() {
                return u64::from_le_bytes(bytes);
            }
        }
    }
    // Fallback: use timestamp (weak but non-zero)
    current_timestamp_ms()
}

#[cfg(not(target_arch = "wasm32"))]
fn get_random_u64() -> u64 {
    // For native builds (primarily used for testing), combine multiple entropy sources
    // This is NOT cryptographically secure but sufficient for request ID uniqueness
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::thread;

    let mut hasher = DefaultHasher::new();
    current_timestamp_ms().hash(&mut hasher);
    thread::current().id().hash(&mut hasher);
    // Add some address-space randomness from stack location
    let stack_var: u8 = 0;
    (std::ptr::from_ref(&stack_var) as usize).hash(&mut hasher);
    hasher.finish()
}

/// Get the current timestamp in milliseconds.
///
/// Uses `js_sys::Date::now()` in WASM, falls back to 0 on native.
#[cfg(target_arch = "wasm32")]
fn current_timestamp_ms() -> u64 {
    js_sys::Date::now() as u64
}

#[cfg(not(target_arch = "wasm32"))]
fn current_timestamp_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_context() {
        let ctx = RequestContext::new();
        assert!(ctx.request_id().starts_with("req-"));
        assert!(ctx.timestamp_ms() > 0);
        assert_eq!(ctx.transport(), Some("wasm-worker"));
    }

    #[test]
    fn test_with_id() {
        let ctx = RequestContext::with_id("custom-id");
        assert_eq!(ctx.request_id(), "custom-id");
    }

    #[test]
    fn test_session_id() {
        let ctx = RequestContext::new().with_session_id("session-123");
        assert_eq!(ctx.session_id(), Some("session-123"));
    }

    #[test]
    fn test_user_id() {
        let ctx = RequestContext::new().with_user_id("user-456");
        assert_eq!(ctx.user_id(), Some("user-456"));
    }

    #[test]
    fn test_headers() {
        let mut headers = HashMap::new();
        headers.insert("User-Agent".to_string(), "TestClient/1.0".to_string());
        headers.insert("Content-Type".to_string(), "application/json".to_string());

        let ctx = RequestContext::new().with_headers(headers);

        // Case-insensitive lookup
        assert_eq!(ctx.header("user-agent"), Some("TestClient/1.0"));
        assert_eq!(ctx.header("USER-AGENT"), Some("TestClient/1.0"));
        assert_eq!(ctx.header("content-type"), Some("application/json"));
        assert_eq!(ctx.header("x-unknown"), None);
    }

    #[test]
    fn test_metadata() {
        let ctx = RequestContext::new()
            .with_metadata("tenant", serde_json::json!("acme"))
            .with_metadata("priority", serde_json::json!(5));

        assert_eq!(ctx.get_metadata("tenant"), Some(&serde_json::json!("acme")));
        assert_eq!(ctx.get_metadata("priority"), Some(&serde_json::json!(5)));
        assert_eq!(ctx.get_metadata("unknown"), None);
    }

    #[test]
    fn test_roles() {
        let ctx = RequestContext::new().with_metadata(
            "auth",
            serde_json::json!({
                "roles": ["admin", "user"]
            }),
        );

        assert!(ctx.has_role("admin"));
        assert!(ctx.has_role("user"));
        assert!(!ctx.has_role("superuser"));
    }

    #[test]
    fn test_authenticated() {
        let ctx = RequestContext::new().with_metadata("authenticated", serde_json::json!(true));
        assert!(ctx.is_authenticated());

        let ctx2 = RequestContext::new();
        assert!(!ctx2.is_authenticated());
    }

    #[test]
    fn test_from_worker_request() {
        let mut headers = HashMap::new();
        headers.insert("authorization".to_string(), "Bearer token123".to_string());

        let ctx = RequestContext::from_worker_request(
            Some("req-abc".into()),
            Some("sess-xyz".into()),
            headers,
        );

        assert_eq!(ctx.request_id(), "req-abc");
        assert_eq!(ctx.session_id(), Some("sess-xyz"));
        assert_eq!(ctx.header("authorization"), Some("Bearer token123"));
    }
}
