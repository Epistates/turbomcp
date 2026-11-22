//! A template over the matcher's limit of 32 variables would be listed but
//! never readable; the build says so, with the limit and the count.
use turbomcp::prelude::*;

#[derive(Clone)]
struct S;

#[server(name = "s", version = "1.0.0")]
impl S {
    #[resource(
        "search://items{?a0,a1,a2,a3,a4,a5,a6,a7,a8,a9,b0,b1,b2,b3,b4,b5,b6,b7,b8,b9,c0,c1,c2,c3,c4,c5,c6,c7,c8,c9,d0,d1,d2}"
    )]
    async fn search(&self) -> McpResult<String> {
        Ok(String::new())
    }
}

fn main() {}
