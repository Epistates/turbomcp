//! MCP client operations modules
//!
//! This module contains focused operation modules for CLIENT-INITIATED MCP operations:
//!
//! - `tools`: Tool operations (list, call)
//! - `resources`: Resource operations (list, read, templates, subscribe/unsubscribe)
//! - `prompts`: Prompt operations (list, get)
//! - `completion`: Argument autocompletion operations
//! - `sampling`: LLM sampling handler registration (SERVER->CLIENT)
//! - `connection`: Connection utilities (ping, set_log_level)
//! - `handlers`: Event handler registration for SERVER->CLIENT requests
//! - `plugins`: Plugin registration and middleware management
//!
//! Note: `roots/list` is a SERVER->CLIENT request (not a client operation).
//! The client should implement a roots handler to respond to server requests.

pub mod completion;
pub mod connection;
pub mod handlers;
pub mod plugins;
pub mod prompts;
pub mod resources;
pub mod sampling;
pub mod tools;
