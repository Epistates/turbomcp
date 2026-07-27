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
//!
//! The three handler markers share one argument grammar — an optional bare
//! string (the URI on `#[resource]`, a description shorthand elsewhere) followed
//! by `key = "…"` pairs — and each accepts the subset that means something for
//! its kind, so `#[prompt(task)]` is a compile error naming `#[prompt]`.
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
/// `scopes("…", …)`, `tags("…", …)`, and the behavior hints `read_only` /
/// `destructive` / `idempotent` / `open_world` (bare = true, or `= false` to
/// declare the opposite — distinct from leaving a hint unset).
///
/// `tags` categorizes the tool for catalog policy and is *not* a security
/// boundary — use `scopes` for that. See [`macro@prompt`] for the shared
/// details.
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
///
/// Either way the resulting name is checked against the spec's rules for tool
/// names (1–128 characters of ASCII letters, digits, `_`, `-`, and `.`) and
/// must be unique within the server.
#[proc_macro_attribute]
pub fn tool(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}

/// Marker: declares a method as an MCP resource. Consumed by [`macro@server`].
///
/// The first argument is the URI and is required — either fixed
/// (`"config://app"`) or an RFC 6570 template (`"file://{+path}"`), in which
/// case every handler argument must name a template variable. It may be
/// followed by `description = "…"`, `name = "…"`, `title = "…"`,
/// `mime_type = "…"`, and `tags("…", …)`:
///
/// ```ignore
/// #[resource(
///     "config://app",
///     name = "app-config",
///     title = "Application configuration",
///     mime_type = "application/json",
/// )]
/// async fn config(&self) -> McpResult<String> { todo!() }
/// ```
///
/// `name` is the resource's programmatic identifier (a client falls back to it
/// for display when there is no `title`); it defaults to the Rust method name.
/// Resources are addressed by URI, so it is the URI — not the name — that must
/// be unique within the server.
#[proc_macro_attribute]
pub fn resource(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}

/// Marker: declares a method as an MCP prompt. Consumed by [`macro@server`].
///
/// Accepts `#[prompt]`, `#[prompt("description")]`, or a list of
/// `description = "…"`, `name = "…"`, `title = "…"`, and `tags("…", …)`. As
/// with `#[tool]`, `name` decouples the wire name from the Rust method name and
/// must be unique within the server.
///
/// # Tags
///
/// `tags("…", …)` — accepted by all three markers — categorizes a component so
/// a catalog policy can decide who is offered it (`admin`, `experimental`,
/// `readonly`, whatever the deployment means). Tags are carried in the
/// component's `_meta` under `io.turbomcp/tags`, so they survive every protocol
/// revision and a mounted sub-server's components carry their own — read them
/// back with `turbomcp::tags`.
///
/// They are **descriptive, not enforcing**: a tag hides nothing and permits
/// nothing by itself. `#[tool(scopes(…))]` is the authorization mechanism.
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
