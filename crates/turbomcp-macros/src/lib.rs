//! TurboMCP v4 procedural macros.
//!
//! [`macro@server`] is the driver: applied to an `impl` block, it reads the
//! `#[tool]` / `#[resource]` / `#[prompt]` methods inside and generates the
//! capability trait impls, JSON schemas (via `schemars`), argument validation,
//! and the `into_server()` / `run_stdio()` entry points.
//!
//! `#[tool]`, `#[resource]`, `#[prompt]`, `#[completion]`, and `#[mcp_header]`
//! are markers that `#[server]` consumes. `#[server]` is an outer attribute,
//! so it always expands first and removes them; a marker macro that runs at
//! all was never consumed (it sits outside a `#[server]` impl), and it says
//! so with a compile error instead of quietly registering nothing.
//!
//! The three handler markers share one argument grammar — an optional bare
//! string (the URI on `#[resource]`, a description shorthand elsewhere) followed
//! by `key = …` pairs — and each accepts the subset that means something for
//! its kind, so `#[prompt(task)]` is a compile error naming `#[prompt]`.
//!
//! Text values (`description`, `title`, `mime_type`, and the server's `name`,
//! `version`, `instructions`, …) take any `&str` expression: a literal,
//! `env!(…)`, `concat!(…)`, `include_str!(…)`, a `const`. A component's `name`
//! stays a literal: dispatch matches on it, and the spec's naming rules are
//! checked at compile time.
//!
//! # Descriptions
//!
//! Without `description = …`, a component is described by its doc comment,
//! `#[doc = include_str!("tool.md")]` included, as a model should read it:
//! each paragraph on one line (doc comments are wrapped for editors), list
//! items and code fences kept, and rustdoc-only sections (`# Errors`,
//! `# Panics`, `# Safety`, `# Examples`) left out.
//!
//! # Inputs
//!
//! A tool's parameters become its input schema, one property each. A
//! `Params<T>` parameter makes `T` the whole input instead (destructure it:
//! `Params(search): Params<Search>`), for an argument type shared by several
//! tools or one that needs serde or schemars attributes. Prompt arguments and
//! template variables are strings on the wire and parse into any `FromStr`
//! parameter type (`times: u32`); a template variable that doesn't parse
//! names no resource, so the read is not found. A context parameter is the
//! one its marker gets (`&CallToolContext` on a `#[tool]`, …).
//!
//! # Cancellation
//!
//! When a client cancels a call (or abandons it: a timeout, an HTTP
//! disconnect), no answer is sent, and the handler is told through its
//! context's `ctx.base.cancellation` token. By default
//! (`Cancellation::Cooperative`) it then runs on to stop its own way, so its
//! cleanup runs; a handler that never looks at the token runs to completion,
//! its result discarded. `ServerBuilder::with_cancellation(Cancellation::Abort)`
//! drops handlers at their next `.await` instead, for servers whose handlers
//! are safe to stop anywhere.
//!
//! # Lifecycle
//!
//! `#[on_initialize]` marks `async fn(&self, ctx: &InitializeContext) ->
//! McpResult<()>`, run when a stateful (`2025-*`) client starts a session; an
//! error turns it away. `#[on_cancelled]` marks a plain `fn(&self, id:
//! &RequestId, reason: Option<&str>)`, told of each cancelled request. Session
//! start and end reach a `SessionObserver` (`ServerBuilder::with_session_observer`).
//!
//! # Metadata
//!
//! Every marker takes `icon("https://…")` (or `icon(src = …, mime_type = …,
//! sizes("48x48"), theme = "dark")`, repeatable) and `meta("key" = <json>)`;
//! a `#[tool]` takes `output_schema = T` for a tool that returns a
//! `CallToolResult` itself; a `#[resource]` takes `size = N`,
//! `audience("user", "assistant")` and `priority = 0.0..=1.0`.
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
/// - `name` / `version` — this server's identity; the crate's
///   (`CARGO_PKG_NAME`, `CARGO_PKG_VERSION`) when left out.
/// - `title` — a human-facing display name.
/// - `description`, `website_url`, `icon(…)` — more of its identity
///   (`2025-11-25` onwards).
/// - `instructions` — guidance returned during discovery.
/// - `protocols("…", …)` — the protocol revisions this server accepts.
///   Defaults to every version the build supports (currently `"2025-06-18"`,
///   `"2025-11-25"` and `"2026-07-28"`). All three are frozen revisions;
///   narrow this set when an application intentionally supports fewer peers:
///
///   ```ignore
///   #[server(name = "prod", version = "1.0.0", protocols("2025-11-25"))]
///   impl Prod {}
///   ```
///
///   A request naming an excluded version is refused with `-32022`
///   (`UnsupportedProtocolVersion`) and the list of versions that *are*
///   served.
///
/// The `impl` block may be generic (`impl<S: Store> App<S>`): every generated
/// impl repeats its generics and where-clause. Handlers take `&self`.
#[proc_macro_attribute]
pub fn server(attr: TokenStream, item: TokenStream) -> TokenStream {
    server::expand(attr.into(), item.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Marker: declares a method as an MCP tool. Consumed by [`macro@server`].
///
/// Accepts `#[tool]`, `#[tool("description")]`, or a list of:
/// `description = …`, `name = "…"`, `title = …`, `task` (or
/// `task = "optional"` / `task = "required"`),
/// `scopes("…", …)`, `tags("…", …)`, `icon(…)`, `meta("key" = …)`,
/// `output_schema = T`, and the behavior hints `read_only` /
/// `destructive` / `idempotent` / `open_world` (bare = true, or `= false` to
/// declare the opposite — distinct from leaving a hint unset).
///
/// `task` lets the tool run as a task (`taskSupport: optional`); `task =
/// "required"` lets it run *only* as one. On `2025-11-25` that needs
/// `ServerBuilder::with_tasks()`, and a required tool called without a task
/// is `-32601`. On `2026-07-28` the Tasks extension (`turbomcp-ext-tasks`)
/// taskifies the tool for a client that declares the extension, and refuses a
/// required tool to one that doesn't with `-32021`. Running as a task, the
/// handler reports through `ctx.task` (status message, polling interval), and
/// its `ctx.progress` reports become the task's status message. Its
/// `ctx.client` asks the user through the task, which reads
/// `input_required` until the answer comes: by `tasks/update` on
/// `2026-07-28`, and on `2025-11-25` as the response to the request the
/// server sends on the client's `tasks/result` stream (or its connection, or
/// the session's `GET` stream).
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
    unconsumed("tool", item)
}

/// Marker: declares a method as an MCP resource. Consumed by [`macro@server`].
///
/// The first argument is the URI and is required — either fixed
/// (`"config://app"`) or an RFC 6570 template (`"file://{+path}"`), in which
/// case every handler argument must name a template variable. It may be
/// followed by `description = …`, `name = "…"`, `title = …`,
/// `mime_type = …`, `tags("…", …)`, `icon(…)`, `meta("key" = …)`,
/// `audience(…)`, `priority = …`, and (on a fixed URI) `size = N`:
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
    unconsumed("resource", item)
}

