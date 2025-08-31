//! # TurboMCP Macros
//!
//! Zero-overhead procedural macros for ergonomic MCP server development, providing
//! compile-time code generation for MCP protocol handlers with graceful shutdown support.
//!
//! ## Features
//!
//! - **`#[server]`** - Convert structs into MCP servers with transport methods and graceful shutdown
//! - **`#[tool]`** - Mark methods as MCP tool handlers with automatic schema generation
//! - **`#[prompt]`** - Mark methods as MCP prompt handlers with template support
//! - **`#[resource]`** - Mark methods as MCP resource handlers with URI templates
//! - **Helper macros** - `mcp_error!`, `mcp_text!`, `tool_result!` for ergonomic content creation
//!
//! ## Usage
//!
//! ```ignore
//! use turbomcp::prelude::*;
//!
//! #[derive(Clone)]
//! struct Calculator {
//!     operations: std::sync::Arc<std::sync::atomic::AtomicU64>,
//! }
//!
//! #[server]
//! impl Calculator {
//!     #[tool("Add two numbers")]
//!     async fn add(&self, a: i32, b: i32) -> McpResult<i32> {
//!         self.operations.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
//!         Ok(a + b)
//!     }
//!     
//!     #[tool("Divide two numbers")]
//!     async fn divide(&self, a: f64, b: f64) -> McpResult<f64> {
//!         if b == 0.0 {
//!             return Err(mcp_error!("Cannot divide by zero"));
//!         }
//!         Ok(a / b)
//!     }
//! }
//! ```

use proc_macro::TokenStream;

mod completion;
mod elicitation;
mod helpers;
mod ping;
mod prompt;
mod resource;
mod schema;
mod server;
mod template;
mod tool;

/// Marks an impl block as a TurboMCP server (idiomatic Rust)
///
/// # Example
///
/// ```text
/// use turbomcp_macros::server;
///
/// struct MyServer {
///     state: String,
/// }
///
/// #[server(name = "MyServer", version = "1.0.0")]
/// impl MyServer {
///     fn new(state: String) -> Self {
///         Self { state }
///     }
///
///     fn get_state(&self) -> &str {
///         &self.state
///     }
/// }
/// ```
#[proc_macro_attribute]
pub fn server(args: TokenStream, input: TokenStream) -> TokenStream {
    // Implementation - only supports impl blocks (the correct pattern)
    match syn::parse::<syn::ItemImpl>(input) {
        Ok(item_impl) => server::generate_server_impl(args, item_impl),
        Err(_) => syn::Error::new(
            proc_macro2::Span::call_site(),
            "The #[server] attribute can only be applied to impl blocks. \
                 This is the idiomatic Rust pattern that separates data from behavior.",
        )
        .to_compile_error()
        .into(),
    }
}

/// Marks a method as a tool handler
///
/// # Example
///
/// ```ignore
/// use turbomcp_macros::tool;
///
/// struct MyServer;
///
/// impl MyServer {
///     #[tool("Add two numbers")]
///     async fn add(&self, a: i32, b: i32) -> turbomcp::McpResult<i32> {
///         Ok(a + b)
///     }
/// }
#[proc_macro_attribute]
pub fn tool(args: TokenStream, input: TokenStream) -> TokenStream {
    tool::generate_tool_impl(args, input)
}

/// Marks a method as a prompt handler
///
/// # Example
///
/// ```ignore
/// # use turbomcp_macros::prompt;
/// # struct MyServer;
/// # impl MyServer {
/// #[prompt("Generate code")]
/// async fn code_prompt(&self, language: String) -> turbomcp::McpResult<String> {
///     Ok(format!("Generated {} code", language))
/// }
/// # }
#[proc_macro_attribute]
pub fn prompt(args: TokenStream, input: TokenStream) -> TokenStream {
    prompt::generate_prompt_impl(args, input)
}

/// Marks a method as a resource handler
///
/// # Example
///
/// ```ignore
/// # use turbomcp_macros::resource;
/// # struct MyServer;
/// # impl MyServer {
/// #[resource("config://settings/{section}")]
/// async fn get_config(&self, section: String) -> turbomcp::McpResult<String> {
///     Ok(format!("Config for section: {}", section))
/// }
/// # }
#[proc_macro_attribute]
pub fn resource(args: TokenStream, input: TokenStream) -> TokenStream {
    resource::generate_resource_impl(args, input)
}

