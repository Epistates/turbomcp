//! One method is one component.
use turbomcp::prelude::*;

#[derive(Clone)]
struct S;

#[server(name = "s", version = "1.0.0")]
impl S {
    #[tool]
    #[prompt]
    async fn both(&self) -> String {
        String::new()
    }
}

fn main() {}
