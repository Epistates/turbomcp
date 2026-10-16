//! `_meta` keys under the spec's prefix (or TurboMCP's own) aren't a
//! component's to set.
use turbomcp::prelude::*;

#[derive(Clone)]
struct S;

#[server(name = "s", version = "1.0.0")]
impl S {
    #[tool(meta("io.modelcontextprotocol/related-task" = "x"))]
    async fn a(&self) -> String {
        String::new()
    }
}

fn main() {}
