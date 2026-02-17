//! Typed middleware for MCP request processing.
//!
//! This module provides a middleware system with typed hooks for each MCP operation.
//! Middleware can intercept, modify, or short-circuit requests at each stage.
//!
//! # Example
//!
//! ```rust,ignore
//! use turbomcp_server::middleware::{McpMiddleware, Next};
//!
//! struct LoggingMiddleware;
//!
//! impl McpMiddleware for LoggingMiddleware {
//!     async fn on_call_tool(
//!         &self,
//!         name: &str,
//!         args: serde_json::Value,
//!         ctx: &RequestContext,
//!         next: Next<'_>,
//!     ) -> McpResult<ToolResult> {
//!         println!("Calling tool: {}", name);
//!         let result = next.call_tool(name, args, ctx).await;
//!         println!("Tool result: {:?}", result.is_ok());
//!         result
//!     }
//! }
//! ```

pub mod typed;

pub use typed::{McpMiddleware, MiddlewareStack, Next};
