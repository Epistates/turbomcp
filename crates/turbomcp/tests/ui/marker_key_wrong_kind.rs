//! Marker keys are gated per kind: `task` is a tool concept, and the error
//! names the marker it was written on rather than a union of every key.
use turbomcp::prelude::*;

#[derive(Clone)]
struct Wrong;

#[server(name = "wrong", version = "1.0.0")]
impl Wrong {
    #[prompt(task)]
    async fn summarize(&self, text: String) -> String {
        text
    }
}

fn main() {}
