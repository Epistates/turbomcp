//! MCP client core implementation
//!
//! This module contains the decomposed client implementation with focused
//! modules for different responsibilities:
//!
//! - `core`: Main `Client<T>` implementation and connection management
//! - `protocol`: ProtocolClient for JSON-RPC communication
//! - `dispatcher`: Message routing for bidirectional communication
//! - `config`: Configuration types and utilities
//! - `builder`: ClientBuilder pattern for construction
//! - `operations`: MCP operations (tools, resources, prompts, etc.)
//! - `systems`: Supporting systems (handlers, plugins, connection)
//!
//! Note: `Client<T>` is now cloneable via `Arc<ClientInner<T>>` - no need for SharedClient!

// Core modules
pub mod config;
pub mod core;
pub mod dispatcher;
pub mod manager;
pub mod operations;
pub mod protocol;

// Design Note: Module decomposition is complete for 2.0.0
//
// The client module is decomposed into focused submodules:
// - config: Connection and initialization configuration
// - core: Core client implementation
// - manager: Session and connection management
// - operations: MCP operation implementations (tools, prompts, resources)
// - protocol: Protocol-level communication
//
// Further decomposition (builder, shared, systems) is not currently needed.
// The current structure balances cohesion and simplicity.

// Re-export main types for backwards compatibility
pub use config::{ConnectionConfig, InitializeResult};
pub use manager::{ConnectionInfo, ConnectionState, ManagerConfig, ServerGroup, SessionManager};
