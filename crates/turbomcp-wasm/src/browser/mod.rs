//! Browser-specific MCP client implementation
//!
//! This module provides an MCP client that works in web browsers using
//! the Fetch API and WebSocket API.

mod client;
mod transport;

pub use client::McpClient;
pub use transport::{FetchTransport, WebSocketTransport};

use wasm_bindgen::prelude::*;

/// Initialize the WASM module (called automatically)
#[wasm_bindgen(start)]
pub fn init() {
    // Set up panic hook for better error messages
    #[cfg(feature = "console-log")]
    console_error_panic_hook::set_once();
}

/// Log a message to the browser console
#[wasm_bindgen]
pub fn log(message: &str) {
    web_sys::console::log_1(&message.into());
}

/// Log an error to the browser console
#[wasm_bindgen]
pub fn error(message: &str) {
    web_sys::console::error_1(&message.into());
}