/// Marker: declares a method as an MCP prompt. Consumed by [`macro@server`].
///
/// Accepts `#[prompt]`, `#[prompt("description")]`, or a list of
/// `description = …`, `name = "…"`, `title = …`, `tags("…", …)`, `icon(…)`
/// and `meta("key" = …)`. As
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
    unconsumed("prompt", item)
}

/// Marker: mirrors a tool parameter into an MCP request header (SEP-2243).
/// Consumed by [`macro@server`].
#[proc_macro_attribute]
pub fn mcp_header(_attr: TokenStream, item: TokenStream) -> TokenStream {
    unconsumed("mcp_header", item)
}

/// Marker: declares the server's `completion/complete` handler. At most one per
/// `impl`; the method takes `neutral::CompleteParams` (and an optional
/// `&CompleteContext`) and returns `McpResult<neutral::CompleteResult>`.
/// Consumed by [`macro@server`].
#[proc_macro_attribute]
pub fn completion(_attr: TokenStream, item: TokenStream) -> TokenStream {
    unconsumed("completion", item)
}

/// Marker: the server's `McpServerCore::on_initialize` hook, run when a
/// stateful (`2025-*`) client starts a session; an error refuses it.
/// `async fn(&self, ctx: &InitializeContext) -> McpResult<()>`. Consumed by
/// [`macro@server`].
#[proc_macro_attribute]
pub fn on_initialize(_attr: TokenStream, item: TokenStream) -> TokenStream {
    unconsumed("on_initialize", item)
}

