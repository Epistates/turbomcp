//! Handlers run concurrently on a shared server: `&mut self` is refused at
//! the receiver, with what to do instead.
use turbomcp::prelude::*;

#[derive(Clone)]
struct S {
    count: u64,
}

#[server(name = "s", version = "1.0.0")]
impl S {
    #[tool]
    async fn bump(&mut self) -> u64 {
        self.count += 1;
        self.count
    }
}

fn main() {}
