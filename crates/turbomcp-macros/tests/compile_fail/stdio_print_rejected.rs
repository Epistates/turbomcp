//! This test should FAIL to compile.
//! Demonstrates that print! is rejected in stdio servers.

use turbomcp_macros::server;
use turbomcp_protocol::Result;

#[derive(Clone)]
pub struct MyServer;

#[server(transports = ["stdio"])]
impl MyServer {
    #[turbomcp_macros::tool("My tool")]
    async fn my_tool(&self) -> Result<String> {
        print!("This is not allowed");
        Ok("result".to_string())
    }
}

fn main() {}
