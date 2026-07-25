//! Two tools may not answer to the same wire name.
use turbomcp::prelude::*;

#[derive(Clone)]
struct Dup;

#[server(name = "dup", version = "1.0.0")]
impl Dup {
    #[tool]
    async fn search(&self) -> String {
        "a".into()
    }

    #[tool(name = "search")]
    async fn search_v2(&self) -> String {
        "b".into()
    }
}

fn main() {}
