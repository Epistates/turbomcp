//! # TurboMCP WASM Macros
//!
//! Zero-boilerplate procedural macros for building MCP servers in WASM environments
//! like Cloudflare Workers, Deno Deploy, and other edge platforms.
//!
//! ## Features
//!
//! - **`#[wasm_server]`** - Transform impl blocks into MCP servers
//! - **`#[tool]`** - Mark methods as MCP tool handlers
//! - **`#[resource]`** - Mark methods as MCP resource handlers
//! - **`#[prompt]`** - Mark methods as MCP prompt handlers
//!
//! ## Example
//!
//! ```ignore
//! use turbomcp_wasm::prelude::*;
//! use serde::Deserialize;
//!
//! #[derive(Clone)]
//! struct MyServer {
//!     greeting: String,
//! }
//!
//! #[derive(Deserialize, schemars::JsonSchema)]
//! struct GreetArgs {
//!     name: String,
//! }
//!
//! #[wasm_server(name = "my-server", version = "1.0.0")]
//! impl MyServer {
//!     #[tool("Greet someone by name")]
//!     async fn greet(&self, args: GreetArgs) -> String {
//!         format!("{}, {}!", self.greeting, args.name)
//!     }
//!
//!     #[tool("Get server status")]
//!     async fn status(&self) -> String {
//!         "Server is running".to_string()
//!     }
//!
//!     #[resource("config://app")]
//!     async fn config(&self, uri: String) -> ResourceResult {
//!         ResourceResult::text(&uri, r#"{"theme": "dark"}"#)
//!     }
//!
//!     #[prompt("Default greeting")]
//!     async fn greeting_prompt(&self) -> PromptResult {
//!         PromptResult::user("Hello! How can I help?")
//!     }
//! }
//!
//! // Generated method:
//! // impl MyServer {
//! //     pub fn into_mcp_server(self) -> McpServer { ... }
//! // }
//! ```

use proc_macro::TokenStream;
use syn::{ItemImpl, parse_macro_input};

mod server;

/// Marks an impl block as a WASM MCP server.
///
/// This macro transforms an impl block with `#[tool]`, `#[resource]`, and `#[prompt]`
/// attributes into a fully-functional MCP server using the builder pattern.
///
/// # Attributes
///
/// - `name` - Server name (required)
/// - `version` - Server version (default: "1.0.0")
/// - `description` - Server description (optional)
///
/// # Example
///
/// ```ignore
/// #[derive(Clone)]
/// struct Calculator;
///
/// #[wasm_server(name = "calculator", version = "2.0.0")]
/// impl Calculator {
///     #[tool("Add two numbers")]
///     async fn add(&self, args: AddArgs) -> i64 {
///         args.a + args.b
///     }
/// }
///
/// // Use it:
/// let server = Calculator.into_mcp_server();
/// ```
#[proc_macro_attribute]
pub fn wasm_server(args: TokenStream, input: TokenStream) -> TokenStream {
    let args = parse_macro_input!(args as server::ServerArgs);
    let input = parse_macro_input!(input as ItemImpl);

    server::generate_wasm_server(args, input)
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}

/// Marks a method as an MCP tool handler.
///
/// This attribute is used inside a `#[wasm_server]` impl block to register
/// tool handlers. The macro extracts the description and generates appropriate
/// builder registration code.
///
/// # Arguments
///
/// - A string literal description of the tool
///
/// # Method Signature
///
/// Tool methods can have various signatures:
/// - `async fn name(&self, args: Args) -> T` - With typed arguments
/// - `async fn name(&self) -> T` - No arguments
/// - Return type `T` must implement `IntoToolResponse`
///
/// # Example
///
/// ```ignore
/// #[tool("Add two numbers together")]
/// async fn add(&self, args: AddArgs) -> i64 {
///     args.a + args.b
/// }
///
/// #[tool("Get current time")]
/// async fn now(&self) -> String {
///     "2024-01-01T00:00:00Z".to_string()
/// }
/// ```
#[proc_macro_attribute]
pub fn tool(_args: TokenStream, input: TokenStream) -> TokenStream {
    // This is a marker attribute - the actual processing happens in #[wasm_server]
    // We just pass through the input unchanged
    input
}

/// Marks a method as an MCP resource handler.
///
/// This attribute is used inside a `#[wasm_server]` impl block to register
/// resource handlers with URI templates.
///
/// # Arguments
///
/// - A string literal URI or URI template (e.g., "config://app" or "file://{path}")
///
/// # Method Signature
///
/// Resource methods must have the signature:
/// - `async fn name(&self, uri: String) -> ResourceResult`
/// - Or `async fn name(&self, uri: String) -> Result<ResourceResult, E>`
///
/// # Example
///
/// ```ignore
/// #[resource("config://app")]
/// async fn read_config(&self, uri: String) -> ResourceResult {
///     ResourceResult::text(&uri, r#"{"theme": "dark"}"#)
/// }
///
/// #[resource("file://{path}")]
/// async fn read_file(&self, uri: String) -> Result<ResourceResult, ToolError> {
///     // Extract path from URI and read file
///     Ok(ResourceResult::text(&uri, "file contents"))
/// }
/// ```
#[proc_macro_attribute]
pub fn resource(_args: TokenStream, input: TokenStream) -> TokenStream {
    // Marker attribute - processing happens in #[wasm_server]
    input
}

/// Marks a method as an MCP prompt handler.
///
/// This attribute is used inside a `#[wasm_server]` impl block to register
/// prompt handlers.
///
/// # Arguments
///
/// - A string literal description of the prompt
///
/// # Method Signature
///
/// Prompt methods can have these signatures:
/// - `async fn name(&self) -> PromptResult` - No arguments
/// - `async fn name(&self, args: Option<Args>) -> PromptResult` - With optional arguments
///
/// # Example
///
/// ```ignore
/// #[prompt("Default greeting prompt")]
/// async fn greeting(&self) -> PromptResult {
///     PromptResult::user("Hello! How can I help you today?")
/// }
///
/// #[prompt("Code review prompt")]
/// async fn review(&self, args: Option<ReviewArgs>) -> PromptResult {
///     let lang = args.map(|a| a.language).unwrap_or("rust".into());
///     PromptResult::user(format!("Review this {} code:", lang))
/// }
/// ```
#[proc_macro_attribute]
pub fn prompt(_args: TokenStream, input: TokenStream) -> TokenStream {
    // Marker attribute - processing happens in #[wasm_server]
    input
}
