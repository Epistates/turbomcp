//! `task` takes `"optional"` or `"required"`; anything else is named at the
//! marker.
use turbomcp::prelude::*;

#[derive(Clone)]
struct Wrong;

#[server(name = "wrong", version = "1.0.0")]
impl Wrong {
    #[tool(task = "always")]
    async fn batch(&self) -> String {
        "batched".into()
    }
}

fn main() {}
