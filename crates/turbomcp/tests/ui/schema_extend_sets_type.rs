//! A tool's input schema is always an object; `schema_extend` can't change it.
use turbomcp::prelude::*;

#[derive(Clone)]
struct S;

#[server(name = "s", version = "1.0.0")]
impl S {
    #[tool(schema_extend = r#"{"type": "array"}"#)]
    async fn odd(&self) -> String {
        String::new()
    }
}

fn main() {}
