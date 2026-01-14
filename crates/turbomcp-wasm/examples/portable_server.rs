//! Portable MCP Server Example
//!
//! This example demonstrates the unified handler architecture that allows you to
//! write MCP servers once and run them on both WASM (Cloudflare Workers, Deno Deploy)
//! and native (stdio, HTTP, TCP) backends.
//!
//! # Key Features
//!
//! - Single server implementation with `#[server]` macro
//! - Handlers use `IntoToolResponse` for ergonomic returns
//! - Argument structs use `schemars::JsonSchema` for schema generation
//! - Two generated methods:
//!   - `into_mcp_server()` - WASM backend (Cloudflare Workers, etc.)
//!   - `into_native_server()` - Native backend (stdio, HTTP, TCP, etc.)
//!
//! # Running this example
//!
//! Native (stdio):
//! ```sh
//! cargo run --example portable_server -p turbomcp-wasm --features native
//! ```
//!
//! # WASM deployment (Cloudflare Workers)
//!
//! For WASM deployment, you would create a separate entry point:
//! ```ignore
//! #[event(fetch)]
//! async fn fetch(req: Request, _env: Env, _ctx: worker::Context) -> Result<Response> {
//!     PortableServer::new("Hello".into())
//!         .into_mcp_server()
//!         .handle(req)
//!         .await
//! }
//! ```

use serde::Deserialize;
use turbomcp_wasm::prelude::*;

/// A portable MCP server that works on both WASM and native backends.
#[derive(Clone)]
struct PortableServer {
    greeting: String,
}

impl PortableServer {
    fn new(greeting: String) -> Self {
        Self { greeting }
    }
}

/// Arguments for the greet tool
#[derive(Deserialize, schemars::JsonSchema)]
struct GreetArgs {
    /// The name of the person to greet
    name: String,
}

/// Arguments for the add tool
#[derive(Deserialize, schemars::JsonSchema)]
struct AddArgs {
    /// First number
    a: i64,
    /// Second number
    b: i64,
}

/// Arguments for the echo tool
#[derive(Deserialize, schemars::JsonSchema)]
struct EchoArgs {
    /// Message to echo back
    message: String,
    /// Number of times to repeat (optional)
    times: Option<u32>,
}

// The #[server] macro generates both:
// - `into_mcp_server()` for WASM backend (always)
// - `into_native_server()` for native backend (when `native` feature is enabled)
#[server(
    name = "portable-server",
    version = "1.0.0",
    description = "A portable MCP server example"
)]
impl PortableServer {
    /// Greet someone by name
    #[tool("Greet someone with a personalized message")]
    async fn greet(&self, args: GreetArgs) -> String {
        format!("{}, {}!", self.greeting, args.name)
    }

    /// Add two numbers together
    #[tool("Add two numbers and return the sum")]
    async fn add(&self, args: AddArgs) -> i64 {
        args.a + args.b
    }

    /// Echo a message back
    #[tool("Echo a message back, optionally repeated")]
    async fn echo(&self, args: EchoArgs) -> String {
        let times = args.times.unwrap_or(1);
        vec![args.message; times as usize].join(" ")
    }

    /// Get server status
    #[tool("Get the current server status")]
    async fn status(&self) -> String {
        "Server is running on portable architecture!".to_string()
    }
}

// Native entry point (when `native` feature is enabled)
#[cfg(feature = "native")]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create the portable server
    let server = PortableServer::new("Hello".into());

    // Print server info
    let (name, version) = PortableServer::server_info();
    eprintln!("Starting {} v{}", name, version);
    eprintln!(
        "Tools available: {:?}",
        PortableServer::get_tools_metadata()
    );

    // Run with STDIO transport (send JSON-RPC messages via stdin)
    eprintln!("Running on STDIO (send JSON-RPC messages via stdin)");
    server.into_native_server()?.run_stdio().await?;

    Ok(())
}

// Stub main for when native feature is not enabled
#[cfg(not(feature = "native"))]
fn main() {
    eprintln!("This example requires the 'native' feature.");
    eprintln!("Run with: cargo run --example portable_server -p turbomcp-wasm --features native");
}
