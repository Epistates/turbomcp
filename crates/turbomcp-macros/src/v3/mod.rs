//! v3 macro implementations - pristine architecture with McpHandler trait.
//!
//! This module contains the v3 TurboMCP macros that generate complete
//! `McpHandler` trait implementations from annotated impl blocks.
//!
//! # Architecture
//!
//! - `server.rs` - The `#[server]` macro that processes impl blocks
//! - `tool.rs` - Tool handler parsing and schema generation
//! - `schema.rs` - JSON Schema generation utilities
//!
//! The macros discover `#[tool]`, `#[resource]`, and `#[prompt]` attributes
//! on methods and generate the appropriate handler implementations.

pub mod schema;
pub mod server;
pub mod tool;
