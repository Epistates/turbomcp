//! A template no URI could ever match fails to compile rather than being
//! listed forever.
use turbomcp::prelude::*;

#[derive(Clone)]
struct S;

#[server(name = "s", version = "1.0.0")]
impl S {
    #[resource("file://{path")]
    async fn file(&self, path: String) -> McpResult<String> {
        Ok(path)
    }
}

fn main() {}
