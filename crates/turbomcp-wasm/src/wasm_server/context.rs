//! Request context for WASM MCP handlers.
//!
//! v3.2 collapsed the previously-parallel WASM `RequestContext` into the
//! single canonical type defined in `turbomcp-core`. This module now
//! re-exports that type and provides WASM-specific construction helpers:
//!
//! - [`new_wasm_context`]: fresh context with a Web-Crypto-generated request
//!   ID (falls back to entropy hashing on native tests).
//! - [`from_worker_request`]: build a context from a Cloudflare Worker request
//!   with request ID, session ID, and HTTP headers.
//! - [`current_timestamp_ms`]: wall-clock milliseconds (JS `Date.now()` on
//!   WASM, `SystemTime` on native).
//!
//! ## Example
//!
//! ```ignore
//! use turbomcp_wasm::wasm_server::context::{new_wasm_context, from_worker_request};
//!
//! async fn my_tool(ctx: &RequestContext, args: MyArgs) -> String {
//!     if let Some(session) = ctx.session_id() {
//!         println!("Session: {session}");
//!     }
//!     if let Some(user_agent) = ctx.header("user-agent") {
//!         println!("User-Agent: {user_agent}");
//!     }
//!     format!("Request ID: {}", ctx.request_id())
//! }
//! ```

use hashbrown::HashMap as HashbrownMap;

pub use turbomcp_core::context::{RequestContext, TransportType};

/// Metadata key under which [`new_wasm_context`] records the request
/// wall-clock timestamp. WASM doesn't have a usable `std::time::Instant`,
/// so the JS `Date.now()` value lives in metadata where tool bodies can
/// read it via `ctx.get_metadata("wasm_timestamp_ms")`.
pub const WASM_TIMESTAMP_METADATA_KEY: &str = "wasm_timestamp_ms";

/// Build a fresh [`RequestContext`] for the Wasm transport with a
/// cryptographically-generated request ID and a wall-clock timestamp stored
/// in metadata under [`WASM_TIMESTAMP_METADATA_KEY`].
pub fn new_wasm_context() -> RequestContext {
    RequestContext::with_id_and_transport(generate_request_id(), TransportType::Wasm).with_metadata(
        WASM_TIMESTAMP_METADATA_KEY,
        serde_json::Value::from(current_timestamp_ms()),
    )
}

/// Build a [`RequestContext`] from an incoming Cloudflare Worker request.
///
/// If `request_id` is `None`, one is generated. `session_id` and `headers`
/// are attached when present. Accepts any `IntoIterator` of `(String, String)`
/// pairs so callers using either `std::collections::HashMap` or
/// `hashbrown::HashMap` work without conversion.
pub fn from_worker_request(
    request_id: Option<String>,
    session_id: Option<String>,
    headers: impl IntoIterator<Item = (String, String)>,
) -> RequestContext {
    let id = request_id.unwrap_or_else(generate_request_id);
    let headers: HashbrownMap<String, String> = headers.into_iter().collect();
    let mut ctx = RequestContext::with_id_and_transport(id, TransportType::Wasm)
        .with_metadata(
            WASM_TIMESTAMP_METADATA_KEY,
            serde_json::Value::from(current_timestamp_ms()),
        )
        .with_headers(headers);
    if let Some(sid) = session_id {
        ctx = ctx.with_session_id(sid);
    }
    ctx
}

/// Generate a unique request ID with cryptographic randomness.
///
/// Uses the Web Crypto API on WASM (`req-{timestamp_hex}-{random_hex}`) and
/// a non-cryptographic fallback on native (tests only).
pub fn generate_request_id() -> String {
    format!("req-{:x}-{:x}", current_timestamp_ms(), get_random_u64())
}

/// Get the current wall-clock timestamp in Unix milliseconds.
///
/// Uses `js_sys::Date::now()` in WASM, `SystemTime` on native.
#[cfg(target_arch = "wasm32")]
pub fn current_timestamp_ms() -> u64 {
    js_sys::Date::now() as u64
}