/// Marker: the server's `McpServerCore::on_cancelled` hook, told when a
/// client cancels a request. A plain `fn(&self, id: &RequestId, reason:
/// Option<&str>)`. Consumed by [`macro@server`].
#[proc_macro_attribute]
pub fn on_cancelled(_attr: TokenStream, item: TokenStream) -> TokenStream {
    unconsumed("on_cancelled", item)
}

/// A marker that expanded on its own: `#[server]` would have removed it, so
/// it isn't inside a `#[server]` impl and would register nothing.
fn unconsumed(name: &str, item: TokenStream) -> TokenStream {
    let message = format!(
        "#[{name}] only works on a method inside a #[server] impl block; \
         here it would register nothing"
    );
    let item = proc_macro2::TokenStream::from(item);
    quote::quote!(::core::compile_error!(#message); #item).into()
}

// Rewrite only generated absolute paths and generated Serde/Schemars crate
// attributes. The user's impl block is never passed through this transform.
fn resolve_facade(tokens: proc_macro2::TokenStream) -> syn::Result<proc_macro2::TokenStream> {
    let name = match proc_macro_crate::crate_name("turbomcp") {
        Ok(proc_macro_crate::FoundCrate::Name(name)) => name,
        // The facade exposes `extern crate self as turbomcp`, including tests.
        Ok(proc_macro_crate::FoundCrate::Itself) => "turbomcp".to_owned(),
        Err(_) => "turbomcp".to_owned(),
    };
    fn rewrite(tokens: proc_macro2::TokenStream, name: &str) -> proc_macro2::TokenStream {
        let mut out = proc_macro2::TokenStream::new();
        let mut colons = 0;
        for token in tokens {
            let next = match token {
                proc_macro2::TokenTree::Group(group) => {
                    let mut replacement =
                        proc_macro2::Group::new(group.delimiter(), rewrite(group.stream(), name));
                    replacement.set_span(group.span());
                    replacement.into()
                }
                proc_macro2::TokenTree::Ident(ident) if colons >= 2 && ident == "turbomcp" => {
                    proc_macro2::Ident::new(name, ident.span()).into()
                }
                proc_macro2::TokenTree::Literal(lit) => {
                    if let Ok(string) = syn::parse_str::<syn::LitStr>(&lit.to_string()) {
                        let value = string.value();
                        if matches!(
                            value.as_str(),
                            "::turbomcp::__macros::serde" | "::turbomcp::__macros::schemars"
                        ) {
                            let rest = value
                                .strip_prefix("::turbomcp::")
                                .expect("known generated crate path");
                            let replacement =
                                syn::LitStr::new(&format!("::{name}::{rest}"), string.span());
                            out.extend(quote::quote!(#replacement));
                            colons = 0;
                            continue;
                        }
                    }
                    lit.into()
                }
                other => other,
            };
            colons = if matches!(&next, proc_macro2::TokenTree::Punct(p) if p.as_char() == ':') {
                colons + 1
            } else {
                0
            };
            out.extend([next]);
        }
        out
    }
    Ok(rewrite(tokens, &name))
}
