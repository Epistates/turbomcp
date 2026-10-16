//! `size` describes one resource; a template stands for many.
use turbomcp::prelude::*;

#[derive(Clone)]
struct S;

#[server(name = "s", version = "1.0.0")]
impl S {
    #[resource("file://{path}", size = 10)]
    async fn file(&self, path: String) -> McpResult<String> {
        Ok(path)
    }
}

fn main() {}
