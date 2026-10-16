//! A prefix length of `0` breaks RFC 6570; the matcher would refuse the
//! template and the resource would be listed but never readable. The same
//! grammar runs at compile time, so it fails here instead.
use turbomcp::prelude::*;

#[derive(Clone)]
struct S;

#[server(name = "s", version = "1.0.0")]
impl S {
    #[resource("file://{path:0}")]
    async fn file(&self, path: String) -> McpResult<String> {
        Ok(path)
    }
}

fn main() {}
