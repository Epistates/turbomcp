//! WASI Preview 2 Runtime Support for TurboMCP
//!
//! This module provides full MCP client functionality for WASI environments,
//! enabling server-side WebAssembly runtimes like Wasmtime, WasmEdge, and Wasmer.
//!
//! # WASI Preview 2 Interfaces Used
//!
//! - `wasi:cli/stdin` / `wasi:cli/stdout` - STDIO transport for MCP JSON-RPC
//! - `wasi:http/outgoing-handler` - HTTP client for HTTP-based MCP servers
//! - `wasi:io/streams` - Streaming I/O primitives
//! - `wasi:clocks/monotonic-clock` - Timing and timeouts
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    WASI Runtime                              │
//! │  (Wasmtime, WasmEdge, Wasmer, etc.)                         │
//! └─────────────────────────────────────────────────────────────┘
//!                              │
//!                              ▼
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    TurboMCP WASI Module                      │
//! │  ┌─────────────────┐    ┌─────────────────────────────────┐ │
//! │  │  StdioTransport │    │      HttpTransport              │ │
//! │  │  (wasi:cli/*)   │    │  (wasi:http/outgoing-handler)   │ │
//! │  └────────┬────────┘    └───────────────┬─────────────────┘ │
//! │           │                             │                    │
//! │           └──────────┬──────────────────┘                    │
//! │                      ▼                                       │
//! │           ┌─────────────────────┐                           │
//! │           │     McpClient       │                           │
//! │           │  (MCP Protocol)     │                           │
//! │           └─────────────────────┘                           │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Usage
//!
//! ## STDIO Transport (for MCP servers)
//!
//! ```ignore
//! use turbomcp_wasm::wasi::{McpClient, StdioTransport};
//!
//! // Create client with STDIO transport
//! let transport = StdioTransport::new();
//! let mut client = McpClient::with_stdio(transport);
//!
//! // Initialize and use
//! client.initialize()?;
//! let tools = client.list_tools()?;
//! ```
//!
//! ## HTTP Transport (for HTTP-based MCP)
//!
//! ```ignore
//! use turbomcp_wasm::wasi::{McpClient, HttpTransport};
//!
//! // Create client with HTTP transport
//! let transport = HttpTransport::new("https://api.example.com/mcp");
//! let mut client = McpClient::with_http(transport);
//!
//! // Initialize and use
//! client.initialize()?;
//! let result = client.call_tool("my_tool", serde_json::json!({"arg": "value"}))?;
//! ```
//!
//! # Building for WASI
//!
//! ```bash
//! # Add the wasm32-wasip2 target
//! rustup target add wasm32-wasip2
//!
//! # Build with WASI feature
//! cargo build -p turbomcp-wasm --target wasm32-wasip2 --features wasi --no-default-features
//!
//! # Run with Wasmtime
//! wasmtime run --wasi http target/wasm32-wasip2/debug/my_mcp_client.wasm
//! ```
//!
//! # Binary Size Optimization
//!
//! For production deployments, use the `wasm-release` profile:
//!
//! ```bash
//! cargo build -p turbomcp-wasm --target wasm32-wasip2 --features wasi \
//!     --no-default-features --profile wasm-release
//! wasm-opt -Oz -o optimized.wasm target/wasm32-wasip2/wasm-release/turbomcp_wasm.wasm
//! ```

mod client;
mod http;
mod stdio;
mod transport;

pub use client::McpClient;
pub use http::HttpTransport;
pub use stdio::StdioTransport;
pub use transport::{Transport, TransportError};

/// WASI runtime information
#[derive(Debug, Clone)]
pub struct WasiRuntime {
    /// Name of the WASI runtime (if detectable)
    pub name: Option<String>,
    /// WASI Preview version (currently always "2")
    pub preview_version: &'static str,
}

impl WasiRuntime {
    /// Get information about the current WASI runtime
    #[must_use]
    pub fn detect() -> Self {
        Self {
            name: None, // Runtime detection is not standardized in WASI
            preview_version: "2",
        }
    }
}

impl Default for WasiRuntime {
    fn default() -> Self {
        Self::detect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wasi_runtime_detect() {
        let runtime = WasiRuntime::detect();
        assert_eq!(runtime.preview_version, "2");
    }
}
