//! Matching schema definitions to the Rust types typify generated for them.

/// The top-level type names in the generated file.
pub fn generated_types(src: &str) -> Vec<&str> {
    src.lines()
        .filter_map(|line| {
            ["pub struct ", "pub enum ", "pub type "]
                .iter()
                .find_map(|kw| line.strip_prefix(kw))
        })
        .filter_map(|rest| {
            rest.split(|c: char| !c.is_alphanumeric() && c != '_')
                .next()
        })
        .filter(|name| !name.is_empty())
        .collect()
}

/// typify's name for a schema definition: the same letters, re-cased
/// (`JSONRPCMessage` is `JsonrpcMessage`).
pub fn rust_name<'a>(def: &str, types: &[&'a str]) -> Option<&'a str> {
    let key = |s: &str| -> String {
        s.chars()
            .filter(char::is_ascii_alphanumeric)
            .map(|c| c.to_ascii_lowercase())
            .collect()
    };
    let wanted = key(def);
    types.iter().copied().find(|ty| key(ty) == wanted)
}
