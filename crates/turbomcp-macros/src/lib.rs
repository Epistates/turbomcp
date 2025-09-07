//! # TurboMCP Macros
//!
//! Zero-overhead procedural macros for ergonomic MCP server development, providing
//! compile-time code generation for MCP protocol handlers with graceful shutdown support.
//!
//! ## Features
//!
//! ### Core Macros
//! - **`#[server]`** - Convert structs into MCP servers with transport methods and graceful shutdown
//! - **`#[tool]`** - Mark methods as MCP tool handlers with automatic schema generation
//! - **`#[prompt]`** - Mark methods as MCP prompt handlers with template support
//! - **`#[resource]`** - Mark methods as MCP resource handlers with URI templates
//!
//! ### Advanced Features (Enhanced in 1.0.3)
//! - **Roots Configuration** - Declarative filesystem roots in `#[server]` macro: `root = "file:///path:Name"`
//! - **Compile-Time Routing** - Zero-cost compile-time router generation (experimental)
//! - **Enhanced Context System** - Improved async handling and error propagation
//! - **Server Attributes** - Support for name, version, description, and roots in server macro
//!
//! ### Helper Macros
//! - **`mcp_error!`** - Ergonomic error creation with formatting
//! - **`mcp_text!`** - Text content creation helpers
//! - **`tool_result!`** - Tool result formatting
//! - **`elicit!`** - High-level elicitation macro for interactive user input
//!
//! ## Usage
//!
//! ### Basic Server with Tools
//!
//! ```ignore
//! use turbomcp::prelude::*;
//!
//! #[derive(Clone)]
//! struct Calculator {
//!     operations: std::sync::Arc<std::sync::atomic::AtomicU64>,
//! }
//!
//! #[server(
//!     name = "calculator-server",
//!     version = "1.0.0",
//!     description = "A mathematical calculator service",
//!     root = "file:///workspace:Project Workspace",
//!     root = "file:///tmp:Temporary Files"
//! )]
//! impl Calculator {
//!     #[tool("Add two numbers")]
//!     async fn add(&self, ctx: Context, a: i32, b: i32) -> McpResult<i32> {
//!         ctx.info(&format!("Adding {} + {}", a, b)).await?;
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
//!
//!     #[resource("calc://history/{operation}")]
//!     async fn history(&self, operation: String) -> McpResult<String> {
//!         Ok(format!("History for {} operations", operation))
//!     }
//!
//!     #[prompt("Generate report for {operation} with {count} operations")]
//!     async fn report(&self, operation: String, count: i32) -> McpResult<String> {
//!         Ok(format!("Generated report for {} ({} operations)", operation, count))
//!     }
//! }
//! ```
//!
//! ### Elicitation Support (New in 1.0.3)
//!
//! ```ignore
//! use turbomcp::prelude::*;
//! use turbomcp::elicitation_api::{string, boolean, ElicitationResult};
//!
//! #[derive(Clone)]
//! struct InteractiveServer;
//!
//! #[server]
//! impl InteractiveServer {
//!     #[tool("Configure with user input")]
//!     async fn configure(&self, ctx: Context) -> McpResult<String> {
//!         let result = elicit!("Configure your preferences")
//!             .field("theme", string()
//!                 .enum_values(vec!["light", "dark"])
//!                 .build())
//!             .field("auto_save", boolean()
//!                 .description("Enable auto-save")
//!                 .build())
//!             .send(&ctx.request)
//!             .await?;
//!
//!         match result {
//!             ElicitationResult::Accept(data) => {
//!                 let theme = data.get::<String>("theme")?;
//!                 Ok(format!("Configured with {} theme", theme))
//!             }
//!             _ => Err(mcp_error!("Configuration cancelled"))
//!         }
//!     }
//! }
//! ```

use proc_macro::TokenStream;

mod attrs;
mod compile_time_router;
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

/// Ergonomic elicitation macro for server-initiated user input
///
/// This macro provides a simple way to request structured input from the client
/// with automatic error handling and context integration.
///
/// # Usage Patterns
///
/// ## Simple Prompt (No Schema)
/// ```ignore
/// use turbomcp::prelude::*;
///
/// // Simple yes/no or text prompt
/// let result = elicit!(ctx, "Continue with deployment?").await?;
/// ```
///
/// ## With Schema Validation
/// ```ignore
/// use turbomcp::prelude::*;
/// use turbomcp_protocol::elicitation::ElicitationSchema;
///
/// let schema = ElicitationSchema::new()
///     .add_string_property("theme", Some("Color theme"))
///     .add_boolean_property("notifications", Some("Enable notifications"));
///
/// let result = elicit!(ctx, "Configure your preferences", schema).await?;
/// ```
///
/// # Arguments
///
/// * `ctx` - The context object (RequestContext with server capabilities)
/// * `message` - The message to display to the user
/// * `schema` - (Optional) The elicitation schema defining expected input
///
/// # Returns
///
/// Returns `Result<ElicitationResult>` which can be:
/// - `ElicitationResult::Accept(data)` - User provided input
/// - `ElicitationResult::Decline(reason)` - User declined
/// - `ElicitationResult::Cancel` - User cancelled
///
/// # When to Use
///
/// Use the macro for:
/// - Simple prompts without complex schemas
/// - Quick confirmation dialogs
/// - Reduced boilerplate in tool handlers
///
/// Use the function API for:
/// - Complex schemas with multiple fields
/// - Reusable elicitation builders
/// - Maximum control over schema construction
///
#[proc_macro]
pub fn elicit(input: TokenStream) -> TokenStream {
    helpers::generate_elicitation(input)
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
