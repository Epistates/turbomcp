//! A marker outside a `#[server]` impl registers nothing, so it is an error
//! rather than a method that silently isn't a tool.

#[derive(Clone)]
struct S;

impl S {
    #[turbomcp::tool]
    async fn forgotten(&self) -> String {
        String::new()
    }
}

fn main() {}
