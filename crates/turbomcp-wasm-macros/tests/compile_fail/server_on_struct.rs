//! The `#[server]` macro must be applied to an `impl` block, not a struct.

use turbomcp_wasm_macros::server;

#[server(name = "bad")]
struct NotAnImpl;

fn main() {}
