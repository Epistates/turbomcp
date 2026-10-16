//! A prompt argument arrives as a string and a failure to parse it goes back
//! to the caller, so its type must be `FromStr` with a `Display` error;
//! anything else is refused at the type, saying what to do.
use turbomcp::prelude::*;

#[derive(Clone)]
struct S;

struct Opaque;

/// Parseable, but its error can't be shown to the caller.
impl std::str::FromStr for Opaque {
    type Err = ();
    fn from_str(_: &str) -> Result<Self, ()> {
        Ok(Self)
    }
}

#[server(name = "s", version = "1.0.0")]
impl S {
    #[prompt]
    async fn p(&self, value: Opaque) -> String {
        let _ = value;
        String::new()
    }
}

fn main() {}
