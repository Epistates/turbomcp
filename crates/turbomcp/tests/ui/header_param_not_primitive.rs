//! `#[mcp_header]` takes a string, integer or bool (or an `Option` of one):
//! "Parameters with type `number` are not permitted."
use turbomcp::prelude::*;

#[derive(Clone)]
struct S;

#[server(name = "s", version = "1.0.0")]
impl S {
    #[tool]
    async fn locate(&self, #[mcp_header] lat: f64) -> String {
        lat.to_string()
    }
}

fn main() {}
