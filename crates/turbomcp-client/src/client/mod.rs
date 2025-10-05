//! MCP client core implementation
//!
//! This module contains the decomposed client implementation with focused
//! modules for different responsibilities:
//!
//! - `core`: Main Client<T> implementation and connection management
//! - `protocol`: ProtocolClient for JSON-RPC communication
//! - `config`: Configuration types and utilities
//! - `builder`: ClientBuilder pattern for construction
//! - `operations`: MCP operations (tools, resources, prompts, etc.)
//! - `systems`: Supporting systems (handlers, plugins, connection)
//!
//! Note: Client<T> is now cloneable via Arc<ClientInner<T>> - no need for SharedClient!

// Core modules
pub mod config;
pub mod core;
pub mod manager;
pub mod operations;
pub mod protocol;

// TODO: Extract these as the decomposition continues
// pub mod builder;
// pub mod shared;
// pub mod systems;

// Re-export main types for backwards compatibility
pub use config::{ConnectionConfig, InitializeResult};
pub use manager::{ConnectionInfo, ConnectionState, ManagerConfig, ServerGroup, SessionManager};
