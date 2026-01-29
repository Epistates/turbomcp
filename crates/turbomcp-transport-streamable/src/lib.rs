//! # TurboMCP Streamable HTTP Transport
//!
//! This crate provides core types for the MCP 2025-11-25 Streamable HTTP transport specification.
//! It is designed to be portable across native and WASM environments.
//!
//! ## Features
//!
//! - **Session Management**: `SessionId`, `Session`, `SessionStore` trait for stateful connections
//! - **SSE Encoding/Decoding**: Pure, no-I/O Server-Sent Events implementation
//! - **Protocol Types**: Request/response types for streamable HTTP endpoints
//!
//! ## Usage
//!
//! ```rust,ignore
//! use turbomcp_transport_streamable::{SessionId, SessionStore, SseEvent, SseEncoder};
//!
//! // Create a new session
//! let session_id = SessionId::new();
//!
//! // Encode an SSE event
//! let event = SseEvent::message("Hello, world!");
//! let encoded = SseEncoder::encode(&event);
//! ```
//!
//! ## no_std Support
//!
//! This crate supports `no_std` environments with the `alloc` feature:
//!
//! ```toml
//! [dependencies]
//! turbomcp-transport-streamable = { version = "3.0", default-features = false, features = ["alloc"] }
//! ```

#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "alloc")]
extern crate alloc;

pub mod config;
mod marker;
pub mod session;
pub mod sse;
pub mod types;

// Re-export main types
pub use config::StreamableConfig;
pub use session::{Session, SessionId, SessionState, SessionStore, StoredEvent};
pub use sse::{SseEncoder, SseEvent, SseEventBuilder, SseParser};
pub use types::{
    HttpMethod, OriginValidation, StreamableError, StreamableRequest, StreamableResponse,
};

/// MCP 2025-11-25 Streamable HTTP header names
pub mod headers {
    /// Session ID header for tracking stateful connections
    pub const MCP_SESSION_ID: &str = "Mcp-Session-Id";

    /// Last event ID header for SSE resumption
    pub const LAST_EVENT_ID: &str = "Last-Event-ID";

    /// Content-Type for JSON responses
    pub const CONTENT_TYPE_JSON: &str = "application/json";

    /// Content-Type for SSE streams
    pub const CONTENT_TYPE_SSE: &str = "text/event-stream";

    /// Accept header value for SSE
    pub const ACCEPT_SSE: &str = "text/event-stream";
}
