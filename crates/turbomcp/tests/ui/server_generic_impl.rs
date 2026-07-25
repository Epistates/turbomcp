//! #[server] cannot be applied to a generic impl block.
use turbomcp::prelude::*;

#[derive(Clone)]
struct Generic<T>(std::marker::PhantomData<T>);

#[server(name = "generic", version = "1.0.0")]
impl<T: Clone + Send + Sync + 'static> Generic<T> {
    #[tool]
    async fn thing(&self) -> String {
        "a".into()
    }
}

fn main() {}
