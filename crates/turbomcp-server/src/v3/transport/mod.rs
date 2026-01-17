//! v3 Transport module - shared abstractions for all transports.
//!
//! This module provides:
//! - Common constants and types
//! - Shared line-based transport runner
//! - Transport-specific implementations
//!
//! # Architecture
//!
//! All transports share a common pattern:
//! 1. Read incoming messages (line-based or frame-based)
//! 2. Parse as JSON-RPC
//! 3. Route to handler
//! 4. Send response
//!
//! The `LineTransportRunner` provides a reusable implementation for
//! line-based protocols (STDIO, TCP, Unix).

mod line;

#[cfg(feature = "stdio")]
pub mod stdio;

#[cfg(feature = "tcp")]
pub mod tcp;

#[cfg(feature = "unix")]
pub mod unix;

#[cfg(feature = "http")]
pub mod http;

#[cfg(feature = "websocket")]
pub mod websocket;

pub use line::{LineReader, LineTransportRunner, LineWriter};

// Re-export the configurable default from config
pub use crate::v3::config::DEFAULT_MAX_MESSAGE_SIZE;

/// Maximum message size for line-based transports.
/// This prevents memory exhaustion from maliciously large messages.
/// Use `ServerConfig::max_message_size` for runtime configuration.
pub const MAX_MESSAGE_SIZE: usize = DEFAULT_MAX_MESSAGE_SIZE;
