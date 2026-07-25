//! Two resources may not claim the same URI — `read_resource` matches on it,
//! so the second arm would be dead.
use turbomcp::prelude::*;

#[derive(Clone)]
struct Dup;

#[server(name = "dup", version = "1.0.0")]
impl Dup {
    #[resource("config://app")]
    async fn config(&self) -> McpResult<String> {
        Ok("a".into())
    }

    #[resource("config://app")]
    async fn config_v2(&self) -> McpResult<String> {
        Ok("b".into())
    }
}

fn main() {}
