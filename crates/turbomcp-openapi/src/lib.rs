//! # TurboMCP OpenAPI Provider
//!
//! Convert OpenAPI specifications to MCP tools and resources at runtime.
//!
//! This crate enables automatic exposure of REST APIs as MCP components:
//! - GET endpoints become MCP resources
//! - POST/PUT/DELETE endpoints become MCP tools
//! - Configurable route mapping patterns
//!
//! # Example
//!
//! ```rust,ignore
//! use turbomcp_openapi::{OpenApiProvider, RouteMapping, McpType};
//!
//! // Load from URL
//! let provider = OpenApiProvider::from_url("https://api.example.com/openapi.json")
//!     .await?
//!     .with_base_url("https://api.example.com")
//!     .with_route_mapping(RouteMapping::new()
//!         .map_method("GET", McpType::Resource)
//!         .map_method("POST", McpType::Tool)
//!         .map_pattern(r"/admin/.*", McpType::Tool));
//!
//! // Use with TurboMCP server
//! let handler = provider.into_handler();
//! ```

#![deny(missing_docs)]
#![warn(missing_debug_implementations)]
#![warn(clippy::all)]
#![allow(clippy::module_name_repetitions)]

mod error;
mod handler;
mod mapping;
mod parser;
mod provider;
mod security;

pub use error::{OpenApiError, Result};
pub use handler::OpenApiHandler;
pub use mapping::{McpType, RouteMapping, RouteRule};
pub use parser::parse_spec;
pub use provider::{ExtractedOperation, ExtractedParameter, OpenApiProvider};

/// Prelude for common imports.
pub mod prelude {
    pub use super::{McpType, OpenApiHandler, OpenApiProvider, RouteMapping, RouteRule};
}