/// Helper macro for creating MCP ContentBlock structures (advanced usage)
///
/// **Note:** Most tool functions should simply return `String` using `format!()`.
/// Only use `mcp_text!()` when manually building CallToolResult structures.
///
/// # Common Usage (90% of cases) ✅
/// ```ignore
/// use turbomcp::prelude::*;
///
/// #[tool("Say hello")]
/// async fn hello(&self, name: String) -> turbomcp::McpResult<String> {
///     Ok(format!("Hello, {}!", name))  // ✅ Use format! for #[tool] returns
/// }
/// ```
///
/// # Advanced Usage (rare) ⚠️
/// ```ignore
/// # use turbomcp_macros::mcp_text;
/// let name = "world";
/// let content_block = mcp_text!("Hello, {}!", name);
/// // Use in manual CallToolResult construction
/// ```
#[proc_macro]
pub fn mcp_text(input: TokenStream) -> TokenStream {
    helpers::generate_text_content(input)
}

/// Helper macro for creating MCP errors
///
/// # Example
///
/// ```ignore
/// # use turbomcp_macros::mcp_error;
/// let error = "connection failed";
/// let result = mcp_error!("Something went wrong: {}", error);
/// ```
#[proc_macro]
pub fn mcp_error(input: TokenStream) -> TokenStream {
    helpers::generate_error(input)
}

/// Helper macro for creating CallToolResult structures (advanced usage)
///
/// **Note:** The `#[tool]` attribute automatically creates CallToolResult for you.
/// Only use `tool_result!()` when manually building responses outside of `#[tool]` functions.
///
/// # Common Usage (automatic) ✅  
/// ```ignore
/// use turbomcp::prelude::*;
///
/// #[tool("Process data")]
/// async fn process(&self, data: String) -> turbomcp::McpResult<String> {
///     Ok(format!("Processed: {}", data))  // ✅ Automatic CallToolResult creation
/// }
/// ```
///
/// # Advanced Usage (manual) ⚠️
/// ```ignore
/// # use turbomcp_macros::{tool_result, mcp_text};
/// let value = 42;
/// let text_content = mcp_text!("Result: {}", value);
/// let result = tool_result!(text_content);  // Manual CallToolResult creation
/// ```
#[proc_macro]
pub fn tool_result(input: TokenStream) -> TokenStream {
    helpers::generate_tool_result(input)
}

/// Marks a method as an elicitation handler for gathering user input
///
/// Elicitation allows servers to request structured input from clients
/// with JSON schema validation and optional default values.
///
/// # Example
///
/// ```ignore
/// # use turbomcp_macros::elicitation;
/// # struct MyServer;
/// # impl MyServer {
/// #[elicitation("Collect user preferences")]
/// async fn get_preferences(&self, schema: serde_json::Value) -> turbomcp::McpResult<serde_json::Value> {
///     // Implementation would send elicitation request to client
///     // and return the structured user input
///     Ok(serde_json::json!({"theme": "dark", "language": "en"}))
/// }
/// # }
#[proc_macro_attribute]
pub fn elicitation(args: TokenStream, input: TokenStream) -> TokenStream {
    elicitation::generate_elicitation_impl(args, input)
}

/// Marks a method as a completion handler for argument autocompletion
///
/// Completion provides intelligent suggestions for tool parameters
/// based on current context and partial input.
///
/// # Example
///
/// ```ignore
/// # use turbomcp_macros::completion;
/// # struct MyServer;
/// # impl MyServer {
/// #[completion("Complete file paths")]
/// async fn complete_file_path(&self, partial: String) -> turbomcp::McpResult<Vec<String>> {
///     // Return completion suggestions based on partial input
///     Ok(vec!["config.json".to_string(), "data.txt".to_string()])
/// }
/// # }
#[proc_macro_attribute]
pub fn completion(args: TokenStream, input: TokenStream) -> TokenStream {
    completion::generate_completion_impl(args, input)
}

/// Marks a method as a resource template handler
///
/// Resource templates use RFC 6570 URI templates for parameterized
/// resource access, enabling dynamic resource URIs.
///
/// # Example
///
/// ```ignore
/// # use turbomcp_macros::template;
/// # struct MyServer;
/// # impl MyServer {
/// #[template("users/{user_id}/profile")]
/// async fn get_user_profile(&self, user_id: String) -> turbomcp::McpResult<String> {
///     // Return resource content for the templated URI
///     Ok(format!("Profile for user: {}", user_id))
/// }
/// # }
#[proc_macro_attribute]
pub fn template(args: TokenStream, input: TokenStream) -> TokenStream {
    template::generate_template_impl(args, input)
}

/// Marks a method as a ping handler for connection health monitoring
///
/// Ping handlers enable bidirectional health checks between
/// clients and servers for connection monitoring.
///
/// # Example
///
/// ```ignore
/// # use turbomcp_macros::ping;
/// # struct MyServer;
/// # impl MyServer {
/// #[ping("Health check")]
/// async fn health_check(&self) -> turbomcp::McpResult<String> {
///     // Return health status information
///     Ok("Server is healthy".to_string())
/// }
/// # }
#[proc_macro_attribute]
pub fn ping(args: TokenStream, input: TokenStream) -> TokenStream {
    ping::generate_ping_impl(args, input)
}
