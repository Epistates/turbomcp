//! A tool name outside the spec's permitted character set is a compile error.
use turbomcp::prelude::*;

#[derive(Clone)]
struct Bad;

#[server(name = "bad", version = "1.0.0")]
impl Bad {
    #[tool(name = "search the web")]
    async fn search(&self) -> String {
        "a".into()
    }
}

fn main() {}
