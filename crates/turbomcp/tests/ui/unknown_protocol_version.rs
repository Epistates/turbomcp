//! `protocols(…)` only accepts versions this build serves.
use turbomcp::prelude::*;

#[derive(Clone)]
struct Bad;

#[server(name = "bad", version = "1.0.0", protocols("1999-01-01"))]
impl Bad {
    #[tool]
    async fn thing(&self) -> String {
        "a".into()
    }
}

fn main() {}
