//! A `Json<T>` return already gives the tool its output schema; a second
//! one would disagree with it sooner or later.
use turbomcp::prelude::*;

#[derive(serde::Serialize, schemars::JsonSchema)]
struct A {
    n: u32,
}

#[derive(Clone)]
struct S;

#[server(name = "s", version = "1.0.0")]
impl S {
    #[tool(output_schema = A)]
    async fn a(&self) -> Json<A> {
        Json(A { n: 1 })
    }
}

fn main() {}
