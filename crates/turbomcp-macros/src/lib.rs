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
///
/// # Arguments
///
/// - `name` / `version` (required) — this server's identity.
/// - `title` — a human-facing display name.
/// - `instructions` — guidance returned during discovery.
/// - `protocols("…", …)` — the protocol revisions this server accepts.
///   Defaults to every version the build supports (currently `"2025-11-25"`
///   and the `"2026-07-28"` draft). Narrow it to pin a server to the frozen
///   stable revision — the draft's wire shapes can still change before it
///   freezes:
///
///   ```ignore
///   #[server(name = "prod", version = "1.0.0", protocols("2025-11-25"))]
///   impl Prod {}
///   ```
///
///   A request naming an excluded version is refused with `-32004` and the
///   list of versions that *are* served.
///
/// The `impl` block must be for a concrete type; generic `impl` blocks are
/// rejected, since the generated trait impls name one type.
#[proc_macro_attribute]
pub fn server(attr: TokenStream, item: TokenStream) -> TokenStream {
    server::expand(attr.into(), item.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Marker: declares a method as an MCP tool. Consumed by [`macro@server`].
///
/// Accepts `#[tool]`, `#[tool("description")]`, or a list of:
/// `description = "…"`, `name = "…"`, `title = "…"`, `task`,
/// `scopes("…", …)`, and the behavior hints `read_only` / `destructive` /
/// `idempotent` / `open_world` (bare = true, or `= false` to declare the
/// opposite — distinct from leaving a hint unset).
///
/// `name` sets the name the tool answers to on the wire, which otherwise
/// defaults to the Rust method name. Set it when the two should not be
/// coupled — renaming a Rust method is otherwise a breaking change for every
/// client — or when the wire name isn't a valid Rust identifier:
///
/// ```ignore
/// #[tool(name = "search.web", description = "Search the web")]
/// async fn search_web(&self, q: String) -> String { todo!() }
/// ```
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
///
/// Accepts `#[prompt]`, `#[prompt("description")]`, or a list of
/// `description = "…"` and `name = "…"`. As with `#[tool]`, `name` decouples
/// the wire name from the Rust method name.
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
