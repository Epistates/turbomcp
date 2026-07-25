//! TurboMCP v4 procedural macros.
//!
//! [`macro@server`] is the driver: applied to an `impl` block, it reads the
//! `#[tool]` / `#[resource]` / `#[prompt]` methods inside and generates the
//! capability trait impls, JSON schemas (via `schemars`), argument validation,
//! and the `into_server()` / `run_stdio()` entry points.
//!
//! `#[tool]`, `#[resource]`, `#[prompt]`, `#[completion]`, and `#[mcp_header]`
//! are inert markers: `#[server]` consumes them. They are defined as pass-through
//! attribute macros only so the names resolve and tooling recognizes them.
#![forbid(unsafe_code)]
#![warn(missing_docs)]

use proc_macro::TokenStream;

mod server;

/// Generate an MCP server from an `impl` block. See the crate docs.
///
/// ```ignore
/// #[server(name = "my-server", version = "1.0.0")]
/// impl MyServer {
///     #[tool]
///     async fn greet(&self, name: String) -> McpResult<String> { Ok(format!("hi {name}")) }
/// }
/// ```
#[proc_macro_attribute]
pub fn server(attr: TokenStream, item: TokenStream) -> TokenStream {
    server::expand(attr.into(), item.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Marker: declares a method as an MCP tool. Consumed by [`macro@server`].
#[proc_macro_attribute]
pub fn tool(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}

/// Marker: declares a method as an MCP resource (the argument is its URI).
/// Consumed by [`macro@server`].
#[proc_macro_attribute]
pub fn resource(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}

/// Marker: declares a method as an MCP prompt. Consumed by [`macro@server`].
#[proc_macro_attribute]
pub fn prompt(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}

/// Marker: mirrors a tool parameter into an MCP request header (SEP-2243).
/// Consumed by [`macro@server`].
#[proc_macro_attribute]
pub fn mcp_header(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}

/// Marker: declares the server's `completion/complete` handler. At most one per
/// `impl`; the method takes `neutral::CompleteParams` (and an optional
/// `&CompleteContext`) and returns `McpResult<neutral::CompleteResult>`.
/// Consumed by [`macro@server`].
#[proc_macro_attribute]
pub fn completion(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}
