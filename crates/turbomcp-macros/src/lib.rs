//! # TurboMCP Macros
//!
//! Zero-overhead procedural macros for ergonomic MCP server development.
//!
//! ## Usage
//!
//! The `#[server]` macro transforms a struct impl block into a complete MCP server
//! with automatic `McpHandler` trait implementation.
//!
//! ```ignore
//! use turbomcp::prelude::*;
//!
//! #[derive(Clone)]
//! struct Calculator;
//!
//! #[server(name = "calculator", version = "1.0.0")]
//! impl Calculator {
//!     /// Add two numbers together
//!     #[tool]
//!     async fn add(
//!         &self,
//!         #[description("First operand")] a: i64,
//!         #[description("Second operand")] b: i64,
//!     ) -> i64 {
//!         a + b
//!     }
//!
//!     /// Greet someone by name
//!     #[tool]
//!     async fn greet(
//!         &self,
//!         #[description("The name of the person to greet")] name: String,
//!     ) -> String {
//!         format!("Hello, {}!", name)
//!     }
//!
//!     /// Get application configuration
//!     #[resource("config://app")]
//!     async fn config(&self, uri: String, ctx: &RequestContext) -> String {
//!         r#"{"debug": true}"#.to_string()
//!     }
//!
//!     /// Generate a greeting prompt
//!     #[prompt]
//!     async fn greeting(&self, name: String, ctx: &RequestContext) -> String {
//!         format!("Hello {}! How can I help you today?", name)
//!     }
//! }
//!
//! #[tokio::main]
//! async fn main() {
//!     Calculator.run().await.unwrap();
//! }
//! ```
//!
//! ## Features
//!
//! - **Zero Boilerplate**: Just add `#[server]`, `#[tool]`, `#[resource]`, `#[prompt]` attributes
//! - **Automatic Schema Generation**: JSON schemas generated from Rust types
//! - **Per-Parameter Documentation**: Use `#[description("...")]` for rich JSON Schema docs
//! - **Type-Safe Parameters**: Function parameters become tool arguments
//! - **Doc Comments**: `///` comments become tool/resource/prompt descriptions
//! - **Complex Type Support**: Use `schemars::JsonSchema` for nested object schemas
//! - **Multiple Transports**: Run on STDIO, HTTP, WebSocket, TCP with `.run_*()` methods
//! - **Portable Code**: Same server works on native and WASM with platform-specific entry points

use proc_macro::TokenStream;

pub(crate) mod v3;

/// Marks an impl block as an MCP server with automatic McpHandler implementation.
///
/// This macro generates a complete `McpHandler` trait implementation by:
/// - Discovering `#[tool]`, `#[resource]`, and `#[prompt]` methods
/// - Parsing function signatures to extract parameters
/// - Extracting doc comments for descriptions
/// - Generating JSON Schema from Rust types
///
/// # Attributes
///
/// - `name = "server-name"` - Server name (defaults to struct name)
/// - `version = "1.0.0"` - Server version (defaults to "1.0.0")
/// - `description = "..."` - Server description
///
/// # Example
///
/// ```ignore
/// use turbomcp::prelude::*;
///
/// #[derive(Clone)]
/// struct MyServer;
///
/// #[server(name = "my-server", version = "1.0.0", description = "A demo server")]
/// impl MyServer {
///     /// Add two numbers
///     #[tool]
///     async fn add(&self, a: i64, b: i64) -> i64 {
///         a + b
///     }
/// }
///
/// #[tokio::main]
/// async fn main() {
///     MyServer.run_stdio().await.unwrap();
/// }
/// ```
#[proc_macro_attribute]
pub fn server(args: TokenStream, input: TokenStream) -> TokenStream {
    v3::server::generate_v3_server(args, input)
}

/// Marks a method as a tool handler within a `#[server]` block.
///
/// Tool methods are automatically discovered by the `#[server]` macro.
/// The function signature determines the tool's input schema:
/// - Parameter names become JSON property names
/// - Parameter types determine JSON schema types
/// - Doc comments become the tool description
///
/// # Supported Types
///
/// - `String`, `&str` -> JSON string
/// - `i32`, `i64`, `u32`, `u64`, `f32`, `f64` -> JSON number
/// - `bool` -> JSON boolean
/// - `Vec<T>` -> JSON array
/// - `Option<T>` -> Optional property
/// - Custom structs with serde -> JSON object
///
/// # Example
///
/// ```ignore
/// #[server]
/// impl MyServer {
///     /// Greet someone by name
///     #[tool]
///     async fn greet(&self, name: String, formal: Option<bool>) -> String {
///         let greeting = if formal.unwrap_or(false) { "Good day" } else { "Hello" };
///         format!("{}, {}!", greeting, name)
///     }
/// }
/// ```
///
/// # With Description
///
/// ```ignore
/// #[tool("Custom description for the tool")]
/// async fn my_tool(&self, arg: String) -> String {
///     // ...
/// }
/// ```
#[proc_macro_attribute]
pub fn tool(_args: TokenStream, input: TokenStream) -> TokenStream {
    // Tool attribute is processed by the #[server] macro
    // When used standalone, just pass through
    input
}

