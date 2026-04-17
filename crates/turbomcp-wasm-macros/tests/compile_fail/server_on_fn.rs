//! The `#[server]` macro rejects being placed on a free function.

use turbomcp_wasm_macros::server;

#[server(name = "bad")]
fn not_an_impl() {}

fn main() {}
