//! A `#[tool]` gets a `&CallToolContext`; `&RequestContext` (in the prelude)
//! is a natural mistake, answered at the parameter with the right type.
use turbomcp::prelude::*;

#[derive(Clone)]
struct S;

#[server(name = "s", version = "1.0.0")]
impl S {
    #[tool]
    async fn who(&self, ctx: &RequestContext) -> String {
        format!("{:?}", ctx.identity)
    }
}

fn main() {}