/// Marks a method as a resource handler within a `#[server]` block.
///
/// Resource methods provide access to data via URIs. The URI template
/// determines how the resource is accessed.
///
/// # URI Templates
///
/// - Static: `"config://app"` - Exact match
/// - Dynamic: `"file://{path}"` - Matches any path
///
/// # Example
///
/// ```ignore
/// #[server]
/// impl MyServer {
///     /// Get application configuration
///     #[resource("config://app")]
///     async fn config(&self, uri: String, ctx: &RequestContext) -> String {
///         r#"{"debug": true}"#.to_string()
///     }
///
///     /// Read a file by path
///     #[resource("file://{path}")]
///     async fn file(&self, uri: String, ctx: &RequestContext) -> String {
///         // uri contains the full matched URI
///         format!("Content of {}", uri)
///     }
/// }
/// ```
///
/// # With MIME Type (HIGH-001)
///
/// ```ignore
/// #[resource("config://app", mime_type = "application/json")]
/// async fn config(&self, uri: String, ctx: &RequestContext) -> String {
///     // ...
/// }
/// ```
#[proc_macro_attribute]
pub fn resource(_args: TokenStream, input: TokenStream) -> TokenStream {
    // Resource attribute is processed by the #[server] macro
    // When used standalone, just pass through
    input
}

/// Marks a method as a prompt handler within a `#[server]` block.
///
/// Prompt methods generate message templates for LLM interactions.
/// Function parameters become prompt arguments (HIGH-002).
///
/// # Example
///
/// ```ignore
/// #[server]
/// impl MyServer {
///     /// Generate a greeting prompt
///     #[prompt]
///     async fn greeting(&self, name: String, ctx: &RequestContext) -> String {
///         format!("Hello {}! How can I help you today?", name)
///     }
///
///     /// Generate a code review prompt
///     #[prompt]
///     async fn code_review(
///         &self,
///         language: String,
///         style: Option<String>,
///         ctx: &RequestContext,
///     ) -> String {
///         let style = style.unwrap_or_else(|| "concise".to_string());
///         format!("Review this {} code in a {} style", language, style)
///     }
/// }
/// ```
#[proc_macro_attribute]
pub fn prompt(_args: TokenStream, input: TokenStream) -> TokenStream {
    // Prompt attribute is processed by the #[server] macro
    // When used standalone, just pass through
    input
}

/// Provides a description for a tool parameter.
///
/// This attribute adds a description to the JSON Schema for the parameter,
/// improving discoverability and documentation for LLM clients.
///
/// # Example
///
/// ```ignore
/// #[server]
/// impl MyServer {
///     /// Search for documents
///     #[tool]
///     async fn search(
///         &self,
///         #[description("The search query string")] query: String,
///         #[description("Maximum number of results to return")] limit: Option<i32>,
///         #[description("Filter by file type (e.g., 'pdf', 'md')")] file_type: Option<String>,
///     ) -> Vec<SearchResult> {
///         // ...
///     }
/// }
/// ```
///
/// This generates JSON Schema with descriptions:
///
/// ```json
/// {
///   "type": "object",
///   "properties": {
///     "query": {
///       "type": "string",
///       "description": "The search query string"
///     },
///     "limit": {
///       "type": "integer",
///       "description": "Maximum number of results to return"
///     },
///     "file_type": {
///       "type": "string",
///       "description": "Filter by file type (e.g., 'pdf', 'md')"
///     }
///   },
///   "required": ["query"]
/// }
/// ```
///
/// # Alternative: Doc Comments
///
/// You can also use doc comments on parameters (if your Rust version supports it):
///
/// ```ignore
/// async fn search(
///     &self,
///     /// The search query string
///     query: String,
/// ) -> Vec<SearchResult>
/// ```
#[proc_macro_attribute]
pub fn description(_args: TokenStream, input: TokenStream) -> TokenStream {
    // Description attribute is processed by the #[server] macro's parameter analysis
    // When used standalone, just pass through
    input
}
