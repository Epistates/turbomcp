//! `#[on_cancelled]` runs as the notification is read, so it is a plain
//! method; an `async` one is refused with the shape it should have.
use turbomcp::prelude::*;

#[derive(Clone)]
struct S;

#[server(name = "s", version = "1.0.0")]
impl S {
    #[on_cancelled]
    async fn note(&self, id: &RequestId, reason: Option<&str>) {
        let _ = (id, reason);
    }

    #[tool]
    async fn t(&self) -> String {
        String::new()
    }
}

fn main() {}