/// Native fallback for [`current_timestamp_ms`] (tests only).
#[cfg(not(target_arch = "wasm32"))]
pub fn current_timestamp_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Get cryptographically secure random `u64`.
#[cfg(target_arch = "wasm32")]
fn get_random_u64() -> u64 {
    if let Some(window) = web_sys::window()
        && let Ok(crypto) = window.crypto()
    {
        let mut bytes = [0u8; 8];
        if crypto.get_random_values_with_u8_array(&mut bytes).is_ok() {
            return u64::from_le_bytes(bytes);
        }
    }
    // Fallback: use timestamp (weak but non-zero)
    current_timestamp_ms()
}

#[cfg(not(target_arch = "wasm32"))]
fn get_random_u64() -> u64 {
    // For native builds (tests only). NOT cryptographically secure.
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::thread;

    let mut hasher = DefaultHasher::new();
    current_timestamp_ms().hash(&mut hasher);
    thread::current().id().hash(&mut hasher);
    let stack_var: u8 = 0;
    (std::ptr::from_ref(&stack_var) as usize).hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_wasm_context_populates_request_id_and_transport() {
        let ctx = new_wasm_context();
        assert!(ctx.request_id().starts_with("req-"));
        assert_eq!(ctx.transport(), TransportType::Wasm);
        let ts = ctx
            .get_metadata(WASM_TIMESTAMP_METADATA_KEY)
            .and_then(|v| v.as_u64())
            .expect("timestamp should be set");
        assert!(ts > 0);
    }

    #[test]
    fn builder_chain_round_trip() {
        let ctx = new_wasm_context()
            .with_session_id("session-123")
            .with_user_id("user-456")
            .with_client_id("client-789");
        assert_eq!(ctx.session_id(), Some("session-123"));
        assert_eq!(ctx.user_id(), Some("user-456"));
        assert_eq!(ctx.client_id(), Some("client-789"));
        assert!(ctx.is_authenticated(), "user_id implies authenticated");
    }

    #[test]
    fn case_insensitive_headers() {
        let mut headers: HashbrownMap<String, String> = HashbrownMap::new();
        headers.insert("User-Agent".into(), "TestClient/1.0".into());
        headers.insert("Content-Type".into(), "application/json".into());
        let ctx = new_wasm_context().with_headers(headers);

        assert_eq!(ctx.header("user-agent"), Some("TestClient/1.0"));
        assert_eq!(ctx.header("USER-AGENT"), Some("TestClient/1.0"));
        assert_eq!(ctx.header("content-type"), Some("application/json"));
        assert_eq!(ctx.header("x-unknown"), None);
    }

    #[test]
    fn metadata_round_trip() {
        let ctx = new_wasm_context()
            .with_metadata("tenant", "acme")
            .with_metadata("priority", 5);
        assert_eq!(ctx.get_metadata("tenant"), Some(&serde_json::json!("acme")));
        assert_eq!(ctx.get_metadata("priority"), Some(&serde_json::json!(5)));
        assert_eq!(ctx.get_metadata("unknown"), None);
    }

    #[test]
    fn roles_from_auth_metadata() {
        let ctx = new_wasm_context()
            .with_metadata("auth", serde_json::json!({ "roles": ["admin", "user"] }));
        assert!(ctx.has_any_role(&["admin"]));
        assert!(ctx.has_any_role(&["user", "other"]));
        assert!(!ctx.has_any_role(&["superuser"]));
    }

    #[test]
    fn from_worker_request_attaches_headers_and_ids() {
        let mut headers: HashbrownMap<String, String> = HashbrownMap::new();
        headers.insert("authorization".into(), "Bearer token123".into());

        let ctx = from_worker_request(Some("req-abc".into()), Some("sess-xyz".into()), headers);

        assert_eq!(ctx.request_id(), "req-abc");
        assert_eq!(ctx.session_id(), Some("sess-xyz"));
        assert_eq!(ctx.header("authorization"), Some("Bearer token123"));
        assert_eq!(ctx.transport(), TransportType::Wasm);
    }

    #[test]
    fn from_worker_request_generates_id_if_missing() {
        let headers: HashbrownMap<String, String> = HashbrownMap::new();
        let ctx = from_worker_request(None, None, headers);
        assert!(ctx.request_id().starts_with("req-"));
    }
}
