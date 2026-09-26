//! A variable the URI may leave out (`{?q}`) must be bound as an `Option`.
use turbomcp::prelude::*;

#[derive(Clone)]
struct S;

#[server(name = "s", version = "1.0.0")]
impl S {
    #[resource("search://items{?q}")]
    async fn search(&self, q: String) -> McpResult<String> {
        Ok(q)
    }
}

fn main() {}
