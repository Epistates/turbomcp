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
//!     Calculator.run_stdio().await.unwrap();
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

mod schema;
mod server;
mod tool;

/// Marks an impl block as an MCP server with automatic McpHandler implementation.
///
/// This macro generates a complete `McpHandler` trait implementation by:
/// - Discovering `#[tool]`, `#[resource]`, and `#[prompt]` methods
/// - Parsing function signatures to extract parameters
/// - Extracting doc comments for descriptions
/// - Generating JSON Schema from Rust types
/// - Deriving the advertised capabilities from what the impl block declares
///
/// # Attributes
///
/// - `name = "server-name"` - Server name (defaults to struct name)
/// - `version = "1.0.0"` - Server version (defaults to "1.0.0")
/// - `description = "..."` - What this implementation *is*
/// - `title = "..."` - Human-readable display name (SEP-973)
/// - `instructions = "..."` - How to *use* this server. Returned as the
///   `initialize` result's `instructions` field, which clients may hand to the
///   model much like a system prompt.
/// - `website_url = "..."` - Homepage for this implementation
/// - `icons = ["https://…/icon.png"]` - Icon sources (SEP-973)
///
/// Every value is an expression, not just a string literal, so server identity
/// can come from the build or the environment. Unknown keys are a compile
/// error rather than a silent no-op.
///
/// # Example
///
/// ```ignore
/// use turbomcp::prelude::*;
///
/// #[derive(Clone)]
/// struct MyServer;
///
/// #[server(
///     name = "my-server",
///     version = env!("CARGO_PKG_VERSION"),
///     description = "A demo server",
///     instructions = "Call `add` for arithmetic. Values are i64."
/// )]
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
///
/// # Handler errors
///
/// A handler whose return type names `McpError` (`McpResult<T>` or
/// `Result<T, McpError>`) gets error-kind-aware dispatch:
///
/// - **Tools** report the failure as a tool execution error (`isError: true`,
///   per SEP-1303, so the model can self-correct) and preserve the
///   classification in `_meta` under `io.turbomcp/errorKind` and
///   `io.turbomcp/errorCode`, plus `io.turbomcp/errorData` when the error
///   carries [`McpError::with_data`](turbomcp_core::error::McpError::with_data).
/// - **Prompts and resources** propagate the error as a JSON-RPC error. A
///   failed render is not a successful one whose text begins "Error:".
///
/// Handlers returning other types keep the plain `Display` conversion, which
/// has no kind to preserve.
///
/// # Optional handlers
///
/// Beyond `#[tool]`, `#[resource]`, and `#[prompt]`, four markers opt into the
/// MCP methods that are optional for a server:
///
/// | Marker | Serves | Capability advertised |
/// |---|---|---|
/// | [`#[completion]`](macro@completion) | `completion/complete` | `completions` |
/// | [`#[subscribe]`](macro@subscribe) | `resources/subscribe` | `resources.subscribe` |
/// | [`#[unsubscribe]`](macro@unsubscribe) | `resources/unsubscribe` | — |
/// | [`#[set_level]`](macro@set_level) | `logging/setLevel` | `logging` |
///
/// Each may appear at most once. Omitting one leaves the trait default, which
/// answers `capability_not_supported`, and the capability stays unadvertised —
/// so what `initialize` claims always matches what the server can serve.
///
/// ```ignore
/// #[server(name = "docs", version = "1.0.0")]
/// impl Docs {
///     #[tool]
///     async fn search(&self, query: String) -> McpResult<String> { /* ... */ }
///
///     #[completion]
///     async fn complete(&self, params: serde_json::Value) -> McpResult<serde_json::Value> {
///         // ...
///     }
///
///     #[subscribe]
///     async fn watch(&self, uri: String, ctx: &RequestContext) -> McpResult<()> {
///         // ...
///     }
/// }
/// ```
///
/// A trailing `ctx: &RequestContext` is optional on all four.
#[proc_macro_attribute]
pub fn server(args: TokenStream, input: TokenStream) -> TokenStream {
    server::generate_server(args, input)
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
///
/// # Cancellation
///
/// Per MCP §Cancellation, a client may send `notifications/cancelled` to
/// abandon an in-flight request. The transport layer signals the matching
/// handler via a `tokio_util::sync::CancellationToken` installed on the
/// `RequestContext`, but cancellation is **cooperative**: the handler must
/// poll `ctx.is_cancelled()` (or `await` on a cancellable future) to honour
/// it. A handler doing pure synchronous CPU work, or holding an `await`
/// inside a non-cancellable future, will run to completion regardless.
///
/// Long-running tools should accept `ctx: &RequestContext` and check
/// cancellation at natural break points:
///
/// ```ignore
/// #[tool]
/// async fn long_task(&self, ctx: &RequestContext, n: u64) -> McpResult<u64> {
///     let mut acc = 0u64;
///     for i in 0..n {
///         if ctx.is_cancelled() {
///             return Err(McpError::cancelled("task cancelled by client"));
///         }
///         acc = acc.wrapping_add(i);
///     }
///     Ok(acc)
/// }
/// ```
#[proc_macro_attribute]
pub fn tool(_args: TokenStream, input: TokenStream) -> TokenStream {
    // Tool attribute must be used within a #[server] impl block
    // When used standalone, emit a compile error with proper span
    if let Ok(func) = syn::parse::<syn::ItemFn>(input.clone()) {
        syn::Error::new(
            func.sig.ident.span(),
            "#[tool] must be used within a #[server] impl block. \
             The #[server] macro discovers tools by scanning impl blocks.\n\n\
             Example:\n\
             \n\
             #[derive(Clone)]\n\
             struct MyServer;\n\
             \n\
             #[server(name = \"my-server\", version = \"1.0.0\")]\n\
             impl MyServer {\n\
                 #[tool]\n\
                 async fn my_tool(&self, arg: String) -> String {\n\
                     // ...\n\
                 }\n\
             }",
        )
        .to_compile_error()
        .into()
    } else {
        // Fallback for non-function items
        let input2 = proc_macro2::TokenStream::from(input);
        syn::Error::new_spanned(
            &input2,
            "#[tool] must be used within a #[server] impl block.",
        )
        .to_compile_error()
        .into()
    }
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
    // Resource attribute must be used within a #[server] impl block
    // When used standalone, emit a compile error with proper span
    if let Ok(func) = syn::parse::<syn::ItemFn>(input.clone()) {
        syn::Error::new(
            func.sig.ident.span(),
            "#[resource] must be used within a #[server] impl block. \
             The #[server] macro discovers resources by scanning impl blocks.\n\n\
             Example:\n\
             \n\
             #[derive(Clone)]\n\
             struct MyServer;\n\
             \n\
             #[server(name = \"my-server\", version = \"1.0.0\")]\n\
             impl MyServer {\n\
                 #[resource(\"config://app\")]\n\
                 async fn config(&self, uri: String, ctx: &RequestContext) -> String {\n\
                     // ...\n\
                 }\n\
             }",
        )
        .to_compile_error()
        .into()
    } else {
        // Fallback for non-function items
        let input2 = proc_macro2::TokenStream::from(input);
        syn::Error::new_spanned(
            &input2,
            "#[resource] must be used within a #[server] impl block.",
        )
        .to_compile_error()
        .into()
    }
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
    // Prompt attribute must be used within a #[server] impl block
    // When used standalone, emit a compile error with proper span
    if let Ok(func) = syn::parse::<syn::ItemFn>(input.clone()) {
        syn::Error::new(
            func.sig.ident.span(),
            "#[prompt] must be used within a #[server] impl block. \
             The #[server] macro discovers prompts by scanning impl blocks.\n\n\
             Example:\n\
             \n\
             #[derive(Clone)]\n\
             struct MyServer;\n\
             \n\
             #[server(name = \"my-server\", version = \"1.0.0\")]\n\
             impl MyServer {\n\
                 #[prompt]\n\
                 async fn greeting(&self, name: String, ctx: &RequestContext) -> String {\n\
                     // ...\n\
                 }\n\
             }",
        )
        .to_compile_error()
        .into()
    } else {
        // Fallback for non-function items
        let input2 = proc_macro2::TokenStream::from(input);
        syn::Error::new_spanned(
            &input2,
            "#[prompt] must be used within a #[server] impl block.",
        )
        .to_compile_error()
        .into()
    }
}

/// Marks a method as the server's argument-completion handler.
///
/// Answers `completion/complete`, which clients call to autocomplete a prompt
/// argument or a resource-template variable. Declaring it makes `#[server]`
/// advertise the `completions` capability during initialization.
///
/// At most one `#[completion]` method may exist per server.
///
/// # Signature
///
/// ```ignore
/// #[completion]
/// async fn complete(
///     &self,
///     params: serde_json::Value,
///     ctx: &RequestContext,
/// ) -> McpResult<serde_json::Value>
/// ```
///
/// `params` is the raw `CompleteRequestParams` shape — `{ ref, argument,
/// context? }` — and the return value is the `CompleteResult` shape,
/// `{ completion: { values, total?, hasMore? } }`. The `ctx` parameter is
/// optional and may be omitted.
///
/// # Example
///
/// ```ignore
/// #[server(name = "docs", version = "1.0.0")]
/// impl Docs {
///     #[completion]
///     async fn complete(&self, params: serde_json::Value) -> McpResult<serde_json::Value> {
///         let prefix = params["argument"]["value"].as_str().unwrap_or("");
///         let values: Vec<&str> = ["rust", "ruby", "racket"]
///             .into_iter()
///             .filter(|lang| lang.starts_with(prefix))
///             .collect();
///         Ok(serde_json::json!({ "completion": { "values": values } }))
///     }
/// }
/// ```
#[proc_macro_attribute]
pub fn completion(_args: TokenStream, input: TokenStream) -> TokenStream {
    marker_outside_server("completion", input)
}

/// Marks a method as the handler for `resources/subscribe`.
///
/// Declaring it makes `#[server]` advertise `resources.subscribe`, committing
/// the server to sending `notifications/resources/updated` when a subscribed
/// resource changes. Emit those with
/// `ctx.notify_client("notifications/resources/updated", ...)`.
///
/// At most one `#[subscribe]` method may exist per server.
///
/// # Signature
///
/// ```ignore
/// #[subscribe]
/// async fn subscribe(&self, uri: String, ctx: &RequestContext) -> McpResult<()>
/// ```
///
/// The `ctx` parameter is optional and may be omitted.
#[proc_macro_attribute]
pub fn subscribe(_args: TokenStream, input: TokenStream) -> TokenStream {
    marker_outside_server("subscribe", input)
}

/// Marks a method as the handler for `resources/unsubscribe`.
///
/// Pairs with [`macro@subscribe`]. A server that declares `#[subscribe]` should
/// declare this too, so clients can cancel what they started.
///
/// # Signature
///
/// ```ignore
/// #[unsubscribe]
/// async fn unsubscribe(&self, uri: String, ctx: &RequestContext) -> McpResult<()>
/// ```
#[proc_macro_attribute]
pub fn unsubscribe(_args: TokenStream, input: TokenStream) -> TokenStream {
    marker_outside_server("unsubscribe", input)
}

/// Marks a method as the handler for `logging/setLevel`.
///
/// Declaring it makes `#[server]` advertise the `logging` capability. The level
/// is the raw spec string: `debug`, `info`, `notice`, `warning`, `error`,
/// `critical`, `alert`, or `emergency`. Persist it and use it to filter the
/// `notifications/message` your server emits.
///
/// At most one `#[set_level]` method may exist per server.
///
/// # Signature
///
/// ```ignore
/// #[set_level]
/// async fn set_level(&self, level: String, ctx: &RequestContext) -> McpResult<()>
/// ```
#[proc_macro_attribute]
pub fn set_level(_args: TokenStream, input: TokenStream) -> TokenStream {
    marker_outside_server("set_level", input)
}

/// Shared diagnostic for the marker attributes that only mean something inside
/// a `#[server]` impl block. They are inert there (the `#[server]` expansion
/// strips them), so reaching the macro body at all means it was used standalone.
fn marker_outside_server(name: &str, input: TokenStream) -> TokenStream {
    let message = format!(
        "#[{name}] must be used within a #[server] impl block. \
         The #[server] macro discovers handlers by scanning impl blocks.\n\n\
         Example:\n\
         \n\
         #[server(name = \"my-server\", version = \"1.0.0\")]\n\
         impl MyServer {{\n\
             #[{name}]\n\
             async fn handler(&self, /* ... */) -> McpResult<()> {{\n\
                 // ...\n\
             }}\n\
         }}"
    );

    if let Ok(func) = syn::parse::<syn::ItemFn>(input.clone()) {
        syn::Error::new(func.sig.ident.span(), message)
            .to_compile_error()
            .into()
    } else {
        let input2 = proc_macro2::TokenStream::from(input);
        syn::Error::new_spanned(&input2, message)
            .to_compile_error()
            .into()
    }
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
    // Description attribute must be used on parameters within a #[tool] method
    // When used standalone, emit a compile error
    let error = quote::quote! {
        compile_error!(
            "#[description] attribute can only be used on parameters within a #[tool] method\n\n\
            Example:\n\
            \n\
            #[server(name = \"my-server\", version = \"1.0.0\")]\n\
            impl MyServer {\n\
                #[tool]\n\
                async fn search(\n\
                    &self,\n\
                    #[description(\"The search query string\")] query: String,\n\
                ) -> Vec<SearchResult> {\n\
                    // ...\n\
                }\n\
            }"
        );
    };

    // Also pass through the original input to avoid cascading errors
    let input_tokens = proc_macro2::TokenStream::from(input);
    let combined = quote::quote! {
        #error
        #input_tokens
    };
    combined.into()
}
