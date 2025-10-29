//! Backend transport implementations
//!
//! This module contains concrete implementations of backend transports
//! for connecting to different types of MCP servers.

pub mod http;

pub use http::HttpBackend;
