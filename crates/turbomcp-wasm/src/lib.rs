//! `TurboMCP` WebAssembly Bindings
//!
//! This crate provides WebAssembly bindings for TurboMCP, enabling MCP clients
//! to run in browsers and WASI environments.
//!
//! # Features
//!
//! - **browser** (default): Browser bindings using wasm-bindgen and web-sys
//! - **wasi**: WASI Preview 2 support for server-side WASM runtimes
//!
//! # Browser Usage
//!
//! ```javascript
//! import init, { McpClient, Tool, Content } from 'turbomcp-wasm';
//!
//! await init();
//!
//! const client = new McpClient("https://api.example.com/mcp");
//! await client.initialize();
//!
//! const tools = await client.listTools();
//! console.log("Available tools:", tools);
//!
//! const result = await client.callTool("my_tool", { arg: "value" });
//! console.log("Result:", result);
//! ```
//!
//! # Binary Size
//!
//! This crate targets minimal binary size with proper optimization:
//!
//! | Configuration | Unoptimized | With wasm-opt |
//! |---------------|-------------|---------------|
//! | Core types    | ~400KB      | ~150KB        |
//! | + JSON        | ~600KB      | ~250KB        |
//! | + HTTP client | ~1.1MB      | ~400KB        |
//!
//! For smallest binaries, build with `--profile wasm-release` and use `wasm-opt -Oz`:
//! ```bash
//! cargo build -p turbomcp-wasm --target wasm32-unknown-unknown --profile wasm-release
//! wasm-opt -Oz -o optimized.wasm target/wasm32-unknown-unknown/wasm-release/turbomcp_wasm.wasm
//! ```

#![deny(unsafe_code)]
#![warn(missing_docs)]
#![cfg_attr(docsrs, feature(doc_cfg))]

// Re-export core types for WASM consumers
pub use turbomcp_core::error::{ErrorKind, McpError};
pub use turbomcp_core::types::{
    capabilities::{ClientCapabilities, ServerCapabilities},
    content::{Content, ResourceContent},
    core::{Implementation, Role},
    initialization::{InitializeRequest, InitializeResult},
    prompts::{GetPromptResult, Prompt, PromptArgument},
    resources::{Resource, ResourceTemplate},
    tools::{CallToolResult, Tool, ToolInputSchema},
};

#[cfg(feature = "browser")]
#[cfg_attr(docsrs, doc(cfg(feature = "browser")))]
pub mod browser;

#[cfg(feature = "wasi")]
#[cfg_attr(docsrs, doc(cfg(feature = "wasi")))]
pub mod wasi;

/// Version of the TurboMCP WASM bindings
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// MCP protocol version supported
pub const PROTOCOL_VERSION: &str = "2025-11-25";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        assert!(!VERSION.is_empty());
    }

    #[test]
    fn test_protocol_version() {
        assert_eq!(PROTOCOL_VERSION, "2025-11-25");
    }

    #[test]
    fn test_core_types_available() {
        // Verify core types are re-exported correctly
        let _impl = Implementation {
            name: "test".to_string(),
            title: None,
            description: None,
            version: "1.0.0".to_string(),
            icon: None,
        };

        let _caps = ClientCapabilities::default();
        let _content = Content::Text {
            text: "hello".to_string(),
            annotations: None,
        };
    }
}
