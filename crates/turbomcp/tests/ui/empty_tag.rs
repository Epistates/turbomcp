//! A tag is a lookup key, so an empty one can never be matched deliberately —
//! it is caught at the literal rather than shipped as an unmatchable `_meta`
//! entry.
use turbomcp::prelude::*;

#[derive(Clone)]
struct Tagged;

#[server(name = "tagged", version = "1.0.0")]
impl Tagged {
    #[tool(tags("admin", ""))]
    async fn wipe(&self) -> String {
        "wiped".into()
    }
}

fn main() {}
